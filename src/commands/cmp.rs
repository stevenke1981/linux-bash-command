//! `cmp`: compares two files byte by byte. Reports the first difference (or every
//! difference with `-l`), or the end of the shorter input. Specification:
//! docs/spec/coreutils-alignment/04-commands-extra.md (cmp, diffutils 3.10).

use std::fs;
use std::io::{self, BufRead, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, open_input, stdout_writer};

const CMD: &str = "cmp";

const OPTS: &[OptSpec] = &[
    OptSpec::flag("print-bytes", Some('b'), Some("print-bytes")),
    OptSpec::req("ignore-initial", Some('i'), Some("ignore-initial")),
    OptSpec::flag("verbose", Some('l'), Some("verbose")),
    OptSpec::req("bytes", Some('n'), Some("bytes")),
    OptSpec::flag("quiet", Some('s'), Some("quiet")),
    OptSpec::flag("silent", None, Some("silent")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Parsed settings. Skips are in bytes; `limit` is `None` when unbounded.
#[derive(Debug, Default)]
struct Options {
    print_bytes: bool,
    verbose: bool,
    quiet: bool,
    limit: Option<u64>,
    skip: [u64; 2],
}

/// One input operand. `size` is known only for regular files and sets the width of the
/// byte-number column under `-l`.
struct Input {
    name: String,
    reader: Box<dyn BufRead>,
    size: Option<u64>,
}

/// A read failure, tagged with the operand it came from.
struct ReadError {
    name: String,
    error: io::Error,
}

/// What one step of the comparison produced.
enum Flow {
    /// Both chunks were compared; `true` stops the run.
    Advanced(bool),
    /// Input `index` ended first (both ended when the index is `None`).
    Ended(Option<usize>),
}

/// Entry point for `cmp` and the `agentcmd cmp` multi-call form.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, OPTS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage(CMD, &err, EXIT_TROUBLE),
    };
    let mut options = Options::default();
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => {
                if let Some(code) = apply_option(id, value, &mut options) {
                    return code;
                }
            }
        }
    }
    if operands.is_empty() {
        eprintln!("{CMD}: missing operand\nTry '{CMD} --help' for more information.");
        return EXIT_TROUBLE;
    }
    if operands.len() > 4 {
        eprintln!(
            "{CMD}: extra operand '{}'\nTry '{CMD} --help' for more information.",
            operands[4]
        );
        return EXIT_TROUBLE;
    }
    if let Err(code) = apply_operand_skips(&operands, &mut options) {
        return code;
    }
    if operands.len() == 1 {
        operands.push("-".to_string());
    }
    compare_operands(&options, &operands[0], &operands[1])
}

/// Applies one option. Returns `Some(code)` when the program must stop right away.
fn apply_option(id: &str, value: Option<String>, options: &mut Options) -> Option<i32> {
    match id {
        "help" => return Some(write_help()),
        "version" => return Some(print_version(CMD)),
        "print-bytes" => options.print_bytes = true,
        "verbose" => options.verbose = true,
        "quiet" | "silent" => options.quiet = true,
        "ignore-initial" => {
            let text = value.unwrap_or_default();
            match parse_skip_pair(&text) {
                Some(pair) => options.skip = pair,
                None => return Some(invalid_value("ignore-initial", &text)),
            }
        }
        "bytes" => {
            let text = value.unwrap_or_default();
            match parse_number(&text) {
                Some(limit) => options.limit = Some(limit),
                None => return Some(invalid_value("bytes", &text)),
            }
        }
        _ => {}
    }
    None
}

/// The optional SKIP1 and SKIP2 operands override `-i`.
fn apply_operand_skips(operands: &[String], options: &mut Options) -> Result<(), i32> {
    for (slot, text) in operands.iter().skip(2).take(2).enumerate() {
        match parse_number(text) {
            Some(skip) => options.skip[slot] = skip,
            None => return Err(invalid_value("ignore-initial", text)),
        }
    }
    Ok(())
}

fn invalid_value(option: &str, text: &str) -> i32 {
    eprintln!("{CMD}: invalid --{option} value '{text}'\nTry '{CMD} --help' for more information.");
    EXIT_TROUBLE
}

/// Parses `SKIP` or `SKIP1:SKIP2` as given to `-i`. A single value applies to both files.
fn parse_skip_pair(text: &str) -> Option<[u64; 2]> {
    match text.split_once(':') {
        Some((first, second)) => Some([parse_number(first)?, parse_number(second)?]),
        None => {
            let skip = parse_number(text)?;
            Some([skip, skip])
        }
    }
}

/// Parses a non-negative count with GNU's optional `0x` hex, leading-zero octal and size
/// suffix (`b`=512, `K`=1024, `KB`=1000, and the same for M, G, T, P, E).
fn parse_number(text: &str) -> Option<u64> {
    let (radix, digits) =
        if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            (16, hex)
        } else if text.len() > 1 && text.starts_with('0') {
            (8, &text[1..])
        } else {
            (10, text)
        };
    let split = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    let (number, suffix) = digits.split_at(split);
    if number.is_empty() {
        return None;
    }
    let value = u64::from_str_radix(number, radix).ok()?;
    value.checked_mul(suffix_factor(suffix)?)
}

fn suffix_factor(suffix: &str) -> Option<u64> {
    const K: u64 = 1024;
    const KB: u64 = 1000;
    Some(match suffix {
        "" => 1,
        "b" => 512,
        "K" | "k" | "KiB" => K,
        "KB" | "kB" => KB,
        "M" | "MiB" => K.pow(2),
        "MB" => KB.pow(2),
        "G" | "GiB" => K.pow(3),
        "GB" => KB.pow(3),
        "T" | "TiB" => K.pow(4),
        "TB" => KB.pow(4),
        "P" | "PiB" => K.pow(5),
        "PB" => KB.pow(5),
        "E" | "EiB" => K.pow(6),
        "EB" => KB.pow(6),
        _ => return None,
    })
}

fn write_help() -> i32 {
    let mut out = stdout_writer();
    if out.write_all(HELP.as_bytes()).is_err() {
        return EXIT_OK;
    }
    finish_output(CMD, &mut out)
}

fn compare_operands(options: &Options, first: &str, second: &str) -> i32 {
    if first == "-" && second == "-" && options.skip[0] == options.skip[1] {
        // The same stream compared with itself.
        return EXIT_OK;
    }
    let left = match open_operand(first) {
        Ok(input) => input,
        Err(code) => return code,
    };
    let right = match open_operand(second) {
        Ok(input) => input,
        Err(code) => return code,
    };
    let mut inputs = [left, right];
    let mut out = stdout_writer();
    let status = match compare_streams(options, &mut inputs, &mut out) {
        Ok(status) => status,
        Err(failure) => {
            eprintln!("{CMD}: {}: {}", failure.name, io_error_text(&failure.error));
            EXIT_TROUBLE
        }
    };
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    status
}

fn open_operand(name: &str) -> Result<Input, i32> {
    let size = if name == "-" || name == "/dev/stdin" {
        None
    } else {
        fs::metadata(name)
            .ok()
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
    };
    match open_input(name) {
        Ok(reader) => Ok(Input {
            name: name.to_string(),
            reader,
            size,
        }),
        Err(error) => {
            eprintln!("{CMD}: {name}: {}", io_error_text(&error));
            Err(EXIT_TROUBLE)
        }
    }
}

fn compare_streams(
    options: &Options,
    inputs: &mut [Input; 2],
    out: &mut dyn Write,
) -> Result<i32, ReadError> {
    skip_bytes(&mut inputs[0], options.skip[0])?;
    skip_bytes(&mut inputs[1], options.skip[1])?;
    let width = column_width(
        [inputs[0].size, inputs[1].size],
        options.skip,
        options.limit,
    );
    let names = (inputs[0].name.clone(), inputs[1].name.clone());
    let mut state = Compare::new(options, width, names);
    loop {
        if state.limit_reached() {
            break;
        }
        match advance(&mut state, inputs, out)? {
            Flow::Advanced(true) => break,
            Flow::Advanced(false) => {}
            Flow::Ended(None) => break,
            Flow::Ended(Some(index)) => {
                state.report_eof(&inputs[index].name);
                return Ok(EXIT_FAIL);
            }
        }
    }
    Ok(if state.differed { EXIT_FAIL } else { EXIT_OK })
}

/// Compares the next buffered chunk of both inputs and consumes what was compared.
fn advance(
    state: &mut Compare<'_>,
    inputs: &mut [Input; 2],
    out: &mut dyn Write,
) -> Result<Flow, ReadError> {
    let [left, right] = inputs;
    let chunk_left = fill(left)?;
    let chunk_right = fill(right)?;
    if chunk_left.is_empty() || chunk_right.is_empty() {
        return Ok(match (chunk_left.is_empty(), chunk_right.is_empty()) {
            (true, true) => Flow::Ended(None),
            (true, false) => Flow::Ended(Some(0)),
            _ => Flow::Ended(Some(1)),
        });
    }
    let len = state.clamp(chunk_left.len().min(chunk_right.len()));
    let (stop, used) = state.feed(&chunk_left[..len], &chunk_right[..len], out);
    left.reader.consume(used);
    right.reader.consume(used);
    Ok(Flow::Advanced(stop))
}

/// Returns the buffered bytes of `input`, tagging a read failure with its name.
fn fill(input: &mut Input) -> Result<&[u8], ReadError> {
    let Input { name, reader, .. } = input;
    reader.fill_buf().map_err(|error| ReadError {
        name: name.clone(),
        error,
    })
}

/// Discards `count` bytes. Stops early at end of input, which the comparison will see.
fn skip_bytes(input: &mut Input, count: u64) -> Result<(), ReadError> {
    let mut remaining = count;
    while remaining > 0 {
        let available = fill(input)?.len();
        if available == 0 {
            break;
        }
        let step = available.min(usize::try_from(remaining).unwrap_or(usize::MAX));
        input.reader.consume(step);
        remaining -= step as u64;
    }
    Ok(())
}

/// Width of the byte-number column under `-l`: the digits of the smallest of the remaining
/// sizes of the regular-file inputs and the `-n` limit, or 1 when none is known.
fn column_width(sizes: [Option<u64>; 2], skip: [u64; 2], limit: Option<u64>) -> usize {
    let remaining = sizes
        .iter()
        .zip(skip)
        .filter_map(|(size, skip)| size.map(|size| size.saturating_sub(skip)))
        .chain(limit);
    match remaining.min() {
        Some(smallest) => smallest.to_string().len(),
        None => 1,
    }
}

/// Comparison state carried across chunks.
struct Compare<'a> {
    options: &'a Options,
    width: usize,
    names: (String, String),
    /// Bytes compared so far (after skipping); the 1-based position of the last one.
    position: u64,
    /// 1 plus the number of newlines among the equal bytes compared so far.
    line: u64,
    /// The last byte compared (equal in both inputs).
    last: Option<u8>,
    differed: bool,
}

impl<'a> Compare<'a> {
    fn new(options: &'a Options, width: usize, names: (String, String)) -> Self {
        Self {
            options,
            width,
            names,
            position: 0,
            line: 1,
            last: None,
            differed: false,
        }
    }

    fn limit_reached(&self) -> bool {
        self.options
            .limit
            .is_some_and(|limit| self.position >= limit)
    }

    /// Caps a chunk length at the remaining `-n` budget.
    fn clamp(&self, len: usize) -> usize {
        match self.options.limit {
            Some(limit) => {
                let left = usize::try_from(limit - self.position).unwrap_or(usize::MAX);
                len.min(left)
            }
            None => len,
        }
    }

    /// Compares two equal-length chunks. Returns whether to stop, and how many bytes were used.
    fn feed(&mut self, xs: &[u8], ys: &[u8], out: &mut dyn Write) -> (bool, usize) {
        let mut start = 0;
        while start < xs.len() {
            let (rest_x, rest_y) = (&xs[start..], &ys[start..]);
            let equal = rest_x
                .iter()
                .zip(rest_y)
                .position(|(x, y)| x != y)
                .unwrap_or(rest_x.len());
            self.count_equal(&rest_x[..equal]);
            start += equal;
            if start == xs.len() {
                break;
            }
            let (x, y) = (xs[start], ys[start]);
            self.position += 1;
            self.differed = true;
            if self.options.quiet || !self.options.verbose {
                if !self.options.quiet {
                    self.report_first_difference(x, y, out);
                }
                return (true, start + 1);
            }
            self.report_verbose_difference(x, y, out);
            start += 1;
        }
        (false, xs.len())
    }

    fn count_equal(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.position += bytes.len() as u64;
        self.line += bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
        self.last = bytes.last().copied();
    }

    /// The plain message: `a b differ: byte N, line M`, plus the two byte values with `-b`.
    fn report_first_difference(&self, x: u8, y: u8, out: &mut dyn Write) {
        let mut text = format!(
            "{} {} differ: byte {}, line {}",
            self.names.0, self.names.1, self.position, self.line
        );
        if self.options.print_bytes {
            text.push_str(&format!(
                " is {:>3} {} {:>3} {}",
                octal(x),
                printable(x),
                octal(y),
                printable(y)
            ));
        }
        let _ = writeln!(out, "{text}");
    }

    /// One `-l` line: the byte number, then the octal values (and characters with `-b`).
    fn report_verbose_difference(&self, x: u8, y: u8, out: &mut dyn Write) {
        let number = self.position;
        let width = self.width;
        let line = if self.options.print_bytes {
            format!(
                "{number:>width$} {:>3} {:<4} {:>3} {}",
                octal(x),
                printable(x),
                octal(y),
                printable(y)
            )
        } else {
            format!("{number:>width$} {:>3} {:>3}", octal(x), octal(y))
        };
        let _ = writeln!(out, "{line}");
    }

    /// The end-of-input message. An input with nothing compared is "which is empty".
    fn report_eof(&self, ended: &str) {
        if self.options.quiet {
            return;
        }
        if self.position == 0 {
            eprintln!("{CMD}: EOF on {ended} which is empty");
            return;
        }
        let suffix = if self.options.verbose {
            String::new()
        } else {
            match self.last {
                Some(b'\n') => format!(", line {}", self.line - 1),
                _ => format!(", in line {}", self.line),
            }
        };
        eprintln!("{CMD}: EOF on {ended} after byte {}{suffix}", self.position);
    }
}

fn octal(byte: u8) -> String {
    format!("{byte:o}")
}

/// GNU's visible form of a byte: control bytes as `^X`, high bytes with an `M-` prefix.
fn printable(byte: u8) -> String {
    let (prefix, low) = if byte >= 0x80 {
        ("M-", byte - 0x80)
    } else {
        ("", byte)
    };
    let body = match low {
        0x7f => "^?".to_string(),
        0..=0x1f => format!("^{}", char::from(low + b'@')),
        _ => char::from(low).to_string(),
    };
    format!("{prefix}{body}")
}

const HELP: &str = "\
Usage: cmp [OPTION]... FILE1 [FILE2 [SKIP1 [SKIP2]]]
Compare two files byte by byte.

The optional SKIP1 and SKIP2 specify the bytes to skip
in each file before comparing.
If FILE1 or FILE2 is '-' or missing, read standard input.

  -b, --print-bytes          print differing bytes
  -i, --ignore-initial=SKIP  skip first SKIP bytes of both inputs
  -i, --ignore-initial=SKIP1:SKIP2
                             skip first SKIP1 bytes of FILE1 and
                             first SKIP2 bytes of FILE2
  -l, --verbose              output byte numbers and values of all
                             differing bytes
  -n, --bytes=LIMIT          compare at most LIMIT bytes
  -s, --quiet, --silent      suppress all normal output
      --help                 display this help and exit
      --version              output version information and exit

SKIP and LIMIT values may be followed by these multiplicative suffixes:
b 512, K 1024, KB 1000, M 1024*1024, MB 1000*1000, and so on for G, T, P, E.

Exit status is 0 if inputs are the same, 1 if different, 2 if trouble.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> (String, String) {
        ("a".to_string(), "b".to_string())
    }

    #[test]
    fn parse_number_accepts_hex_octal_and_suffixes() {
        assert_eq!(parse_number("42"), Some(42));
        assert_eq!(parse_number("0x1f"), Some(31));
        assert_eq!(parse_number("010"), Some(8));
        assert_eq!(parse_number("0"), Some(0));
        assert_eq!(parse_number("2b"), Some(1024));
        assert_eq!(parse_number("1KB"), Some(1000));
        assert_eq!(parse_number("1K"), Some(1024));
        assert_eq!(parse_number("1MB"), Some(1_000_000));
    }

    #[test]
    fn parse_number_rejects_signs_and_unknown_suffixes() {
        assert_eq!(parse_number("-1"), None);
        assert_eq!(parse_number("1q"), None);
        assert_eq!(parse_number("K"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn skip_pair_splits_on_colon_or_duplicates_a_single_value() {
        assert_eq!(parse_skip_pair("3"), Some([3, 3]));
        assert_eq!(parse_skip_pair("1:2"), Some([1, 2]));
        assert_eq!(parse_skip_pair("1:2:3"), None);
    }

    #[test]
    fn printable_uses_caret_and_meta_notation() {
        assert_eq!(printable(b'a'), "a");
        assert_eq!(printable(0x0a), "^J");
        assert_eq!(printable(0x7f), "^?");
        assert_eq!(printable(0xa0), "M- ");
        assert_eq!(printable(0xc1), "M-A");
    }

    #[test]
    fn column_width_uses_smallest_known_remaining_size() {
        assert_eq!(column_width([Some(1000), Some(99)], [0, 0], None), 2);
        assert_eq!(column_width([Some(1000), Some(1000)], [400, 400], None), 3);
        assert_eq!(column_width([None, None], [0, 0], None), 1);
        assert_eq!(column_width([Some(12), Some(12)], [0, 0], Some(6)), 1);
    }

    #[test]
    fn equal_chunks_report_nothing_and_count_lines() {
        let options = Options::default();
        let mut state = Compare::new(&options, 1, names());
        let mut out = Vec::new();
        let (stop, used) = state.feed(b"a\nb\n", b"a\nb\n", &mut out);
        assert!(!stop);
        assert_eq!(used, 4);
        assert!(out.is_empty());
        assert_eq!(state.line, 3);
        assert!(!state.differed);
    }

    #[test]
    fn first_difference_reports_position_and_line() {
        let options = Options::default();
        let mut state = Compare::new(&options, 1, names());
        let mut out = Vec::new();
        let (stop, _) = state.feed(b"abc\nxef\n", b"abc\ndef\n", &mut out);
        assert!(stop);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "a b differ: byte 5, line 2\n"
        );
    }

    #[test]
    fn verbose_mode_lists_every_difference() {
        let options = Options {
            verbose: true,
            print_bytes: true,
            ..Options::default()
        };
        let mut state = Compare::new(&options, 2, names());
        let mut out = Vec::new();
        let (stop, _) = state.feed(b"a\x0ab", b"a\x0bc", &mut out);
        assert!(!stop);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            " 2  12 ^J    13 ^K\n 3 142 b    143 c\n"
        );
    }
}
