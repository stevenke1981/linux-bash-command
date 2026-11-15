use std::fs::{self, Metadata};
use std::path::Path;

pub fn mode_string(path: &Path, metadata: &fs::Metadata) -> String {
    let mut text = String::from(type_char(metadata));
    text.push_str(&permission_triads(path, metadata));
    text
}

fn type_char(metadata: &fs::Metadata) -> char {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return 'l';
    }
    if file_type.is_dir() {
        return 'd';
    }
    special_type_char(metadata).unwrap_or('-')
}

#[cfg(unix)]
fn special_type_char(metadata: &fs::Metadata) -> Option<char> {
    use std::os::unix::fs::FileTypeExt;
    let file_type = metadata.file_type();
    if file_type.is_block_device() {
        Some('b')
    } else if file_type.is_char_device() {
        Some('c')
    } else if file_type.is_fifo() {
        Some('p')
    } else if file_type.is_socket() {
        Some('s')
    } else {
        None
    }
}

#[cfg(not(unix))]
fn special_type_char(_metadata: &fs::Metadata) -> Option<char> {
    None
}

fn permission_triads(path: &Path, metadata: &fs::Metadata) -> String {
    let mode = permission_bits_at(path, metadata);
    let mut text = String::with_capacity(9);
    for (shift, special, upper, lower) in [
        (6, 0o4000, 'S', 's'),
        (3, 0o2000, 'S', 's'),
        (0, 0o1000, 'T', 't'),
    ] {
        let bits = (mode >> shift) & 0o7;
        text.push(if bits & 4 != 0 { 'r' } else { '-' });
        text.push(if bits & 2 != 0 { 'w' } else { '-' });
        text.push(match (mode & special != 0, bits & 1 != 0) {
            (true, true) => lower,
            (true, false) => upper,
            (false, true) => 'x',
            (false, false) => '-',
        });
    }
    text
}

#[cfg(unix)]
pub fn is_executable(_path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
pub fn is_executable(path: &Path, metadata: &fs::Metadata) -> bool {
    const EXECUTABLE_EXTENSIONS: [&str; 5] = ["exe", "bat", "cmd", "com", "ps1"];
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    metadata.is_file()
        && EXECUTABLE_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
}

/// The single-letter type used by `-type` and `%y`.
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

/// Path-aware Unix mode bits or the documented Windows approximation.
pub fn permission_bits_at(path: &Path, meta: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = path;
        meta.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let mut mode = if meta.is_dir() || is_executable(path, meta) {
            0o755
        } else {
            0o644
        };
        if meta.permissions().readonly() {
            mode &= !0o222;
        }
        mode
    }
}

/// Compatibility for callers without a name; executable extensions require `_at`.
pub fn permission_bits(meta: &Metadata) -> u32 {
    permission_bits_at(Path::new(""), meta)
}

pub fn find_mode_string(path: &Path, meta: &Metadata) -> String {
    mode_string(path, meta)
}

pub fn allocated_bytes(meta: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        meta.len().div_ceil(4096).saturating_mul(4096)
    }
}

pub fn owner_ids(meta: &Metadata) -> (u32, u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.uid(), meta.gid())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (0, 0)
    }
}

mod identity;
mod owners;
pub use identity::{change_time, file_identity, file_key, same_file, FileId, FileKey};
pub use owners::{group_id, group_name, user_id, user_name};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regular_file_and_directory_types_and_modes() {
        let root = std::env::temp_dir().join(format!("agentcmd-meta-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("sample.txt");
        fs::write(&file, b"test").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        assert_eq!(kind_char(&metadata), 'f');
        assert_eq!(mode_string(&file, &metadata).len(), 10);
        assert_eq!(find_mode_string(&file, &metadata).len(), 10);
        assert_eq!(kind_char(&fs::metadata(&root).unwrap()), 'd');
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn windows_modes_follow_synthesized_permissions() {
        let root =
            std::env::temp_dir().join(format!("agentcmd-meta-windows-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let plain = root.join("file.txt");
        let executable = root.join("tool.EXE");
        fs::write(&plain, b"").unwrap();
        fs::write(&executable, b"").unwrap();
        let metadata = fs::metadata(&plain).unwrap();
        assert_eq!(mode_string(&plain, &metadata), "-rw-r--r--");
        assert_eq!(permission_bits(&metadata), 0o644);
        let metadata = fs::metadata(&executable).unwrap();
        assert_eq!(mode_string(&executable, &metadata), "-rwxr-xr-x");
        assert_eq!(permission_bits_at(&executable, &metadata), 0o755);
        let metadata = fs::metadata(&root).unwrap();
        assert_eq!(mode_string(&root, &metadata), "drwxr-xr-x");
        assert_eq!(permission_bits(&metadata), 0o755);
        let original_permissions = fs::metadata(&plain).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        fs::set_permissions(&plain, permissions).unwrap();
        let metadata = fs::metadata(&plain).unwrap();
        assert_eq!(mode_string(&plain, &metadata), "-r--r--r--");
        assert_eq!(permission_bits(&metadata), 0o444);
        fs::set_permissions(&plain, original_permissions).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
