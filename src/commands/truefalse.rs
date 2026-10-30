//! `true` and `false`: the arguments are ignored. `--help` and `--version` are honoured
//! only when they are the sole argument, as GNU does. `false --help` and
//! `false --version` exit 1.

use std::io::Write;

use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const TRUE_HELP: &str = r#"Usage: true [ignored command line arguments]
  or:  true OPTION
Exit with a status code indicating success.

      --help     display this help and exit
      --version  output version information and exit
"#;

const FALSE_HELP: &str = r#"Usage: false [ignored command line arguments]
  or:  false OPTION
Exit with a status code indicating failure.

      --help     display this help and exit
      --version  output version information and exit
"#;

/// A lone `--help` or `--version`, the only arguments these commands look at.
#[derive(Debug, PartialEq, Eq)]
enum Lone {
    Help,
    Version,
}

fn lone_option(args: &[String]) -> Option<Lone> {
    match args {
        [only] if only == "--help" => Some(Lone::Help),
        [only] if only == "--version" => Some(Lone::Version),
        _ => None,
    }
}

/// Writes `text` to stdout; a closed pipe is silent success.
fn emit(cmd: &str, text: &str) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{cmd}: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    finish_output(cmd, &mut out)
}

pub fn run_true(args: &[String]) -> i32 {
    match lone_option(args) {
        Some(Lone::Help) => emit("true", TRUE_HELP),
        Some(Lone::Version) => print_version("true"),
        None => EXIT_OK,
    }
}

pub fn run_false(args: &[String]) -> i32 {
    // GNU: both `false --help` and `false --version` exit with failure.
    match lone_option(args) {
        Some(Lone::Help) => {
            emit("false", FALSE_HELP);
            EXIT_FAIL
        }
        Some(Lone::Version) => {
            print_version("false");
            EXIT_FAIL
        }
        None => EXIT_FAIL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lone_help_and_version_are_recognised() {
        assert_eq!(lone_option(&args(&["--help"])), Some(Lone::Help));
        assert_eq!(lone_option(&args(&["--version"])), Some(Lone::Version));
    }

    #[test]
    fn help_is_ignored_next_to_other_arguments() {
        assert_eq!(lone_option(&args(&["--help", "x"])), None);
        assert_eq!(lone_option(&args(&["-h"])), None);
        assert_eq!(lone_option(&[]), None);
    }

    #[test]
    fn true_ignores_arguments_and_exits_zero() {
        assert_eq!(run_true(&args(&["a", "--bogus", "-x"])), 0);
        assert_eq!(run_true(&[]), 0);
    }

    #[test]
    fn false_exits_one_even_with_help_or_version() {
        assert_eq!(run_false(&args(&["a"])), 1);
        assert_eq!(run_false(&args(&["--help", "x"])), 1);
    }

    #[test]
    fn help_texts_name_their_command() {
        assert!(TRUE_HELP.starts_with("Usage: true "));
        assert!(FALSE_HELP.starts_with("Usage: false "));
        assert!(TRUE_HELP.contains("--version"));
    }
}
