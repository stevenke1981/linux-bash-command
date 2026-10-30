//! File-system helpers for gzip: suffix rules, output creation, metadata copy and the
//! overwrite prompt.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use filetime::FileTime;

/// Returns the name with `suffix` removed, or `None` when `path` does not end with it.
/// With the default `.gz` suffix, `.tgz` maps to `.tar` as GNU gzip does.
pub fn strip_suffix(path: &str, suffix: &str) -> Option<String> {
    if !suffix.is_empty() && path.len() > suffix.len() && path.ends_with(suffix) {
        return Some(path[..path.len() - suffix.len()].to_string());
    }
    if suffix == ".gz" && path.len() > 4 && path.ends_with(".tgz") {
        return Some(format!("{}.tar", &path[..path.len() - 4]));
    }
    None
}

/// True when `path` already ends with the compression suffix.
pub fn has_suffix(path: &str, suffix: &str) -> bool {
    !suffix.is_empty() && path.len() > suffix.len() && path.ends_with(suffix)
}

/// The final component of `path`, as bytes, for the name stored in a header.
pub fn base_name_bytes(path: &str) -> Vec<u8> {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.into_bytes()
}

/// Modification time in whole seconds, clamped to the 32-bit range gzip headers use.
pub fn mtime_secs(meta: &Metadata) -> u32 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

/// Creates the output file. Without `force` an existing file is never truncated.
pub fn create_output(path: &str, force: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    options.open(path)
}

/// Gives `dst` the permissions and modification time of the input. `mtime` overrides the
/// input's time when a decompressed header carried one.
pub fn copy_metadata(src: &Metadata, dst: &str, mtime: Option<u32>) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(dst)?;
    file.set_permissions(src.permissions())?;
    drop(file);
    let time = match mtime {
        Some(secs) if secs != 0 => FileTime::from_unix_time(i64::from(secs), 0),
        _ => FileTime::from_last_modification_time(src),
    };
    filetime::set_file_mtime(dst, time)
}

/// Removes the input after a successful run. On Windows a read-only attribute is cleared
/// first, so the behaviour matches Unix.
pub fn remove_input(path: &str) -> io::Result<()> {
    if cfg!(windows) {
        if let Ok(meta) = fs::metadata(path) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                fs::set_permissions(path, perms)?;
            }
        }
    }
    fs::remove_file(path)
}

/// Asks whether to overwrite `path`. Only used when standard input is a terminal.
pub fn confirm_overwrite(path: &str) -> bool {
    eprint!("gzip: {path} already exists; do you wish to overwrite (y or n)? ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    match io::stdin().lock().read_line(&mut answer) {
        Ok(_) => matches!(answer.trim_start().chars().next(), Some('y' | 'Y')),
        Err(_) => false,
    }
}

/// True when the file named `path` exists, whether or not it is a link.
pub fn exists(path: &str) -> bool {
    fs::symlink_metadata(path).is_ok()
}

pub fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

pub fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_stripping_follows_gnu_rules() {
        assert_eq!(strip_suffix("a.txt.gz", ".gz").as_deref(), Some("a.txt"));
        assert_eq!(strip_suffix("a.tgz", ".gz").as_deref(), Some("a.tar"));
        assert_eq!(strip_suffix("a.zz", ".zz").as_deref(), Some("a"));
        assert_eq!(strip_suffix("a.txt", ".gz"), None);
        assert_eq!(strip_suffix(".gz", ".gz"), None);
    }

    #[test]
    fn has_suffix_requires_a_nonempty_stem() {
        assert!(has_suffix("x.gz", ".gz"));
        assert!(!has_suffix(".gz", ".gz"));
        assert!(!has_suffix("x.gz", ""));
    }

    #[test]
    fn stored_name_is_the_base_name() {
        assert_eq!(base_name_bytes("dir/sub/file.txt"), b"file.txt");
        assert_eq!(base_name_bytes("file"), b"file");
    }

    #[test]
    fn create_output_refuses_to_overwrite_without_force() {
        let dir = std::env::temp_dir().join(format!("agentcmd-gzip-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.gz");
        let text = path.to_string_lossy().into_owned();
        fs::write(&path, b"keep").unwrap();
        let error = create_output(&text, false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(create_output(&text, true).is_ok());
        assert_eq!(fs::read(&path).unwrap(), b"");
        fs::remove_dir_all(&dir).unwrap();
    }
}
