//! `test` and `[`: evaluate a conditional expression and set the exit status.
//!
//! Exit status: 0 when the expression is true, 1 when false, 2 on a syntax or operand
//! error. `test` never treats `--help` specially; `[` does when it is the only argument.

mod expr;
mod files;

use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};

const BRACKET_HELP: &str = "\
Usage: [ EXPRESSION ]
  or:  [ OPTION
Exit with the status determined by EXPRESSION.

      --help     display this help and exit
      --version  output version information and exit

An omitted EXPRESSION defaults to false.  Otherwise, EXPRESSION is true or
false and sets the exit status.  It is one of:

  ( EXPRESSION )               EXPRESSION is true
  ! EXPRESSION                 EXPRESSION is false
  EXPRESSION1 -a EXPRESSION2   both EXPRESSION1 and EXPRESSION2 are true
  EXPRESSION1 -o EXPRESSION2   either EXPRESSION1 or EXPRESSION2 is true

  -n STRING                    the length of STRING is nonzero
  -z STRING                    the length of STRING is zero
  STRING1 = STRING2            the strings are equal
  STRING1 != STRING2           the strings are not equal
  INTEGER1 -eq INTEGER2        INTEGER1 is equal to INTEGER2
  INTEGER1 -ne INTEGER2        INTEGER1 is not equal to INTEGER2
  INTEGER1 -lt INTEGER2        INTEGER1 is less than INTEGER2
  INTEGER1 -le INTEGER2        INTEGER1 is less than or equal to INTEGER2
  INTEGER1 -gt INTEGER2        INTEGER1 is greater than INTEGER2
  INTEGER1 -ge INTEGER2        INTEGER1 is greater than or equal to INTEGER2
  FILE1 -nt FILE2              FILE1 is newer than FILE2
  FILE1 -ot FILE2              FILE1 is older than FILE2
  FILE1 -ef FILE2              FILE1 and FILE2 are the same file
  -e FILE                      FILE exists
  -f FILE                      FILE exists and is a regular file
  -d FILE                      FILE exists and is a directory
  -h FILE, -L FILE             FILE exists and is a symbolic link
  -s FILE                      FILE exists and has a size greater than zero
  -r FILE, -w FILE, -x FILE    FILE exists and is readable, writable, executable
  -t FD                        file descriptor FD is a terminal

Exit status is 0 if EXPRESSION is true, 1 if false, and 2 on an error.
";

/// Entry point for `test`.
pub fn run(args: &[String]) -> i32 {
    finish("test", expr::evaluate(args))
}

/// Entry point for `[`, which requires a final `]` argument.
pub fn run_bracket(args: &[String]) -> i32 {
    match args {
        [only] if only == "--help" => {
            print!("{BRACKET_HELP}");
            return EXIT_OK;
        }
        [only] if only == "--version" => return print_version("["),
        _ => {}
    }
    let Some((last, rest)) = args.split_last() else {
        eprintln!("[: missing ']'");
        return EXIT_TROUBLE;
    };
    if last != "]" {
        eprintln!("[: missing ']'");
        return EXIT_TROUBLE;
    }
    finish("[", expr::evaluate(rest))
}

fn finish(name: &str, result: Result<bool, String>) -> i32 {
    match result {
        Ok(true) => EXIT_OK,
        Ok(false) => EXIT_FAIL,
        Err(message) => {
            eprintln!("{name}: {message}");
            EXIT_TROUBLE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn bracket_requires_closing_bracket() {
        assert_eq!(run_bracket(&strings(&["a"])), EXIT_TROUBLE);
        assert_eq!(run_bracket(&[]), EXIT_TROUBLE);
        assert_eq!(run_bracket(&strings(&["a", "=", "a", "]"])), EXIT_OK);
        assert_eq!(run_bracket(&strings(&["]"])), EXIT_FAIL);
    }

    #[test]
    fn test_treats_help_as_a_string() {
        assert_eq!(run(&strings(&["--help"])), EXIT_OK);
        assert_eq!(run(&strings(&["--version"])), EXIT_OK);
        assert_eq!(run(&[]), EXIT_FAIL);
    }

    #[test]
    fn errors_map_to_status_two() {
        assert_eq!(run(&strings(&["a", "b"])), EXIT_TROUBLE);
        assert_eq!(run(&strings(&["-eq", "1"])), EXIT_TROUBLE);
        assert_eq!(run(&strings(&["a", "-eq", "1"])), EXIT_TROUBLE);
    }
}
