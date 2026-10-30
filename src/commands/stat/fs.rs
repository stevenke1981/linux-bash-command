//! `stat -f`: file-system status (`statvfs`/`statfs`). Available on Unix only; on Windows
//! the option is reported as unsupported by the caller.

#![cfg_attr(not(unix), allow(dead_code))]

use std::io;

use super::format::expand_with;

/// The fields `stat -f` prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsInfo {
    pub name: String,
    /// File-system id, printed as hex.
    pub fsid: u64,
    /// Maximum file-name length.
    pub namelen: u64,
    /// Magic number of the file system (`f_type`), 0 when not available.
    pub kind: u64,
    /// Block size used for the counts below (`f_bsize`).
    pub bsize: u64,
    /// Fundamental block size (`f_frsize`).
    pub frsize: u64,
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
}

/// Windows has no `statvfs`; the caller refuses `-f` there before reaching this point.
#[cfg(not(unix))]
pub fn read(_name: &str) -> io::Result<FsInfo> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Reads the file-system status for `path`. The field casts are needed on Unix systems
/// whose `statvfs` fields are narrower than 64 bits, so the lint is allowed here.
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
pub fn read(name: &str) -> io::Result<FsInfo> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(std::path::Path::new(name).as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut vfs = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c_path` is a valid NUL-terminated string and `vfs` is written on success.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), vfs.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statvfs returned 0, so the structure is initialised.
    let vfs = unsafe { vfs.assume_init() };
    Ok(FsInfo {
        name: name.to_string(),
        fsid: vfs.f_fsid as u64,
        namelen: vfs.f_namemax as u64,
        kind: file_system_type(&c_path),
        bsize: vfs.f_bsize as u64,
        frsize: vfs.f_frsize as u64,
        blocks: vfs.f_blocks as u64,
        bfree: vfs.f_bfree as u64,
        bavail: vfs.f_bavail as u64,
        files: vfs.f_files as u64,
        ffree: vfs.f_ffree as u64,
    })
}

/// The `f_type` magic from `statfs`. Only Linux exposes it; other Unix systems report 0.
#[cfg(target_os = "linux")]
fn file_system_type(path: &std::ffi::CStr) -> u64 {
    let mut buf = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `path` is NUL-terminated and `buf` is written on success.
    let rc = unsafe { libc::statfs(path.as_ptr(), buf.as_mut_ptr()) };
    if rc != 0 {
        return 0;
    }
    // SAFETY: statfs returned 0, so the structure is initialised.
    let buf = unsafe { buf.assume_init() };
    buf.f_type as u64
}

#[cfg(all(unix, not(target_os = "linux")))]
fn file_system_type(_path: &std::ffi::CStr) -> u64 {
    0
}

/// Type name for a `f_type` magic, as GNU prints it.
pub fn type_name(kind: u64) -> String {
    let name = match kind {
        0xEF53 => "ext2/ext3",
        0x9123_683E => "btrfs",
        0x5846_5342 => "xfs",
        0x0102_1994 => "tmpfs",
        0x794C_7630 => "overlayfs",
        0x9FA0 => "proc",
        0x6265_6572 => "sysfs",
        0x6969 => "nfs",
        0x2FC1_2FC1 => "zfs",
        0x4D44 => "msdos",
        _ => return format!("UNKNOWN (0x{kind:x})"),
    };
    name.to_string()
}

/// The default `stat -f` layout (GNU 9.4).
pub fn default_layout(info: &FsInfo) -> String {
    let mut out = String::new();
    out.push_str(&format!("  File: \"{}\"\n", info.name));
    out.push_str(&format!(
        "    ID: {:016x} Namelen: {:<7} Type: {}\n",
        info.fsid,
        info.namelen,
        type_name(info.kind)
    ));
    out.push_str(&format!(
        "Block size: {:<10} Fundamental block size: {}\n",
        info.bsize, info.frsize
    ));
    out.push_str(&format!(
        "Blocks: Total: {:<10} Free: {:<10} Available: {}\n",
        info.blocks, info.bfree, info.bavail
    ));
    out.push_str(&format!(
        "Inodes: Total: {:<10} Free: {}\n",
        info.files, info.ffree
    ));
    out
}

/// The `-t` terse layout for `stat -f`.
pub fn terse_layout(info: &FsInfo) -> String {
    format!(
        "{} {:016x} {} {:x} {} {} {} {} {} {} {}\n",
        info.name,
        info.fsid,
        info.namelen,
        info.kind,
        info.bsize,
        info.frsize,
        info.blocks,
        info.bfree,
        info.bavail,
        info.files,
        info.ffree
    )
}

/// Expands a `-c`/`--printf` format for `stat -f`.
pub fn expand(format: &str, info: &FsInfo) -> String {
    expand_with(format, &|conv: &str| match conv {
        "a" => info.bavail.to_string(),
        "b" => info.blocks.to_string(),
        "c" => info.files.to_string(),
        "d" => info.ffree.to_string(),
        "f" => info.bfree.to_string(),
        "i" => format!("{:016x}", info.fsid),
        "l" => info.namelen.to_string(),
        "n" => info.name.clone(),
        "s" => info.bsize.to_string(),
        "S" => info.frsize.to_string(),
        "t" => format!("{:x}", info.kind),
        "T" => type_name(info.kind),
        "%" => "%".to_string(),
        _ => "?".to_string(),
    })
}
