//! File metadata shared by `ls -l`, `find` and `printf`: type letters, `ls`-style mode
//! strings, permission bits (synthesised on Windows) and owner and link fields.

use std::fs::Metadata;
use std::path::Path;

/// The single-letter type used by `find -type` and `%y`: `f d l b c p s`, or `U` when the
/// type is none of these.
pub fn kind_char(meta: &Metadata) -> char {
    let kind = meta.file_type();
    if kind.is_symlink() {
        'l'
    } else if kind.is_dir() {
        'd'
    } else if kind.is_file() {
        'f'
    } else {
        other_kind_char(meta)
    }
}

#[cfg(unix)]
fn other_kind_char(meta: &Metadata) -> char {
    use std::os::unix::fs::FileTypeExt;
    let kind = meta.file_type();
    if kind.is_block_device() {
        'b'
    } else if kind.is_char_device() {
        'c'
    } else if kind.is_fifo() {
        'p'
    } else if kind.is_socket() {
        's'
    } else {
        'U'
    }
}

#[cfg(not(unix))]
fn other_kind_char(_meta: &Metadata) -> char {
    'U'
}

/// The ten-character `ls -l` mode string, such as `drwxr-xr-x`. Regular files and unknown
/// types show `-`.
pub fn mode_string(path: &Path, meta: &Metadata) -> String {
    let kind = match kind_char(meta) {
        'f' | 'U' => '-',
        other => other,
    };
    let bits = permission_bits(path, meta);
    let mut text = String::with_capacity(10);
    text.push(kind);
    for shift in [6, 3, 0] {
        let triple = (bits >> shift) & 0o7;
        text.push(if triple & 0o4 != 0 { 'r' } else { '-' });
        text.push(if triple & 0o2 != 0 { 'w' } else { '-' });
        text.push(if triple & 0o1 != 0 { 'x' } else { '-' });
    }
    text
}

/// Permission bits (`st_mode & 07777`). On Windows they are synthesised: directories and
/// executable names (`.exe .com .bat .cmd .ps1`) get `0755`, other files `0644`, and a
/// read-only entry loses every write bit.
#[cfg(unix)]
pub fn permission_bits(_path: &Path, meta: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o7777
}

/// See the Unix variant. Windows has no mode bits, so the value is synthesised.
#[cfg(not(unix))]
pub fn permission_bits(path: &Path, meta: &Metadata) -> u32 {
    let mut bits = if meta.is_dir() || has_executable_extension(path) {
        0o755
    } else {
        0o644
    };
    if meta.permissions().readonly() {
        bits &= !0o222;
    }
    bits
}

#[cfg_attr(unix, allow(dead_code))]
fn has_executable_extension(path: &Path) -> bool {
    const EXECUTABLE_EXTENSIONS: [&str; 5] = ["exe", "com", "bat", "cmd", "ps1"];
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXECUTABLE_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// Owner and group ids. Windows reports `(0, 0)`.
#[cfg(unix)]
pub fn uid_gid(meta: &Metadata) -> (u32, u32) {
    use std::os::unix::fs::MetadataExt;
    (meta.uid(), meta.gid())
}

/// Owner and group ids. Windows reports `(0, 0)`.
#[cfg(not(unix))]
pub fn uid_gid(_meta: &Metadata) -> (u32, u32) {
    (0, 0)
}

/// Inode number and hard-link count. Windows reports `(0, 1)`.
#[cfg(unix)]
pub fn inode_nlink(meta: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.ino(), meta.nlink())
}

/// Inode number and hard-link count. Windows reports `(0, 1)`.
#[cfg(not(unix))]
pub fn inode_nlink(_meta: &Metadata) -> (u64, u64) {
    (0, 1)
}

/// `(volume, file index, hard-link count)` of the file a path names. The first two identify
/// a file across hard links. With `follow` false a symbolic link is identified by its own
/// entry. `None` when the path cannot be opened or inspected.
#[cfg(unix)]
pub fn path_identity(path: &Path, follow: bool) -> Option<(u64, u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = if follow {
        std::fs::metadata(path)
    } else {
        std::fs::symlink_metadata(path)
    }
    .ok()?;
    Some((meta.dev(), meta.ino(), meta.nlink()))
}

/// See the Unix variant. The identity comes from `GetFileInformationByHandle`, so the
/// file is opened briefly (with `FILE_FLAG_OPEN_REPARSE_POINT` when not following links).
#[cfg(windows)]
pub fn path_identity(path: &Path, follow: bool) -> Option<(u64, u64, u64)> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
    let mut flags = FILE_FLAG_BACKUP_SEMANTICS;
    if !follow {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT;
    }
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the call; the handle is
    // checked before use and closed exactly once.
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            flags,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        let ok = GetFileInformationByHandle(handle, &mut info);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Some((
            u64::from(info.dwVolumeSerialNumber),
            index,
            u64::from(info.nNumberOfLinks),
        ))
    }
}

/// Identity on platforms without one: `None`, so callers fall back to canonical paths.
#[cfg(not(any(unix, windows)))]
pub fn path_identity(_path: &Path, _follow: bool) -> Option<(u64, u64, u64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("agentcmd-meta-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn directory_and_file_modes_on_disk() {
        let dir = temp("modes");
        let file = dir.join("plain.txt");
        std::fs::write(&file, b"x").unwrap();
        let dir_meta = std::fs::symlink_metadata(&dir).unwrap();
        let file_meta = std::fs::symlink_metadata(&file).unwrap();
        assert!(mode_string(&dir, &dir_meta).starts_with('d'));
        assert_eq!(kind_char(&file_meta), 'f');
        assert_eq!(mode_string(&file, &file_meta).len(), 10);
        assert!(mode_string(&file, &file_meta).starts_with('-'));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn mode_string_formats_permission_triads() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("triads");
        let file = dir.join("f");
        std::fs::write(&file, b"x").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o754)).unwrap();
        let meta = std::fs::symlink_metadata(&file).unwrap();
        assert_eq!(mode_string(&file, &meta), "-rwxr-xr--");
        assert_eq!(permission_bits(&file, &meta), 0o754);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(not(unix))]
    #[test]
    fn mode_string_formats_synthesised_triads() {
        let dir = temp("triads");
        let file = dir.join("f");
        std::fs::write(&file, b"x").unwrap();
        let meta = std::fs::symlink_metadata(&file).unwrap();
        assert_eq!(mode_string(&file, &meta), "-rw-r--r--");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn owner_and_links_are_reported() {
        let dir = temp("owner");
        let file = dir.join("f");
        std::fs::write(&file, b"x").unwrap();
        let meta = std::fs::symlink_metadata(&file).unwrap();
        let (_, links) = inode_nlink(&meta);
        assert!(links >= 1);
        if cfg!(windows) {
            assert_eq!(uid_gid(&meta), (0, 0));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn windows_style_extension_rule() {
        assert!(has_executable_extension(Path::new("tool.EXE")));
        assert!(has_executable_extension(Path::new("run.ps1")));
        assert!(has_executable_extension(Path::new("a.Bat")));
        assert!(!has_executable_extension(Path::new("notes.txt")));
        assert!(!has_executable_extension(Path::new("noext")));
    }

    #[cfg(not(unix))]
    #[test]
    fn windows_synthesised_bits_follow_the_rules() {
        let dir = temp("winbits");
        let file = dir.join("plain.txt");
        std::fs::write(&file, b"x").unwrap();
        let meta = std::fs::symlink_metadata(&file).unwrap();
        assert_eq!(permission_bits(&file, &meta), 0o644);
        let exe = dir.join("tool.exe");
        std::fs::write(&exe, b"x").unwrap();
        let exe_meta = std::fs::symlink_metadata(&exe).unwrap();
        assert_eq!(permission_bits(&exe, &exe_meta), 0o755);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
