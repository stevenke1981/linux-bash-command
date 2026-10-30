//! `numfmt`: convert numbers to and from human-readable units (SI, IEC and IEC-I).
//!
//! Numbers come from the operands (one per line) or from standard input, where a line is split
//! into fields by whitespace or by `-d` and only the selected fields are converted; separators
//! are copied through unchanged. The conversion itself lives in [`convert`].

mod convert;

use std::io::{self, BufRead, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

use convert::{convert_number, parse_format, Format};

const CMD: &str = "numfmt";

/// Exit status for conversion errors, as in GNU.
const EXIT_CONVERSION: i32 = 2;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("debug", None, Some("debug")),
    OptSpec::req("delimiter", Some('d'), Some("delimiter")),
    OptSpec::req("field", None, Some("field")),
    OptSpec::req("format", None, Some("format")),
    OptSpec::req("from", None, Some("from")),
    OptSpec::req("from-unit", None, Some("from-unit")),
    OptSpec::flag("grouping", None, Some("grouping")),
    OptSpec::opt("header", None, Some("header")),
    OptSpec::req("invalid", None, Some("invalid")),
    OptSpec::req("padding", None, Some("padding")),
    OptSpec::req("round", None, Some("round")),
    OptSpec::req("suffix", None, Some("suffix")),
    OptSpec::req("to", None, Some("to")),
    OptSpec::req("to-unit", None, Some("to-unit")),
    OptSpec::flag("zero", Some('z'), Some("zero-terminated")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: numfmt [OPTION]... [NUMBER]...\n\
Reformat NUMBER(s), or the numbers from standard input if none are specified.\n\n\
      --debug           print warnings about invalid input\n\
  -d, --delimiter=X     use X instead of whitespace for field delimiter\n\
      --field=FIELDS    replace the numbers in these input fields; see FIELDS below\n\
      --format=FORMAT   use printf style floating-point FORMAT; see FORMAT below\n\
      --from=UNIT       auto-scale input numbers to UNITs; see UNIT below\n\
      --from-unit=N     specify the input unit size (instead of the default 1)\n\
      --grouping        use locale-defined grouping of digits (no-op in the C locale)\n\
      --header[=N]      print the first N lines without conversion (default 1)\n\
      --invalid=MODE    failure mode for invalid numbers: abort, fail, warn, ignore\n\
      --padding=N       pad the output to N characters; negative N left-aligns\n\
      --round=METHOD    rounding method: up, down, from-zero, towards-zero, nearest\n\
      --suffix=SUFFIX   add SUFFIX to output numbers\n\
      --to=UNIT         auto-scale output numbers to UNITs; see UNIT below\n\
      --to-unit=N       the output unit size (instead of the default 1)\n\
  -z, --zero-terminated  line delimiter is NUL, not newline\n\
      --help            display this help and exit\n\
      --version         output version information and exit\n\n\
UNIT options: none, si (powers of 1000), iec (powers of 1024), iec-i (1024 with an\n\
i suffix); auto (input only) accepts both the si and iec-i forms.\n\n\
FORMAT must contain exactly one %f directive, optionally with flags, width and\n\
precision: %[0][-][WIDTH][.PRECISION]f.\n\n\
Exit status: 0 on success, 1 on usage error, 2 on conversion error.\n";

const UNIT_CHOICES: &[&str] = &["none", "si", "iec", "iec-i"];
const FROM_CHOICES: &[&str] = &["none", "auto", "si", "iec", "iec-i"];
const ROUND_CHOICES: &[&str] = &["up", "down", "from-zero", "towards-zero", "nearest"];
const INVALID_CHOICES: &[&str] = &["abort", "fail", "warn", "ignore"];

/// Input or output unit system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scale {
    None,
    Auto,
    Si,
    Iec,
    IecI,
}

/// Rounding method for values that are not exact at the output precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Round {
    Up,
    Down,
    FromZero,
    TowardsZero,
    Nearest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Invalid {
    Abort,
    Fail,
    Warn,
    Ignore,
}

/// Which fields of a line are converted (1-based, inclusive ranges).
#[derive(Clone)]
enum Fields {
    All,
    Ranges(Vec<(usize, usize)>),
}

impl Fields {
    fn contains(&self, field: usize) -> bool {
        match self {
            Fields::All => true,
            Fields::Ranges(ranges) => ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&field)),
        }
    }
}

struct Config {
    from: Scale,
    to: Scale,
    from_unit: f64,
    to_unit: f64,
    round: Round,
    format: Option<Format>,
    padding: i64,
    suffix: String,
    fields: Fields,
    delim: Option<u8>,
    invalid: Invalid,
    header: usize,
    zero: bool,
}

/// Runs the conversion; `status` becomes `EXIT_CONVERSION` when an invalid number was
/// reported under `--invalid=fail`.
struct Converter<'a> {
    config: &'a Config,
    status: i32,
}

enum Failure {
    Abort(i32),
    Write(io::Error),
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Failure::Write(error)
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
    let config = match config_from(&items) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let operands: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(text) => Some(text.clone()),
            Item::Opt { .. } => None,
        })
        .collect();
    let mut out = stdout_writer();
    let mut converter = Converter {
        config: &config,
        status: EXIT_OK,
    };
    let result = if operands.is_empty() {
        converter.stream(&mut out)
    } else {
        converter.operands(&operands, &mut out)
    };
    let code = match result {
        Ok(()) => converter.status,
        Err(Failure::Abort(code)) => code,
        Err(Failure::Write(error)) if is_broken_pipe(&error) => EXIT_OK,
        Err(Failure::Write(error)) => {
            eprintln!("{CMD}: write error: {}", io_error_text(&error));
            EXIT_FAIL
        }
    };
    let flushed = finish_output(CMD, &mut out);
    if code == EXIT_OK {
        flushed
    } else {
        code
    }
}

fn usage_error(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

/// GNU `argmatch` wording for a bad choice.
fn bad_choice(option: &str, value: &str, choices: &[&str]) -> i32 {
    let mut message =
        format!("{CMD}: invalid argument '{value}' for '--{option}'\nValid arguments are:\n");
    for choice in choices {
        message.push_str(&format!("  - '{choice}'\n"));
    }
    message.push_str(&format!("Try '{CMD} --help' for more information."));
    eprintln!("{message}");
    EXIT_FAIL
}

fn config_from(items: &[Item]) -> Result<Config, i32> {
    let mut config = Config {
        from: Scale::None,
        to: Scale::None,
        from_unit: 1.0,
        to_unit: 1.0,
        round: Round::FromZero,
        format: None,
        padding: 0,
        suffix: String::new(),
        fields: Fields::Ranges(vec![(1, 1)]),
        delim: None,
        invalid: Invalid::Abort,
        header: 0,
        zero: false,
    };
    for item in items {
        let Item::Opt { id, value } = item else {
            continue;
        };
        let text = value.clone().unwrap_or_default();
        match *id {
            "delimiter" => config.delim = Some(single_delimiter(&text)?),
            "field" => config.fields = parse_fields(&text)?,
            "format" => {
                config.format = Some(parse_format(&text).map_err(|message| usage_error(&message))?);
            }
            "from" => config.from = scale_choice("from", &text, FROM_CHOICES)?,
            "from-unit" => config.from_unit = unit_size(&text)?,
            "to" => config.to = scale_choice("to", &text, UNIT_CHOICES)?,
            "to-unit" => config.to_unit = unit_size(&text)?,
            "header" => config.header = header_count(value.as_deref())?,
            "invalid" => config.invalid = invalid_choice(&text)?,
            "padding" => config.padding = padding_value(&text)?,
            "round" => config.round = round_choice(&text)?,
            "suffix" => config.suffix = text,
            "zero" => config.zero = true,
            _ => {}
        }
    }
    Ok(config)
}

fn single_delimiter(text: &str) -> Result<u8, i32> {
    match text.as_bytes() {
        [byte] => Ok(*byte),
        _ => Err(usage_error("the delimiter must be a single character")),
    }
}

fn scale_choice(option: &str, text: &str, choices: &[&str]) -> Result<Scale, i32> {
    let scale = match text {
        "none" => Scale::None,
        "auto" if option == "from" => Scale::Auto,
        "si" => Scale::Si,
        "iec" => Scale::Iec,
        "iec-i" => Scale::IecI,
        _ => return Err(bad_choice(option, text, choices)),
    };
    Ok(scale)
}

fn round_choice(text: &str) -> Result<Round, i32> {
    Ok(match text {
        "up" => Round::Up,
        "down" => Round::Down,
        "from-zero" => Round::FromZero,
        "towards-zero" => Round::TowardsZero,
        "nearest" => Round::Nearest,
        _ => return Err(bad_choice("round", text, ROUND_CHOICES)),
    })
}

fn invalid_choice(text: &str) -> Result<Invalid, i32> {
    Ok(match text {
        "abort" => Invalid::Abort,
        "fail" => Invalid::Fail,
        "warn" => Invalid::Warn,
        "ignore" => Invalid::Ignore,
        _ => return Err(bad_choice("invalid", text, INVALID_CHOICES)),
    })
}

fn unit_size(text: &str) -> Result<f64, i32> {
    match text.parse::<u64>() {
        Ok(size) if size > 0 => Ok(size as f64),
        _ => Err(usage_error(&format!("invalid unit size: '{text}'"))),
    }
}

fn padding_value(text: &str) -> Result<i64, i32> {
    text.parse::<i64>()
        .map_err(|_| usage_error(&format!("invalid padding value '{text}'")))
}

fn header_count(value: Option<&str>) -> Result<usize, i32> {
    let Some(text) = value else { return Ok(1) };
    match text.parse::<usize>() {
        Ok(count) if count > 0 => Ok(count),
        _ => Err(usage_error(&format!("invalid header value '{text}'"))),
    }
}

/// Parses `--field`: `-` for all fields, or a comma-separated list of `N`, `N-M`, `N-`, `-M`.
fn parse_fields(text: &str) -> Result<Fields, i32> {
    if text == "-" {
        return Ok(Fields::All);
    }
    let invalid = || usage_error(&format!("invalid field value '{text}'"));
    let mut ranges = Vec::new();
    for part in text.split(',') {
        let (lo, hi) = match part.split_once('-') {
            Some((lo, hi)) => {
                let lo = if lo.is_empty() {
                    Some(1)
                } else {
                    lo.parse::<usize>().ok()
                };
                let hi = if hi.is_empty() {
                    Some(usize::MAX)
                } else {
                    hi.parse::<usize>().ok()
                };
                (lo.ok_or_else(invalid)?, hi.ok_or_else(invalid)?)
            }
            None => {
                let field = part.parse::<usize>().map_err(|_| invalid())?;
                (field, field)
            }
        };
        if lo == 0 || hi == 0 {
            return Err(usage_error("fields are numbered from 1"));
        }
        if lo > hi {
            return Err(invalid());
        }
        ranges.push((lo, hi));
    }
    Ok(Fields::Ranges(ranges))
}

impl Converter<'_> {
    /// Converts standard input line by line.
    fn stream(&mut self, out: &mut impl Write) -> Result<(), Failure> {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let delim = self.terminator();
        let mut line = Vec::new();
        let mut line_number = 0;
        loop {
            line.clear();
            if reader.read_until(delim, &mut line)? == 0 {
                return Ok(());
            }
            let terminated = line.last() == Some(&delim);
            if terminated {
                line.pop();
            }
            line_number += 1;
            self.line(&line, line_number, terminated, delim, out)?;
        }
    }

    /// Converts each operand as one terminated line.
    fn operands(&mut self, operands: &[String], out: &mut impl Write) -> Result<(), Failure> {
        let delim = self.terminator();
        for (index, operand) in operands.iter().enumerate() {
            self.line(operand.as_bytes(), index + 1, true, delim, out)?;
        }
        Ok(())
    }

    fn terminator(&self) -> u8 {
        if self.config.zero {
            0
        } else {
            b'\n'
        }
    }

    /// Converts one line, or copies it inside `--header`. The terminator is written only if
    /// the input had one, as GNU does for an unterminated last line.
    fn line(
        &mut self,
        line: &[u8],
        number: usize,
        terminated: bool,
        delim: u8,
        out: &mut impl Write,
    ) -> Result<(), Failure> {
        let converted = if number <= self.config.header {
            line.to_vec()
        } else {
            self.convert_line(line)?
        };
        out.write_all(&converted)?;
        if terminated {
            out.write_all(&[delim])?;
        }
        Ok(())
    }

    fn convert_line(&mut self, line: &[u8]) -> Result<Vec<u8>, Failure> {
        let (lead, mut items) = split_line(line, self.config.delim);
        if items.is_empty() && self.config.fields.contains(1) {
            // An empty or blank line has one empty field, which is not a number.
            items.push((Vec::new(), Vec::new()));
        }
        let mut result = lead;
        for (index, (text, separator)) in items.iter().enumerate() {
            let converted = if self.config.fields.contains(index + 1) {
                self.convert_field(text)?
            } else {
                text.clone()
            };
            result.extend_from_slice(&converted);
            result.extend_from_slice(separator);
        }
        Ok(result)
    }

    /// Converts one field; invalid input is handled by the `--invalid` policy.
    fn convert_field(&mut self, text: &[u8]) -> Result<Vec<u8>, Failure> {
        match convert_number(text, self.config) {
            Ok(converted) => Ok(converted),
            Err(message) => {
                self.invalid(&message)?;
                Ok(text.to_vec())
            }
        }
    }

    fn invalid(&mut self, message: &str) -> Result<(), Failure> {
        match self.config.invalid {
            Invalid::Abort => {
                eprintln!("{CMD}: {message}");
                Err(Failure::Abort(EXIT_CONVERSION))
            }
            Invalid::Fail => {
                eprintln!("{CMD}: {message}");
                self.status = EXIT_CONVERSION;
                Ok(())
            }
            Invalid::Warn => {
                eprintln!("{CMD}: {message}");
                Ok(())
            }
            Invalid::Ignore => Ok(()),
        }
    }
}

/// A field and the separator that follows it.
type Piece = (Vec<u8>, Vec<u8>);

/// Splits a line into its leading separator and `(field, following separator)` pairs. With
/// `-d`, every delimiter byte separates fields; otherwise runs of blanks do, and the leading
/// blanks become the separator before the first field.
fn split_line(line: &[u8], delim: Option<u8>) -> (Vec<u8>, Vec<Piece>) {
    let mut index = 0;
    let mut lead = Vec::new();
    if delim.is_none() {
        index = line
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(line.len());
        lead = line[..index].to_vec();
    }
    let mut pieces = Vec::new();
    while index < line.len() {
        let found = match delim {
            Some(byte) => line[index..].iter().position(|&b| b == byte),
            None => line[index..].iter().position(|b| b.is_ascii_whitespace()),
        };
        let text_end = found.map_or(line.len(), |offset| index + offset);
        let mut sep_end = text_end;
        match delim {
            Some(_) => {
                if sep_end < line.len() {
                    sep_end += 1;
                }
            }
            None => {
                while sep_end < line.len() && line[sep_end].is_ascii_whitespace() {
                    sep_end += 1;
                }
            }
        }
        pieces.push((
            line[index..text_end].to_vec(),
            line[text_end..sep_end].to_vec(),
        ));
        index = sep_end;
    }
    (lead, pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_lists_parse_like_gnu() {
        assert!(matches!(parse_fields("-"), Ok(Fields::All)));
        let fields = parse_fields("1,3-4").ok().unwrap();
        assert!(fields.contains(1) && fields.contains(4) && !fields.contains(2));
        assert!(parse_fields("0").is_err());
        assert!(parse_fields("x").is_err());
        assert!(parse_fields("3-1").is_err());
    }

    #[test]
    fn whitespace_fields_keep_their_separators() {
        let (lead, items) = split_line(b"  a  4096 ", None);
        assert_eq!(lead, b"  ");
        assert_eq!(
            items,
            vec![
                (b"a".to_vec(), b"  ".to_vec()),
                (b"4096".to_vec(), b" ".to_vec())
            ]
        );
    }

    #[test]
    fn delimiter_fields_keep_empty_fields() {
        let (lead, items) = split_line(b"a,,b", Some(b','));
        assert!(lead.is_empty());
        assert_eq!(items.len(), 3);
        assert_eq!(items[1], (Vec::new(), b",".to_vec()));
        assert!(split_line(b"", Some(b',')).1.is_empty());
    }

    #[test]
    fn choices_and_sizes_are_validated() {
        assert!(matches!(round_choice("nearest"), Ok(Round::Nearest)));
        assert!(round_choice("bogus").is_err());
        assert!(invalid_choice("warn").is_ok());
        assert!(unit_size("0").is_err());
        assert_eq!(header_count(None).ok(), Some(1));
        assert!(header_count(Some("0")).is_err());
        assert!(padding_value("-8").is_ok());
    }
}
