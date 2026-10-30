//! `rev`: reverses the characters of each line. UTF-8 sequences stay intact; a line that
//! contains an invalid sequence is reversed byte by byte instead. Specification:
//! docs/spec/coreutils-alignment/04-commands-extra.md (rev, util-linux 2.39).

use std::io::{self, BufRead, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};

const CMD: &str = "rev";

const OPTS: &[OptSpec] = &[
    OptSpec::flag("help", Some('h'), Some("help")),
    OptSpec::flag("version", Some('V'), Some("version")),
];

/// Entry point for `rev` and the `agentcmd rev` multi-call form.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, OPTS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage(CMD, &err, EXIT_FAIL),
    };
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id: "help", .. } => return write_help(),
            Item::Opt { id: "version", .. } => return print_version(CMD),
            Item::Opt { .. } => {}
        }
    }
    if operands.is_empty() {
        operands.push("-".to_string());
    }
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    for name in &operands {
        match reverse_operand(name, &mut out) {
            Ok(()) => {}
            Err(Failure::Open(error)) => {
                eprintln!("{CMD}: cannot open {name}: {}", io_error_text(&error));
                status = EXIT_FAIL;
            }
            Err(Failure::Read(error)) => {
                eprintln!("{CMD}: {name}: read error: {}", io_error_text(&error));
                status = EXIT_FAIL;
            }
            Err(Failure::Write(error)) if is_broken_pipe(&error) => return EXIT_OK,
            Err(Failure::Write(error)) => {
                eprintln!("{CMD}: write error: {}", io_error_text(&error));
                return EXIT_FAIL;
            }
        }
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    status
}

fn write_help() -> i32 {
    let mut out = stdout_writer();
    if out.write_all(HELP.as_bytes()).is_err() {
        return EXIT_OK;
    }
    finish_output(CMD, &mut out)
}

/// Why one operand could not be copied, separating input from output failures.
enum Failure {
    Open(io::Error),
    Read(io::Error),
    Write(io::Error),
}

fn reverse_operand(name: &str, out: &mut dyn Write) -> Result<(), Failure> {
    let mut reader = open_input(name).map_err(Failure::Open)?;
    let mut line = Vec::new();
    let mut reversed = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).map_err(Failure::Read)? == 0 {
            return Ok(());
        }
        let newline = line.last() == Some(&b'\n');
        let body = if newline {
            &line[..line.len() - 1]
        } else {
            &line[..]
        };
        reversed.clear();
        reverse_line(body, &mut reversed);
        if newline {
            reversed.push(b'\n');
        }
        out.write_all(&reversed).map_err(Failure::Write)?;
    }
}

/// Appends `line` reversed by character, or by byte when it is not valid UTF-8.
fn reverse_line(line: &[u8], out: &mut Vec<u8>) {
    match std::str::from_utf8(line) {
        Ok(text) => {
            for ch in text.chars().rev() {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
        }
        Err(_) => out.extend(line.iter().rev()),
    }
}

const HELP: &str = "\
Usage: rev [options] [file ...]

Reverse lines characterwise.

Options:
 -h, --help     display this help
 -V, --version  display version

For more details see rev(1).
";

#[cfg(test)]
mod tests {
    use super::*;

    fn reversed(line: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        reverse_line(line, &mut out);
        out
    }

    #[test]
    fn ascii_lines_reverse_bytewise() {
        assert_eq!(reversed(b"abc"), b"cba");
        assert_eq!(reversed(b""), b"");
    }

    #[test]
    fn multibyte_characters_stay_whole() {
        assert_eq!(reversed("héllo".as_bytes()), "olléh".as_bytes());
        assert_eq!(reversed("日本".as_bytes()), "本日".as_bytes());
    }

    #[test]
    fn invalid_sequences_reverse_by_byte() {
        assert_eq!(reversed(b"ab\xffcd"), b"dc\xffba");
        assert_eq!(reversed(b"ab\xe6\x97"), b"\x97\xe6ba");
    }

    #[test]
    fn carriage_return_is_an_ordinary_character() {
        assert_eq!(reversed(b"abc\r"), b"\rcba");
    }

    #[test]
    fn stream_keeps_newlines_and_missing_final_newline() {
        let mut out = Vec::new();
        let mut reader: &[u8] = b"ab\n\ncd";
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).unwrap() > 0 {
            let body = line.strip_suffix(b"\n").unwrap_or(&line).to_vec();
            let mut piece = Vec::new();
            reverse_line(&body, &mut piece);
            if line.ends_with(b"\n") {
                piece.push(b'\n');
            }
            out.extend(piece);
            line.clear();
        }
        assert_eq!(out, b"ba\n\ndc");
    }
}
