//! `tac`: concatenate files and print the records in reverse order.
//!
//! A record ends at each separator (the newline by default), or starts at it with `-b`. The
//! separator is a fixed string, or a BRE with `-r`. An empty `-s` argument means NUL.
//!
//! Each input is read into memory in full, because reversal needs the whole file.

use std::io::{self, Read, Write};

use regex::bytes::Regex;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};
use crate::common::regex::bre_to_regex;

const CMD: &str = "tac";

const SPECS: &[OptSpec] = &[
    OptSpec::flag("before", Some('b'), Some("before")),
    OptSpec::flag("regex", Some('r'), Some("regex")),
    OptSpec::req("separator", Some('s'), Some("separator")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: tac [OPTION]... [FILE]...\n\
Write each FILE to standard output, last line first. With no FILE, or when\n\
FILE is -, read standard input.\n\n\
  -b, --before             attach the separator before instead of after\n\
  -r, --regex              interpret the separator as a basic regular expression\n\
  -s, --separator=STRING   use STRING as the separator instead of newline\n\
      --help               display this help and exit\n\
      --version            output version information and exit\n\n\
Exit status: 0 on success, 1 if an input could not be read.\n";

/// How the separator is matched.
enum Separator {
    Literal(Vec<u8>),
    Pattern(Regex),
}

struct Options {
    before: bool,
    separator: Separator,
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    for item in &items {
        if let Item::Opt { id, .. } = item {
            match *id {
                "help" => {
                    print!("{HELP}");
                    return EXIT_OK;
                }
                "version" => return print_version(CMD),
                _ => {}
            }
        }
    }
    let options = match options_from(&items) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let mut files: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(name) => Some(name.clone()),
            Item::Opt { .. } => None,
        })
        .collect();
    if files.is_empty() {
        files.push("-".to_owned());
    }
    run_files(&options, &files)
}

fn options_from(items: &[Item]) -> Result<Options, i32> {
    let mut before = false;
    let mut regex = false;
    let mut separator = b"\n".to_vec();
    for item in items {
        if let Item::Opt { id, value } = item {
            match *id {
                "before" => before = true,
                "regex" => regex = true,
                "separator" => {
                    let text = value.as_deref().unwrap_or("");
                    separator = if text.is_empty() {
                        vec![0]
                    } else {
                        text.as_bytes().to_vec()
                    };
                }
                _ => {}
            }
        }
    }
    let separator = if regex {
        Separator::Pattern(compile(&String::from_utf8_lossy(&separator))?)
    } else {
        Separator::Literal(separator)
    };
    Ok(Options { before, separator })
}

fn compile(pattern: &str) -> Result<Regex, i32> {
    let translated = bre_to_regex(pattern).map_err(|error| {
        eprintln!("{CMD}: {error}");
        EXIT_FAIL
    })?;
    Regex::new(&translated).map_err(|error| {
        eprintln!("{CMD}: invalid regular expression: {error}");
        EXIT_FAIL
    })
}

/// Each input is reversed on its own, in operand order.
fn run_files(options: &Options, files: &[String]) -> i32 {
    let mut status = EXIT_OK;
    let mut out = stdout_writer();
    for name in files {
        let mut data = Vec::new();
        if let Err(message) = read_into(name, &mut data) {
            eprintln!("{CMD}: failed to open '{name}' for reading: {message}");
            status = EXIT_FAIL;
            continue;
        }
        for (start, end) in records(options, &data).into_iter().rev() {
            if let Err(error) = out.write_all(&data[start..end]) {
                return write_failed(&error);
            }
        }
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed == EXIT_OK {
        status
    } else {
        flushed
    }
}

fn read_into(name: &str, data: &mut Vec<u8>) -> Result<(), String> {
    let mut reader = open_input(name).map_err(|error| io_error_text(&error))?;
    reader
        .read_to_end(data)
        .map(|_| ())
        .map_err(|error| io_error_text(&error))
}

fn write_failed(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_FAIL
}

/// Byte ranges of the records in `data`, in input order. Together they cover all of `data`.
fn records(options: &Options, data: &[u8]) -> Vec<(usize, usize)> {
    let matches = separator_matches(&options.separator, data);
    let mut bounds: Vec<(usize, usize)> = Vec::with_capacity(matches.len() + 2);
    if options.before {
        let mut starts: Vec<usize> = matches.iter().map(|&(start, _)| start).collect();
        starts.insert(0, 0);
        starts.push(data.len());
        for pair in starts.windows(2) {
            if pair[0] < pair[1] {
                bounds.push((pair[0], pair[1]));
            }
        }
    } else {
        let mut prev = 0;
        for &(_, end) in &matches {
            bounds.push((prev, end));
            prev = end;
        }
        if prev < data.len() {
            bounds.push((prev, data.len()));
        }
    }
    bounds
}

/// Non-overlapping, non-empty separator matches from left to right.
fn separator_matches(separator: &Separator, data: &[u8]) -> Vec<(usize, usize)> {
    match separator {
        Separator::Literal(needle) => literal_matches(needle, data),
        Separator::Pattern(regex) => regex
            .find_iter(data)
            .filter(|found| found.end() > found.start())
            .map(|found| (found.start(), found.end()))
            .collect(),
    }
}

fn literal_matches(needle: &[u8], data: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    if needle.is_empty() {
        return found;
    }
    let mut pos = 0;
    while pos + needle.len() <= data.len() {
        if &data[pos..pos + needle.len()] == needle {
            found.push((pos, pos + needle.len()));
            pos += needle.len();
        } else {
            pos += 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(before: bool, separator: &str, regex: bool) -> Options {
        let separator = if regex {
            Separator::Pattern(compile(separator).unwrap())
        } else {
            Separator::Literal(separator.as_bytes().to_vec())
        };
        Options { before, separator }
    }

    fn reversed(options: &Options, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for (start, end) in records(options, data).into_iter().rev() {
            out.extend_from_slice(&data[start..end]);
        }
        out
    }

    #[test]
    fn reverses_newline_records() {
        assert_eq!(
            reversed(&options(false, "\n", false), b"1\n2\n3\n"),
            b"3\n2\n1\n"
        );
    }

    #[test]
    fn keeps_unterminated_last_record_in_place() {
        assert_eq!(reversed(&options(false, "\n", false), b"l1\nl2"), b"l2l1\n");
        assert_eq!(reversed(&options(false, "X", false), b"aXbXc"), b"cbXaX");
    }

    #[test]
    fn before_mode_attaches_separator_to_the_following_record() {
        assert_eq!(
            reversed(&options(true, "\n", false), b"1\n2\n3\n"),
            b"\n\n3\n21"
        );
        assert_eq!(
            reversed(&options(true, "2", false), b"1\n2\n3\n"),
            b"2\n3\n1\n"
        );
    }

    #[test]
    fn regex_separator_matches_single_characters() {
        assert_eq!(
            reversed(&options(false, "[0-9]", true), b"a1b22c\n"),
            b"c\n2b2a1"
        );
        assert_eq!(
            reversed(&options(true, "[0-9]", true), b"a1b22c\n"),
            b"2c\n21ba"
        );
    }

    #[test]
    fn empty_input_produces_nothing() {
        assert!(records(&options(false, "\n", false), b"").is_empty());
    }

    #[test]
    fn literal_matches_do_not_overlap() {
        assert_eq!(literal_matches(b"aa", b"aaaa"), vec![(0, 2), (2, 4)]);
        assert!(literal_matches(b"", b"abc").is_empty());
    }
}
