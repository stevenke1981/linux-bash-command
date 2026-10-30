//! `which`: locates programs on `PATH` (debianutils subset: `-a`, `-s`).
//!
//! A name containing a slash is checked where it points, relative to the current directory,
//! and printed as given. Any other name is joined to each `PATH` entry as `entry/name`,
//! without normalising (an empty entry means `.`), as debianutils does. Windows also tries
//! each `PATHEXT` extension, and never searches the current directory.

use std::io::Write;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::path::display_path;

const HELP: &str = r#"Usage: which [-as] PROGRAM...
Locate each PROGRAM in the directories listed in PATH and print its path.

  -a       print every match in PATH, not only the first
  -s       print nothing; the exit status reports whether all were found
      --help     display this help and exit
      --version  output version information and exit

Exit status: 0 if all PROGRAMs were found, 1 if any was not, 2 for usage errors.
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("all", Some('a'), None),
    OptSpec::flag("silent", Some('s'), None),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

#[derive(Clone, Copy)]
struct Mode {
    all: bool,
    silent: bool,
}

fn print_help() -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(HELP.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("which: write error: {error}");
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output("which", &mut out)
}

/// Extensions to try after a bare name. Unix never adds one.
fn extensions() -> Vec<String> {
    if !cfg!(windows) {
        return vec![String::new()];
    }
    let mut list: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(str::to_owned)
        .collect();
    if list.is_empty() {
        list = vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()];
    }
    list
}

/// Candidate file names for `name`. On Windows a name that already has a known extension
/// is tried as written first.
fn candidates(name: &str) -> Vec<String> {
    let exts = extensions();
    let mut out = Vec::new();
    if cfg!(windows) {
        let has_known = Path::new(name)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                exts.iter()
                    .any(|known| known.trim_start_matches('.').eq_ignore_ascii_case(ext))
            });
        if has_known {
            out.push(name.to_owned());
        }
    }
    out.extend(exts.iter().map(|ext| format!("{name}{ext}")));
    out
}

/// True when `path` is a file that can be run.
fn is_runnable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    executable(&metadata)
}

#[cfg(unix)]
fn executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &std::fs::Metadata) -> bool {
    true
}

/// Every place `name` can be found, in search order, as printed strings.
fn locate(name: &str, path_var: Option<&std::ffi::OsStr>) -> Vec<String> {
    let mut found = Vec::new();
    if name.is_empty() {
        return found;
    }
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
        // A name with a separator is checked where it points, never looked up in PATH.
        if let Some(path) = candidates(name)
            .into_iter()
            .find(|c| is_runnable(Path::new(c)))
        {
            found.push(display_path(Path::new(&path)));
        }
        return found;
    }
    let Some(path_var) = path_var else {
        return found;
    };
    for dir in std::env::split_paths(path_var) {
        let dir = dir.to_string_lossy();
        let dir = if dir.is_empty() { "." } else { dir.as_ref() };
        for candidate in candidates(name) {
            let joined = format!("{dir}/{candidate}");
            if is_runnable(Path::new(&joined)) {
                found.push(display_path(Path::new(&joined)));
                break;
            }
        }
    }
    found
}

/// Searches every operand. Returns the status: 0 when all were found, 1 otherwise.
fn search(names: &[String], mode: Mode) -> i32 {
    let path_var = std::env::var_os("PATH");
    let mut text = String::new();
    let mut status = EXIT_OK;
    for name in names {
        let matches = locate(name, path_var.as_deref());
        if matches.is_empty() {
            status = EXIT_FAIL;
            continue;
        }
        if mode.silent {
            continue;
        }
        let shown: &[String] = if mode.all { &matches } else { &matches[..1] };
        for path in shown {
            text.push_str(path);
            text.push('\n');
        }
    }
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("which: write error: {error}");
            return EXIT_FAIL;
        }
    }
    let flushed = finish_output("which", &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    status
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("which", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("which", &error, EXIT_TROUBLE),
    };
    let mut mode = Mode {
        all: false,
        silent: false,
    };
    let mut names = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return print_help(),
            Item::Opt { id: "version", .. } => return print_version("which"),
            Item::Opt { id: "all", .. } => mode.all = true,
            Item::Opt { id: "silent", .. } => mode.silent = true,
            Item::Opt { .. } => {}
            Item::Operand(text) => names.push(text),
        }
    }
    if names.is_empty() {
        return EXIT_FAIL;
    }
    search(&names, mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_add_path_extensions_only_on_windows() {
        let list = candidates("tool");
        if cfg!(windows) {
            assert!(list.iter().any(|c| c.eq_ignore_ascii_case("tool.exe")));
        } else {
            assert_eq!(list, vec!["tool".to_string()]);
        }
    }

    #[test]
    fn extensions_default_to_windows_list_when_unset() {
        if cfg!(windows) {
            assert!(extensions().len() >= 4);
        } else {
            assert_eq!(extensions(), vec![String::new()]);
        }
    }

    #[test]
    fn empty_and_missing_names_are_not_found() {
        assert!(locate("", None).is_empty());
        assert!(locate("agentcmd-no-such-program-xyz", None).is_empty());
    }

    #[test]
    fn finds_sh_on_unix_and_reports_only_the_first_match() {
        if cfg!(unix) {
            let path = std::env::var_os("PATH").unwrap_or_default();
            let found = locate("sh", Some(&path));
            assert!(!found.is_empty());
            assert!(found[0].ends_with("/sh"));
        }
    }

    #[test]
    fn absolute_names_are_checked_directly() {
        let exe = std::env::current_exe().unwrap();
        let found = locate(&exe.to_string_lossy(), None);
        assert_eq!(found.len(), 1);
        assert!(locate("/no/such/dir/tool-xyz", None).is_empty());
    }

    #[test]
    fn help_lists_both_options() {
        assert!(HELP.contains("-a"));
        assert!(HELP.contains("-s"));
    }
}
