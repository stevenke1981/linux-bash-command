//! `printf`: format and print data, following GNU coreutils.
//!
//! The submodules hold the parts shared with `seq`: C float formatting in `float`, and the
//! format engine in `format`. Numeric argument parsing (`num`) and `%q` quoting (`quote`)
//! are private.

pub mod float;
mod format;
mod num;
mod quote;

use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const NAME: &str = "printf";

const HELP: &str = "\
Usage: printf FORMAT [ARGUMENT]...
  or:  printf OPTION

Print ARGUMENT(s) according to FORMAT, or execute according to OPTION:

      --help     display this help and exit
      --version  output version information and exit

FORMAT controls the output as in C printf.  Interpreted sequences are:

  \\\"        double quote
  \\\\        backslash
  \\a        alert (BEL)
  \\b        backspace
  \\c        produce no further output
  \\e        escape
  \\f        form feed
  \\n        new line
  \\r        carriage return
  \\t        horizontal tab
  \\v        vertical tab
  \\NNN      byte with octal value NNN (1 to 3 digits)
  \\xHH      byte with hexadecimal value HH (1 to 2 digits)
  \\uHHHH    Unicode character with hexadecimal value HHHH (1 to 4 digits)
  \\UHHHHHHHH  Unicode character with hexadecimal value HHHHHHHH (1 to 8 digits)
  %%        a literal %

Each ARGUMENT is converted by its directive:

  %d %i                  signed decimal integer
  %o %u %x %X            unsigned octal, decimal and hexadecimal integer
  %f %F %e %E %g %G %a %A  floating point
  %c                     first character of the argument
  %s                     string
  %b                     string with backslash escapes (\\c stops output)
  %q                     string quoted for reuse as shell input

Field width and precision may be numbers or * (taken from the arguments).
FORMAT is reused while arguments remain.

Exit status is 0 on success, 1 on failure.
";

/// Entry point for `printf`. Only a sole `--help` or `--version` is an option.
pub fn run(args: &[String]) -> i32 {
    if let [only] = args {
        match only.as_str() {
            "--help" => {
                print!("{HELP}");
                return EXIT_OK;
            }
            "--version" => return print_version(NAME),
            _ => {}
        }
    }
    let Some(format) = args.first() else {
        eprintln!("{NAME}: missing operand\nTry '{NAME} --help' for more information.");
        return EXIT_FAIL;
    };
    let mut out = stdout_writer();
    let status = match format::execute(format, &args[1..], &mut out) {
        Ok(status) => status,
        Err(error) if is_broken_pipe(&error) => return EXIT_OK,
        Err(error) => {
            eprintln!("{NAME}: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
    };
    let flushed = finish_output(NAME, &mut out);
    if flushed != EXIT_OK {
        flushed
    } else {
        status
    }
}
