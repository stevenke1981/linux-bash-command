//! `fold`: wrap each input line to a maximum width.
//!
//! Columns follow GNU: a tab advances to the next multiple of 8, a backspace moves back one
//! column and a carriage return returns to column 0. `-b` counts every byte as one column.

use std::io::{self, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const CMD: &str = "fold";
const DEFAULT_WIDTH: usize = 80;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("bytes", Some('b'), Some("bytes")),
    OptSpec::flag("spaces", Some('s'), Some("spaces")),
    OptSpec::req("width", Some('w'), Some("width")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: fold [OPTION]... [FILE]...\n\
Wrap input lines in each FILE, writing to standard output. With no FILE,\n\
or when FILE is -, read standard input.\n\n\
  -b, --bytes         count bytes rather than columns\n\
  -s, --spaces        break at spaces\n\
  -w, --width=WIDTH   use WIDTH columns instead of 80\n\
      --help          display this help and exit\n\
      --version       output version information and exit\n\n\
A legacy -N option is the same as -w N.\n\
Exit status: 0 on success, 1 if an input could not be read.\n";

#[derive(Clone, Copy, Debug)]
struct Settings {
    width: usize,
    bytes: bool,
    spaces: bool,
}

pub fn run(args: &[String]) -> i32 {
    let args = expand_legacy_width(args);
    let items = match Parser::new(CMD, SPECS).parse(&args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    if let Some(code) = early_option(&items) {
        return code;
    }
    let settings = match settings_from(&items) {
        Ok(settings) => settings,
        Err(code) => return code,
    };
    let files = operands(&items);
    run_files(settings, &files)
}

/// Rewrites a legacy `-N` argument (digits only) to `-w N`.
fn expand_legacy_width(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut options_open = true;
    for arg in args {
        if arg == "--" {
            options_open = false;
        }
        let legacy = options_open
            && arg.len() > 1
            && arg.starts_with('-')
            && arg[1..].bytes().all(|b| b.is_ascii_digit());
        if legacy {
            out.push("-w".to_owned());
            out.push(arg[1..].to_owned());
        } else {
            out.push(arg.clone());
        }
    }
    out
}

fn early_option(items: &[Item]) -> Option<i32> {
    for item in items {
        if let Item::Opt { id, .. } = item {
            match *id {
                "help" => {
                    print!("{HELP}");
                    return Some(EXIT_OK);
                }
                "version" => return Some(print_version(CMD)),
                _ => {}
            }
        }
    }
    None
}

fn settings_from(items: &[Item]) -> Result<Settings, i32> {
    let mut settings = Settings {
        width: DEFAULT_WIDTH,
        bytes: false,
        spaces: false,
    };
    for item in items {
        if let Item::Opt { id, value } = item {
            match *id {
                "bytes" => settings.bytes = true,
                "spaces" => settings.spaces = true,
                "width" => settings.width = parse_width(value.as_deref().unwrap_or(""))?,
                _ => {}
            }
        }
    }
    Ok(settings)
}

fn parse_width(text: &str) -> Result<usize, i32> {
    match text.parse::<usize>() {
        Ok(width) if width > 0 => Ok(width),
        _ => {
            eprintln!(
                "{CMD}: invalid number of columns: '{text}'\nTry '{CMD} --help' for more information."
            );
            Err(EXIT_FAIL)
        }
    }
}

fn operands(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(name) => Some(name.clone()),
            Item::Opt { .. } => None,
        })
        .collect()
}

fn run_files(settings: Settings, files: &[String]) -> i32 {
    let default = ["-".to_owned()];
    let names: &[String] = if files.is_empty() { &default } else { files };
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    for name in names {
        match fold_file(&settings, name, &mut out) {
            Ok(()) => {}
            Err(FoldError::Input(error)) => {
                report(CMD, name, &error);
                status = EXIT_FAIL;
            }
            Err(FoldError::Output(error)) => return output_failed(&error),
        }
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed == EXIT_OK {
        status
    } else {
        flushed
    }
}

enum FoldError {
    Input(io::Error),
    Output(io::Error),
}

fn fold_file(settings: &Settings, name: &str, out: &mut impl Write) -> Result<(), FoldError> {
    let mut reader = open_input(name).map_err(FoldError::Input)?;
    let mut record = Vec::new();
    let mut folded = Vec::new();
    while read_record(reader.as_mut(), b'\n', &mut record).map_err(FoldError::Input)? {
        folded.clear();
        let terminated = record.last() == Some(&b'\n');
        let body = if terminated {
            &record[..record.len() - 1]
        } else {
            &record[..]
        };
        fold_line(settings, body, terminated, &mut folded);
        out.write_all(&folded).map_err(FoldError::Output)?;
    }
    Ok(())
}

fn output_failed(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_FAIL
}

/// Column after `byte` is written at column `col`.
fn advance(col: usize, byte: u8, bytes: bool) -> usize {
    if bytes {
        return col + 1;
    }
    match byte {
        b'\t' => (col | 7) + 1,
        0x08 => col.saturating_sub(1),
        b'\r' => 0,
        _ => col + 1,
    }
}

/// Folds one line (without its newline) into `out`. `terminated` says whether to end the
/// final output line with a newline.
fn fold_line(settings: &Settings, body: &[u8], terminated: bool, out: &mut Vec<u8>) {
    let mut seg: Vec<u8> = Vec::with_capacity(body.len());
    let mut col = 0usize;
    for &byte in body {
        while col > 0 && advance(col, byte, settings.bytes) > settings.width {
            let keep = break_point(&seg, settings.spaces);
            out.extend_from_slice(&seg[..keep]);
            out.push(b'\n');
            seg.drain(..keep);
            col = columns_of(&seg, settings.bytes);
        }
        seg.push(byte);
        col = advance(col, byte, settings.bytes);
    }
    out.extend_from_slice(&seg);
    if terminated {
        out.push(b'\n');
    }
}

/// How many bytes of `seg` go on the current line: everything, or with `-s` up to and
/// including the last space when there is one.
fn break_point(seg: &[u8], spaces: bool) -> usize {
    if spaces {
        if let Some(index) = seg.iter().rposition(|&b| b == b' ') {
            return index + 1;
        }
    }
    seg.len()
}

fn columns_of(seg: &[u8], bytes: bool) -> usize {
    seg.iter().fold(0, |col, &byte| advance(col, byte, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(text: &[u8], width: usize, bytes: bool, spaces: bool) -> Vec<u8> {
        let settings = Settings {
            width,
            bytes,
            spaces,
        };
        let mut out = Vec::new();
        let (body, terminated) = match text.last() {
            Some(b'\n') => (&text[..text.len() - 1], true),
            _ => (text, false),
        };
        fold_line(&settings, body, terminated, &mut out);
        out
    }

    #[test]
    fn breaks_at_width() {
        assert_eq!(fold(b"abcdefgh\n", 3, false, false), b"abc\ndef\ngh\n");
    }

    #[test]
    fn breaks_at_spaces_when_asked() {
        assert_eq!(
            fold(b"hello world foo\n", 8, false, true),
            b"hello \nworld \nfoo\n"
        );
    }

    #[test]
    fn tabs_count_to_the_next_stop() {
        assert_eq!(fold(b"a\tbc\td\n", 4, false, false), b"a\n\t\nbc\n\t\nd\n");
        assert_eq!(fold(b"a\tbc\td\n", 4, true, false), b"a\tbc\n\td\n");
    }

    #[test]
    fn backspace_and_carriage_return_move_the_column() {
        assert_eq!(fold(b"ab\x08cd\re\n", 3, false, false), b"ab\x08cd\re\n");
    }

    #[test]
    fn last_line_without_newline_stays_unterminated() {
        assert_eq!(fold(b"abcd", 2, false, false), b"ab\ncd");
    }

    #[test]
    fn legacy_width_is_rewritten() {
        let args = vec!["-4".to_owned(), "--".to_owned(), "-5".to_owned()];
        assert_eq!(expand_legacy_width(&args), vec!["-w", "4", "--", "-5"]);
    }

    #[test]
    fn rejects_zero_width() {
        assert!(parse_width("0").is_err());
        assert_eq!(parse_width("12"), Ok(12));
    }
}
