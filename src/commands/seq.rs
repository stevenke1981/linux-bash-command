//! `seq`: print a sequence of numbers.
//!
//! Numbers written in plain decimal are iterated exactly as scaled integers, so `0.1`
//! steps never drift. Exponent notation, `inf` and very long operands fall back to `f64`.

use std::io::{self, Write};

use crate::commands::printf::float::{format_float, Spec};
use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{is_broken_pipe, stdout_writer};

const NAME: &str = "seq";

const HELP: &str = "\
Usage: seq [OPTION]... LAST
  or:  seq [OPTION]... FIRST LAST
  or:  seq [OPTION]... FIRST INCREMENT LAST
Print numbers from FIRST to LAST, in steps of INCREMENT.

Mandatory arguments to long options are mandatory for short options too.
  -f, --format=FORMAT      use printf style floating-point FORMAT (one of %a %e %f %g)
  -s, --separator=STRING   use STRING to separate numbers (default: \\n)
  -w, --equal-width        equalize width by padding with leading zeroes
      --help               display this help and exit
      --version            output version information and exit

If FIRST or INCREMENT is omitted, it defaults to 1.  That is, an omitted
INCREMENT defaults to 1 even when LAST is smaller than FIRST.
FIRST, INCREMENT and LAST are decimal numbers (exponents and inf are accepted).
INCREMENT must not be zero.
";

const OPTIONS: &[OptSpec] = &[
    OptSpec::req("format", Some('f'), Some("format")),
    OptSpec::req("separator", Some('s'), Some("separator")),
    OptSpec::flag("equal", Some('w'), Some("equal-width")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// One operand: an exact scaled integer (`mant / 10^scale`) or an `f64`.
#[derive(Clone, Copy, Debug)]
enum Num {
    Exact { mant: i128, scale: u32 },
    Approx { value: f64, scale: u32 },
}

impl Num {
    fn scale(self) -> u32 {
        match self {
            Num::Exact { scale, .. } | Num::Approx { scale, .. } => scale,
        }
    }

    fn as_f64(self) -> f64 {
        match self {
            Num::Exact { mant, scale } => mant as f64 / 10f64.powi(scale as i32),
            Num::Approx { value, .. } => value,
        }
    }

    fn is_zero(self) -> bool {
        match self {
            Num::Exact { mant, .. } => mant == 0,
            Num::Approx { value, .. } => value == 0.0,
        }
    }
}

/// The iteration: exact integers at a common scale, or `f64` values.
#[derive(Debug)]
enum Plan {
    Exact {
        first: i128,
        step: i128,
        last: i128,
        scale: u32,
        precision: u32,
    },
    Approx {
        first: f64,
        step: f64,
        last: f64,
        precision: u32,
    },
}

/// One value produced by the iteration.
#[derive(Clone, Copy)]
enum Step {
    Exact(i128),
    Approx(f64),
}

/// A `-f` format: text around one floating point conversion.
#[derive(Debug)]
struct FloatFormat {
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    conv: u8,
    spec: Spec,
}

/// Entry point for `seq`.
pub fn run(args: &[String]) -> i32 {
    let mut parser = Parser::new(NAME, OPTIONS).no_permute();
    parser.numeric_short = true;
    let items = match parser.parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    let mut format_text = None;
    let mut separator = "\n".to_owned();
    let mut equal_width = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(NAME),
            Item::Opt {
                id: "format",
                value,
            } => format_text = value,
            Item::Opt {
                id: "separator",
                value,
            } => separator = value.unwrap_or_default(),
            Item::Opt { id: "equal", .. } => equal_width = true,
            Item::Opt { .. } => {}
            Item::Operand(text) => operands.push(text),
        }
    }
    let (first, step, last) = match operands.as_slice() {
        [] => return usage_error("missing operand"),
        [last] => ("1".to_owned(), "1".to_owned(), last.clone()),
        [first, last] => (first.clone(), "1".to_owned(), last.clone()),
        [first, step, last] => (first.clone(), step.clone(), last.clone()),
        [_, _, _, extra, ..] => return usage_error(&format!("extra operand '{extra}'")),
    };
    match sequence(
        &first,
        &step,
        &last,
        format_text.as_deref(),
        equal_width,
        &separator,
    ) {
        Ok(()) => EXIT_OK,
        Err(Problem::Usage(message)) => usage_error(&message),
        Err(Problem::Format(message)) => {
            eprintln!("{NAME}: {message}");
            EXIT_FAIL
        }
        Err(Problem::Io(error)) if is_broken_pipe(&error) => EXIT_OK,
        Err(Problem::Io(error)) => {
            eprintln!("{NAME}: write error: {}", io_error_text(&error));
            EXIT_FAIL
        }
    }
}

fn usage_error(message: &str) -> i32 {
    let error = UsageError {
        message: message.to_owned(),
    };
    report_usage(NAME, &error, EXIT_FAIL)
}

/// Why `seq` stopped before or while printing.
enum Problem {
    /// Bad operand; printed with the `Try` line.
    Usage(String),
    /// Bad `-f` format; printed alone.
    Format(String),
    Io(io::Error),
}

impl From<io::Error> for Problem {
    fn from(error: io::Error) -> Self {
        Problem::Io(error)
    }
}

fn sequence(
    first: &str,
    step: &str,
    last: &str,
    format_text: Option<&str>,
    equal_width: bool,
    separator: &str,
) -> Result<(), Problem> {
    let first_num = parse_number(first).map_err(Problem::Usage)?;
    let step_num = parse_number(step).map_err(Problem::Usage)?;
    let last_num = parse_number(last).map_err(Problem::Usage)?;
    if step_num.is_zero() {
        return Err(Problem::Usage(format!(
            "invalid Zero increment value: '{step}'"
        )));
    }
    let format = match format_text {
        Some(text) => Some(parse_float_format(text).map_err(Problem::Format)?),
        None => None,
    };
    let plan = make_plan(first_num, step_num, last_num);
    let precision = plan_precision(&plan);
    let width = match (&format, equal_width) {
        (None, true) => Some(equal_width_of(first_num, step_num, last_num, precision)),
        _ => None,
    };
    print_plan(&plan, precision, width, format.as_ref(), separator)?;
    Ok(())
}

/// Parses an operand: optional white space and sign, decimal digits with an optional
/// point and exponent, a `0x` integer, or `inf`/`infinity`.
fn parse_number(text: &str) -> Result<Num, String> {
    let invalid = || format!("invalid floating point argument: '{text}'");
    let trimmed = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, body) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let lower = body.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" {
        let value = if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        return Ok(Num::Approx { value, scale: 0 });
    }
    if lower.starts_with("nan") {
        return Err(format!("invalid 'not-a-number' argument: '{text}'"));
    }
    if let Some(hex) = lower.strip_prefix("0x") {
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let magnitude = u128::from_str_radix(hex, 16).unwrap_or(u128::MAX);
        return Ok(match i128::try_from(magnitude) {
            Ok(mant) => Num::Exact {
                mant: if negative { -mant } else { mant },
                scale: 0,
            },
            Err(_) => Num::Approx {
                value: if negative {
                    -(magnitude as f64)
                } else {
                    magnitude as f64
                },
                scale: 0,
            },
        });
    }
    let (int_part, frac_part, exponent) = split_decimal(body).ok_or_else(invalid)?;
    Ok(decimal_num(negative, int_part, frac_part, exponent))
}

/// Splits `digits[.digits][e[+-]digits]` into its parts. `None` when malformed.
fn split_decimal(body: &str) -> Option<(&str, &str, i64)> {
    let bytes = body.as_bytes();
    let int_end = bytes
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(bytes.len());
    let int_part = &body[..int_end];
    let mut index = int_end;
    let mut frac_part = "";
    if bytes.get(index) == Some(&b'.') {
        let frac_end = bytes[index + 1..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .map_or(bytes.len(), |p| index + 1 + p);
        frac_part = &body[index + 1..frac_end];
        index = frac_end;
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut exponent = 0i64;
    if matches!(bytes.get(index), Some(b'e') | Some(b'E')) {
        let rest = &body[index + 1..];
        let sign_len = usize::from(rest.starts_with(['+', '-']));
        let digits = rest[sign_len..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits == 0 {
            return None;
        }
        let magnitude: i64 = rest[sign_len..sign_len + digits]
            .parse()
            .unwrap_or(i64::MAX / 4);
        exponent = if rest.starts_with('-') {
            -magnitude
        } else {
            magnitude
        };
        index += 1 + sign_len + digits;
    }
    (index == bytes.len()).then_some((int_part, frac_part, exponent))
}

/// Builds the number `[-]int.frac * 10^exponent`, exact when it fits.
fn decimal_num(negative: bool, int_part: &str, frac_part: &str, exponent: i64) -> Num {
    let digits = format!("{int_part}{frac_part}");
    let scale_raw = frac_part.len() as i64 - exponent;
    let sign = if negative { -1 } else { 1 };
    let exact = if digits.len() <= 36 {
        digits.parse::<i128>().ok()
    } else {
        None
    };
    if let Some(mant) = exact {
        if (-38..=0).contains(&scale_raw) {
            let factor = 10i128.checked_pow(scale_raw.unsigned_abs() as u32);
            if let Some(scaled) = factor.and_then(|f| mant.checked_mul(f)) {
                return Num::Exact {
                    mant: sign * scaled,
                    scale: 0,
                };
            }
        } else if (1..=30).contains(&scale_raw) {
            return Num::Exact {
                mant: sign * mant,
                scale: scale_raw as u32,
            };
        }
    }
    let value = format!("{digits}e{}", -scale_raw)
        .parse::<f64>()
        .unwrap_or(f64::NAN);
    Num::Approx {
        value: sign as f64 * value,
        scale: scale_raw.clamp(0, 1000) as u32,
    }
}

fn make_plan(first: Num, step: Num, last: Num) -> Plan {
    let precision = first.scale().max(step.scale());
    let scale = precision.max(last.scale());
    let exact = (
        exact_at(first, scale),
        exact_at(step, scale),
        exact_at(last, scale),
    );
    match exact {
        (Some(first), Some(step), Some(last)) => Plan::Exact {
            first,
            step,
            last,
            scale,
            precision,
        },
        _ => Plan::Approx {
            first: first.as_f64(),
            step: step.as_f64(),
            last: last.as_f64(),
            precision,
        },
    }
}

/// `n` as an integer at `scale`, when it is exact and representable.
fn exact_at(n: Num, scale: u32) -> Option<i128> {
    match n {
        Num::Exact { mant, scale: own } => {
            let factor = 10i128.checked_pow(scale.checked_sub(own)?)?;
            mant.checked_mul(factor)
        }
        Num::Approx { .. } => None,
    }
}

/// The scale of an exact plan (0 for `f64` plans).
fn plan_scale(plan: &Plan) -> u32 {
    match plan {
        Plan::Exact { scale, .. } => *scale,
        Plan::Approx { .. } => 0,
    }
}

/// Text of one value in the default format: `precision` fraction digits, no sign-aware
/// padding.
fn plain_step_text(step: Step, scale: u32, precision: u32) -> String {
    match step {
        Step::Exact(mant) => fixed_text(mant / 10i128.pow(scale - precision), precision),
        Step::Approx(value) => format!("{value:.p$}", p = precision as usize),
    }
}

fn plan_precision(plan: &Plan) -> u32 {
    match plan {
        Plan::Exact { precision, .. } | Plan::Approx { precision, .. } => *precision,
    }
}

/// Calls `visit` for each value from the first through the last, in order.
fn walk(plan: &Plan, mut visit: impl FnMut(Step) -> io::Result<()>) -> io::Result<()> {
    match *plan {
        Plan::Exact {
            first, step, last, ..
        } => {
            let mut current = first;
            while if step > 0 {
                current <= last
            } else {
                current >= last
            } {
                visit(Step::Exact(current))?;
                match current.checked_add(step) {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }
        Plan::Approx {
            first, step, last, ..
        } => {
            let mut current = first;
            while if step > 0.0 {
                current <= last
            } else {
                current >= last
            } {
                visit(Step::Approx(current))?;
                current += step;
            }
        }
    }
    Ok(())
}

/// The field width for `-w`: the widest of FIRST, LAST and INCREMENT at the output precision.
fn equal_width_of(first: Num, step: Num, last: Num, precision: u32) -> usize {
    [first, step, last]
        .into_iter()
        .map(|n| plain_text(n, precision).len())
        .max()
        .unwrap_or(1)
}

fn plain_text(n: Num, precision: u32) -> String {
    match n {
        Num::Exact { mant, scale } => {
            let value = if scale >= precision {
                mant / 10i128.pow(scale - precision)
            } else {
                mant * 10i128.pow(precision - scale)
            };
            fixed_text(value, precision)
        }
        Num::Approx { value, .. } => format!("{value:.p$}", p = precision as usize),
    }
}

/// Renders `value * 10^-decimals` with exactly `decimals` fraction digits.
fn fixed_text(value: i128, decimals: u32) -> String {
    let negative = value < 0;
    let mut digits = value.unsigned_abs().to_string();
    let decimals = decimals as usize;
    if decimals > 0 {
        if digits.len() <= decimals {
            digits.insert_str(0, &"0".repeat(decimals + 1 - digits.len()));
        }
        digits.insert(digits.len() - decimals, '.');
    }
    if negative {
        digits.insert(0, '-');
    }
    digits
}

/// Pads `text` with leading zeros to `width`, after any minus sign.
fn zero_pad(text: &str, width: usize) -> String {
    if text.len() >= width {
        return text.to_owned();
    }
    let fill = "0".repeat(width - text.len());
    match text.strip_prefix('-') {
        Some(rest) => format!("-{fill}{rest}"),
        None => format!("{fill}{text}"),
    }
}

fn print_plan(
    plan: &Plan,
    precision: u32,
    width: Option<usize>,
    format: Option<&FloatFormat>,
    separator: &str,
) -> Result<(), Problem> {
    let mut out = stdout_writer();
    let mut printed = 0usize;
    let scale = plan_scale(plan);
    walk(plan, |step| {
        if printed > 0 {
            out.write_all(separator.as_bytes())?;
        }
        printed += 1;
        let text = match format {
            Some(format) => format_with(format, step_value(step, scale)),
            None => {
                let text = plain_step_text(step, scale, precision);
                match width {
                    Some(width) => zero_pad(&text, width),
                    None => text,
                }
            }
        };
        out.write_all(text.as_bytes())
    })?;
    if printed > 0 {
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
}

fn step_value(step: Step, scale: u32) -> f64 {
    match step {
        Step::Exact(mant) => mant as f64 / 10f64.powi(scale as i32),
        Step::Approx(value) => value,
    }
}

fn format_with(format: &FloatFormat, value: f64) -> String {
    let mut body = Vec::new();
    format_float(&mut body, value, format.conv, &format.spec);
    let mut text = format.prefix.clone();
    text.extend_from_slice(&body);
    text.extend_from_slice(&format.suffix);
    String::from_utf8_lossy(&text).into_owned()
}

/// Parses a `-f` format: literal text, `%%`, and exactly one floating point conversion.
fn parse_float_format(text: &str) -> Result<FloatFormat, String> {
    let bytes = text.as_bytes();
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut found: Option<(u8, Spec)> = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let target = if found.is_some() {
            &mut suffix
        } else {
            &mut prefix
        };
        if byte != b'%' {
            target.push(byte);
            index += 1;
            continue;
        }
        if bytes.get(index + 1) == Some(&b'%') {
            target.push(b'%');
            index += 2;
            continue;
        }
        if found.is_some() {
            return Err(format!("format '{text}' has too many % directives"));
        }
        let (directive, next) = parse_directive(text, index + 1)?;
        found = Some(directive);
        index = next;
    }
    let Some((conv, spec)) = found else {
        return Err(format!("format '{text}' has no % directive"));
    };
    Ok(FloatFormat {
        prefix,
        suffix,
        conv,
        spec,
    })
}

/// Parses flags, width, precision and the conversion after `%`.
fn parse_directive(text: &str, start: usize) -> Result<((u8, Spec), usize), String> {
    let bytes = text.as_bytes();
    let mut index = start;
    let mut spec = Spec::default();
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'-' => spec.left = true,
            b'+' => spec.plus = true,
            b' ' => spec.space = true,
            b'#' => spec.alt = true,
            b'0' => spec.zero = true,
            b'\'' => {}
            _ => break,
        }
        index += 1;
    }
    if bytes.get(index) == Some(&b'*') {
        return Err(format!("format '{text}' has unknown %* directive"));
    }
    let width_end = index
        + bytes[index..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
    spec.width = text[index..width_end].parse().unwrap_or(0);
    index = width_end;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        if bytes.get(index) == Some(&b'*') {
            return Err(format!("format '{text}' has unknown %* directive"));
        }
        let end = index
            + bytes[index..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
        spec.precision = Some(text[index..end].parse().unwrap_or(0));
        index = end;
    }
    while bytes.get(index).is_some_and(|b| b"Lhlzjt".contains(b)) {
        index += 1;
    }
    match bytes.get(index) {
        None => Err(format!("format '{text}' ends in %")),
        Some(&conv) if b"aAeEfFgG".contains(&conv) => Ok(((conv, spec), index + 1)),
        Some(&conv) => Err(format!(
            "format '{text}' has unknown %{} directive",
            conv as char
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(first: &str, step: &str, last: &str) -> String {
        let plan_first = parse_number(first).unwrap();
        let plan_step = parse_number(step).unwrap();
        let plan_last = parse_number(last).unwrap();
        let plan = make_plan(plan_first, plan_step, plan_last);
        let precision = plan_precision(&plan);
        let scale = plan_scale(&plan);
        let mut texts = Vec::new();
        walk(&plan, |step| {
            texts.push(plain_step_text(step, scale, precision));
            Ok(())
        })
        .unwrap();
        texts.join(" ")
    }

    #[test]
    fn decimal_steps_do_not_drift() {
        let plan = make_plan(
            parse_number("0").unwrap(),
            parse_number("0.1").unwrap(),
            parse_number("1").unwrap(),
        );
        assert_eq!(plan_precision(&plan), 1);
        let mut count = 0;
        walk(&plan, |_| {
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, 11);
    }

    #[test]
    fn precision_comes_from_first_and_step_only() {
        assert_eq!(lines("1", "1.5", "4"), "1.0 2.5 4.0");
        assert_eq!(lines("0.5", "1", "2.50"), "0.5 1.5 2.5");
        assert_eq!(lines("10", "-3", "1"), "10 7 4 1");
    }

    #[test]
    fn exponent_notation_sets_precision_like_gnu() {
        assert_eq!(lines("1", "1e-2", "1.03"), "1.00 1.01 1.02 1.03");
        assert_eq!(lines("1e1", "1", "12"), "10 11 12");
    }

    #[test]
    fn invalid_and_zero_increment_are_rejected() {
        assert!(parse_number("abc").is_err());
        assert!(parse_number("2 ").is_err());
        assert!(parse_number("nan").unwrap_err().contains("not-a-number"));
        assert_eq!(parse_number("0x10").map(|n| n.as_f64()), Ok(16.0));
    }

    #[test]
    fn equal_width_pads_after_the_sign() {
        assert_eq!(zero_pad("-5", 3), "-05");
        assert_eq!(zero_pad("7", 3), "007");
        assert_eq!(zero_pad("1234", 2), "1234");
    }

    #[test]
    fn float_format_requires_one_float_directive() {
        assert!(parse_float_format("x")
            .unwrap_err()
            .contains("has no % directive"));
        assert!(parse_float_format("%d")
            .unwrap_err()
            .contains("unknown %d directive"));
        assert!(parse_float_format("%f %f")
            .unwrap_err()
            .contains("too many"));
        let format = parse_float_format("n%05.1f:%%").unwrap();
        assert_eq!(format_with(&format, 1.0), "n001.0:%");
    }
}
