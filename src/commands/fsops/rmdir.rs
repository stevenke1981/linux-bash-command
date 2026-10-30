//! `rmdir`: remove empty directories, optionally with their empty parents (`-p`).

use std::fs;
use std::io;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

use super::helpers::{os_text, print_help, say, strip_trailing_seps};

const HELP: &str = r#"Usage: rmdir [OPTION]... DIRECTORY...
Remove the DIRECTORY(ies), if they are empty.

      --ignore-fail-on-non-empty
                    ignore each failure that is solely because a directory
                      is non-empty
  -p, --parents     remove DIRECTORY and its ancestors; e.g.,
                      'rmdir -p a/b/c' is similar to rmdir a/b/c a/b a
  -v, --verbose     output a diagnostic for every directory processed
      --help        display this help and exit
      --version     output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("parents", Some('p'), Some("parents")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("ignore", None, Some("ignore-fail-on-non-empty")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

#[derive(Default)]
struct Options {
    parents: bool,
    verbose: bool,
    ignore_non_empty: bool,
}

/// The directory to try after `path` when `-p` is given, or `None` at the top.
fn parent_of(path: &str) -> Option<&str> {
    let trimmed = strip_trailing_seps(path);
    let parent = Path::new(trimmed).parent()?.to_str()?;
    if parent.is_empty() || parent == "." || parent == "/" || parent == "\\" {
        None
    } else {
        Some(parent)
    }
}

/// Removes one directory. Returns `(status, keep_going)`.
fn remove_one(path: &str, opts: &Options) -> (i32, bool) {
    if opts.verbose {
        say(&format!("rmdir: removing directory, '{path}'"));
    }
    match fs::remove_dir(path) {
        Ok(()) => (EXIT_OK, true),
        Err(error) => {
            if opts.ignore_non_empty && is_not_empty(&error) {
                return (EXIT_OK, false);
            }
            eprintln!("rmdir: failed to remove '{path}': {}", os_text(&error));
            (EXIT_FAIL, false)
        }
    }
}

fn is_not_empty(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::DirectoryNotEmpty
        || (cfg!(windows) && error.raw_os_error() == Some(145))
}

/// Removes one operand, and with `-p` its ancestors while they are empty.
fn remove_operand(operand: &str, opts: &Options) -> i32 {
    let (mut status, mut keep_going) = remove_one(strip_trailing_seps(operand), opts);
    if !opts.parents {
        return status;
    }
    let mut current = strip_trailing_seps(operand);
    while keep_going {
        let Some(parent) = parent_of(current) else {
            break;
        };
        let (next_status, next_going) = remove_one(parent, opts);
        status = status.max(next_status);
        keep_going = next_going;
        current = parent;
    }
    status
}

/// Entry point for `rmdir`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("rmdir", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("rmdir", &error, EXIT_FAIL),
    };
    let mut opts = Options::default();
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, .. } => match id {
                "help" => return print_help("rmdir", HELP),
                "version" => return print_version("rmdir"),
                "parents" => opts.parents = true,
                "verbose" => opts.verbose = true,
                "ignore" => opts.ignore_non_empty = true,
                _ => {}
            },
        }
    }
    if operands.is_empty() {
        return report_usage(
            "rmdir",
            &UsageError {
                message: "missing operand".to_string(),
            },
            EXIT_FAIL,
        );
    }
    let mut status = EXIT_OK;
    for operand in &operands {
        status = status.max(remove_operand(operand, &opts));
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents_stop_at_relative_and_absolute_roots() {
        assert_eq!(parent_of("a/b/c"), Some("a/b"));
        assert_eq!(parent_of("a/b/"), Some("a"));
        assert_eq!(parent_of("a"), None);
        assert_eq!(parent_of("/x"), None);
        assert_eq!(parent_of("./a"), None);
    }

    #[test]
    fn non_empty_detection_uses_kind() {
        assert!(is_not_empty(&io::Error::from(
            io::ErrorKind::DirectoryNotEmpty
        )));
        assert!(!is_not_empty(&io::Error::from(io::ErrorKind::NotFound)));
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    #[test]
    fn removes_an_empty_directory() {
        let t = TempDir::new("rmdir-empty");
        fs::create_dir(t.at("d")).unwrap();
        let path = t.at("d");
        assert_eq!(
            remove_one(path.to_str().unwrap(), &Options::default()),
            (EXIT_OK, true)
        );
        assert!(!path.exists());
    }

    #[test]
    fn non_empty_directory_fails_and_stays() {
        let t = TempDir::new("rmdir-full");
        t.write("d/f", "x");
        let path = t.at("d");
        let (status, keep_going) = remove_one(path.to_str().unwrap(), &Options::default());
        assert_eq!((status, keep_going), (EXIT_FAIL, false));
        assert!(path.is_dir());
    }

    #[test]
    fn ignore_flag_turns_non_empty_into_success() {
        let t = TempDir::new("rmdir-ignore");
        t.write("d/f", "x");
        let opts = Options {
            ignore_non_empty: true,
            ..Options::default()
        };
        let path = t.at("d");
        assert_eq!(remove_one(path.to_str().unwrap(), &opts), (EXIT_OK, false));
    }

    #[test]
    fn parents_walk_up_while_directories_empty() {
        let t = TempDir::new("rmdir-parents");
        fs::create_dir_all(t.at("a/b/c")).unwrap();
        // A sentinel keeps the walk from climbing above the scratch directory.
        t.write("keep", "k");
        let opts = Options {
            parents: true,
            ..Options::default()
        };
        let leaf = t.at("a/b/c");
        assert_eq!(remove_operand(leaf.to_str().unwrap(), &opts), EXIT_FAIL);
        assert!(!t.at("a").exists());
        assert!(t.at("keep").exists());
    }
}
