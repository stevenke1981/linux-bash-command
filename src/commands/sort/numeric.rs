//! Numeric sort keys: `-n` (exact decimal), `-g` (floating point with hex and
//! exponents), `-h` (human-readable sizes compared by unit first) and `-M` (month names).

use std::cmp::Ordering;

fn is_blank(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// An exact decimal read from a key: sign, integer digits and fraction digits, both with
/// insignificant zeros removed so that equal values compare equal.
#[derive(Debug, PartialEq, Eq)]
struct Decimal<'a> {
    negative: bool,
    int: &'a [u8],
    frac: &'a [u8],
}

/// Reads the leading decimal number of `s` (`-n` syntax: blanks, optional `-`, digits,
/// optional `.` and digits; no `+`, no exponent). Anything else reads as zero.
fn parse_decimal(s: &[u8]) -> Decimal<'_> {
    let mut i = 0;
    while i < s.len() && is_blank(s[i]) {
        i += 1;
    }
    let negative = i < s.len() && s[i] == b'-';
    if negative {
        i += 1;
    }
    let int_start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let int_all = &s[int_start..i];
    let mut frac: &[u8] = &[];
    if i < s.len() && s[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        frac = &s[frac_start..j];
    }
    let int = trim_leading_zeros(int_all);
    let frac = trim_trailing_zeros(frac);
    Decimal {
        negative: negative && !(int.is_empty() && frac.is_empty()),
        int,
        frac,
    }
}

fn trim_leading_zeros(digits: &[u8]) -> &[u8] {
    let skip = digits.iter().take_while(|b| **b == b'0').count();
    &digits[skip..]
}

fn trim_trailing_zeros(digits: &[u8]) -> &[u8] {
    let keep = digits.iter().rposition(|b| *b != b'0').map_or(0, |p| p + 1);
    &digits[..keep]
}

/// Orders two non-negative decimals by magnitude.
fn magnitude(a: &Decimal, b: &Decimal) -> Ordering {
    a.int
        .len()
        .cmp(&b.int.len())
        .then_with(|| a.int.cmp(b.int))
        .then_with(|| a.frac.cmp(b.frac))
}

/// `sort -n`: compares the leading decimal numbers exactly, without precision loss.
pub fn compare_numeric(a: &[u8], b: &[u8]) -> Ordering {
    let x = parse_decimal(a);
    let y = parse_decimal(b);
    match (x.negative, y.negative) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => magnitude(&x, &y),
        (true, true) => magnitude(&y, &x),
    }
}

/// The value `sort -g` assigns to a key: unparsable text sorts first, then NaN, then
/// numbers (infinities included) in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum General {
    Invalid,
    NaN,
    Number(f64),
}

/// Parses the leading floating-point number of `s` the way `strtold` does in the C locale:
/// leading whitespace, sign, decimal digits with an optional fraction and exponent, hex
/// integers and fractions, `inf`/`infinity` and `nan`.
pub fn parse_general(s: &[u8]) -> General {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let negative = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let rest = &s[i..];
    let lower: Vec<u8> = rest
        .iter()
        .take(8)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if lower.starts_with(b"inf") {
        return General::Number(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if lower.starts_with(b"nan") {
        return General::NaN;
    }
    if lower.starts_with(b"0x") {
        if let Some(value) = parse_hex(&rest[2..]) {
            return General::Number(if negative { -value } else { value });
        }
    }
    match parse_dec_float(rest) {
        Some(value) => General::Number(if negative { -value } else { value }),
        None => General::Invalid,
    }
}

fn parse_dec_float(s: &[u8]) -> Option<f64> {
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i;
    if i < s.len() && s[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - frac_start;
    }
    if digits == 0 {
        return None;
    }
    let mut end = i;
    if end < s.len() && (s[end] == b'e' || s[end] == b'E') {
        let mut j = end + 1;
        if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            end = j;
        }
    }
    let text = std::str::from_utf8(&s[..end]).ok()?;
    text.parse::<f64>().ok()
}

fn parse_hex(s: &[u8]) -> Option<f64> {
    let mut value = 0.0_f64;
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_hexdigit() {
        value = value * 16.0 + f64::from(hex_value(s[i]));
        i += 1;
    }
    let mut scale = 1.0 / 16.0;
    let mut any = i > 0;
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_hexdigit() {
            value += f64::from(hex_value(s[i])) * scale;
            scale /= 16.0;
            i += 1;
            any = true;
        }
    }
    any.then_some(value)
}

fn hex_value(byte: u8) -> u32 {
    (byte as char).to_digit(16).unwrap_or(0)
}

fn general_rank(value: General) -> u8 {
    match value {
        General::Invalid => 0,
        General::NaN => 1,
        General::Number(_) => 2,
    }
}

/// `sort -g`: general floating-point comparison (see [`General`] for the ordering).
pub fn compare_general(a: &[u8], b: &[u8]) -> Ordering {
    let x = parse_general(a);
    let y = parse_general(b);
    match (x, y) {
        (General::Number(p), General::Number(q)) => p.partial_cmp(&q).unwrap_or(Ordering::Equal),
        _ => general_rank(x).cmp(&general_rank(y)),
    }
}

/// Unit exponent of a human-readable suffix: `K`/`k` is 1 up to `Q` is 10, and none is 0.
fn unit_exponent(byte: u8) -> u32 {
    match byte {
        b'K' | b'k' => 1,
        b'M' => 2,
        b'G' => 3,
        b'T' => 4,
        b'P' => 5,
        b'E' => 6,
        b'Z' => 7,
        b'Y' => 8,
        b'R' => 9,
        b'Q' => 10,
        _ => 0,
    }
}

/// A value read by `sort -h`: sign, unit exponent and the mantissa text.
struct Human<'a> {
    negative: bool,
    exponent: u32,
    mantissa: Decimal<'a>,
}

fn parse_human(s: &[u8]) -> Human<'_> {
    let mantissa = parse_decimal(s);
    let mut i = 0;
    while i < s.len() && is_blank(s[i]) {
        i += 1;
    }
    if i < s.len() && s[i] == b'-' {
        i += 1;
    }
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    let exponent = s.get(i).map_or(0, |b| unit_exponent(*b));
    Human {
        negative: mantissa.negative,
        exponent,
        mantissa,
    }
}

/// `sort -h`: the sign decides first, then the unit exponent (`1K` < `1M`, and `2000` <
/// `1K` because no unit ranks below any unit), then the mantissa.
pub fn compare_human(a: &[u8], b: &[u8]) -> Ordering {
    let x = parse_human(a);
    let y = parse_human(b);
    match (x.negative, y.negative) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => x
            .exponent
            .cmp(&y.exponent)
            .then_with(|| magnitude(&x.mantissa, &y.mantissa)),
        (true, true) => y
            .exponent
            .cmp(&x.exponent)
            .then_with(|| magnitude(&y.mantissa, &x.mantissa)),
    }
}

const MONTHS: [&[u8]; 12] = [
    b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV", b"DEC",
];

/// Month number (1 to 12) of a key whose first three non-blank characters name a month
/// (case-insensitive), or 0.
pub fn month_number(s: &[u8]) -> u8 {
    let mut i = 0;
    while i < s.len() && is_blank(s[i]) {
        i += 1;
    }
    let head: Vec<u8> = s[i..]
        .iter()
        .take(3)
        .map(|b| b.to_ascii_uppercase())
        .collect();
    MONTHS
        .iter()
        .position(|name| *name == head.as_slice())
        .map_or(0, |index| index as u8 + 1)
}

/// `sort -M`: unknown text sorts before `JAN`, months in calendar order.
pub fn compare_month(a: &[u8], b: &[u8]) -> Ordering {
    month_number(a).cmp(&month_number(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_comparison_is_exact() {
        assert_eq!(compare_numeric(b"10", b"9"), Ordering::Greater);
        assert_eq!(compare_numeric(b"-.5", b"-0"), Ordering::Less);
        assert_eq!(compare_numeric(b"  -3", b"-3.0"), Ordering::Equal);
        assert_eq!(compare_numeric(b"0.15", b"0.2"), Ordering::Less);
        assert_eq!(compare_numeric(b"+4", b"0"), Ordering::Equal);
        assert_eq!(
            compare_numeric(b"99999999999999999999", b"100000000000000000000"),
            Ordering::Less
        );
    }

    #[test]
    fn general_handles_special_values() {
        assert_eq!(parse_general(b"x"), General::Invalid);
        assert_eq!(parse_general(b"nan"), General::NaN);
        assert_eq!(parse_general(b"-inf"), General::Number(f64::NEG_INFINITY));
        assert_eq!(parse_general(b"0x10"), General::Number(16.0));
        assert_eq!(parse_general(b"1,5"), General::Number(1.0));
        assert_eq!(parse_general(b" 1e3"), General::Number(1000.0));
        assert_eq!(compare_general(b"x", b"nan"), Ordering::Less);
        assert_eq!(compare_general(b"nan", b"-1"), Ordering::Less);
    }

    #[test]
    fn human_orders_units_before_mantissa() {
        assert_eq!(compare_human(b"2000", b"1K"), Ordering::Less);
        assert_eq!(compare_human(b"1M", b"1000K"), Ordering::Greater);
        assert_eq!(compare_human(b"1R", b"1Q"), Ordering::Less);
        assert_eq!(compare_human(b"9K", b"10k"), Ordering::Less);
        assert_eq!(compare_human(b"-1M", b"-5K"), Ordering::Less);
        assert_eq!(compare_human(b"1K", b"1K"), Ordering::Equal);
    }

    #[test]
    fn month_names_are_case_insensitive() {
        assert_eq!(month_number(b"  feb"), 2);
        assert_eq!(month_number(b"DECEMBER"), 12);
        assert_eq!(month_number(b"foo"), 0);
        assert_eq!(compare_month(b"JAN", b"foo"), Ordering::Greater);
        assert_eq!(compare_month(b"Mar", b"Feb"), Ordering::Greater);
    }
}
