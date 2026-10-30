//! Number parsing, scaling, rounding and output text for `numfmt`.
//!
//! Values are `f64`. A value below the unit base is rounded to nearest-even, as GNU does when it
//! leaves a number unscaled; scaled values, `--to-unit`, and `--format` precision use the
//! `--round` method.

use super::{Config, Round, Scale};

/// Unit letters, indexed by the number of powers applied (`' '` for none).
const PREFIXES: &[u8] = b" KMGTPEZY";

/// A parsed `--format`: literal text around one `%[0][-][W][.P]f` directive.
#[derive(Clone, Debug)]
pub(super) struct Format {
    prefix: String,
    postfix: String,
    zero: bool,
    left: bool,
    width: usize,
    precision: Option<usize>,
}

/// Parses `%[0][\'][-][WIDTH][.PRECISION]f` with literal text around it.
pub(super) fn parse_format(text: &str) -> Result<Format, String> {
    let percent_count = text.matches('%').count();
    if percent_count == 0 {
        return Err(format!("format '{text}' has no % directive"));
    }
    if percent_count > 1 {
        return Err(format!("format '{text}' has too many % directives"));
    }
    let start = text.find('%').unwrap_or(0);
    let bad = || format!("invalid format '{text}', directive must be %[0]['][-][N][.][N]f");
    let rest = &text[start + 1..];
    let bytes = rest.as_bytes();
    let mut pos = 0;
    let mut format = Format {
        prefix: text[..start].to_owned(),
        postfix: String::new(),
        zero: false,
        left: false,
        width: 0,
        precision: None,
    };
    if bytes.get(pos) == Some(&b'0') {
        format.zero = true;
        pos += 1;
    }
    if bytes.get(pos) == Some(&b'\'') {
        pos += 1;
    }
    if bytes.get(pos) == Some(&b'-') {
        format.left = true;
        pos += 1;
    }
    let width_start = pos;
    while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
        pos += 1;
    }
    if pos > width_start {
        format.width = rest[width_start..pos].parse().map_err(|_| bad())?;
    }
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let precision_start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        let digits = &rest[precision_start..pos];
        format.precision = Some(if digits.is_empty() {
            0
        } else {
            digits.parse().map_err(|_| bad())?
        });
    }
    if bytes.get(pos) != Some(&b'f') {
        return Err(bad());
    }
    format.postfix = rest[pos + 1..].to_owned();
    Ok(format)
}

/// Converts one field's text. `Err` carries the GNU diagnostic for an invalid number.
pub(super) fn convert_number(text: &[u8], config: &Config) -> Result<Vec<u8>, String> {
    let token = String::from_utf8_lossy(text).into_owned();
    let trimmed = token.trim_start();
    let parsed = parse_number(trimmed, config.from).map_err(|kind| match kind {
        NumberError::Invalid => format!("invalid number: '{token}'"),
        NumberError::Suffix => format!("invalid suffix in input: '{token}'"),
        NumberError::Rejected => {
            format!("rejecting suffix in input: '{token}' (consider using --from)")
        }
    })?;
    let value = parsed.value * config.from_unit;
    let unchanged = config.to == Scale::None
        && !parsed.scaled
        && config.from_unit == 1.0
        && config.to_unit == 1.0
        && config.format.is_none();
    let number = if unchanged {
        plain_text(trimmed)
    } else {
        render(value, config)
    };
    let body = format!("{number}{}", config.suffix);
    Ok(finish_field(&body, config).into_bytes())
}

/// An input number printed as it was read; GNU adds the leading zero to `.5`.
fn plain_text(text: &str) -> String {
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'+' | b'-') => text.split_at(1),
        _ => ("", text),
    };
    if digits.starts_with('.') {
        format!("{sign}0{digits}")
    } else {
        text.to_owned()
    }
}

/// The number text (without padding, format or suffix text) for a value in base units.
fn render(value: f64, config: &Config) -> String {
    let precision = config.format.as_ref().and_then(|format| format.precision);
    let scaled = value / config.to_unit;
    let (base, with_i) = match config.to {
        Scale::None => {
            let places = precision.unwrap_or(0);
            return format_decimal(round_to(scaled, places, config.round), places);
        }
        Scale::Si | Scale::Auto => (1000.0, false),
        Scale::Iec => (1024.0, false),
        Scale::IecI => (1024.0, true),
    };
    let (number, index) = scale_to_unit(scaled, base, precision, config.round);
    let mut text = number;
    if index > 0 {
        text.push(PREFIXES[index] as char);
        if with_i {
            text.push('i');
        }
    }
    text
}

/// Divides by `base` until the rounded value is below it. A value that rounds up to a full
/// unit moves to the next prefix. Returns the text and the number of divisions.
fn scale_to_unit(value: f64, base: f64, precision: Option<usize>, round: Round) -> (String, usize) {
    let mut scaled = value;
    let mut index = 0;
    loop {
        let places = precision.unwrap_or(if index > 0 && scaled.abs() < 10.0 {
            1
        } else {
            0
        });
        let mode = if index == 0 && precision.is_none() {
            Round::Nearest
        } else {
            round
        };
        let rounded = round_to(scaled, places, mode);
        if rounded.abs() >= base && index + 1 < PREFIXES.len() {
            scaled /= base;
            index += 1;
            continue;
        }
        return (format_decimal(rounded, places), index);
    }
}

/// Rounds to `places` decimal places. Values within 1e-9 of an integer count as that integer,
/// so that `1.1 * 10` is not rounded up.
fn round_to(value: f64, places: usize, mode: Round) -> f64 {
    let factor = 10f64.powi(places as i32);
    let x = value * factor;
    let nearest = x.round();
    let x = if (x - nearest).abs() < 1e-9 {
        nearest
    } else {
        match mode {
            Round::Up => x.ceil(),
            Round::Down => x.floor(),
            Round::FromZero => {
                if x < 0.0 {
                    x.floor()
                } else {
                    x.ceil()
                }
            }
            Round::TowardsZero => x.trunc(),
            Round::Nearest => x.round_ties_even(),
        }
    };
    x / factor
}

fn format_decimal(value: f64, places: usize) -> String {
    format!("{value:.places$}")
}

/// Applies `--format` around the number and `--padding` around the whole field.
fn finish_field(body: &str, config: &Config) -> String {
    let mut text = match &config.format {
        Some(format) => format!(
            "{}{}{}",
            format.prefix,
            pad_inner(body, format),
            format.postfix
        ),
        None => body.to_owned(),
    };
    let width = config.padding.unsigned_abs() as usize;
    let length = text.chars().count();
    if length >= width {
        return text;
    }
    let fill = " ".repeat(width - length);
    if config.padding < 0 {
        text.push_str(&fill);
        text
    } else {
        format!("{fill}{text}")
    }
}

/// Pads the number, its unit and suffix to the format's width.
fn pad_inner(body: &str, format: &Format) -> String {
    let length = body.chars().count();
    if length >= format.width {
        return body.to_owned();
    }
    let fill = format.width - length;
    if format.left {
        format!("{body}{}", " ".repeat(fill))
    } else if format.zero {
        format!("{}{body}", "0".repeat(fill))
    } else {
        format!("{}{body}", " ".repeat(fill))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum NumberError {
    Invalid,
    Suffix,
    Rejected,
}

struct Parsed {
    value: f64,
    /// True when the input carried a unit suffix.
    scaled: bool,
}

/// Reads a number with an optional sign, decimal point and unit suffix allowed by `scale`.
fn parse_number(text: &str, scale: Scale) -> Result<Parsed, NumberError> {
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end += 1;
    }
    let digits_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut digit_count = end - digits_start;
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let fraction_start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        digit_count += end - fraction_start;
    }
    if digit_count == 0 {
        return Err(NumberError::Invalid);
    }
    let number: f64 = text[..end].parse().map_err(|_| NumberError::Invalid)?;
    let rest = &text[end..];
    if rest.is_empty() {
        return Ok(Parsed {
            value: number,
            scaled: false,
        });
    }
    let factor = suffix_factor(rest, scale)?;
    Ok(Parsed {
        value: number * factor,
        scaled: true,
    })
}

/// The multiplier for a unit suffix such as `K` or `Mi`. A well-formed suffix that `scale`
/// does not accept is `Rejected` (with `Scale::None`) or `Suffix` otherwise.
fn suffix_factor(rest: &str, scale: Scale) -> Result<f64, NumberError> {
    let (letter, with_i) = match rest.as_bytes() {
        [letter] => (*letter, false),
        [letter, b'i'] => (*letter, true),
        _ => return Err(NumberError::Suffix),
    };
    let Some(index) = PREFIXES
        .iter()
        .skip(1)
        .position(|&p| p == letter)
        .map(|i| i + 1)
    else {
        return Err(NumberError::Suffix);
    };
    if scale == Scale::None {
        return Err(NumberError::Rejected);
    }
    let base: f64 = match (scale, with_i) {
        (Scale::Si, false) | (Scale::Auto, false) => 1000.0,
        (Scale::Iec, false) | (Scale::IecI, _) | (Scale::Auto, true) => 1024.0,
        _ => return Err(NumberError::Suffix),
    };
    Ok(base.powi(index as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            from: Scale::None,
            to: Scale::None,
            from_unit: 1.0,
            to_unit: 1.0,
            round: Round::FromZero,
            format: None,
            padding: 0,
            suffix: String::new(),
            fields: super::super::Fields::Ranges(vec![(1, 1)]),
            delim: None,
            invalid: super::super::Invalid::Abort,
            header: 0,
            zero: false,
        }
    }

    fn convert(text: &str, config: &Config) -> Result<String, String> {
        convert_number(text.as_bytes(), config).map(|bytes| String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn scales_to_si_and_iec_with_one_decimal_below_ten() {
        let mut cfg = config();
        cfg.to = Scale::Si;
        assert_eq!(convert("1234", &cfg).unwrap(), "1.3K");
        assert_eq!(convert("999999", &cfg).unwrap(), "1.0M");
        cfg.to = Scale::Iec;
        assert_eq!(convert("4096", &cfg).unwrap(), "4.0K");
        assert_eq!(convert("2049", &cfg).unwrap(), "2.1K");
        cfg.to = Scale::IecI;
        assert_eq!(convert("1536", &cfg).unwrap(), "1.5Ki");
    }

    #[test]
    fn values_below_the_base_round_to_nearest_even() {
        let mut cfg = config();
        cfg.to = Scale::Si;
        cfg.round = Round::Up;
        assert_eq!(convert("1.5", &cfg).unwrap(), "2");
        assert_eq!(convert("0.5", &cfg).unwrap(), "0");
        assert_eq!(convert("2.1", &cfg).unwrap(), "2");
    }

    #[test]
    fn parses_suffixes_according_to_the_input_scale() {
        let mut cfg = config();
        cfg.from = Scale::Si;
        assert_eq!(convert("1K", &cfg).unwrap(), "1000");
        cfg.from = Scale::Iec;
        assert_eq!(convert("4K", &cfg).unwrap(), "4096");
        assert!(convert("4Ki", &cfg).is_err());
        cfg.from = Scale::Auto;
        assert_eq!(convert("4Ki", &cfg).unwrap(), "4096");
        assert_eq!(convert("4K", &cfg).unwrap(), "4000");
    }

    #[test]
    fn unchanged_numbers_keep_their_text() {
        let cfg = config();
        assert_eq!(convert("1.50", &cfg).unwrap(), "1.50");
        assert_eq!(convert(".5", &cfg).unwrap(), "0.5");
        assert_eq!(
            convert("1K", &cfg).unwrap_err(),
            "rejecting suffix in input: '1K' (consider using --from)"
        );
        assert_eq!(
            convert("12abc", &cfg).unwrap_err(),
            "invalid suffix in input: '12abc'"
        );
        assert_eq!(convert("abc", &cfg).unwrap_err(), "invalid number: 'abc'");
        assert_eq!(convert("", &cfg).unwrap_err(), "invalid number: ''");
    }

    #[test]
    fn to_unit_uses_the_rounding_method() {
        let mut cfg = config();
        cfg.to_unit = 1000.0;
        cfg.round = Round::Up;
        assert_eq!(convert("1200", &cfg).unwrap(), "2");
        cfg.round = Round::Down;
        assert_eq!(convert("1800", &cfg).unwrap(), "1");
    }

    #[test]
    fn rounding_modes_round_as_named() {
        assert_eq!(round_to(1.2, 0, Round::Up), 2.0);
        assert_eq!(round_to(1.9, 0, Round::Down), 1.0);
        assert_eq!(round_to(-1.2, 0, Round::FromZero), -2.0);
        assert_eq!(round_to(-1.9, 0, Round::TowardsZero), -1.0);
        assert_eq!(round_to(2.5, 0, Round::Nearest), 2.0);
        assert_eq!(round_to(1.1 * 10.0, 0, Round::Up), 11.0);
    }

    #[test]
    fn format_wraps_the_number_and_padding_wraps_the_field() {
        let mut cfg = config();
        cfg.format = Some(parse_format("x%.2fy").unwrap());
        assert_eq!(convert("1500", &cfg).unwrap(), "x1500.00y");
        cfg.format = Some(parse_format("%10f").unwrap());
        cfg.to = Scale::Iec;
        assert_eq!(convert("2048", &cfg).unwrap(), "      2.0K");
        cfg.format = None;
        cfg.padding = -8;
        assert_eq!(convert("4096", &cfg).unwrap(), "4.0K    ");
        cfg.padding = 6;
        assert_eq!(convert("5", &cfg).unwrap(), "     5");
    }

    #[test]
    fn suffix_is_appended_to_every_output_number() {
        let mut cfg = config();
        cfg.to = Scale::Iec;
        cfg.suffix = "B".into();
        assert_eq!(convert("4096", &cfg).unwrap(), "4.0KB");
    }

    #[test]
    fn format_directive_is_validated() {
        assert!(parse_format("%d").is_err());
        assert!(parse_format("%f %f").unwrap_err().contains("too many"));
        assert!(parse_format("no directive").unwrap_err().contains("no %"));
        assert!(parse_format("%08.2f").is_ok());
    }
}
