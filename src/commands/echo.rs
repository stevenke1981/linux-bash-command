//! `echo`: prints its arguments. Option words are only `-n`, `-e` and `-E` combinations, and
//! `--help`/`--version` count only as the sole argument (GNU behaviour).

use std::io::Write;

use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const HELP: &str = r#"Usage: echo [SHORT-OPTION]... [STRING]...
  or:  echo LONG-OPTION
Echo the STRING(s) to standard output.

  -n             do not output the trailing newline
  -e             enable interpretation of backslash escapes
  -E             disable interpretation of backslash escapes (default)
      --help     display this help and exit
      --version  output version information and exit

If -e is in effect, the following sequences are recognized:

  \\      backslash
  \a      alert (BEL)
  \b      backspace
  \c      produce no further output
  \e      escape
  \f      form feed
  \n      new line
  \r      carriage return
  \t      horizontal tab
  \v      vertical tab
  \0NNN   byte with octal value NNN (1 to 3 digits)
  \xHH    byte with hexadecimal value HH (1 to 2 digits)
"#;

/// Output options taken from the leading option words.
#[derive(Debug, PartialEq, Eq)]
struct Flags {
    newline: bool,
    escapes: bool,
}

/// True for a word such as `-n`, `-e`, `-neE`: a dash followed only by `n`, `e` or `E`.
/// A lone `-` or `--` is text.
fn is_option_word(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with('-') && arg[1..].chars().all(|c| matches!(c, 'n' | 'e' | 'E'))
}

/// Consumes leading option words and returns the flags with the remaining operands.
fn parse_flags(args: &[String]) -> (Flags, &[String]) {
    let mut flags = Flags {
        newline: true,
        escapes: false,
    };
    let mut consumed = 0;
    for arg in args {
        if !is_option_word(arg) {
            break;
        }
        for letter in arg[1..].chars() {
            match letter {
                'n' => flags.newline = false,
                'e' => flags.escapes = true,
                _ => flags.escapes = false,
            }
        }
        consumed += 1;
    }
    (flags, &args[consumed..])
}

/// Reads up to `max` digits in `radix` from the start of `bytes`: the value and the count.
fn take_digits(bytes: &[u8], radix: u32, max: usize) -> (u32, usize) {
    let mut value = 0;
    let mut used = 0;
    for &byte in bytes.iter().take(max) {
        match (byte as char).to_digit(radix) {
            Some(digit) => {
                value = value * radix + digit;
                used += 1;
            }
            None => break,
        }
    }
    (value, used)
}

/// Decodes `echo -e` escapes. Returns the bytes and whether `\c` stopped the output.
///
/// Follows GNU echo: `\0NNN` takes up to three octal digits after the zero, `\NNN` (first
/// digit 1 to 7) up to three digits including the first, `\xHH` up to two hex digits.
/// Unknown escapes keep their backslash.
fn decode(text: &str) -> (Vec<u8>, bool) {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' || index + 1 == bytes.len() {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        let code = bytes[index + 1];
        index += 2;
        let simple = match code {
            b'\\' => Some(b'\\'),
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'e' => Some(0x1b),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(0x0b),
            _ => None,
        };
        if let Some(byte) = simple {
            out.push(byte);
            continue;
        }
        match code {
            b'c' => return (out, true),
            b'0' => {
                let (value, used) = take_digits(&bytes[index..], 8, 3);
                index += used;
                out.push(value as u8);
            }
            b'1'..=b'7' => {
                let (value, used) = take_digits(&bytes[index - 1..], 8, 3);
                index += used - 1;
                out.push(value as u8);
            }
            b'x' => {
                let (value, used) = take_digits(&bytes[index..], 16, 2);
                if used == 0 {
                    out.extend_from_slice(b"\\x");
                } else {
                    index += used;
                    out.push(value as u8);
                }
            }
            _ => {
                out.push(b'\\');
                out.push(code);
            }
        }
    }
    (out, false)
}

/// Produces the bytes `echo` writes for `args`.
fn render(args: &[String]) -> Vec<u8> {
    let (flags, operands) = parse_flags(args);
    let joined = operands.join(" ");
    let (mut bytes, stopped) = if flags.escapes {
        decode(&joined)
    } else {
        (joined.into_bytes(), false)
    };
    if flags.newline && !stopped {
        bytes.push(b'\n');
    }
    bytes
}

/// Writes `bytes` to stdout. A closed pipe is silent success.
fn emit(bytes: &[u8]) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(bytes) {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("echo: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    finish_output("echo", &mut out)
}

pub fn run(args: &[String]) -> i32 {
    if let [only] = args {
        match only.as_str() {
            "--help" => return emit(HELP.as_bytes()),
            "--version" => return print_version("echo"),
            _ => {}
        }
    }
    emit(&render(args))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn option_words_are_only_n_e_and_capital_e_letters() {
        assert!(is_option_word("-n"));
        assert!(is_option_word("-neE"));
        assert!(!is_option_word("-"));
        assert!(!is_option_word("--"));
        assert!(!is_option_word("-x"));
        assert!(!is_option_word("-nx"));
        assert!(!is_option_word("text"));
    }

    #[test]
    fn plain_arguments_are_joined_with_spaces_and_a_newline() {
        assert_eq!(render(&args(&["a", "b"])), b"a b\n");
        assert_eq!(render(&[]), b"\n");
    }

    #[test]
    fn n_suppresses_the_newline_and_later_e_or_capital_e_wins() {
        assert_eq!(render(&args(&["-n", "x"])), b"x");
        assert_eq!(render(&args(&["-e", "-E", "a\\tb"])), b"a\\tb\n");
        assert_eq!(render(&args(&["-E", "-e", "a\\tb"])), b"a\tb\n");
    }

    #[test]
    fn escapes_follow_echo_dialect() {
        assert_eq!(render(&args(&["-e", "\\0101\\x41\\xZZ"])), b"AA\\xZZ\n");
        assert_eq!(render(&args(&["-e", "a\\cb"])), b"a");
        assert_eq!(render(&args(&["-e", "\\q"])), b"\\q\n");
    }

    #[test]
    fn bare_octal_escapes_take_up_to_three_digits() {
        assert_eq!(decode("\\1x"), (b"\x01x".to_vec(), false));
        assert_eq!(decode("\\101"), (b"A".to_vec(), false));
        assert_eq!(decode("\\1011"), (b"A1".to_vec(), false));
        assert_eq!(decode("\\0"), (b"\0".to_vec(), false));
        assert_eq!(decode("\\08"), (vec![0u8, b'8'], false));
    }

    #[test]
    fn hex_escapes_take_up_to_two_digits() {
        assert_eq!(decode("\\x4a!"), (b"J!".to_vec(), false));
        assert_eq!(decode("\\x"), (b"\\x".to_vec(), false));
        assert_eq!(decode("tail\\"), (b"tail\\".to_vec(), false));
    }

    #[test]
    fn non_option_words_are_text() {
        assert_eq!(render(&args(&["-nx", "y"])), b"-nx y\n");
        assert_eq!(render(&args(&["--", "x"])), b"-- x\n");
        assert_eq!(render(&args(&["-"])), b"-\n");
    }
}
