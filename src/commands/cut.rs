//! `cut`: select bytes or delimiter-separated fields from each line.
//!
//! `-c` is treated as `-b`, as in GNU coreutils 9.4 (byte semantics in the C locale).

use std::io::{self, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const CMD: &str = "cut";

const SPECS: &[OptSpec] = &[
    OptSpec::req("bytes", Some('b'), Some("bytes")),
    OptSpec::req("chars", Some('c'), Some("characters")),
    OptSpec::req("fields", Some('f'), Some("fields")),
    OptSpec::req("delimiter", Some('d'), Some("delimiter")),
    OptSpec::flag("complement", None, Some("complement")),
    OptSpec::flag("only", Some('s'), Some("only-delimited")),
    OptSpec::req("output", None, Some("output-delimiter")),
    OptSpec::flag("zero", Some('z'), Some("zero-terminated")),
    OptSpec::flag("n", Some('n'), None),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: cut OPTION... [FILE]...\n\
Print selected parts of lines from each FILE to standard output. With no FILE,\n\
or when FILE is -, read standard input.\n\n\
  -b, --bytes=LIST        select only these bytes\n\
  -c, --characters=LIST   select only these bytes (same as -b in the C locale)\n\
  -d, --delimiter=DELIM   use DELIM instead of TAB for field delimiter (one byte)\n\
  -f, --fields=LIST       select only these fields; also print any line that\n\
                          contains no delimiter character, unless -s is given\n\
  -n                      accepted for compatibility; has no effect\n\
  -s, --only-delimited    do not print lines not containing delimiters\n\
      --complement        complement the set of selected bytes, characters or fields\n\
      --output-delimiter=STRING\n\
                          use STRING as the output delimiter (default: the input\n\
                          delimiter); with -b or -c, it separates non-adjacent ranges\n\
  -z, --zero-terminated   line delimiter is NUL, not newline\n\
      --help              display this help and exit\n\
      --version           output version information and exit\n\n\
Each LIST is made up of one range, or many ranges separated by commas.\n\
A range is N, N-, -M, or N-M, counted from 1. Ranges may overlap; output\n\
keeps the input order.\n\
Exit status: 0 on success, 1 if an error occurred.\n";

/// The selected positions: 1-based inclusive ranges, sorted and merged.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ranges(Vec<(u64, u64)>);

impl Ranges {
    fn contains(&self, pos: u64) -> bool {
        self.0.iter().any(|&(lo, hi)| lo <= pos && pos <= hi)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Bytes,
    Fields,
}

#[derive(Debug)]
struct Config {
    mode: Mode,
    ranges: Ranges,
    complement: bool,
    delim: u8,
    only_delimited: bool,
    out_delim: Option<Vec<u8>>,
    terminator: u8,
}

impl Config {
    fn selected(&self, pos: u64) -> bool {
        self.ranges.contains(pos) != self.complement
    }
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    if let Some(code) = early_option(&items) {
        return code;
    }
    let config = match build_config(&items) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let files = operands(&items);
    run_files(&config, &files)
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

fn operands(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(name) => Some(name.clone()),
            Item::Opt { .. } => None,
        })
        .collect()
}

/// Settings collected from the options, before validation.
#[derive(Default)]
struct Raw {
    mode: Option<Mode>,
    list: Option<String>,
    delim: Option<String>,
    complement: bool,
    only: bool,
    output: Option<String>,
    zero: bool,
}

fn build_config(items: &[Item]) -> Result<Config, i32> {
    let mut raw = Raw::default();
    for item in items {
        if let Item::Opt { id, value } = item {
            collect_option(&mut raw, id, value.as_deref())?;
        }
    }
    let mode = raw
        .mode
        .ok_or_else(|| usage_error("you must specify a list of bytes, characters, or fields"))?;
    let list = raw.list.unwrap_or_default();
    let ranges = parse_list(&list, mode)?;
    let delim = resolve_delim(raw.delim.as_deref(), mode, raw.only)?;
    let terminator = if raw.zero { 0 } else { b'\n' };
    Ok(Config {
        mode,
        ranges,
        complement: raw.complement,
        delim: delim.unwrap_or(b'\t'),
        only_delimited: raw.only,
        out_delim: raw.output.map(String::into_bytes),
        terminator,
    })
}

fn collect_option(raw: &mut Raw, id: &str, value: Option<&str>) -> Result<(), i32> {
    let text = value.unwrap_or_default().to_owned();
    match id {
        "bytes" | "chars" => set_mode(raw, Mode::Bytes, text)?,
        "fields" => set_mode(raw, Mode::Fields, text)?,
        "delimiter" => raw.delim = Some(text),
        "complement" => raw.complement = true,
        "only" => raw.only = true,
        "output" => raw.output = Some(text),
        "zero" => raw.zero = true,
        _ => {}
    }
    Ok(())
}

fn set_mode(raw: &mut Raw, mode: Mode, list: String) -> Result<(), i32> {
    if raw.mode.is_some() {
        return Err(usage_error("only one list may be specified"));
    }
    raw.mode = Some(mode);
    raw.list = Some(list);
    Ok(())
}

/// Validates `-d` and `-s` against the list kind and returns the delimiter byte if given.
fn resolve_delim(delim: Option<&str>, mode: Mode, only: bool) -> Result<Option<u8>, i32> {
    if mode == Mode::Bytes {
        if delim.is_some() {
            return Err(usage_error(
                "an input delimiter may be specified only when operating on fields",
            ));
        }
        if only {
            return Err(usage_error(
                "suppressing non-delimited lines makes sense\n\tonly when operating on fields",
            ));
        }
    }
    match delim {
        None => Ok(None),
        Some("") => Ok(Some(0)),
        Some(text) if text.len() == 1 => Ok(Some(text.as_bytes()[0])),
        Some(_) => Err(usage_error("the delimiter must be a single character")),
    }
}

fn usage_error(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

/// Parses a LIST such as `1,3-5,7-` into merged ranges.
fn parse_list(list: &str, mode: Mode) -> Result<Ranges, i32> {
    let mut ranges = Vec::new();
    for item in list.split(',') {
        ranges.push(parse_item(item, mode)?);
    }
    Ok(Ranges(merge(ranges)))
}

fn parse_item(item: &str, mode: Mode) -> Result<(u64, u64), i32> {
    if item.is_empty() {
        return Err(usage_error("invalid byte or field list"));
    }
    if item == "-" {
        return Err(usage_error("invalid range with no endpoint: -"));
    }
    match item.split_once('-') {
        None => {
            let pos = parse_position(item, mode)?;
            Ok((pos, pos))
        }
        Some(("", end)) => Ok((1, parse_position(end, mode)?)),
        Some((start, "")) => Ok((parse_position(start, mode)?, u64::MAX)),
        Some((start, end)) => {
            let lo = parse_position(start, mode)?;
            let hi = parse_position(end, mode)?;
            if lo > hi {
                return Err(usage_error("invalid decreasing range"));
            }
            Ok((lo, hi))
        }
    }
}

fn parse_position(text: &str, mode: Mode) -> Result<u64, i32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        let shown = match mode {
            Mode::Bytes => "byte/character position",
            Mode::Fields => "field value",
        };
        return Err(usage_error(&format!("invalid {shown} '{text}'")));
    }
    let value = text.parse::<u64>().map_err(|_| {
        usage_error(&format!(
            "{} '{text}' is too large",
            match mode {
                Mode::Bytes => "byte/character offset",
                Mode::Fields => "field number",
            }
        ))
    })?;
    if value == 0 {
        let message = match mode {
            Mode::Bytes => "byte/character positions are numbered from 1",
            Mode::Fields => "fields are numbered from 1",
        };
        return Err(usage_error(message));
    }
    Ok(value)
}

fn merge(mut ranges: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    for (lo, hi) in ranges {
        match merged.last_mut() {
            Some(last) if lo <= last.1.saturating_add(1) => last.1 = last.1.max(hi),
            _ => merged.push((lo, hi)),
        }
    }
    merged
}

/// Result of cutting one input.
enum Outcome {
    Done,
    /// An input could not be read; processing continues with the next one.
    FileFailed,
    /// Output failed; processing stops with this status.
    Stop(i32),
}

fn run_files(config: &Config, files: &[String]) -> i32 {
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    let default = ["-".to_owned()];
    let names: &[String] = if files.is_empty() { &default } else { files };
    for name in names {
        match cut_one(config, name, &mut out) {
            Outcome::Done => {}
            Outcome::FileFailed => status = EXIT_FAIL,
            Outcome::Stop(code) => return code,
        }
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed == EXIT_OK {
        status
    } else {
        flushed
    }
}

fn cut_one(config: &Config, name: &str, out: &mut impl Write) -> Outcome {
    let mut reader = match open_input(name) {
        Ok(reader) => reader,
        Err(error) => {
            report(CMD, name, &error);
            return Outcome::FileFailed;
        }
    };
    let mut record = Vec::new();
    let mut line = Vec::new();
    loop {
        match read_record(reader.as_mut(), config.terminator, &mut record) {
            Ok(false) => return Outcome::Done,
            Ok(true) => {}
            Err(error) => {
                report(CMD, name, &error);
                return Outcome::FileFailed;
            }
        }
        line.clear();
        if !cut_record(config, &record, &mut line) {
            continue;
        }
        if let Err(error) = out.write_all(&line) {
            return Outcome::Stop(write_failed(&error));
        }
    }
}

fn write_failed(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_FAIL
}

/// Builds the output for one record, terminator included. Returns `false` when the record
/// is suppressed (`-s` on a line without a delimiter).
fn cut_record(config: &Config, record: &[u8], out: &mut Vec<u8>) -> bool {
    let body = record.strip_suffix(&[config.terminator]).unwrap_or(record);
    let emit = match config.mode {
        Mode::Bytes => {
            cut_bytes(config, body, out);
            true
        }
        Mode::Fields => cut_fields(config, body, out),
    };
    if emit {
        out.push(config.terminator);
    }
    emit
}

fn cut_bytes(config: &Config, body: &[u8], out: &mut Vec<u8>) {
    let mut last: Option<u64> = None;
    for (index, &byte) in body.iter().enumerate() {
        let pos = index as u64 + 1;
        if !config.selected(pos) {
            continue;
        }
        if let (Some(prev), Some(sep)) = (last, config.out_delim.as_deref()) {
            if prev + 1 != pos {
                out.extend_from_slice(sep);
            }
        }
        out.push(byte);
        last = Some(pos);
    }
}

/// Returns `false` when the line must be suppressed. A line without a delimiter is copied
/// unchanged (the terminator is added by the caller).
fn cut_fields(config: &Config, body: &[u8], out: &mut Vec<u8>) -> bool {
    if !body.contains(&config.delim) {
        if config.only_delimited {
            return false;
        }
        out.extend_from_slice(body);
        return true;
    }
    let sep: Vec<u8> = config
        .out_delim
        .clone()
        .unwrap_or_else(|| vec![config.delim]);
    let mut first = true;
    for (index, field) in body.split(|&b| b == config.delim).enumerate() {
        if !config.selected(index as u64 + 1) {
            continue;
        }
        if !first {
            out.extend_from_slice(&sep);
        }
        out.extend_from_slice(field);
        first = false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(mode: Mode, list: &str) -> Config {
        Config {
            mode,
            ranges: parse_list(list, mode).unwrap(),
            complement: false,
            delim: b'\t',
            only_delimited: false,
            out_delim: None,
            terminator: b'\n',
        }
    }

    fn cut_line(config: &Config, record: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        cut_record(config, record, &mut out);
        out
    }

    #[test]
    fn merges_and_orders_ranges() {
        assert_eq!(
            parse_list("3,1-2,5-", Mode::Bytes).unwrap(),
            Ranges(vec![(1, 3), (5, u64::MAX)])
        );
        assert_eq!(
            parse_list("-2", Mode::Fields).unwrap(),
            Ranges(vec![(1, 2)])
        );
    }

    #[test]
    fn rejects_bad_lists() {
        assert!(parse_list("0", Mode::Bytes).is_err());
        assert!(parse_list("3-1", Mode::Fields).is_err());
        assert!(parse_list("x", Mode::Fields).is_err());
        assert!(parse_list("-", Mode::Bytes).is_err());
        assert!(parse_list("1,,2", Mode::Bytes).is_err());
        assert!(parse_list("99999999999999999999", Mode::Bytes).is_err());
    }

    #[test]
    fn bytes_respect_complement_and_output_delimiter() {
        let mut cfg = config(Mode::Bytes, "1,3");
        assert_eq!(cut_line(&cfg, b"abcdef\n"), b"ac\n");
        cfg.out_delim = Some(b":".to_vec());
        assert_eq!(cut_line(&cfg, b"abcdef\n"), b"a:c\n");
        cfg.complement = true;
        assert_eq!(cut_line(&cfg, b"abc\n"), b"b\n");
    }

    #[test]
    fn fields_keep_lines_without_delimiter_unless_suppressed() {
        let mut cfg = config(Mode::Fields, "2");
        cfg.delim = b',';
        assert_eq!(cut_line(&cfg, b"a,b,c\n"), b"b\n");
        assert_eq!(cut_line(&cfg, b"plain\n"), b"plain\n");
        cfg.only_delimited = true;
        assert!(!cut_record(&cfg, b"plain\n", &mut Vec::new()));
    }

    #[test]
    fn fields_handle_empty_fields_and_complement() {
        let mut cfg = config(Mode::Fields, "1,3");
        cfg.delim = b',';
        assert_eq!(cut_line(&cfg, b",x,\n"), b",\n");
        cfg.complement = true;
        assert_eq!(cut_line(&cfg, b"a,b,c\n"), b"b\n");
    }

    #[test]
    fn terminator_is_added_when_missing() {
        let cfg = config(Mode::Bytes, "1");
        assert_eq!(cut_line(&cfg, b"xyz"), b"x\n");
    }

    #[test]
    fn empty_delimiter_means_nul() {
        assert_eq!(resolve_delim(Some(""), Mode::Fields, false), Ok(Some(0)));
        assert!(resolve_delim(Some("ab"), Mode::Fields, false).is_err());
        assert!(resolve_delim(Some(","), Mode::Bytes, false).is_err());
    }

    #[test]
    fn merge_joins_adjacent_ranges() {
        assert_eq!(merge(vec![(1, 2), (3, 4)]), vec![(1, 4)]);
    }
}
