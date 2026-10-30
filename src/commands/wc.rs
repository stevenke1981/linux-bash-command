//! `wc`: print newline, word, character, byte and maximum line length counts.
//!
//! Column widths follow GNU: a single count for a single operand is unpadded; `--total=only`
//! is unpadded; otherwise the width is the number of digits in the total size of the regular
//! inputs, and at least 7 when any input is a pipe, device or directory.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};
use crate::common::path::display_path;

const CMD: &str = "wc";
const READ_CHUNK: usize = 64 * 1024;
const NON_REGULAR_MIN_WIDTH: usize = 7;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("bytes", Some('c'), Some("bytes")),
    OptSpec::flag("chars", Some('m'), Some("chars")),
    OptSpec::flag("lines", Some('l'), Some("lines")),
    OptSpec::flag("words", Some('w'), Some("words")),
    OptSpec::flag("max", Some('L'), Some("max-line-length")),
    OptSpec::req("files0", None, Some("files0-from")),
    OptSpec::req("total", None, Some("total")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: wc [OPTION]... [FILE]...\n  or:  wc [OPTION]... --files0-from=F\n\
Print newline, word, and byte counts for each FILE, and a total line if\n\
more than one FILE is specified. A word is a non-zero-length sequence of\n\
characters delimited by white space. With no FILE, or when FILE is -,\n\
read standard input.\n\n\
The options below may be used to select which counts are printed, always in\n\
the order: newline, word, character, byte, maximum line length.\n\
  -c, --bytes            print the byte counts\n\
  -m, --chars            print the character counts (UTF-8; invalid bytes count 1)\n\
  -l, --lines            print the newline counts\n\
  -L, --max-line-length  print the length of the longest line\n\
  -w, --words            print the word counts\n\
      --files0-from=F    read input file names from F, NUL-terminated\n\
                         (use - to read names from standard input)\n\
      --total=WHEN       when to print a line with total counts;\n\
                         WHEN is auto, always, only, or never\n\
      --help             display this help and exit\n\
      --version          output version information and exit\n\n\
Exit status: 0 on success, 1 if an input could not be read.\n";

/// Which counts were requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wants {
    lines: bool,
    words: bool,
    chars: bool,
    bytes: bool,
    max: bool,
}

impl Wants {
    fn count(self) -> usize {
        [self.lines, self.words, self.chars, self.bytes, self.max]
            .iter()
            .filter(|on| **on)
            .count()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    lines: u64,
    words: u64,
    chars: u64,
    bytes: u64,
    max: u64,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.lines += other.lines;
        self.words += other.words;
        self.chars += other.chars;
        self.bytes += other.bytes;
        self.max = self.max.max(other.max);
    }

    fn values(&self, wants: Wants) -> Vec<u64> {
        let mut out = Vec::with_capacity(5);
        for (on, value) in [
            (wants.lines, self.lines),
            (wants.words, self.words),
            (wants.chars, self.chars),
            (wants.bytes, self.bytes),
            (wants.max, self.max),
        ] {
            if on {
                out.push(value);
            }
        }
        out
    }
}

/// When the total line is printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TotalMode {
    Auto,
    Always,
    Only,
    Never,
}

/// What the size pre-pass learned about an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Regular(u64),
    /// Pipe, device, or directory: forces the minimum width.
    Other,
    /// Could not be examined; it is reported when it is processed.
    Missing,
}

/// One input in processing order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// Standard input, with the operand name when one was given.
    Stdin(Option<String>),
    File(String),
    /// A `--files0-from` entry that cannot be used; the message is printed in order.
    Invalid(String),
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    if let Some(early) = early_option(&items) {
        return early;
    }
    let mut wants = Wants {
        lines: false,
        words: false,
        chars: false,
        bytes: false,
        max: false,
    };
    let mut total = TotalMode::Auto;
    let mut files0: Option<String> = None;
    let mut operands = Vec::new();
    for item in &items {
        match item {
            Item::Opt { id, value } => {
                if let Err(code) =
                    apply_option(id, value.as_deref(), &mut wants, &mut total, &mut files0)
                {
                    return code;
                }
            }
            Item::Operand(name) => operands.push(name.clone()),
        }
    }
    if wants.count() == 0 {
        wants.lines = true;
        wants.words = true;
        wants.bytes = true;
    }
    let targets = match build_targets(files0.as_deref(), &operands) {
        Ok(targets) => targets,
        Err(code) => return code,
    };
    // GNU does not size names read from standard input before counting them.
    let sized = files0.as_deref() != Some("-");
    run_targets(&targets, wants, total, sized)
}

/// Handles `--help` and `--version` in argument order, as GNU does.
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

fn apply_option(
    id: &str,
    value: Option<&str>,
    wants: &mut Wants,
    total: &mut TotalMode,
    files0: &mut Option<String>,
) -> Result<(), i32> {
    match id {
        "bytes" => wants.bytes = true,
        "chars" => wants.chars = true,
        "lines" => wants.lines = true,
        "words" => wants.words = true,
        "max" => wants.max = true,
        "files0" => *files0 = value.map(str::to_owned),
        "total" => *total = parse_total(value.unwrap_or("auto"))?,
        _ => {}
    }
    Ok(())
}

fn parse_total(text: &str) -> Result<TotalMode, i32> {
    match text {
        "auto" => Ok(TotalMode::Auto),
        "always" => Ok(TotalMode::Always),
        "only" => Ok(TotalMode::Only),
        "never" => Ok(TotalMode::Never),
        _ => {
            eprintln!(
                "{CMD}: invalid argument '{text}' for '--total'\n\
Valid arguments are:\n  - 'auto'\n  - 'always'\n  - 'only'\n  - 'never'\n\
Try '{CMD} --help' for more information."
            );
            Err(EXIT_FAIL)
        }
    }
}

fn build_targets(files0: Option<&str>, operands: &[String]) -> Result<Vec<Target>, i32> {
    match files0 {
        None if operands.is_empty() => Ok(vec![Target::Stdin(None)]),
        None => Ok(operands.iter().map(|name| operand_target(name)).collect()),
        Some(list) => {
            if let Some(extra) = operands.first() {
                eprintln!(
                    "{CMD}: extra operand '{extra}'\n\
file operands cannot be combined with --files0-from\n\
Try '{CMD} --help' for more information."
                );
                return Err(EXIT_FAIL);
            }
            read_file_list(list)
        }
    }
}

fn operand_target(name: &str) -> Target {
    if name == "-" {
        Target::Stdin(Some(name.to_owned()))
    } else {
        Target::File(name.to_owned())
    }
}

/// Reads NUL-terminated names from `list` (`-` is standard input).
fn read_file_list(list: &str) -> Result<Vec<Target>, i32> {
    let mut reader = match open_input(list) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!(
                "{CMD}: cannot open '{list}' for reading: {}",
                io_error_text(&error)
            );
            return Err(EXIT_FAIL);
        }
    };
    let mut targets = Vec::new();
    let mut record = Vec::new();
    let mut index = 0usize;
    while read_record(reader.as_mut(), 0, &mut record).unwrap_or(false) {
        index += 1;
        let name = record.strip_suffix(b"\0").unwrap_or(&record);
        targets.push(file_list_target(list, index, name));
    }
    Ok(targets)
}

fn file_list_target(list: &str, index: usize, name: &[u8]) -> Target {
    if name.is_empty() {
        return Target::Invalid(format!("{list}:{index}: invalid zero-length file name"));
    }
    let text = String::from_utf8_lossy(name).into_owned();
    if text == "-" && list == "-" {
        return Target::Invalid(
            "when reading file names from stdin, no file name of '-' allowed".to_owned(),
        );
    }
    operand_target(&text)
}

fn run_targets(targets: &[Target], wants: Wants, total: TotalMode, sized: bool) -> i32 {
    let kinds: Vec<Kind> = targets.iter().map(target_kind).collect();
    let width = if sized {
        number_width(&kinds, wants, total, targets.len())
    } else {
        1
    };
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    let mut sum = Counts::default();
    for target in targets {
        let (name, result) = match target {
            Target::Invalid(message) => {
                eprintln!("{CMD}: {message}");
                status = EXIT_FAIL;
                continue;
            }
            Target::Stdin(name) => (name.clone(), count_stdin(wants)),
            Target::File(name) => (Some(name.clone()), count_file(name, wants)),
        };
        let counts = match result {
            Ok(counts) => counts,
            Err((error, is_dir)) => {
                let shown = name.clone().unwrap_or_else(|| "-".to_owned());
                report(CMD, &shown, &error);
                status = EXIT_FAIL;
                if !is_dir {
                    continue;
                }
                Counts::default()
            }
        };
        sum.add(&counts);
        if total != TotalMode::Only {
            let line = format_row(&counts, wants, width, name.as_deref());
            if let Err(error) = out.write_all(line.as_bytes()) {
                return write_failed(&error);
            }
        }
    }
    if matches!(total, TotalMode::Always | TotalMode::Only)
        || (total == TotalMode::Auto && targets.len() > 1)
    {
        let name = (total != TotalMode::Only).then_some("total");
        let line = format_row(&sum, wants, width, name);
        if let Err(error) = out.write_all(line.as_bytes()) {
            return write_failed(&error);
        }
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed == EXIT_OK {
        status
    } else {
        flushed
    }
}

fn write_failed(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_FAIL
}

fn target_kind(target: &Target) -> Kind {
    match target {
        Target::Stdin(_) => stdin_kind(),
        Target::File(name) => path_kind(Path::new(name)),
        Target::Invalid(_) => Kind::Missing,
    }
}

fn path_kind(path: &Path) -> Kind {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => Kind::Regular(meta.len()),
        Ok(_) => Kind::Other,
        Err(_) => Kind::Missing,
    }
}

#[cfg(unix)]
fn stdin_kind() -> Kind {
    path_kind(Path::new("/dev/stdin"))
}

#[cfg(not(unix))]
fn stdin_kind() -> Kind {
    Kind::Other
}

/// Column width: 1 for a single column with a single operand or for `--total=only`;
/// otherwise the digits of the regular-file size total, at least 7 when a non-regular input
/// was seen.
fn number_width(kinds: &[Kind], wants: Wants, total: TotalMode, operands: usize) -> usize {
    if total == TotalMode::Only || (wants.count() == 1 && operands == 1) {
        return 1;
    }
    let mut size_total: u64 = 0;
    let mut min_width = 1;
    for kind in kinds {
        match kind {
            Kind::Regular(size) => size_total = size_total.saturating_add(*size),
            Kind::Other => min_width = NON_REGULAR_MIN_WIDTH,
            Kind::Missing => {}
        }
    }
    digits(size_total).max(min_width)
}

fn digits(value: u64) -> usize {
    value.to_string().len()
}

fn format_row(counts: &Counts, wants: Wants, width: usize, name: Option<&str>) -> String {
    let fields: Vec<String> = counts
        .values(wants)
        .iter()
        .map(|value| format!("{value:>width$}"))
        .collect();
    let mut line = fields.join(" ");
    if let Some(name) = name {
        line.push(' ');
        line.push_str(&display_path(Path::new(name)));
    }
    line.push('\n');
    line
}

fn count_stdin(wants: Wants) -> Result<Counts, (io::Error, bool)> {
    let mut reader = open_input("-").map_err(|error| (error, false))?;
    count_reader(reader.as_mut(), wants).map_err(|error| (error, false))
}

fn count_file(name: &str, wants: Wants) -> Result<Counts, (io::Error, bool)> {
    let path = Path::new(name);
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => {
            return Err((io::Error::from(io::ErrorKind::IsADirectory), true));
        }
        Ok(_) => {}
        Err(error) => return Err((error, false)),
    }
    let mut reader = open_input(name).map_err(|error| (error, false))?;
    count_reader(reader.as_mut(), wants).map_err(|error| (error, false))
}

fn count_reader(reader: &mut dyn BufRead, wants: Wants) -> io::Result<Counts> {
    let mut scan = Scan::default();
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let read = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        scan.feed(&buf[..read], wants);
    }
    Ok(scan.finish(wants))
}

/// Streaming state carried across read chunks.
#[derive(Default)]
struct Scan {
    counts: Counts,
    in_word: bool,
    column: u64,
    utf8_carry: Vec<u8>,
}

impl Scan {
    fn feed(&mut self, chunk: &[u8], wants: Wants) {
        self.counts.bytes += chunk.len() as u64;
        for &byte in chunk {
            if byte == b'\n' {
                self.counts.lines += 1;
            }
            if wants.words {
                self.word_byte(byte);
            }
            if wants.max {
                self.width_byte(byte);
            }
        }
        if wants.chars {
            self.counts.chars += count_chars(&mut self.utf8_carry, chunk);
        }
    }

    fn word_byte(&mut self, byte: u8) {
        if is_space(byte) {
            self.in_word = false;
        } else if !self.in_word {
            self.in_word = true;
            self.counts.words += 1;
        }
    }

    fn width_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.end_line(),
            b'\r' | b'\x0c' => {
                self.end_line();
                self.column = 0;
            }
            b'\t' => self.column = (self.column / 8 + 1) * 8,
            b' '..=b'~' => self.column += 1,
            // UTF-8 lead bytes count as one character; continuation bytes add nothing.
            0x80..=0xff if byte & 0xc0 != 0x80 => self.column += 1,
            _ => {}
        }
        self.counts.max = self.counts.max.max(self.column);
    }

    fn end_line(&mut self) {
        self.counts.max = self.counts.max.max(self.column);
        self.column = 0;
    }

    fn finish(mut self, wants: Wants) -> Counts {
        if wants.chars {
            self.counts.chars += self.utf8_carry.len() as u64;
        }
        if wants.max {
            self.end_line();
        }
        self.counts
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Counts UTF-8 characters in `chunk`, carrying an incomplete trailing sequence in `carry`.
/// Each invalid byte counts as one character.
fn count_chars(carry: &mut Vec<u8>, chunk: &[u8]) -> u64 {
    let mut buf = std::mem::take(carry);
    buf.extend_from_slice(chunk);
    let mut count = 0u64;
    let mut rest: &[u8] = &buf;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                count += text.chars().count() as u64;
                break;
            }
            Err(error) => {
                let good = error.valid_up_to();
                if let Ok(text) = std::str::from_utf8(&rest[..good]) {
                    count += text.chars().count() as u64;
                }
                match error.error_len() {
                    Some(len) => {
                        count += len as u64;
                        rest = &rest[good + len..];
                    }
                    None => {
                        carry.extend_from_slice(&rest[good..]);
                        break;
                    }
                }
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Wants {
        Wants {
            lines: true,
            words: true,
            chars: true,
            bytes: true,
            max: true,
        }
    }

    fn count(data: &[u8], wants: Wants) -> Counts {
        let mut reader: &[u8] = data;
        count_reader(&mut reader, wants).unwrap()
    }

    #[test]
    fn counts_lines_words_and_bytes() {
        let counts = count(b"hello world\nfoo\n", all());
        assert_eq!(
            (counts.lines, counts.words, counts.bytes, counts.chars),
            (2, 3, 16, 16)
        );
    }

    #[test]
    fn words_split_on_c_locale_space_only() {
        assert_eq!(count(b"a b\x0bc\x0cd\re\n", all()).words, 5);
        assert_eq!(count(b"a\x00b\n", all()).words, 1);
    }

    #[test]
    fn chars_count_utf8_and_invalid_bytes_once() {
        let wants = Wants {
            lines: false,
            words: false,
            chars: true,
            bytes: true,
            max: false,
        };
        let counts = count("é€\n".as_bytes(), wants);
        assert_eq!((counts.chars, counts.bytes), (3, 6));
        let invalid = count(b"\x80\xff x", wants);
        assert_eq!(invalid.chars, 4);
    }

    #[test]
    fn chars_survive_split_sequences_across_chunks() {
        let mut carry = Vec::new();
        let euro = "€".as_bytes();
        let first = count_chars(&mut carry, &euro[..1]);
        let second = count_chars(&mut carry, &euro[1..]);
        assert_eq!((first, second, carry.len()), (0, 1, 0));
    }

    #[test]
    fn max_line_length_expands_tabs_and_resets_on_cr() {
        assert_eq!(count(b"a\tb\n", all()).max, 9);
        assert_eq!(count(b"abcdef\rxy\n", all()).max, 6);
        assert_eq!(count(b"\x01ab\x7f\n", all()).max, 2);
    }

    #[test]
    fn lines_and_words_carry_across_chunks() {
        let mut scan = Scan::default();
        let wants = all();
        scan.feed(b"hel", wants);
        scan.feed(b"lo wo", wants);
        scan.feed(b"rld\n", wants);
        let counts = scan.finish(wants);
        assert_eq!((counts.words, counts.lines, counts.bytes), (2, 1, 12));
    }

    #[test]
    fn width_rules_match_gnu() {
        let plain = Wants {
            lines: true,
            words: false,
            chars: false,
            bytes: false,
            max: false,
        };
        assert_eq!(
            number_width(&[Kind::Regular(9999)], plain, TotalMode::Auto, 1),
            1
        );
        assert_eq!(
            number_width(&[Kind::Regular(16)], all(), TotalMode::Auto, 1),
            2
        );
        assert_eq!(number_width(&[Kind::Other], all(), TotalMode::Auto, 1), 7);
        assert_eq!(
            number_width(
                &[Kind::Regular(16), Kind::Regular(2)],
                plain,
                TotalMode::Auto,
                2
            ),
            2
        );
        assert_eq!(
            number_width(&[Kind::Regular(16)], all(), TotalMode::Only, 3),
            1
        );
    }

    #[test]
    fn parses_total_modes() {
        assert_eq!(parse_total("always"), Ok(TotalMode::Always));
        assert_eq!(parse_total("never"), Ok(TotalMode::Never));
        assert_eq!(parse_total("bogus"), Err(EXIT_FAIL));
    }

    #[test]
    fn format_row_pads_and_names() {
        let counts = Counts {
            lines: 2,
            words: 3,
            chars: 0,
            bytes: 16,
            max: 0,
        };
        let lwc = Wants {
            lines: true,
            words: true,
            chars: false,
            bytes: true,
            max: false,
        };
        assert_eq!(
            format_row(&counts, lwc, 2, Some("a.txt")),
            " 2  3 16 a.txt\n"
        );
        let lines = Wants {
            lines: true,
            words: false,
            chars: false,
            bytes: false,
            max: false,
        };
        assert_eq!(format_row(&counts, lines, 1, None), "2\n");
    }
}
