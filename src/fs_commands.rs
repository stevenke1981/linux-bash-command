use crate::common::args::prepare_legacy;
use crate::common::error::{io_error_text, EXIT_FAIL};
use crate::common::path::strip_verbatim;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod ls;
mod ls_format;
mod ls_layout;
mod ls_options;
mod ls_quote;
mod ls_sort;
mod ls_time;

pub use ls::ls;

use crate::display_path;

/// Accepts `--color`, `--colour` and their `=WHEN` forms. Output is never colored.
pub(crate) fn is_color_option(arg: &str) -> bool {
    let when = arg
        .strip_prefix("--color")
        .or_else(|| arg.strip_prefix("--colour"));
    match when {
        Some("") => true,
        Some(value) => matches!(
            value,
            "=auto"
                | "=always"
                | "=never"
                | "=yes"
                | "=no"
                | "=tty"
                | "=force"
                | "=none"
                | "=if-tty"
        ),
        None => false,
    }
}

const PWD_LONGS: &[&str] = &["help", "logical", "physical"];

pub fn pwd(args: &[String]) -> i32 {
    let expanded = match prepare_legacy("pwd", args, PWD_LONGS, &[], "", EXIT_FAIL) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let args = expanded.as_slice();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("Usage: pwd [-L | -P]\nPrint the current working directory.\n\nOptions:\n  -L, --logical   Print $PWD when it names the current directory (default)\n  -P, --physical  Print the directory with symbolic links resolved\n  -h, --help      Show this help\n\nExit status: 0 on success, 1 on error.");
        return 0;
    }
    let physical = match parse_pwd_mode(args) {
        Ok(physical) => physical,
        Err(code) => return code,
    };
    let resolved = match env::current_dir() {
        Ok(cwd) if physical => physical_path(&cwd),
        Ok(cwd) => Ok(logical_path(cwd)),
        Err(error) => Err(error),
    };
    match resolved {
        Ok(path) => match writeln!(io::stdout().lock(), "{}", display_path(&path)) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("pwd: cannot write output: {}", io_error_text(&error));
                1
            }
        },
        Err(error) => {
            eprintln!(
                "pwd: cannot determine current directory: {}",
                io_error_text(&error)
            );
            1
        }
    }
}

/// Returns true for `-P` (physical) and false for `-L` (logical); the last one wins.
fn parse_pwd_mode(args: &[String]) -> Result<bool, i32> {
    let mut physical = false;
    let mut options = true;
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if options && arg == "--physical" {
            physical = true;
        } else if options && arg == "--logical" {
            physical = false;
        } else if options && arg.starts_with("--") {
            eprintln!("pwd: unknown option: {arg}");
            return Err(1);
        } else if options && arg.len() > 1 && arg.starts_with('-') {
            for flag in arg[1..].chars() {
                match flag {
                    'P' => physical = true,
                    'L' => physical = false,
                    _ => {
                        eprintln!("pwd: unknown option: -{flag}");
                        return Err(1);
                    }
                }
            }
        } else {
            eprintln!("pwd: this command does not accept arguments");
            return Err(1);
        }
    }
    Ok(physical)
}

fn physical_path(cwd: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(cwd).map(|path| strip_verbatim(&path))
}

/// `$PWD` is used only when it is an absolute path to the current directory.
fn logical_path(cwd: PathBuf) -> PathBuf {
    match env::var_os("PWD").map(PathBuf::from) {
        Some(candidate) if candidate.is_absolute() && same_directory(&candidate, &cwd) => candidate,
        _ => cwd,
    }
}

fn same_directory(left: &Path, right: &Path) -> bool {
    matches!(
        (fs::canonicalize(left), fs::canonicalize(right)),
        (Ok(a), Ok(b)) if a == b
    )
}
