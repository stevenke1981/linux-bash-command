//! `join`: merge the lines of two sorted files on a common field.
//!
//! The merge reads one line ahead on each side, so an out-of-order line is noticed while the
//! preceding group of equal keys is being collected, as GNU `join` does. Without
//! `--nocheck-order` a disorder is reported; with `--check-order` it stops the run at once.

use std::cmp::Ordering;
use std::io::{self, BufRead, BufWriter, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const CMD: &str = "join";

const SPECS: &[OptSpec] = &[
    OptSpec::req("unpaired", Some('a'), None),
    OptSpec::req("empty", Some('e'), None),
    OptSpec::flag("ignore", Some('i'), Some("ignore-case")),
    OptSpec::req("key", Some('j'), None),
    OptSpec::req("format", Some('o'), None),
    OptSpec::req("separator", Some('t'), None),
    OptSpec::req("key1", Some('1'), None),
    OptSpec::req("key2", Some('2'), None),
    OptSpec::req("only", Some('v'), None),
    OptSpec::flag("check", None, Some("check-order")),
    OptSpec::flag("nocheck", None, Some("nocheck-order")),
    OptSpec::flag("header", None, Some("header")),
    OptSpec::flag("zero", Some('z'), Some("zero-terminated")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = r"Usage: join [OPTION]... FILE1 FILE2
For each pair of input lines with identical join fields, write a line to
standard output.  The default join field is the first, delimited by blanks.

When FILE1 or FILE2 (not both) is -, read standard input.

  -a FILENUM        also print unpairable lines from file FILENUM, where
                      FILENUM is 1 or 2, corresponding to FILE1 or FILE2
  -e EMPTY          replace missing input fields with EMPTY
  -i, --ignore-case ignore differences in case when comparing fields
  -j FIELD          equivalent to '-1 FIELD -2 FIELD'
  -o FORMAT         obey FORMAT while constructing output line
  -t CHAR           use CHAR as input and output field separator
  -v FILENUM        like -a FILENUM, but suppress joined output lines
  -1 FIELD          join on this FIELD of file 1
  -2 FIELD          join on this FIELD of file 2
  --check-order     check that the input is correctly sorted, even
                      if all input lines are pairable
  --nocheck-order   do not check that the input is correctly sorted
  --header          treat the first line in each file as field headers,
                      print them without trying to pair them
  -z, --zero-terminated     line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

Unless -t CHAR is given, leading blanks separate fields and are ignored,
else fields are separated by CHAR.  Any FIELD is a positive number.
FORMAT is one or more comma or blank separated specifications: FILENUM.FIELD,
or 0 for the join field, or 'auto' to use the fields of the first line of
each file.
";

/// One item of an `-o` list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spec {
    /// `0`: the join field.
    Key,
    /// `FILENUM.FIELD`, with the file as an index (0 or 1) and the field as a 0-based index.
    Field(usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Format {
    /// No `-o`: join field, then the rest of each line.
    Default,
    /// `-o auto`: the join field, then as many fields as the first line of each file has.
    Auto,
    /// An explicit list.
    List(Vec<Spec>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Default,
    Strict,
    Off,
}

struct Config {
    keys: [usize; 2],
    separator: Option<u8>,
    /// Print unpaired lines of each file (`-a`, `-v`).
    unpaired: [bool; 2],
    /// Print joined lines (false with `-v`).
    pairs: bool,
    ignore_case: bool,
    empty: Vec<u8>,
    format: Format,
    delim: u8,
    check: Check,
    header: bool,
}

/// One input line split into fields.
#[derive(Debug)]
struct Record {
    fields: Vec<Vec<u8>>,
    line_number: u64,
    text: Vec<u8>,
}

/// One input file with its look-behind state for the order check.
struct Side {
    name: String,
    reader: Box<dyn BufRead>,
    previous: Option<Vec<u8>>,
    lines: u64,
    reported: bool,
    disorder: bool,
    /// Field count of the first line, for `-o auto`.
    auto_count: Option<usize>,
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
    let config = match build_config(&items) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let files: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(text) => Some(text.as_str()),
            Item::Opt { .. } => None,
        })
        .collect();
    if files.len() < 2 {
        let message = match files.first() {
            Some(first) => format!("missing operand after '{first}'"),
            None => "missing operand".to_owned(),
        };
        eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
        return EXIT_FAIL;
    }
    if files.len() > 2 {
        eprintln!(
            "{CMD}: extra operand '{}'\nTry '{CMD} --help' for more information.",
            files[2]
        );
        return EXIT_FAIL;
    }
    if files[0] == "-" && files[1] == "-" {
        eprintln!("{CMD}: both files cannot be standard input");
        return EXIT_FAIL;
    }
    let mut sides = Vec::new();
    for name in files {
        match open_input(name) {
            Ok(reader) => sides.push(Side::new(name, reader)),
            Err(error) => {
                eprintln!("{CMD}: {name}: {}", io_error_text(&error));
                return EXIT_FAIL;
            }
        }
    }
    let mut out = stdout_writer();
    let mut right = sides.pop().expect("two sides");
    let mut left = sides.pop().expect("two sides");
    match merge(&config, &mut left, &mut right, &mut out) {
        Ok(()) => {}
        Err(Stop::Status(code)) => return code,
        Err(Stop::Write(error)) => {
            if is_broken_pipe(&error) {
                return EXIT_OK;
            }
            eprintln!("{CMD}: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
    }
    if left.disorder || right.disorder {
        eprintln!("{CMD}: input is not in sorted order");
        let _ = finish_output(CMD, &mut out);
        return EXIT_FAIL;
    }
    finish_output(CMD, &mut out)
}

/// Why the merge stopped early: an exit status already reported, or a write failure.
enum Stop {
    Status(i32),
    Write(io::Error),
}

impl From<io::Error> for Stop {
    fn from(error: io::Error) -> Self {
        Stop::Write(error)
    }
}

impl Side {
    fn new(name: &str, reader: Box<dyn BufRead>) -> Self {
        Self {
            name: name.to_owned(),
            reader,
            previous: None,
            lines: 0,
            reported: false,
            disorder: false,
            auto_count: None,
        }
    }

    /// Reads the next record without any order check.
    fn read_raw(&mut self, delim: u8, separator: Option<u8>) -> io::Result<Option<Record>> {
        let mut buffer = Vec::new();
        if !read_record(self.reader.as_mut(), delim, &mut buffer)? {
            return Ok(None);
        }
        if buffer.last() == Some(&delim) {
            buffer.pop();
        }
        self.lines += 1;
        let fields = split_fields(&buffer, separator);
        if self.auto_count.is_none() {
            self.auto_count = Some(fields.len().saturating_sub(1));
        }
        Ok(Some(Record {
            fields,
            line_number: self.lines,
            text: buffer,
        }))
    }
}

fn build_config(items: &[Item]) -> Result<Config, String> {
    let mut config = Config {
        keys: [0, 0],
        separator: None,
        unpaired: [false, false],
        pairs: true,
        ignore_case: false,
        empty: Vec::new(),
        format: Format::Default,
        delim: b'\n',
        check: Check::Default,
        header: false,
    };
    for item in items {
        let Item::Opt { id, value } = item else {
            continue;
        };
        let value = value.as_deref().unwrap_or("");
        match *id {
            "unpaired" => config.unpaired[file_number(value)?] = true,
            "only" => {
                let file = file_number(value)?;
                config.unpaired[file] = true;
                config.pairs = false;
            }
            "empty" => config.empty = value.as_bytes().to_vec(),
            "ignore" => config.ignore_case = true,
            "key" => {
                let field = field_number(value)?;
                config.keys = [field, field];
            }
            "key1" => config.keys[0] = field_number(value)?,
            "key2" => config.keys[1] = field_number(value)?,
            "format" => config.format = parse_format(value)?,
            "separator" => config.separator = Some(parse_separator(value)?),
            "check" => config.check = Check::Strict,
            "nocheck" => config.check = Check::Off,
            "header" => config.header = true,
            "zero" => config.delim = 0,
            _ => {}
        }
    }
    Ok(config)
}

/// A `-a`/`-v` argument: 1 or 2, returned as an index.
fn file_number(value: &str) -> Result<usize, String> {
    match value {
        "1" => Ok(0),
        "2" => Ok(1),
        _ => Err(format!("invalid field number: '{value}'")),
    }
}

/// A `-1`/`-2`/`-j` argument: a positive field number, returned as a 0-based index.
fn field_number(value: &str) -> Result<usize, String> {
    match value.parse::<usize>() {
        Ok(number) if number > 0 => Ok(number - 1),
        _ => Err(format!("invalid field number: '{value}'")),
    }
}

fn parse_separator(value: &str) -> Result<u8, String> {
    match value.as_bytes() {
        [] => Ok(b'\n'),
        [byte] => Ok(*byte),
        _ => Err(format!("multi-character tab '{value}'")),
    }
}

fn parse_format(value: &str) -> Result<Format, String> {
    if value == "auto" {
        return Ok(Format::Auto);
    }
    let mut specs = Vec::new();
    for token in value.split(|c: char| c == ',' || c.is_whitespace()) {
        if token.is_empty() {
            continue;
        }
        if token == "0" {
            specs.push(Spec::Key);
            continue;
        }
        let Some((file, field)) = token.split_once('.') else {
            return Err(format!("invalid field number: '{token}'"));
        };
        let file = match file {
            "1" => 0,
            "2" => 1,
            _ => return Err(format!("invalid file number in field spec: '{token}'")),
        };
        let field = field_number(field)?;
        specs.push(Spec::Field(file, field));
    }
    if specs.is_empty() {
        return Err("invalid field specification".to_owned());
    }
    Ok(Format::List(specs))
}

/// Splits a line into fields: runs of blanks by default (leading blanks ignored), or
/// exactly the separator byte when one was given.
fn split_fields(line: &[u8], separator: Option<u8>) -> Vec<Vec<u8>> {
    match separator {
        Some(sep) => line.split(|b| *b == sep).map(<[u8]>::to_vec).collect(),
        None => line
            .split(|b| *b == b' ' || *b == b'\t')
            .filter(|field| !field.is_empty())
            .map(<[u8]>::to_vec)
            .collect(),
    }
}

fn compare_keys(left: &[u8], right: &[u8], ignore_case: bool) -> Ordering {
    if ignore_case {
        left.iter()
            .map(u8::to_ascii_uppercase)
            .cmp(right.iter().map(u8::to_ascii_uppercase))
    } else {
        left.cmp(right)
    }
}

fn key_of(record: &Record, index: usize) -> Vec<u8> {
    record.fields.get(index).cloned().unwrap_or_default()
}

/// The order check is skipped for empty lines, which have no join field.
fn is_checkable(record: &Record) -> bool {
    !record.fields.is_empty()
}

/// Reads the next record from `side`, checking that it is not out of order.
fn next_record(
    config: &Config,
    side: &mut Side,
    index: usize,
    out: &mut impl Write,
) -> Result<Option<Record>, Stop> {
    let record = side.read_raw(config.delim, config.separator)?;
    let Some(record) = record else {
        return Ok(None);
    };
    if config.check == Check::Off || !is_checkable(&record) {
        return Ok(Some(record));
    }
    let key = key_of(&record, config.keys[index]);
    if let Some(previous) = side.previous.as_deref() {
        if compare_keys(&key, previous, config.ignore_case) == Ordering::Less {
            let text = String::from_utf8_lossy(&record.text);
            let message = format!(
                "{}:{}: is not sorted: {text}",
                side.name, record.line_number
            );
            match config.check {
                Check::Strict => {
                    let _ = out.flush();
                    eprintln!("{CMD}: {message}");
                    return Err(Stop::Status(EXIT_FAIL));
                }
                _ => {
                    if !side.reported {
                        eprintln!("{CMD}: {message}");
                        side.reported = true;
                    }
                    side.disorder = true;
                }
            }
        }
    }
    side.previous = Some(key);
    Ok(Some(record))
}

fn merge(
    config: &Config,
    left: &mut Side,
    right: &mut Side,
    out: &mut BufWriter<io::Stdout>,
) -> Result<(), Stop> {
    let sides = [config.keys[0], config.keys[1]];
    if config.header {
        let head_left = left.read_raw(config.delim, config.separator)?;
        let head_right = right.read_raw(config.delim, config.separator)?;
        match (head_left, head_right) {
            (Some(a), Some(b)) => {
                if config.pairs {
                    let key = key_of(&a, sides[0]);
                    emit(config, out, &key, Some(&a), Some(&b), &*left, &*right)?;
                }
            }
            (Some(a), None) if config.unpaired[0] => {
                let key = key_of(&a, sides[0]);
                emit(config, out, &key, Some(&a), None, &*left, &*right)?;
            }
            (None, Some(b)) if config.unpaired[1] => {
                let key = key_of(&b, sides[1]);
                emit(config, out, &key, None, Some(&b), &*left, &*right)?;
            }
            _ => {}
        }
    }
    let mut current_left = next_record(config, left, 0, out)?;
    let mut current_right = next_record(config, right, 1, out)?;
    loop {
        match (current_left.take(), current_right.take()) {
            (None, None) => break,
            (Some(a), None) => {
                if config.unpaired[0] {
                    let key = key_of(&a, sides[0]);
                    emit(config, out, &key, Some(&a), None, &*left, &*right)?;
                }
                current_left = next_record(config, left, 0, out)?;
            }
            (None, Some(b)) => {
                if config.unpaired[1] {
                    let key = key_of(&b, sides[1]);
                    emit(config, out, &key, None, Some(&b), &*left, &*right)?;
                }
                current_right = next_record(config, right, 1, out)?;
            }
            (Some(a), Some(b)) => {
                let key_a = key_of(&a, sides[0]);
                let key_b = key_of(&b, sides[1]);
                match compare_keys(&key_a, &key_b, config.ignore_case) {
                    Ordering::Less => {
                        if config.unpaired[0] {
                            emit(config, out, &key_a, Some(&a), None, &*left, &*right)?;
                        }
                        current_left = next_record(config, left, 0, out)?;
                        current_right = Some(b);
                    }
                    Ordering::Greater => {
                        if config.unpaired[1] {
                            emit(config, out, &key_b, None, Some(&b), &*left, &*right)?;
                        }
                        current_right = next_record(config, right, 1, out)?;
                        current_left = Some(a);
                    }
                    Ordering::Equal => {
                        let mut group_left = vec![a];
                        let mut group_right = vec![b];
                        loop {
                            match next_record(config, left, 0, out)? {
                                Some(record)
                                    if compare_keys(
                                        &key_of(&record, sides[0]),
                                        &key_a,
                                        config.ignore_case,
                                    ) == Ordering::Equal =>
                                {
                                    group_left.push(record);
                                }
                                other => {
                                    current_left = other;
                                    break;
                                }
                            }
                        }
                        loop {
                            match next_record(config, right, 1, out)? {
                                Some(record)
                                    if compare_keys(
                                        &key_of(&record, sides[1]),
                                        &key_b,
                                        config.ignore_case,
                                    ) == Ordering::Equal =>
                                {
                                    group_right.push(record);
                                }
                                other => {
                                    current_right = other;
                                    break;
                                }
                            }
                        }
                        if config.pairs {
                            for x in &group_left {
                                for y in &group_right {
                                    emit(config, out, &key_a, Some(x), Some(y), &*left, &*right)?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Writes one output line for a pair, or for an unpaired record when one side is `None`.
fn emit(
    config: &Config,
    out: &mut BufWriter<io::Stdout>,
    key: &[u8],
    a: Option<&Record>,
    b: Option<&Record>,
    left: &Side,
    right: &Side,
) -> Result<(), Stop> {
    let parts = match &config.format {
        Format::Default => default_parts(config, key, a, b),
        Format::Auto => auto_parts(config, key, a, b, left, right),
        Format::List(specs) => list_parts(config, key, specs, a, b),
    };
    let separator = config.separator.unwrap_or(b' ');
    let mut line = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            line.push(separator);
        }
        line.extend_from_slice(part);
    }
    line.push(config.delim);
    out.write_all(&line)?;
    Ok(())
}

/// Join field, then the other fields of each record (file 1 first).
fn default_parts(
    config: &Config,
    key: &[u8],
    a: Option<&Record>,
    b: Option<&Record>,
) -> Vec<Vec<u8>> {
    let mut parts = vec![key.to_vec()];
    if let Some(record) = a {
        parts.extend(rest_of(record, config.keys[0]));
    }
    if let Some(record) = b {
        parts.extend(rest_of(record, config.keys[1]));
    }
    parts
}

fn rest_of(record: &Record, key_index: usize) -> Vec<Vec<u8>> {
    record
        .fields
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != key_index)
        .map(|(_, field)| field.clone())
        .collect()
}

/// `-o auto`: the join field, then the non-join fields of each file as counted from the
/// first line of that file, with missing or empty values filled by `-e`.
fn auto_parts(
    config: &Config,
    key: &[u8],
    a: Option<&Record>,
    b: Option<&Record>,
    left: &Side,
    right: &Side,
) -> Vec<Vec<u8>> {
    let mut parts = vec![key.to_vec()];
    let count_left = left.auto_count.unwrap_or(0);
    let count_right = right.auto_count.unwrap_or(0);
    for index in 0..count_left {
        parts.push(fill(
            config,
            a.and_then(|r| nth_rest(r, config.keys[0], index)),
        ));
    }
    for index in 0..count_right {
        parts.push(fill(
            config,
            b.and_then(|r| nth_rest(r, config.keys[1], index)),
        ));
    }
    parts
}

/// The `index`-th field of a record that is not the join field.
fn nth_rest(record: &Record, key_index: usize, index: usize) -> Option<Vec<u8>> {
    let position = if index < key_index { index } else { index + 1 };
    record.fields.get(position).cloned()
}

/// An explicit `-o` list.
fn list_parts(
    config: &Config,
    key: &[u8],
    specs: &[Spec],
    a: Option<&Record>,
    b: Option<&Record>,
) -> Vec<Vec<u8>> {
    specs
        .iter()
        .map(|spec| match *spec {
            Spec::Key => key.to_vec(),
            Spec::Field(file, index) => {
                let record = if file == 0 { a } else { b };
                fill(config, record.and_then(|r| r.fields.get(index).cloned()))
            }
        })
        .collect()
}

/// A field value, or `-e` text when the field is missing or empty.
fn fill(config: &Config, value: Option<Vec<u8>>) -> Vec<u8> {
    match value {
        Some(value) if !value.is_empty() => value,
        _ => config.empty.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_split_fields_and_separator_is_exact() {
        assert_eq!(
            split_fields(b"  a  b\tc ", None),
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
        assert_eq!(
            split_fields(b"a::b", Some(b':')),
            vec![b"a".to_vec(), Vec::new(), b"b".to_vec()]
        );
        assert!(split_fields(b"", None).is_empty());
    }

    #[test]
    fn keys_compare_bytewise_or_folded() {
        assert_eq!(compare_keys(b"a", b"B", false), Ordering::Greater);
        assert_eq!(compare_keys(b"a", b"A", true), Ordering::Equal);
        assert_eq!(compare_keys(b"a", b"b", true), Ordering::Less);
    }

    #[test]
    fn format_lists_parse_and_reject_bad_fields() {
        assert_eq!(
            parse_format("1.2,2.1 0").unwrap(),
            Format::List(vec![Spec::Field(0, 1), Spec::Field(1, 0), Spec::Key])
        );
        assert_eq!(parse_format("auto").unwrap(), Format::Auto);
        assert_eq!(
            parse_format("2.x").unwrap_err(),
            "invalid field number: 'x'"
        );
        assert_eq!(
            parse_format("foo").unwrap_err(),
            "invalid field number: 'foo'"
        );
        assert_eq!(
            parse_format("3.1").unwrap_err(),
            "invalid file number in field spec: '3.1'"
        );
    }

    #[test]
    fn field_numbers_start_at_one() {
        assert_eq!(field_number("1").unwrap(), 0);
        assert_eq!(field_number("0").unwrap_err(), "invalid field number: '0'");
        assert_eq!(file_number("3").unwrap_err(), "invalid field number: '3'");
    }

    #[test]
    fn empty_separator_means_newline() {
        assert_eq!(parse_separator("").unwrap(), b'\n');
        assert_eq!(
            parse_separator("ab").unwrap_err(),
            "multi-character tab 'ab'"
        );
    }
}
