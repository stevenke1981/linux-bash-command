//! Platform layer for `stat`: reads the stat fields of a path into [`RawStat`].
//!
//! Unix uses the real `st_*` fields. Windows synthesises what it can (see
//! `docs/spec` 02-conventions §7): owner and group are `0`, the inode and link count come
//! from `common::meta`, and the mode is built from the file type and permission bits.

use std::fs::Metadata;
use std::io;
use std::path::Path;

/// One `struct stat`, with times as `(seconds, nanoseconds)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawStat {
    pub dev: u64,
    pub ino: u64,
    /// Full `st_mode`, including the file-type bits.
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u64,
    pub size: u64,
    /// 512-byte units allocated.
    pub blocks: u64,
    pub blksize: u64,
    pub atime: (i64, u32),
    pub mtime: (i64, u32),
    pub ctime: (i64, u32),
    pub btime: Option<(i64, u32)>,
}

pub const S_IFMT: u32 = 0o170000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFLNK: u32 = 0o120000;

/// Reads `meta`, the metadata of `path` (already followed or not, as the caller chose).
#[cfg(unix)]
pub fn raw_stat(_path: &Path, meta: &Metadata) -> RawStat {
    use std::os::unix::fs::MetadataExt;
    RawStat {
        dev: meta.dev(),
        ino: meta.ino(),
        mode: meta.mode(),
        nlink: meta.nlink(),
        uid: meta.uid(),
        gid: meta.gid(),
        rdev: meta.rdev(),
        size: meta.size(),
        blocks: meta.blocks(),
        blksize: meta.blksize(),
        atime: (meta.atime(), clamp_nanos(meta.atime_nsec())),
        mtime: (meta.mtime(), clamp_nanos(meta.mtime_nsec())),
        ctime: (meta.ctime(), clamp_nanos(meta.ctime_nsec())),
        btime: meta.created().ok().and_then(system_time_pair),
    }
}

#[cfg(unix)]
fn clamp_nanos(nanos: i64) -> u32 {
    u32::try_from(nanos.clamp(0, 999_999_999)).unwrap_or(0)
}

/// Windows variant: synthesised mode, owner `0`, times from the file system.
#[cfg(not(unix))]
pub fn raw_stat(path: &Path, meta: &Metadata) -> RawStat {
    let kind = if meta.file_type().is_symlink() {
        S_IFLNK
    } else if meta.is_dir() {
        S_IFDIR
    } else {
        S_IFREG
    };
    // Identity is read from the file itself, so du and stat can tell entries apart. A
    // symbolic link is identified by its own entry, not its target.
    let (dev, ino, nlink) =
        crate::common::meta::path_identity(path, !meta.file_type().is_symlink()).unwrap_or_else(
            || {
                let (ino, nlink) = crate::common::meta::inode_nlink(meta);
                (0, ino, nlink)
            },
        );
    let permissions = crate::common::meta::permission_bits(path, meta);
    let modified = meta
        .modified()
        .ok()
        .and_then(system_time_pair)
        .unwrap_or((0, 0));
    let accessed = meta
        .accessed()
        .ok()
        .and_then(system_time_pair)
        .unwrap_or(modified);
    let blocks = meta.len().div_ceil(4096) * 8;
    RawStat {
        dev,
        ino,
        mode: kind | permissions,
        nlink,
        uid: 0,
        gid: 0,
        rdev: 0,
        size: meta.len(),
        blocks,
        blksize: 4096,
        atime: accessed,
        mtime: modified,
        ctime: modified,
        btime: meta.created().ok().and_then(system_time_pair),
    }
}

/// Splits a `SystemTime` into seconds and nanoseconds since the epoch.
pub fn system_time_pair(time: std::time::SystemTime) -> Option<(i64, u32)> {
    let timestamp = jiff::Timestamp::try_from(time).ok()?;
    Some((timestamp.as_second(), timestamp.subsec_nanosecond() as u32))
}

/// Owner name for `uid`, or `None` when the account database has no entry.
#[cfg(unix)]
pub fn user_name(uid: u32) -> Option<String> {
    // SAFETY: getpwuid returns a pointer into static storage (or null); the name is copied
    // out before any other libc call that could reuse that storage.
    unsafe {
        let entry = libc::getpwuid(uid as libc::uid_t);
        if entry.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*entry).pw_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Group name for `gid`, or `None` when the group database has no entry.
#[cfg(unix)]
pub fn group_name(gid: u32) -> Option<String> {
    // SAFETY: as for `user_name`; getgrgid returns static storage or null.
    unsafe {
        let entry = libc::getgrgid(gid as libc::gid_t);
        if entry.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*entry).gr_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Windows has no account database; `stat` prints `-` for owner and group names instead.
#[cfg(not(unix))]
#[allow(dead_code)]
pub fn user_name(_uid: u32) -> Option<String> {
    None
}

/// See [`user_name`].
#[cfg(not(unix))]
#[allow(dead_code)]
pub fn group_name(_gid: u32) -> Option<String> {
    None
}

/// Device id of `path` (following symlinks), used to find the mount point.
pub fn device_of(path: &Path) -> io::Result<u64> {
    Ok(raw_stat(path, &std::fs::metadata(path)?).dev)
}

/// The file-type word of `%F`.
pub fn kind_word(mode: u32) -> &'static str {
    match mode & S_IFMT {
        S_IFREG => "regular file",
        S_IFDIR => "directory",
        S_IFLNK => "symbolic link",
        0o060000 => "block special file",
        0o020000 => "character special file",
        0o010000 => "fifo",
        0o140000 => "socket",
        _ => "weird file",
    }
}

/// `ls`-style mode text such as `drwxr-xr-x` or `-rwsr-xr-x`.
pub fn mode_text(mode: u32) -> String {
    let kind = match mode & S_IFMT {
        S_IFREG => '-',
        S_IFDIR => 'd',
        S_IFLNK => 'l',
        0o060000 => 'b',
        0o020000 => 'c',
        0o010000 => 'p',
        0o140000 => 's',
        _ => '?',
    };
    let mut text = String::with_capacity(10);
    text.push(kind);
    for (shift, special, special_char) in [(6, 0o4000, 's'), (3, 0o2000, 's'), (0, 0o1000, 't')] {
        let triple = (mode >> shift) & 0o7;
        text.push(if triple & 0o4 != 0 { 'r' } else { '-' });
        text.push(if triple & 0o2 != 0 { 'w' } else { '-' });
        let executable = triple & 0o1 != 0;
        text.push(match (executable, mode & special != 0) {
            (true, true) => special_char,
            (false, true) => special_char.to_ascii_uppercase(),
            (true, false) => 'x',
            (false, false) => '-',
        });
    }
    text
}

/// Splits a device number into `(major, minor)` with the glibc encoding.
pub fn split_device(dev: u64) -> (u64, u64) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    (major, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_text_regular_and_directory() {
        assert_eq!(mode_text(S_IFREG | 0o644), "-rw-r--r--");
        assert_eq!(mode_text(S_IFDIR | 0o755), "drwxr-xr-x");
        assert_eq!(mode_text(S_IFLNK | 0o777), "lrwxrwxrwx");
    }

    #[test]
    fn mode_text_special_bits() {
        assert_eq!(mode_text(S_IFREG | 0o4755), "-rwsr-xr-x");
        assert_eq!(mode_text(S_IFREG | 0o4644), "-rwSr--r--");
        assert_eq!(mode_text(S_IFDIR | 0o1777), "drwxrwxrwt");
        assert_eq!(mode_text(S_IFDIR | 0o1666), "drw-rw-rwT");
    }

    #[test]
    fn kind_words() {
        assert_eq!(kind_word(S_IFREG), "regular file");
        assert_eq!(kind_word(S_IFDIR), "directory");
        assert_eq!(kind_word(S_IFLNK), "symbolic link");
        assert_eq!(kind_word(0o010000), "fifo");
        assert_eq!(kind_word(0), "weird file");
    }

    #[test]
    fn device_split_matches_glibc() {
        // 254,0 is the device reported for the test volume: 254 << 8.
        assert_eq!(split_device(254 << 8), (254, 0));
        assert_eq!(split_device(0x0801), (8, 1));
        assert_eq!(
            split_device(0x1234_5678_9abc_def0),
            (0x1234_5cde, 0x1_2345_6789_abf0)
        );
    }

    #[cfg(unix)]
    #[test]
    fn clamp_nanos_keeps_range() {
        assert_eq!(clamp_nanos(-5), 0);
        assert_eq!(clamp_nanos(1_000_000_000), 999_999_999);
        assert_eq!(clamp_nanos(42), 42);
    }

    #[test]
    fn raw_stat_of_this_file_has_regular_mode() {
        let path = Path::new(file!());
        let meta = std::fs::metadata(path).unwrap();
        let raw = raw_stat(path, &meta);
        assert_eq!(raw.mode & S_IFMT, S_IFREG);
        assert_eq!(raw.size, meta.len());
        assert!(raw.mtime.0 > 0);
    }

    #[test]
    fn system_time_pair_handles_epoch() {
        // Windows stores times in 100 ns ticks, so the sub-second part must be a multiple of
        // 100 ns to survive on every platform.
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(7, 500);
        assert_eq!(system_time_pair(t), Some((7, 500)));
    }
}
