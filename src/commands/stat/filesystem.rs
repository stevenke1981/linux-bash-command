use super::format::render_with_bytes;
use std::io;
use std::path::Path;

pub struct FileSystem {
    pub blocks: u64,
    pub free: u64,
    pub available: u64,
    pub files: Option<u64>,
    pub free_files: Option<u64>,
    pub block_size: u64,
    pub transfer_size: u64,
    pub name_max: u64,
    pub id: String,
    pub kind: String,
    pub kind_name: String,
}

pub fn default_format(terse: bool) -> &'static str {
    if terse {
        "%n %i %l %t %s %S %b %f %a %c %d\n"
    } else {
        "  File: \"%n\"\n    ID: %-8i Namelen: %-7l Type: %T\nBlock size: %-10s Fundamental block size: %S\nBlocks: Total: %-10b Free: %-10f Available: %a\nInodes: Total: %-10c Free: %d\n"
    }
}

pub fn render(path: &Path, template: &[u8]) -> io::Result<Vec<u8>> {
    let info = query(path)?;
    Ok(render_with_bytes(template, |directive| {
        let number = match directive {
            "a" => Some(info.available),
            "b" => Some(info.blocks),
            "c" => info.files,
            "d" => info.free_files,
            "f" => Some(info.free),
            "l" => Some(info.name_max),
            "s" => Some(info.transfer_size),
            "S" => Some(info.block_size),
            _ => None,
        };
        if let Some(number) = number {
            return (number.to_string(), Some(10));
        }
        if matches!(directive, "i" | "t") {
            let value = if directive == "i" {
                &info.id
            } else {
                &info.kind
            };
            return (
                value.clone(),
                u64::from_str_radix(value, 16).ok().map(|_| 16),
            );
        }
        (
            match directive {
                "n" => crate::display_path(path),
                "T" => info.kind_name.clone(),
                "c" | "d" => "-".into(),
                "%" => "%".into(),
                _ => "?".into(),
            },
            None,
        )
    }))
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
pub fn query(path: &Path) -> io::Result<FileSystem> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL terminated and value points to writable statvfs storage.
    if unsafe { libc::statvfs(path.as_ptr(), value.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let value = unsafe { value.assume_init() };
    let (kind, kind_name, id) = filesystem_type(&path)?;
    Ok(FileSystem {
        blocks: value.f_blocks as u64,
        free: value.f_bfree as u64,
        available: value.f_bavail as u64,
        files: Some(value.f_files as u64),
        free_files: Some(value.f_ffree as u64),
        block_size: value.f_frsize as u64,
        transfer_size: value.f_bsize as u64,
        name_max: value.f_namemax as u64,
        id,
        kind,
        kind_name,
    })
}

#[cfg(unix)]
fn filesystem_type(path: &std::ffi::CStr) -> io::Result<(String, String, String)> {
    let mut value = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: a valid C path and writable statfs output buffer are supplied.
    if unsafe { libc::statfs(path.as_ptr(), value.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let value = unsafe { value.assume_init() };
    // The opaque filesystem identifier is initialized by statfs; copy only its bytes.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref(&value.f_fsid).cast::<u8>(),
            std::mem::size_of_val(&value.f_fsid),
        )
    };
    let id = bytes
        .iter()
        .rev()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .trim_start_matches('0')
        .to_owned();
    let id = if id.is_empty() { "0".into() } else { id };
    #[cfg(target_os = "linux")]
    let name = match value.f_type as u64 {
        0xef53 => "ext2/ext3",
        0x794c7630 => "overlayfs",
        0x01021994 => "tmpfs",
        0x9fa0 => "proc",
        0x62656572 => "sysfs",
        0x9123683e => "btrfs",
        0x58465342 => "xfs",
        0x6969 => "nfs",
        0xff534d42 => "cifs",
        0x65735546 => "fuseblk",
        0x2fc12fc1 => "zfs",
        0x858458f6 => "ramfs",
        _ => "UNKNOWN",
    }
    .to_owned();
    #[cfg(not(target_os = "linux"))]
    let name = unsafe { std::ffi::CStr::from_ptr(value.f_fstypename.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    Ok((format!("{:x}", value.f_type), name, id))
}

#[cfg(windows)]
pub fn query(path: &Path) -> io::Result<FileSystem> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetVolumeInformationW, GetVolumePathNameW,
    };
    let path = std::fs::canonicalize(path)?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = vec![0u16; 32768];
    // SAFETY: all strings are NUL terminated and buffer lengths match allocations.
    if unsafe { GetVolumePathNameW(path.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let (mut sectors, mut sector_size, mut free_clusters, mut clusters) = (0, 0, 0, 0);
    if unsafe {
        GetDiskFreeSpaceW(
            root.as_ptr(),
            &mut sectors,
            &mut sector_size,
            &mut free_clusters,
            &mut clusters,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let (mut available, mut total, mut free) = (0, 0, 0);
    if unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut available, &mut total, &mut free) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let (mut serial, mut maximum, mut flags) = (0, 0, 0);
    let mut kind = [0u16; 256];
    if unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            &mut maximum,
            &mut flags,
            kind.as_mut_ptr(),
            kind.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let block_size = u64::from(sectors) * u64::from(sector_size);
    Ok(FileSystem {
        blocks: total / block_size,
        free: free / block_size,
        available: available / block_size,
        files: None,
        free_files: None,
        block_size,
        transfer_size: block_size,
        name_max: u64::from(maximum),
        id: format!("{serial:x}"),
        kind: "?".into(),
        kind_name: String::from_utf16_lossy(
            &kind[..kind.iter().position(|ch| *ch == 0).unwrap_or(kind.len())],
        ),
    })
}
