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

#[cfg(unix)]
fn permission_triads(_path: &Path, metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode();
    let mut text = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        text.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        text.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        text.push(if bits & 0o1 != 0 { 'x' } else { '-' });
    }
    text
}

/// Windows has no mode bits: `r` always, `w` unless read-only, `x` for directories and executable extensions.
#[cfg(not(unix))]
fn permission_triads(path: &Path, metadata: &fs::Metadata) -> String {
    let writable = if metadata.permissions().readonly() {
        '-'
    } else {
        'w'
    };
    let executable = if metadata.is_dir() || is_executable(path, metadata) {
        'x'
    } else {
        '-'
    };
    let triad = format!("r{writable}{executable}");
    triad.repeat(3)
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

/// Permission bits. On Unix these are the real mode bits. Windows has no mode bits, so
/// they are approximated: 0444 for read-only entries, 0755 for directories, else 0644.
pub fn permission_bits(meta: &Metadata) -> u32 {
    permission_bits_impl(meta)
}

#[cfg(unix)]
fn permission_bits_impl(meta: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn permission_bits_impl(meta: &Metadata) -> u32 {
    if meta.permissions().readonly() {
        0o444
    } else if meta.is_dir() {
        0o755
    } else {
        0o644
    }
}

pub fn find_mode_string(meta: &Metadata) -> String {
    let kind = match kind_char(meta) {
        'f' => '-',
        other => other,
    };
    let bits = permission_bits(meta);
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
        assert_eq!(find_mode_string(&metadata).len(), 10);
        assert_eq!(kind_char(&fs::metadata(&root).unwrap()), 'd');
        fs::remove_dir_all(root).unwrap();
    }
}
