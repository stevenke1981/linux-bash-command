//! Helpers shared by the file-system commands: OS error wording, interactive prompts,
//! GNU-style argument matching, umask, identity checks and symbolic links.

use std::fs::{self, FileType, Metadata};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::common::error::{io_error_text, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::meta::path_identity;

/// Separators accepted when splitting operands. Windows also treats `\` as one.
pub fn is_sep(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// GNU wording for an OS error. Adds the two codes `io_error_text` does not map.
pub fn os_text(error: &io::Error) -> String {
    #[cfg(unix)]
    {
        match error.raw_os_error() {
            Some(1) => return "Operation not permitted".to_string(), // EPERM
            Some(27) => return "File too large".to_string(),         // EFBIG
            _ => {}
        }
    }
    io_error_text(error)
}

/// Prints `text` to stderr and reads one line from stdin. Only a line starting with `y` or
/// `Y` answers yes; end of input answers no.
pub fn ask(text: &str) -> bool {
    let mut err = io::stderr().lock();
    let _ = write!(err, "{text}");
    let _ = err.flush();
    drop(err);
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(count) if count > 0 => matches!(line.chars().next(), Some('y' | 'Y')),
        _ => false,
    }
}

/// True when standard input is a terminal (GNU `rm` only asks about write-protected files then).
pub fn stdin_is_tty() -> bool {
    io::stdin().is_terminal()
}

/// Writes one line of normal output. A closed pipe is ignored, as the other commands do.
pub fn say(text: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{text}");
}

/// Writes a help text to stdout. A closed pipe is silent success.
pub fn print_help(cmd: &str, text: &str) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{cmd}: write error: {}", os_text(&error));
        return EXIT_FAIL;
    }
    finish_output(cmd, &mut out)
}

/// One entry of an [`argmatch`] table: the accepted spellings and the value they select.
pub type ArgGroup<'a, T> = (&'a [&'a str], T);

/// Matches `text` against `groups` the way GNU `argmatch` does: an exact spelling wins,
/// otherwise a unique prefix. Errors carry the GNU message without the `cmd: ` prefix.
pub fn argmatch<T: Copy>(opt: &str, text: &str, groups: &[ArgGroup<'_, T>]) -> Result<T, String> {
    if let Some((_, value)) = groups.iter().find(|(names, _)| names.contains(&text)) {
        return Ok(*value);
    }
    let hits: Vec<&ArgGroup<'_, T>> = groups
        .iter()
        .filter(|(names, _)| names.iter().any(|name| name.starts_with(text)))
        .collect();
    let valid = valid_list(groups);
    match hits.as_slice() {
        [(_, value)] if !text.is_empty() => Ok(*value),
        [] | [_] => Err(format!("invalid argument '{text}' for '{opt}'\n{valid}")),
        _ => Err(format!("ambiguous argument '{text}' for '{opt}'\n{valid}")),
    }
}

fn valid_list<T>(groups: &[ArgGroup<'_, T>]) -> String {
    let mut out = String::from("Valid arguments are:");
    for (names, _) in groups {
        let quoted: Vec<String> = names.iter().map(|name| format!("'{name}'")).collect();
        out.push_str("\n  - ");
        out.push_str(&quoted.join(", "));
    }
    out
}

/// The process umask. Read by setting it to 0 and restoring it; the CLI is single-threaded.
#[cfg(unix)]
pub fn umask() -> u32 {
    // SAFETY: umask has no memory-safety preconditions; the previous value is restored at once.
    unsafe {
        let old = libc::umask(0o022);
        libc::umask(old);
        old as u32
    }
}

/// Windows has no umask; the conventional default is used for who-less mode clauses.
#[cfg(not(unix))]
pub fn umask() -> u32 {
    0o022
}

/// Final component of an operand, ignoring trailing separators (`a/b/` gives `b`).
/// A bare root (`/`) is returned unchanged.
pub fn base_name(text: &str) -> &str {
    let trimmed = strip_trailing_seps(text);
    trimmed
        .rsplit(is_sep)
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed)
}

/// Removes trailing separators. A string made only of separators becomes one separator.
pub fn strip_trailing_seps(text: &str) -> &str {
    let trimmed = text.trim_end_matches(is_sep);
    if trimmed.is_empty() && !text.is_empty() {
        &text[..1]
    } else {
        trimmed
    }
}

/// `dir/name` where `name` is the last component of `source`.
pub fn join_name(dir: &Path, source: &str) -> PathBuf {
    dir.join(base_name(source))
}

/// True when both paths name the same file, following symbolic links.
pub fn same_file(a: &Path, b: &Path) -> bool {
    if fs::metadata(a).is_err() || fs::metadata(b).is_err() {
        return false;
    }
    match (path_identity(a, true), path_identity(b, true)) {
        (Some(ka), Some(kb)) => (ka.0, ka.1) == (kb.0, kb.1),
        _ => match (fs::canonicalize(a), fs::canonicalize(b)) {
            (Ok(ca), Ok(cb)) => ca == cb,
            _ => false,
        },
    }
}

/// True when both names refer to the same directory entry, without following symbolic
/// links (a link and its target are different entries).
pub fn same_entry(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::symlink_metadata(a), fs::symlink_metadata(b)) else {
        return false;
    };
    match (path_identity(a, false), path_identity(b, false)) {
        (Some(ka), Some(kb)) => (ka.0, ka.1) == (kb.0, kb.1),
        _ => match (fs::canonicalize(a), fs::canonicalize(b)) {
            (Ok(ca), Ok(cb)) => {
                ca == cb && ma.file_type().is_symlink() == mb.file_type().is_symlink()
            }
            _ => false,
        },
    }
}

/// `(device, inode)` of a file on Unix. Windows has no stable handle-free identity.
#[cfg(unix)]
pub fn file_key(meta: &Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
pub fn file_key(_meta: &Metadata) -> Option<(u64, u64)> {
    None
}

/// Device number of a file, used by `--one-file-system`.
#[cfg(unix)]
pub fn device_of(meta: &Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.dev())
}

#[cfg(not(unix))]
pub fn device_of(_meta: &Metadata) -> Option<u64> {
    None
}

/// Whether a symbolic link points at a directory and must be removed with `remove_dir`
/// on Windows. Always false elsewhere.
#[cfg(windows)]
pub fn link_is_dir(kind: FileType) -> bool {
    use std::os::windows::fs::FileTypeExt;
    kind.is_symlink_dir()
}

#[cfg(not(windows))]
pub fn link_is_dir(_kind: FileType) -> bool {
    false
}

/// Removes a symbolic link itself (never its target).
pub fn remove_link(path: &Path, dir_link: bool) -> io::Result<()> {
    if cfg!(windows) && dir_link {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

/// Creates a symbolic link. Windows needs to know whether the target is a directory.
pub fn make_symlink(target: &Path, link: &Path, target_is_dir: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        let _ = target_is_dir;
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        if target_is_dir {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
}

/// Symbolic-link error text. On Windows a privilege failure gets a hint about Developer Mode.
pub fn symlink_error_text(error: &io::Error) -> String {
    let text = os_text(error);
    if cfg!(windows)
        && (error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1314))
    {
        format!("{text} (creating symbolic links on Windows requires Developer Mode or an elevated shell)")
    } else {
        text
    }
}

/// Clears the read-only attribute so the file can be removed. Windows refuses to delete
/// read-only files; Unix does not look at the file's own mode when unlinking.
#[cfg(windows)]
pub fn clear_readonly(path: &Path) {
    if let Ok(meta) = fs::symlink_metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

#[cfg(not(windows))]
pub fn clear_readonly(_path: &Path) {}

/// Splits `text` at its last `=` for `--name=value` style arguments done by hand.
pub fn split_value(text: &str) -> (&str, Option<&str>) {
    match text.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (text, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_name_ignores_trailing_separators() {
        assert_eq!(base_name("a/b/"), "b");
        assert_eq!(base_name("file"), "file");
        assert_eq!(base_name("/"), "/");
        assert_eq!(base_name("dir//"), "dir");
    }

    #[test]
    fn strip_keeps_a_single_root_separator() {
        assert_eq!(strip_trailing_seps("x///"), "x");
        assert_eq!(strip_trailing_seps("///"), "/");
        assert_eq!(strip_trailing_seps(""), "");
    }

    #[test]
    fn argmatch_accepts_exact_and_unique_prefix() {
        let groups: &[ArgGroup<'_, u8>] = &[(&["never", "no", "none"], 0), (&["always", "yes"], 1)];
        assert_eq!(argmatch("--x", "none", groups), Ok(0));
        assert_eq!(argmatch("--x", "alw", groups), Ok(1));
    }

    #[test]
    fn argmatch_reports_invalid_and_ambiguous() {
        let groups: &[ArgGroup<'_, u8>] = &[(&["alpha"], 0), (&["alps"], 1)];
        let invalid = argmatch("--x", "zeta", groups).unwrap_err();
        assert!(invalid.starts_with("invalid argument 'zeta' for '--x'"));
        assert!(invalid.contains("  - 'alpha'"));
        let ambiguous = argmatch("--x", "al", groups).unwrap_err();
        assert!(ambiguous.starts_with("ambiguous argument 'al' for '--x'"));
    }

    #[test]
    fn same_file_detects_hard_links_and_distinct_files() {
        let dir = std::env::temp_dir().join(format!("agentcmd-helpers-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let a = dir.join("a");
        let b = dir.join("b");
        let c = dir.join("c");
        fs::write(&a, b"x").unwrap();
        fs::write(&c, b"y").unwrap();
        let _ = fs::remove_file(&b);
        fs::hard_link(&a, &b).unwrap();
        assert!(same_file(&a, &b));
        assert!(!same_file(&a, &c));
        assert!(!same_file(&a, &dir.join("missing")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn os_text_maps_eperm_and_efbig() {
        #[cfg(unix)]
        {
            assert_eq!(
                os_text(&io::Error::from_raw_os_error(1)),
                "Operation not permitted"
            );
            assert_eq!(os_text(&io::Error::from_raw_os_error(27)), "File too large");
        }
        assert_eq!(
            os_text(&io::Error::from(io::ErrorKind::NotFound)),
            "No such file or directory"
        );
    }

    #[test]
    fn split_value_separates_on_first_equals() {
        assert_eq!(split_value("a=b=c"), ("a", Some("b=c")));
        assert_eq!(split_value("flag"), ("flag", None));
    }
}

/// A uniquely named scratch directory for unit tests. It lives under the system temp
/// directory and is removed on drop; nothing outside it is touched.
#[cfg(test)]
pub mod testdir {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("agentcmd-fsops-{tag}-{}-{id}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create test dir");
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        /// Absolute path of `name` inside the scratch directory.
        pub fn at(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        /// Writes `text` to `name`, creating parent directories inside the scratch area.
        pub fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.at(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create test parent");
            }
            fs::write(&path, text).expect("write test file");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
