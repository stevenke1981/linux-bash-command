//! `shuf`: write a random permutation of its input lines, or random picks with `-r`.
//!
//! The input is `-e` operands, an integer range (`-i LO-HI`) or one file (stdin by default).
//! Lines are read in full, because a permutation needs all of them. A range is never
//! materialised: its permutation is generated with a sparse Fisher-Yates swap map, so memory
//! grows with the number of lines written. `--random-source` seeds a ChaCha generator from the
//! SHA-256 digest of the first 64 KiB of the file, so the same file gives the same output.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};

use rand::rngs::{StdRng, ThreadRng};
use rand::{RngExt, SeedableRng};
use sha2::{Digest, Sha256};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const CMD: &str = "shuf";

/// Bytes of the random source that feed the seed.
const SEED_BYTES: u64 = 64 * 1024;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("echo", Some('e'), Some("echo")),
    OptSpec::req("range", Some('i'), Some("input-range")),
    OptSpec::req("count", Some('n'), Some("head-count")),
    OptSpec::req("output", Some('o'), Some("output")),
    OptSpec::req("random-source", None, Some("random-source")),
    OptSpec::flag("repeat", Some('r'), Some("repeat")),
    OptSpec::flag("zero", Some('z'), Some("zero-terminated")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: shuf [OPTION]... [FILE]\n\
  or:  shuf -e [OPTION]... [ARG]...\n\
  or:  shuf -i LO-HI [OPTION]...\n\
Write a random permutation of the input lines to standard output.\n\
With no FILE, or when FILE is -, read standard input.\n\n\
  -e, --echo                treat each ARG as an input line\n\
  -i, --input-range=LO-HI   treat each number LO through HI as an input line\n\
  -n, --head-count=COUNT    output at most COUNT lines\n\
  -o, --output=FILE         write result to FILE instead of standard output\n\
      --random-source=FILE  get random bytes from FILE\n\
  -r, --repeat              output lines can be repeated\n\
  -z, --zero-terminated     line delimiter is NUL, not newline\n\
      --help                display this help and exit\n\
      --version             output version information and exit\n\n\
Exit status: 0 on success, 1 on error.\n";

/// Where the lines to shuffle come from.
enum Input {
    /// `-e`: the operands themselves.
    Args(Vec<String>),
    /// `-i`: the integers `lo..lo + size`.
    Range { lo: u128, size: u128 },
    /// A file operand, or `-` for standard input.
    File(String),
}

struct Options {
    input: Input,
    limit: Option<u64>,
    repeat: bool,
    delim: u8,
    output: Option<String>,
    random_source: Option<String>,
}

/// Source of random numbers: the thread generator, or a generator seeded from a file.
enum Random {
    Thread(ThreadRng),
    Seeded(Box<StdRng>),
}

impl Random {
    /// A value in `0..bound`; `bound` must be at least 1.
    fn below(&mut self, bound: u128) -> u128 {
        match self {
            Random::Thread(rng) => rng.random_range(0..bound),
            Random::Seeded(rng) => rng.random_range(0..bound),
        }
    }
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
    let mut random = match &options.random_source {
        Some(path) => match seeded(path) {
            Ok(random) => random,
            Err(message) => {
                eprintln!("{CMD}: {message}");
                return EXIT_FAIL;
            }
        },
        None => Random::Thread(rand::rng()),
    };
    run_with(&options, &mut random)
}

/// Builds the options from the parsed items, reporting usage errors.
fn options_from(items: &[Item]) -> Result<Options, i32> {
    let mut echo = false;
    let mut range = None;
    let mut limit = None;
    let mut repeat = false;
    let mut delim = b'\n';
    let mut output = None;
    let mut random_source = None;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text.clone()),
            Item::Opt { id, value } => {
                let text = value.clone().unwrap_or_default();
                match *id {
                    "echo" => echo = true,
                    "range" => range = Some(text),
                    "count" => limit = Some(parse_count(&text)?),
                    "output" => output = Some(text),
                    "random-source" => random_source = Some(text),
                    "repeat" => repeat = true,
                    "zero" => delim = 0,
                    _ => {}
                }
            }
        }
    }
    let input = input_from(echo, range, operands)?;
    Ok(Options {
        input,
        limit,
        repeat,
        delim,
        output,
        random_source,
    })
}

fn input_from(echo: bool, range: Option<String>, operands: Vec<String>) -> Result<Input, i32> {
    if echo && range.is_some() {
        return Err(usage_error("cannot combine -e and -i options"));
    }
    if let Some(text) = range {
        if let Some(extra) = operands.first() {
            return Err(usage_error(&format!("extra operand '{extra}'")));
        }
        let (lo, size) = parse_range(&text)?;
        return Ok(Input::Range { lo, size });
    }
    if echo {
        return Ok(Input::Args(operands));
    }
    if operands.len() > 1 {
        return Err(usage_error(&format!("extra operand '{}'", operands[1])));
    }
    Ok(Input::File(
        operands
            .into_iter()
            .next()
            .unwrap_or_else(|| "-".to_owned()),
    ))
}

/// A usage error: message, then the `Try` line.
fn usage_error(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

/// Parses `-n`: decimal digits, saturating at the largest 64-bit count.
fn parse_count(text: &str) -> Result<u64, i32> {
    let digits = text.strip_prefix('+').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        eprintln!("{CMD}: invalid line count: '{text}'");
        return Err(EXIT_FAIL);
    }
    Ok(digits.parse::<u64>().unwrap_or(u64::MAX))
}

/// Parses `LO-HI` into the first value and the number of values. `HI + 1 == LO` is an empty
/// range; a range that starts after `HI + 1` is invalid.
fn parse_range(text: &str) -> Result<(u128, u128), i32> {
    let invalid = || {
        eprintln!("{CMD}: invalid input range: '{text}'");
        EXIT_FAIL
    };
    let Some((lo_text, hi_text)) = text.split_once('-') else {
        return Err(invalid());
    };
    let (Some(lo), Some(hi)) = (parse_bound(lo_text), parse_bound(hi_text)) else {
        return Err(invalid());
    };
    // GNU cannot count a range that ends at the largest 64-bit value.
    if lo > hi + 1 || hi == u128::from(u64::MAX) {
        return Err(invalid());
    }
    Ok((lo, hi + 1 - lo))
}

/// Parses one bound: decimal digits with an optional `+`, within the 64-bit range.
fn parse_bound(text: &str) -> Option<u128> {
    let digits = text.strip_prefix('+').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u128>()
        .ok()
        .filter(|value| *value <= u128::from(u64::MAX))
}

/// Seeds a generator from the first bytes of `path`. An empty source is an error, as in GNU.
fn seeded(path: &str) -> Result<Random, String> {
    let reader = open_input(path).map_err(|error| format!("{path}: {}", io_error_text(&error)))?;
    let mut bytes = Vec::new();
    reader
        .take(SEED_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{path}: {}", io_error_text(&error)))?;
    if bytes.is_empty() {
        return Err(format!("'{path}': end of file"));
    }
    let seed: [u8; 32] = Sha256::digest(&bytes).into();
    Ok(Random::Seeded(Box::new(StdRng::from_seed(seed))))
}

/// Reads the input, writes the result and returns the exit status.
fn run_with(options: &Options, random: &mut Random) -> i32 {
    let source = match load_source(&options.input, options.delim) {
        Ok(source) => source,
        Err(code) => return code,
    };
    if options.repeat && source.is_empty() && options.limit != Some(0) {
        eprintln!("{CMD}: no lines to repeat");
        return EXIT_FAIL;
    }
    let mut out: Box<dyn Write> = match &options.output {
        Some(path) => match File::create(path) {
            Ok(file) => Box::new(BufWriter::new(file)),
            Err(error) => {
                report(CMD, path, &error);
                return EXIT_FAIL;
            }
        },
        None => Box::new(stdout_writer()),
    };
    let written = source.emit(options, random, &mut out);
    if let Err(error) = written {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{CMD}: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    finish_output(CMD, &mut out)
}

/// The lines to pick from.
enum Source {
    Lines(Vec<Vec<u8>>),
    Range { lo: u128, size: u128 },
}

impl Source {
    fn is_empty(&self) -> bool {
        match self {
            Source::Lines(lines) => lines.is_empty(),
            Source::Range { size, .. } => *size == 0,
        }
    }

    fn emit(&self, options: &Options, random: &mut Random, out: &mut dyn Write) -> io::Result<()> {
        let count = options.limit.map(u128::from);
        match (self, options.repeat) {
            (Source::Lines(lines), false) => {
                let mut items: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
                let take = count.map_or(items.len(), |c| c.min(items.len() as u128) as usize);
                for index in 0..take {
                    let pick = index + random.below((items.len() - index) as u128) as usize;
                    items.swap(index, pick);
                    write_record(out, items[index], options.delim)?;
                }
                Ok(())
            }
            (Source::Lines(lines), true) => {
                let mut remaining = count;
                while remaining != Some(0) {
                    let pick = random.below(lines.len() as u128) as usize;
                    write_record(out, &lines[pick], options.delim)?;
                    remaining = remaining.map(|left| left - 1);
                }
                Ok(())
            }
            (Source::Range { lo, size }, false) => {
                emit_range_permutation(*lo, *size, count, random, out, options.delim)
            }
            (Source::Range { lo, size }, true) => {
                let mut remaining = count;
                while remaining != Some(0) {
                    let pick = random.below(*size);
                    write_number(out, lo + pick, options.delim)?;
                    remaining = remaining.map(|left| left - 1);
                }
                Ok(())
            }
        }
    }
}

/// Writes up to `count` values of `lo..lo + size` in random order.
fn emit_range_permutation(
    lo: u128,
    size: u128,
    count: Option<u128>,
    random: &mut Random,
    out: &mut dyn Write,
    delim: u8,
) -> io::Result<()> {
    for_each_range_pick(lo, size, count, random, |value| {
        write_number(out, value, delim)
    })
}

/// Calls `sink` with up to `count` values of `lo..lo + size`, in random order. The shuffle is
/// sparse: a Fisher-Yates pass that stores only the positions it has swapped.
fn for_each_range_pick(
    lo: u128,
    size: u128,
    count: Option<u128>,
    random: &mut Random,
    mut sink: impl FnMut(u128) -> io::Result<()>,
) -> io::Result<()> {
    let take = count.map_or(size, |c| c.min(size));
    let mut moved: HashMap<u128, u128> = HashMap::new();
    for index in 0..take {
        let pick = index + random.below(size - index);
        let value_at_pick = moved.get(&pick).copied().unwrap_or(pick);
        let value_at_index = moved.get(&index).copied().unwrap_or(index);
        moved.insert(pick, value_at_index);
        sink(lo + value_at_pick)?;
    }
    Ok(())
}

fn write_record(out: &mut dyn Write, record: &[u8], delim: u8) -> io::Result<()> {
    out.write_all(record)?;
    out.write_all(&[delim])
}

fn write_number(out: &mut dyn Write, value: u128, delim: u8) -> io::Result<()> {
    write_record(out, value.to_string().as_bytes(), delim)
}

/// Collects the input lines (or the `-e` operands, or the range) before anything is written.
fn load_source(input: &Input, delim: u8) -> Result<Source, i32> {
    match input {
        Input::Args(args) => Ok(Source::Lines(
            args.iter().map(|arg| arg.as_bytes().to_vec()).collect(),
        )),
        Input::Range { lo, size } => Ok(Source::Range {
            lo: *lo,
            size: *size,
        }),
        Input::File(name) => read_lines(name, delim).map(Source::Lines).map_err(|error| {
            report(CMD, name, &error);
            EXIT_FAIL
        }),
    }
}

fn read_lines(name: &str, delim: u8) -> io::Result<Vec<Vec<u8>>> {
    let mut reader = open_input(name)?;
    let mut lines = Vec::new();
    let mut buf = Vec::new();
    while read_record(reader.as_mut(), delim, &mut buf)? {
        if buf.last() == Some(&delim) {
            buf.pop();
        }
        lines.push(buf.clone());
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread() -> Random {
        Random::Thread(rand::rng())
    }

    fn seeded_from_bytes(bytes: &[u8]) -> Random {
        let seed: [u8; 32] = Sha256::digest(bytes).into();
        Random::Seeded(Box::new(StdRng::from_seed(seed)))
    }

    fn permutation_of_range(
        lo: u128,
        size: u128,
        count: Option<u128>,
        random: &mut Random,
    ) -> Vec<u128> {
        let mut out = Vec::new();
        for_each_range_pick(lo, size, count, random, |value| {
            out.push(value);
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn sparse_range_permutation_covers_every_value_once() {
        let values = permutation_of_range(10, 50, None, &mut thread());
        let mut sorted = values.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (10..60).collect::<Vec<u128>>());
    }

    #[test]
    fn range_permutation_with_count_stops_early_and_stays_in_range() {
        let values = permutation_of_range(1, 1_000_000_000_000, Some(5), &mut thread());
        assert_eq!(values.len(), 5);
        assert!(values.iter().all(|v| (1..=1_000_000_000_000).contains(v)));
        let mut unique = values.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn seeded_generators_are_reproducible() {
        let first = permutation_of_range(0, 100, None, &mut seeded_from_bytes(b"seed"));
        let second = permutation_of_range(0, 100, None, &mut seeded_from_bytes(b"seed"));
        let other = permutation_of_range(0, 100, None, &mut seeded_from_bytes(b"other"));
        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    #[test]
    fn range_parsing_follows_gnu_bounds() {
        assert_eq!(parse_range("1-3").ok(), Some((1, 3)));
        assert_eq!(parse_range("3-2").ok(), Some((3, 0)));
        assert_eq!(parse_range("+4-4").ok(), Some((4, 1)));
        assert!(parse_range("5-1").is_err());
        assert!(parse_range("-5-10").is_err());
        assert!(parse_range("1-").is_err());
        assert!(parse_range("0-18446744073709551616").is_err());
        assert!(parse_range("0-18446744073709551615").is_err());
        assert_eq!(
            parse_range("0-18446744073709551614").ok(),
            Some((0, (1u128 << 64) - 1))
        );
    }

    #[test]
    fn count_parsing_rejects_signs_and_words() {
        assert_eq!(parse_count("7").ok(), Some(7));
        assert_eq!(parse_count("99999999999999999999999").ok(), Some(u64::MAX));
        assert!(parse_count("-1").is_err());
        assert!(parse_count("1e3").is_err());
        assert!(parse_count("").is_err());
    }

    #[test]
    fn repeat_picks_come_from_the_source() {
        let lines = Source::Lines(vec![b"a".to_vec(), b"b".to_vec()]);
        let options = Options {
            input: Input::Args(Vec::new()),
            limit: Some(20),
            repeat: true,
            delim: b'\n',
            output: None,
            random_source: None,
        };
        let mut out = Vec::new();
        lines.emit(&options, &mut thread(), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 20);
        assert!(text.lines().all(|line| line == "a" || line == "b"));
    }

    #[test]
    fn input_selection_rejects_conflicts() {
        assert!(input_from(true, Some("1-2".into()), Vec::new()).is_err());
        assert!(input_from(false, Some("1-2".into()), vec!["x".into()]).is_err());
        assert!(input_from(false, None, vec!["a".into(), "b".into()]).is_err());
        assert!(
            matches!(input_from(false, None, Vec::new()), Ok(Input::File(name)) if name == "-")
        );
    }
}
