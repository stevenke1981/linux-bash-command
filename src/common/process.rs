//! External command helpers: PATH/PATHEXT lookup and exit status mapping for `env`,
//! `xargs`, `timeout`, `nice` and `find -exec`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

/// Finds the program `name` the way a shell would for a non-builtin command.
///
/// A name containing a path separator is returned unchanged (the caller reports the
/// failure when spawning it). Otherwise each `PATH` directory is searched in order. On
/// Windows the `PATHEXT` extensions are tried and the current directory is never searched.
pub fn resolve_command(name: &str) -> Option<PathBuf> {
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
        return Some(PathBuf::from(name));
    }
    let path_var = std::env::var_os("PATH");
    let pathext = std::env::var("PATHEXT").ok();
    resolve_in(name, path_var.as_deref(), pathext.as_deref())
}

/// Pure form of [`resolve_command`] for a bare command name.
pub fn resolve_in(name: &str, path_var: Option<&OsStr>, pathext: Option<&str>) -> Option<PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            // POSIX: an empty PATH entry means the current directory. Windows never searches it.
            if cfg!(windows) {
                continue;
            }
        }
        for candidate in candidates(&dir, name, pathext) {
            if is_runnable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn candidates(dir: &Path, name: &str, pathext: Option<&str>) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return vec![dir.join(name)];
    }
    let extensions = windows_extensions(pathext);
    let has_known_extension = Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| {
            extensions
                .iter()
                .any(|known| known.trim_start_matches('.').eq_ignore_ascii_case(ext))
        });
    let mut out = Vec::new();
    if has_known_extension {
        out.push(dir.join(name));
    }
    out.extend(
        extensions
            .iter()
            .map(|ext| dir.join(format!("{name}{ext}"))),
    );
    out
}

fn windows_extensions(pathext: Option<&str>) -> Vec<String> {
    let parsed: Vec<String> = pathext
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(str::to_owned)
        .collect();
    if parsed.is_empty() {
        vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()]
    } else {
        parsed
    }
}

fn is_runnable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    executable_bit(&metadata)
}

#[cfg(unix)]
fn executable_bit(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable_bit(_metadata: &std::fs::Metadata) -> bool {
    true
}

/// Exit status for a program that could not be started: 127 when it does not exist,
/// 126 for any other failure (such as a permission error).
pub fn spawn_error_status(error: &std::io::Error) -> i32 {
    if error.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

/// Converts a finished child's status to an exit code. A child killed by a signal on
/// Unix yields `128 + signal`, as shells report it.
pub fn exit_status_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    signal_code(&status).unwrap_or(1)
}

#[cfg(unix)]
fn signal_code(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| 128 + signal)
}

#[cfg(not(unix))]
fn signal_code(_status: &ExitStatus) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agentcmd-process-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_with_separators_are_returned_as_is() {
        assert_eq!(resolve_command("./tool"), Some(PathBuf::from("./tool")));
        assert_eq!(
            resolve_command("/no/such/dir/tool"),
            Some(PathBuf::from("/no/such/dir/tool"))
        );
    }

    #[test]
    fn missing_program_and_missing_path_resolve_to_none() {
        assert_eq!(resolve_in("agentcmd-absent-xyz", None, None), None);
        let dir = temp_dir("missing");
        let path = std::env::join_paths([dir.as_path()]).unwrap();
        assert_eq!(resolve_in("agentcmd-absent-xyz", Some(&path), None), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finds_an_executable_in_path_order() {
        let first = temp_dir("first");
        let second = temp_dir("second");
        let tool = if cfg!(windows) { "tool.EXE" } else { "tool" };
        for dir in [&first, &second] {
            let file = dir.join(tool);
            std::fs::write(&file, b"x").unwrap();
            make_executable(&file);
        }
        let path = std::env::join_paths([first.as_path(), second.as_path()]).unwrap();
        let found = resolve_in("tool", Some(&path), Some(".EXE")).unwrap();
        assert_eq!(found.parent(), Some(first.as_path()));
        std::fs::remove_dir_all(&first).unwrap();
        std::fs::remove_dir_all(&second).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn skips_files_without_execute_permission() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("noexec");
        let tool = dir.join("plain");
        std::fs::write(&tool, b"x").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
        let path = std::env::join_paths([dir.as_path()]).unwrap();
        assert_eq!(resolve_in("plain", Some(&path), None), None);
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolve_in("plain", Some(&path), None), Some(tool));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn windows_extension_rules() {
        assert_eq!(
            windows_extensions(Some(".EXE;;.cmd ")),
            vec![".EXE".to_owned(), ".cmd".to_owned()]
        );
        assert_eq!(windows_extensions(None).len(), 4);
        assert_eq!(windows_extensions(Some(" ; ")).len(), 4);
    }

    #[test]
    fn windows_candidates_try_extensions() {
        let dir = Path::new("dir");
        let names = candidates(dir, "tool", Some(".EXE;.BAT"));
        if cfg!(windows) {
            assert_eq!(names, vec![dir.join("tool.EXE"), dir.join("tool.BAT")]);
        } else {
            assert_eq!(names, vec![dir.join("tool")]);
        }
        let known = candidates(dir, "tool.bat", Some(".EXE;.BAT"));
        if cfg!(windows) {
            assert_eq!(known[0], dir.join("tool.bat"));
        }
    }

    #[test]
    fn spawn_errors_map_to_shell_statuses() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(spawn_error_status(&missing), 127);
        assert_eq!(spawn_error_status(&denied), 126);
    }

    #[test]
    fn normal_exit_codes_pass_through() {
        let status = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .args(if cfg!(windows) {
                vec!["/C", "exit 3"]
            } else {
                vec!["-c", "exit 3"]
            })
            .status()
            .unwrap();
        assert_eq!(exit_status_code(status), 3);
    }

    #[cfg(unix)]
    #[test]
    fn signal_death_maps_to_128_plus_signal() {
        let status = std::process::Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .unwrap();
        assert_eq!(exit_status_code(status), 128 + 15);
    }

    #[cfg(unix)]
    fn make_executable(file: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_file: &Path) {}
}
