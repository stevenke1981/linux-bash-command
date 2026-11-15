use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use rand::{rngs::StdRng, RngExt, SeedableRng};
use std::{
    collections::HashMap,
    io::{BufRead, BufWriter, Read, Write},
};

const HELP: &str = "Usage: shuf [OPTION]... [FILE]\nWrite a random permutation of input lines to standard output.\n  -e, --echo                 treat each ARG as an input line\n  -i, --input-range=LO-HI    treat each number as an input line\n  -n, --head-count=COUNT    output at most COUNT lines\n  -o, --output=FILE         write to FILE instead of standard output\n      --random-source=FILE  get random bytes from FILE\n  -r, --repeat              output lines can be repeated\n  -z, --zero-terminated     line delimiter is NUL, not newline\n      --help                display this help\n      --version             output version information\n";

#[derive(Default)]
struct Options {
    echo: bool,
    range: Option<(u64, u64)>,
    count: Option<usize>,
    output: Option<String>,
    random_source: Option<String>,
    repeat: bool,
    delimiter: u8,
    files: Vec<String>,
}

enum Input {
    Rows(Vec<Vec<u8>>),
    Range { low: u64, high: u64 },
}
impl Input {
    fn len(&self) -> u128 {
        match self {
            Self::Rows(rows) => rows.len() as u128,
            Self::Range { low: 1, high: 0 } => 0,
            Self::Range { low, high } => u128::from(*high) - u128::from(*low) + 1,
        }
    }
    fn row(&self, index: u128) -> Vec<u8> {
        match self {
            Self::Rows(rows) => rows[index as usize].clone(),
            Self::Range { low, .. } => (u128::from(*low) + index).to_string().into_bytes(),
        }
    }
}

fn parse_range(text: &str) -> Result<(u64, u64), String> {
    let Some((low, high)) = text.split_once('-') else {
        return Err(format!("invalid input range: '{text}'"));
    };
    let low = low
        .parse::<u64>()
        .map_err(|_| format!("invalid input range: '{text}'"))?;
    let high = high
        .parse::<u64>()
        .map_err(|_| format!("invalid input range: '{text}'"))?;
    if low > high && (low, high) != (1, 0) {
        return Err(format!("invalid input range: '{text}'"));
    }
    Ok((low, high))
}

fn options(args: &[String]) -> Result<Options, i32> {
    let specs = [
        OptSpec::flag("echo", 'e'),
        OptSpec::value("input-range", 'i'),
        OptSpec::value("head-count", 'n'),
        OptSpec::value("output", 'o'),
        OptSpec::value("random-source", '\0'),
        OptSpec::flag("repeat", 'r'),
        OptSpec::flag("zero-terminated", 'z'),
    ];
    let items = args::parse_command("shuf", args, HELP, &specs, true)?;
    let mut result = Options {
        delimiter: b'\n',
        ..Options::default()
    };
    for item in items {
        match item {
            Item::Opt("echo", _) => result.echo = true,
            Item::Opt("input-range", Some(text)) => {
                result.range = Some(parse_range(&text).map_err(|e| error::usage("shuf", e, 1))?)
            }
            Item::Opt("head-count", Some(text)) => {
                result.count = Some(text.parse::<usize>().map_err(|_| {
                    error::usage("shuf", format!("invalid line count: '{text}'"), 1)
                })?)
            }
            Item::Opt("output", Some(path)) => result.output = Some(path),
            Item::Opt("random-source", Some(path)) => result.random_source = Some(path),
            Item::Opt("repeat", _) => result.repeat = true,
            Item::Opt("zero-terminated", _) => result.delimiter = 0,
            Item::Operand(path) => result.files.push(path),
            Item::Opt(_, _) => unreachable!(),
        }
    }
    if result.echo && result.range.is_some() {
        return Err(error::usage(
            "shuf",
            "cannot combine --echo and --input-range",
            1,
        ));
    }
    if result.range.is_some() && !result.files.is_empty() {
        return Err(error::usage("shuf", "extra operand with --input-range", 1));
    }
    if !result.echo && result.files.len() > 1 {
        return Err(error::usage(
            "shuf",
            format!("extra operand '{}'", result.files[1]),
            1,
        ));
    }
    Ok(result)
}

/// The random-source file is opened and read only when a nontrivial draw is required.
enum RandomSource {
    System(Box<Option<StdRng>>),
    File {
        path: String,
        reader: Option<Box<dyn BufRead>>,
        bit_pool: u128,
        bits_available: u32,
    },
}
impl RandomSource {
    fn new(path: Option<&str>) -> Self {
        match path {
            Some(path) => Self::File {
                path: path.to_owned(),
                reader: None,
                bit_pool: 0,
                bits_available: 0,
            },
            None => Self::System(Box::new(None)),
        }
    }

    fn below(&mut self, upper: u128) -> Result<u128, String> {
        if upper <= 1 {
            return Ok(0);
        }
        match self {
            Self::System(rng) => {
                if rng.is_none() {
                    **rng = Some(
                        StdRng::try_from_rng(&mut rand::rng())
                            .map_err(|e| format!("cannot initialize random generator: {e}"))?,
                    );
                }
                Ok(rng.as_mut().as_mut().unwrap().random_range(0..upper))
            }
            Self::File {
                path,
                reader,
                bit_pool,
                bits_available,
            } => {
                if reader.is_none() {
                    *reader =
                        Some(io::input(path).map_err(|e| {
                            format!("random source {path}: {}", error::io_text(&e))
                        })?);
                }
                let bits = 128 - (upper - 1).leading_zeros();
                loop {
                    while *bits_available < bits {
                        let mut byte = [0u8; 1];
                        reader
                            .as_mut()
                            .unwrap()
                            .read_exact(&mut byte)
                            .map_err(|e| format!("random source {path}: {}", error::io_text(&e)))?;
                        *bit_pool |= u128::from(byte[0]) << *bits_available;
                        *bits_available += 8;
                    }
                    let value = *bit_pool & ((1u128 << bits) - 1);
                    *bit_pool >>= bits;
                    *bits_available -= bits;
                    if value < upper {
                        return Ok(value);
                    }
                }
            }
        }
    }
}

fn read_input(options: &Options) -> Result<Input, String> {
    if let Some((low, high)) = options.range {
        return Ok(Input::Range { low, high });
    }
    let mut rows = Vec::new();
    if options.echo {
        rows.extend(options.files.iter().map(|s| s.as_bytes().to_vec()));
    } else {
        let name = options.files.first().map(String::as_str).unwrap_or("-");
        let mut reader = io::input(name).map_err(|e| format!("{name}: {}", error::io_text(&e)))?;
        io::records(&mut reader, options.delimiter, |row| {
            rows.push(row.to_vec());
            Ok(())
        })
        .map_err(|e| error::io_text(&e).to_string())?;
    }
    Ok(Input::Rows(rows))
}

fn sparse_unique<R: RandomSourceTrait>(
    low: u64,
    high: u64,
    count: usize,
    rng: &mut R,
) -> Result<Vec<Vec<u8>>, String> {
    let total = if (low, high) == (1, 0) {
        0
    } else {
        u128::from(high) - u128::from(low) + 1
    };
    if total == 0 {
        return Ok(Vec::new());
    }
    let take = (count as u128).min(total);
    let mut swaps = HashMap::<u128, u128>::with_capacity((take as usize).saturating_mul(2));
    let mut output = Vec::with_capacity(take as usize);
    for index in 0..take {
        let at = index + rng.below(total - index)?;
        let chosen = swaps.remove(&at).unwrap_or(at);
        let replacement = swaps.remove(&index).unwrap_or(index);
        if at != index {
            swaps.insert(at, replacement);
        }
        output.push((u128::from(low) + chosen).to_string().into_bytes());
    }
    Ok(output)
}

trait RandomSourceTrait {
    fn below(&mut self, upper: u128) -> Result<u128, String>;
}
impl RandomSourceTrait for RandomSource {
    fn below(&mut self, upper: u128) -> Result<u128, String> {
        Self::below(self, upper)
    }
}
impl RandomSourceTrait for StdRng {
    fn below(&mut self, upper: u128) -> Result<u128, String> {
        Ok(self.random_range(0..upper))
    }
}

fn random_order<R: RandomSourceTrait>(
    rows: &mut [Vec<u8>],
    count: usize,
    rng: &mut R,
) -> Result<(), String> {
    for index in 0..count {
        let at = index + rng.below((rows.len() - index) as u128)? as usize;
        rows.swap(index, at);
    }
    Ok(())
}

fn sample(input: Input, options: &Options, rng: &mut RandomSource) -> Result<Vec<Vec<u8>>, String> {
    let total = input.len();
    let requested = options.count.map(|count| count as u128).unwrap_or(total);
    let count = requested.min(total).min(usize::MAX as u128) as usize;
    if count == 0 {
        return Ok(Vec::new());
    }
    match input {
        Input::Range { low, high } if total != 0 => sparse_unique(low, high, count, rng),
        Input::Rows(mut rows) => {
            let take = count.min(rows.len());
            random_order(&mut rows, take, rng)?;
            rows.truncate(take);
            Ok(rows)
        }
        Input::Range { .. } => Ok(Vec::new()),
    }
}

fn write_rows(rows: &[Vec<u8>], delimiter: u8, out: &mut dyn Write) -> std::io::Result<()> {
    for row in rows {
        out.write_all(row)?;
        out.write_all(&[delimiter])?;
    }
    Ok(())
}

enum StreamFailure {
    Random(String),
    Write(std::io::Error),
}
fn stream_repeats(
    input: &Input,
    delimiter: u8,
    rng: &mut RandomSource,
    out: &mut dyn Write,
    remaining: Option<usize>,
) -> Result<(), StreamFailure> {
    let total = input.len();
    if total == 0 && remaining != Some(0) {
        return Err(StreamFailure::Random(
            "cannot repeat from an empty input".into(),
        ));
    }
    let mut written = 0usize;
    while remaining.is_none_or(|count| written < count) {
        let index = match rng.below(total) {
            Ok(index) => index,
            Err(message) => {
                out.flush().map_err(StreamFailure::Write)?;
                return Err(StreamFailure::Random(message));
            }
        };
        out.write_all(&input.row(index))
            .map_err(StreamFailure::Write)?;
        out.write_all(&[delimiter]).map_err(StreamFailure::Write)?;
        written += 1;
        if written.is_multiple_of(64) {
            out.flush().map_err(StreamFailure::Write)?;
        }
    }
    out.flush().map_err(StreamFailure::Write)
}

fn stream_error(error: StreamFailure, path: Option<&str>) -> i32 {
    match error {
        StreamFailure::Random(message) => {
            eprintln!("shuf: {message}");
            1
        }
        StreamFailure::Write(error) if error.kind() == std::io::ErrorKind::BrokenPipe => 0,
        StreamFailure::Write(error) => {
            if let Some(path) = path {
                eprintln!("shuf: {path}: {}", crate::common::error::io_text(&error));
            } else {
                eprintln!(
                    "shuf: write error: {}",
                    crate::common::error::io_text(&error)
                );
            }
            1
        }
    }
}

fn repeat_output(
    input: &Input,
    options: &Options,
    rng: &mut RandomSource,
    remaining: Option<usize>,
) -> i32 {
    if let Some(path) = options.output.as_deref() {
        let file = match std::fs::File::create(path) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("shuf: {path}: {}", error::io_text(&error));
                return 1;
            }
        };
        let mut out = BufWriter::new(file);
        match stream_repeats(input, options.delimiter, rng, &mut out, remaining) {
            Ok(()) => 0,
            Err(failure) => stream_error(failure, Some(path)),
        }
    } else {
        let stdout = std::io::stdout();
        let mut out = BufWriter::new(stdout.lock());
        match stream_repeats(input, options.delimiter, rng, &mut out, remaining) {
            Ok(()) => 0,
            Err(failure) => stream_error(failure, None),
        }
    }
}

fn zero_count_output(options: &Options) -> Option<i32> {
    if options.count != Some(0) {
        return None;
    }
    if let Some(path) = options.output.as_deref() {
        return Some(match std::fs::File::create(path) {
            Ok(_) => 0,
            Err(error) => {
                eprintln!("shuf: {path}: {}", error::io_text(&error));
                1
            }
        });
    }
    Some(io::output("shuf", |_| Ok(())))
}

pub fn run(args: &[String]) -> i32 {
    let options = match options(args) {
        Ok(options) => options,
        Err(code) => return code,
    };
    if let Some(code) = zero_count_output(&options) {
        return code;
    }
    let input = match read_input(&options) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("shuf: {error}");
            return 1;
        }
    };
    let mut random = RandomSource::new(options.random_source.as_deref());
    if options.repeat {
        return repeat_output(&input, &options, &mut random, options.count);
    }
    let rows = match sample(input, &options, &mut random) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("shuf: {error}");
            return 1;
        }
    };
    if let Some(path) = options.output {
        return match std::fs::File::create(&path)
            .and_then(|mut file| write_rows(&rows, options.delimiter, &mut file))
        {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("shuf: {path}: {}", error::io_text(&error));
                1
            }
        };
    }
    io::output("shuf", |out| write_rows(&rows, options.delimiter, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_unsigned_ranges_and_the_empty_sentinel() {
        assert_eq!(
            parse_range("0-18446744073709551615").unwrap(),
            (0, u64::MAX)
        );
        assert_eq!(parse_range("1-0").unwrap(), (1, 0));
        assert_eq!(
            parse_range("18446744073709551615-18446744073709551615").unwrap(),
            (u64::MAX, u64::MAX)
        );
        assert!(parse_range("-1-5").is_err());
        assert!(parse_range("5-1").is_err());
    }

    #[test]
    fn sparse_sampling_is_unique_for_wide_unsigned_ranges() {
        let mut rng = StdRng::seed_from_u64(17);
        let values = sparse_unique(0, u64::MAX, 64, &mut rng).unwrap();
        let set: std::collections::HashSet<_> = values.iter().cloned().collect();
        assert_eq!(values.len(), 64);
        assert_eq!(set.len(), 64);
        assert!(values
            .iter()
            .all(|value| { std::str::from_utf8(value).unwrap().parse::<u64>().is_ok() }));
        assert_eq!(
            sparse_unique(u64::MAX, u64::MAX, 1, &mut rng).unwrap(),
            [u64::MAX.to_string().into_bytes()]
        );
        assert!(sparse_unique(1, 0, 4, &mut rng).unwrap().is_empty());
    }

    #[test]
    fn source_reads_on_demand_and_reports_eof_only_when_needed() {
        let mut source = RandomSource::File {
            path: "unit source".into(),
            reader: Some(Box::new(Cursor::new([0x03]))),
            bit_pool: 0,
            bits_available: 0,
        };
        assert_eq!(source.below(1).unwrap(), 0);
        assert_eq!(source.below(4).unwrap(), 3);
        assert_eq!(source.below(2).unwrap(), 0);
        for _ in 0..5 {
            assert_eq!(source.below(2).unwrap(), 0);
        }
        assert!(source.below(2).unwrap_err().contains("unit source"));
    }

    #[test]
    fn random_source_reuses_unused_bits_between_draws() {
        let mut source = RandomSource::File {
            path: "one byte".into(),
            reader: Some(Box::new(Cursor::new([0x00]))),
            bit_pool: 0,
            bits_available: 0,
        };
        for _ in 0..8 {
            assert_eq!(source.below(2).unwrap(), 0);
        }
        assert_eq!(source.below(1).unwrap(), 0);
        assert!(source.below(2).unwrap_err().contains("one byte"));
    }

    #[test]
    fn zero_count_and_singleton_do_not_read_random_source() {
        let options = Options {
            echo: true,
            count: Some(0),
            ..Options::default()
        };
        let mut source = RandomSource::new(Some("nonexistent"));
        assert!(sample(Input::Rows(Vec::new()), &options, &mut source)
            .unwrap()
            .is_empty());
        assert_eq!(source.below(1).unwrap(), 0);
    }
}
