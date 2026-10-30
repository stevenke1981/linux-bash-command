//! `split`: split a file into pieces by lines, bytes or chunk count.
//!
//! Output names are `PREFIX` plus a suffix (`-a`, `-d`, `-x`), optionally followed by
//! `--additional-suffix`. `--filter` (running a shell command per piece) is not supported.

mod modes;
mod names;
mod output;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, unsupported, EXIT_FAIL, EXIT_OK};

use names::{Alphabet, Namer};

const CMD: &str = "split";

const SPECS: &[OptSpec] = &[
    OptSpec::req("suffix_len", Some('a'), Some("suffix-length")),
    OptSpec::req("additional", None, Some("additional-suffix")),
    OptSpec::req("bytes", Some('b'), Some("bytes")),
    OptSpec::req("line_bytes", Some('C'), Some("line-bytes")),
    OptSpec::flag("decimal", Some('d'), None),
    OptSpec::opt("numeric", None, Some("numeric-suffixes")),
    OptSpec::flag("hex", Some('x'), None),
    OptSpec::opt("hex_long", None, Some("hex-suffixes")),
    OptSpec::flag("elide", Some('e'), Some("elide-empty-files")),
    OptSpec::req("lines", Some('l'), Some("lines")),
    OptSpec::req("number", Some('n'), Some("number")),
    OptSpec::req("separator", Some('t'), Some("separator")),
    OptSpec::flag("verbose", None, Some("verbose")),
    OptSpec::flag("unbuffered", Some('u'), None),
    OptSpec::req("filter", None, Some("filter")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = r"Usage: split [OPTION]... [FILE [PREFIX]]
Output pieces of FILE to PREFIXaa, PREFIXab, ...;
default size is 1000 lines, and default PREFIX is 'x'.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -a, --suffix-length=N   generate suffixes of length N (default 2)
      --additional-suffix=SUFFIX  append an additional SUFFIX to file names
  -b, --bytes=SIZE        put SIZE bytes per output file
  -C, --line-bytes=SIZE   put at most SIZE bytes of records per output file
  -d                      use numeric suffixes starting at 0
      --numeric-suffixes[=FROM]  same as -d, but allow setting the start value
  -x                      use hex suffixes starting at 0
      --hex-suffixes[=FROM]  same as -x, but allow setting the start value
  -e, --elide-empty-files  do not generate empty output files with '-n'
  -l, --lines=NUMBER      put NUMBER lines/records per output file
  -n, --number=CHUNKS     generate CHUNKS output files; see explanation below
  -t, --separator=SEP     use SEP instead of newline as the record separator;
                            '\0' (zero) specifies the NUL character
  -u                      ignored (accepted for compatibility)
      --verbose           print a diagnostic just before each output file is opened
      --help              display this help and exit
      --version           output version information and exit

The SIZE argument is an integer and optional unit (example: 10K is 10*1024).
Units are K,M,G,T,P,E,Z,Y (powers of 1024) or KB,MB,... (powers of 1000).

CHUNKS may be:
  N       split into N files based on size of input
  K/N     output only the Kth of N chunks
  l/N     split into N chunks based on number of lines
  l/K/N   output only the Kth of N chunks based on number of lines
  r/N     like 'l' but use round robin distribution
  r/K/N   likewise, but only output Kth of N to stdout
";

/// How the input is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Lines(u64),
    Bytes(u64),
    LineBytes(u64),
    Chunks {
        strategy: Strategy,
        count: u64,
        /// Only this chunk (1-based) goes to standard output.
        only: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Strategy {
    Bytes,
    Lines,
    RoundRobin,
}

pub(super) struct Options {
    mode: Mode,
    namer: Namer,
    elide: bool,
    verbose: bool,
    separator: u8,
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
                "filter" => return unsupported(CMD, "--filter"),
                _ => {}
            }
        }
    }
    let operands: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(text) => Some(text.as_str()),
            Item::Opt { .. } => None,
        })
        .collect();
    if operands.len() > 2 {
        return usage_error(&format!("extra operand '{}'", operands[2]));
    }
    let options = match build_options(&items) {
        Ok(options) => options,
        Err(OptionError::Usage(message)) => return usage_error(&message),
        Err(OptionError::Plain(message)) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let input = operands.first().copied().unwrap_or("-");
    match modes::split(&options, input) {
        Ok(()) => EXIT_OK,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            EXIT_FAIL
        }
    }
}

/// Usage errors print the message and the `Try` line.
fn usage_error(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

enum OptionError {
    /// A usage problem: printed with the `Try` line.
    Usage(String),
    /// A value problem: printed alone.
    Plain(String),
}

fn build_options(items: &[Item]) -> Result<Options, OptionError> {
    let mut mode: Option<Mode> = None;
    let mut suffix_length: Option<u32> = None;
    let mut alphabet = Alphabet::Lower;
    let mut start = 0u64;
    let mut additional = String::new();
    let mut elide = false;
    let mut verbose = false;
    let mut separator = b'\n';
    for item in items {
        let Item::Opt { id, value } = item else {
            continue;
        };
        let value = value.as_deref().unwrap_or("");
        match *id {
            "lines" => set_mode(&mut mode, Mode::Lines(positive_count(value, "lines")?))?,
            "bytes" => set_mode(&mut mode, Mode::Bytes(positive_size(value, "bytes")?))?,
            "line_bytes" => {
                set_mode(&mut mode, Mode::LineBytes(positive_size(value, "lines")?))?;
            }
            "number" => set_mode(&mut mode, parse_chunks(value)?)?,
            "suffix_len" => {
                let length = value
                    .parse::<u32>()
                    .map_err(|_| OptionError::Plain(format!("invalid suffix length: '{value}'")))?;
                suffix_length = if length == 0 { None } else { Some(length) };
            }
            "additional" => additional = value.to_owned(),
            "decimal" => {
                alphabet = Alphabet::Digits;
                start = 0;
            }
            "numeric" => {
                alphabet = Alphabet::Digits;
                start = parse_start(value)?;
            }
            "hex" => {
                alphabet = Alphabet::Hex;
                start = 0;
            }
            "hex_long" => {
                alphabet = Alphabet::Hex;
                start = parse_start(value)?;
            }
            "elide" => elide = true,
            "verbose" => verbose = true,
            "separator" => separator = parse_separator(value)?,
            _ => {}
        }
    }
    let mode = mode.unwrap_or(Mode::Lines(1000));
    let namer = Namer::new(
        &prefix_operand(items),
        &additional,
        alphabet,
        suffix_length,
        start,
    );
    Ok(Options {
        mode,
        namer,
        elide,
        verbose,
        separator,
    })
}

/// The second operand, or the default prefix `x`.
fn prefix_operand(items: &[Item]) -> String {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(text) => Some(text.clone()),
            Item::Opt { .. } => None,
        })
        .nth(1)
        .unwrap_or_else(|| "x".to_owned())
}

fn set_mode(slot: &mut Option<Mode>, mode: Mode) -> Result<(), OptionError> {
    match slot {
        Some(existing) if std::mem::discriminant(existing) != std::mem::discriminant(&mode) => Err(
            OptionError::Usage("cannot split in more than one way".to_owned()),
        ),
        _ => {
            *slot = Some(mode);
            Ok(())
        }
    }
}

fn positive_count(value: &str, what: &str) -> Result<u64, OptionError> {
    match value.parse::<u64>() {
        Ok(count) if count > 0 => Ok(count),
        _ => Err(OptionError::Plain(format!(
            "invalid number of {what}: '{value}'"
        ))),
    }
}

fn positive_size(value: &str, what: &str) -> Result<u64, OptionError> {
    match parse_size(value) {
        Some(size) if size > 0 => Ok(size),
        _ => Err(OptionError::Plain(format!(
            "invalid number of {what}: '{value}'"
        ))),
    }
}

/// A size with an optional unit: `b` (512), `K`/`KiB` (1024), `KB` (1000), and up.
fn parse_size(text: &str) -> Option<u64> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let number: u64 = text[..digits].parse().ok()?;
    let multiplier: u64 = match &text[digits..] {
        "" => 1,
        "b" => 512,
        "KB" => 1000,
        "K" | "KiB" => 1 << 10,
        "MB" => 1_000_000,
        "M" | "MiB" => 1 << 20,
        "GB" => 1_000_000_000,
        "G" | "GiB" => 1 << 30,
        "TB" => 1_000_000_000_000,
        "T" | "TiB" => 1 << 40,
        _ => return None,
    };
    number.checked_mul(multiplier)
}

/// `-n` argument: `N`, `K/N`, `l/N`, `l/K/N`, `r/N` or `r/K/N`.
fn parse_chunks(value: &str) -> Result<Mode, OptionError> {
    let (strategy, rest) = if let Some(rest) = value.strip_prefix("l/") {
        (Strategy::Lines, rest)
    } else if let Some(rest) = value.strip_prefix("r/") {
        (Strategy::RoundRobin, rest)
    } else {
        (Strategy::Bytes, value)
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let invalid_count =
        |text: &str| OptionError::Plain(format!("invalid number of chunks: '{text}'"));
    match parts.as_slice() {
        [count] => {
            let count = count
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid_count(count))?;
            Ok(Mode::Chunks {
                strategy,
                count,
                only: None,
            })
        }
        [chunk, count] => {
            let count = count
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid_count(count))?;
            let only = chunk.parse::<u64>().map_err(|_| invalid_count(rest))?;
            if only == 0 || only > count {
                return Err(OptionError::Plain(format!(
                    "invalid chunk number: '{chunk}'"
                )));
            }
            Ok(Mode::Chunks {
                strategy,
                count,
                only: Some(only),
            })
        }
        _ => Err(invalid_count(rest)),
    }
}

fn parse_start(value: &str) -> Result<u64, OptionError> {
    if value.is_empty() {
        return Ok(0);
    }
    value
        .parse::<u64>()
        .map_err(|_| OptionError::Plain(format!("invalid suffix start '{value}'")))
}

/// `-t SEP`: one byte, `\0` meaning NUL.
fn parse_separator(value: &str) -> Result<u8, OptionError> {
    match value.as_bytes() {
        [byte] => Ok(*byte),
        [b'\\', b'0'] => Ok(0),
        [] => Err(OptionError::Plain("empty record separator".to_owned())),
        _ => Err(OptionError::Plain(format!(
            "multi-character separator '{value}'"
        ))),
    }
}
