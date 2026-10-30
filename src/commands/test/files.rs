//! File predicates for `test`. A missing file makes every predicate false. Symbolic links
//! are followed except for `-h` and `-L`.
//!
//! Windows has no owner, group, setuid, setgid, sticky or device-file bits, so those
//! predicates are false there. `-O` and `-G` are true for any existing file, and `-r -w -x`
//! use the permission bits synthesised by `common::meta`.

use std::fs::{self, Metadata};
use std::time::SystemTime;

/// Evaluates a unary file predicate such as `-f` or `-r`.
pub fn file_test(op: &str, path: &str) -> bool {
    if op == "-h" || op == "-L" {
        return fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    }
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    match op {
        "-e" => true,
        "-f" => meta.is_file(),
        "-d" => meta.is_dir(),
        "-s" => meta.len() > 0,
        "-r" | "-w" | "-x" => access(path, &meta, op),
        "-O" => owned_by_user(&meta),
        "-G" => owned_by_group(&meta),
        "-N" => modified_after_access(&meta),
        _ => special(op, &meta),
    }
}

/// `-nt`: FILE1 is newer than FILE2. A FILE2 that does not exist counts as older.
pub fn newer(left: &str, right: &str) -> bool {
    compare_mtime(left, right, |a, b| a > b)
}

/// `-ot`: FILE1 is older than FILE2. A FILE2 that does not exist counts as older.
pub fn older(left: &str, right: &str) -> bool {
    compare_mtime(left, right, |a, b| a < b)
}

fn compare_mtime(left: &str, right: &str, cmp: fn(SystemTime, SystemTime) -> bool) -> bool {
    let Some(a) = mtime(left) else {
        return false;
    };
    match mtime(right) {
        Some(b) => cmp(a, b),
        None => true,
    }
}

fn mtime(path: &str) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// `-ef`: both names refer to the same file.
#[cfg(unix)]
pub fn same(left: &str, right: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// `-ef`: both names resolve to the same canonical path.
#[cfg(not(unix))]
pub fn same(left: &str, right: &str) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `-t FD`: the descriptor is a terminal.
#[cfg(unix)]
pub fn is_terminal(fd: i32) -> bool {
    // SAFETY: isatty only inspects the descriptor number and never dereferences memory.
    unsafe { libc::isatty(fd) == 1 }
}

/// `-t FD`: only the standard descriptors can be checked.
#[cfg(not(unix))]
pub fn is_terminal(fd: i32) -> bool {
    use std::io::IsTerminal;
    match fd {
        0 => std::io::stdin().is_terminal(),
        1 => std::io::stdout().is_terminal(),
        2 => std::io::stderr().is_terminal(),
        _ => false,
    }
}

#[cfg(unix)]
fn access(path: &str, _meta: &Metadata, op: &str) -> bool {
    let mode = match op {
        "-r" => libc::R_OK,
        "-w" => libc::W_OK,
        _ => libc::X_OK,
    };
    let Ok(cpath) = std::ffi::CString::new(path) else {
        return false;
    };
    // SAFETY: cpath is a valid NUL-terminated string that outlives the call.
    unsafe { libc::access(cpath.as_ptr(), mode) == 0 }
}

#[cfg(not(unix))]
fn access(path: &str, meta: &Metadata, op: &str) -> bool {
    use std::path::Path;
    let bits = crate::common::meta::permission_bits(Path::new(path), meta);
    let mask = match op {
        "-r" => 0o444,
        "-w" => 0o222,
        _ => 0o111,
    };
    bits & mask != 0
}

#[cfg(unix)]
fn owned_by_user(meta: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and cannot fail.
    meta.uid() == unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn owned_by_user(_meta: &Metadata) -> bool {
    true
}

#[cfg(unix)]
fn owned_by_group(meta: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getegid has no preconditions and cannot fail.
    meta.gid() == unsafe { libc::getegid() }
}

#[cfg(not(unix))]
fn owned_by_group(_meta: &Metadata) -> bool {
    true
}

#[cfg(unix)]
fn special(op: &str, meta: &Metadata) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let kind = meta.file_type();
    match op {
        "-g" => meta.mode() & 0o2000 != 0,
        "-u" => meta.mode() & 0o4000 != 0,
        "-k" => meta.mode() & 0o1000 != 0,
        "-b" => kind.is_block_device(),
        "-c" => kind.is_char_device(),
        "-p" => kind.is_fifo(),
        "-S" => kind.is_socket(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn special(_op: &str, _meta: &Metadata) -> bool {
    false
}

/// `-N`: modified since last read.
fn modified_after_access(meta: &Metadata) -> bool {
    match (meta.modified(), meta.accessed()) {
        (Ok(modified), Ok(accessed)) => modified > accessed,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("agentcmd-test-files-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn path(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn existence_type_and_size() {
        let dir = Scratch::new("type");
        fs::write(dir.path("file.txt"), b"data").unwrap();
        fs::write(dir.path("empty"), b"").unwrap();
        fs::create_dir(dir.path("sub")).unwrap();
        assert!(file_test("-e", &dir.path("file.txt")));
        assert!(file_test("-f", &dir.path("file.txt")));
        assert!(!file_test("-d", &dir.path("file.txt")));
        assert!(file_test("-d", &dir.path("sub")));
        assert!(file_test("-s", &dir.path("file.txt")));
        assert!(!file_test("-s", &dir.path("empty")));
        assert!(!file_test("-e", &dir.path("missing")));
    }

    #[test]
    fn readable_and_writable_files() {
        let dir = Scratch::new("perm");
        fs::write(dir.path("file.txt"), b"x").unwrap();
        assert!(file_test("-r", &dir.path("file.txt")));
        assert!(file_test("-w", &dir.path("file.txt")));
        assert!(!file_test("-r", &dir.path("missing")));
    }

    #[test]
    fn newer_and_older_compare_timestamps() {
        let dir = Scratch::new("time");
        fs::write(dir.path("old"), b"1").unwrap();
        let old_time = SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(dir.path("old"))
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        fs::write(dir.path("new"), b"2").unwrap();
        assert!(newer(&dir.path("new"), &dir.path("old")));
        assert!(older(&dir.path("old"), &dir.path("new")));
        assert!(newer(&dir.path("new"), &dir.path("missing")));
        assert!(!newer(&dir.path("missing"), &dir.path("old")));
    }

    #[test]
    fn same_file_detects_identity() {
        let dir = Scratch::new("same");
        fs::write(dir.path("a"), b"1").unwrap();
        fs::write(dir.path("b"), b"1").unwrap();
        assert!(same(&dir.path("a"), &dir.path("a")));
        assert!(!same(&dir.path("a"), &dir.path("b")));
        assert!(!same(&dir.path("a"), &dir.path("missing")));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_tests_do_not_follow() {
        let dir = Scratch::new("link");
        fs::write(dir.path("target"), b"1").unwrap();
        std::os::unix::fs::symlink(dir.path("target"), dir.path("link")).unwrap();
        assert!(file_test("-h", &dir.path("link")));
        assert!(file_test("-L", &dir.path("link")));
        assert!(!file_test("-h", &dir.path("target")));
        assert!(file_test("-f", &dir.path("link")));
    }

    #[test]
    fn unknown_operator_is_false() {
        assert!(!file_test("-Q", "/"));
    }
}
