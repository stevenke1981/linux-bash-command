//! awk values: numbers, strings and strnums (POSIX), and their conversions.
//!
//! Strings are byte strings. A strnum is input text (fields, `getline` input, `ARGV`,
//! `ENVIRON`, `-v` values) that looks like a number; it compares numerically.

use std::borrow::Cow;
use std::cmp::Ordering;

use super::format::sprintf;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Uninit,
    Num(f64),
    Str(Vec<u8>),
    StrNum(Vec<u8>, f64),
}

impl Value {
    /// Builds a value from input text: a strnum when the text looks numeric, else a string.
    pub fn from_input(bytes: Vec<u8>) -> Value {
        match looks_numeric(&bytes) {
            Some(number) => Value::StrNum(bytes, number),
            None => Value::Str(bytes),
        }
    }

    /// Numeric value (POSIX conversion of strings and uninitialized values).
    pub fn to_num(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) | Value::StrNum(_, n) => *n,
            Value::Str(bytes) => str_to_num(bytes),
        }
    }

    /// Truth value: numbers are true when non-zero, strings when non-empty.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(n) | Value::StrNum(_, n) => *n != 0.0,
            Value::Str(bytes) => !bytes.is_empty(),
        }
    }

    /// True when comparisons use numbers: numbers, strnums and uninitialized values.
    pub fn is_numeric(&self) -> bool {
        !matches!(self, Value::Str(_))
    }

    /// String value, converting numbers with `CONVFMT`.
    pub fn to_str<'a>(&'a self, convfmt: &[u8]) -> Cow<'a, [u8]> {
        match self {
            Value::Uninit => Cow::Borrowed(&[]),
            Value::Num(n) => Cow::Owned(number_to_string(*n, convfmt)),
            Value::Str(bytes) | Value::StrNum(bytes, _) => Cow::Borrowed(bytes),
        }
    }

    /// String value for `print`, converting numbers with `OFMT`.
    pub fn to_output<'a>(&'a self, ofmt: &[u8]) -> Cow<'a, [u8]> {
        match self {
            Value::Num(n) => Cow::Owned(number_to_string(*n, ofmt)),
            other => other.to_str(ofmt),
        }
    }
}

/// Converts a number to a string: integral values print as exact integers (as gawk does),
/// everything else is formatted with `fmt` (`CONVFMT` or `OFMT`).
pub fn number_to_string(n: f64, fmt: &[u8]) -> Vec<u8> {
    if n.is_nan() {
        return (if n.is_sign_negative() { "-nan" } else { "nan" })
            .as_bytes()
            .to_vec();
    }
    if n.is_infinite() {
        return (if n < 0.0 { "-inf" } else { "inf" }).as_bytes().to_vec();
    }
    if n.fract() == 0.0 {
        if n > i64::MIN as f64 && n < i64::MAX as f64 {
            return (n as i64).to_string().into_bytes();
        }
        return format!("{n:.0}").into_bytes();
    }
    sprintf(fmt, &[Value::Num(n)], fmt)
}

/// The numeric prefix of `bytes` (leading blanks allowed), or 0 when there is none.
pub fn str_to_num(bytes: &[u8]) -> f64 {
    let start = skip_blanks(bytes, 0);
    let end = scan_number(bytes, start);
    if end == start {
        return 0.0;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// The number value of `bytes` when the whole text (ignoring surrounding blanks) is a
/// decimal number; `None` otherwise.
pub fn looks_numeric(bytes: &[u8]) -> Option<f64> {
    let start = skip_blanks(bytes, 0);
    let end = scan_number(bytes, start);
    if end == start {
        return None;
    }
    if skip_blanks(bytes, end) != bytes.len() {
        return None;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()?
        .parse::<f64>()
        .ok()
}

/// Orders two values by POSIX comparison rules: numerically when both are numeric, else as
/// strings.
pub fn compare(a: &Value, b: &Value, convfmt: &[u8]) -> Ordering {
    if a.is_numeric() && b.is_numeric() {
        a.to_num()
            .partial_cmp(&b.to_num())
            .unwrap_or(Ordering::Equal)
    } else {
        a.to_str(convfmt).cmp(&b.to_str(convfmt))
    }
}

fn skip_blanks(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
    {
        index += 1;
    }
    index
}

/// Returns the end of the decimal number starting at `start`, or `start` if there is none.
/// Grammar: sign? (digits ('.' digits?)? | '.' digits) (('e'|'E') sign? digits)?
fn scan_number(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    if matches!(bytes.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let mantissa_start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let mut digits = i - mantissa_start;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        let fraction_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        digits += i - fraction_start;
    }
    if digits == 0 {
        return start;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if bytes.get(j).is_some_and(u8::is_ascii_digit) {
            while bytes.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            i = j;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONV: &[u8] = b"%.6g";

    #[test]
    fn string_to_number_takes_the_numeric_prefix() {
        assert_eq!(str_to_num(b"  12abc"), 12.0);
        assert_eq!(str_to_num(b"-3.5e2x"), -350.0);
        assert_eq!(str_to_num(b".5"), 0.5);
        assert_eq!(str_to_num(b"1e"), 1.0);
        assert_eq!(str_to_num(b"0x1A"), 0.0);
        assert_eq!(str_to_num(b"abc"), 0.0);
        assert_eq!(str_to_num(b""), 0.0);
        assert_eq!(str_to_num(b"+"), 0.0);
    }

    #[test]
    fn looks_numeric_requires_the_whole_text() {
        assert_eq!(looks_numeric(b" 42 "), Some(42.0));
        assert_eq!(looks_numeric(b"4 2"), None);
        assert_eq!(looks_numeric(b"1e5"), Some(100000.0));
        assert_eq!(looks_numeric(b"1e"), None);
        assert_eq!(looks_numeric(b"."), None);
        assert_eq!(looks_numeric(b""), None);
        assert_eq!(looks_numeric(b"-.5"), Some(-0.5));
    }

    #[test]
    fn integral_numbers_print_as_integers() {
        assert_eq!(number_to_string(3.0, CONV), b"3".to_vec());
        assert_eq!(number_to_string(-0.0, CONV), b"0".to_vec());
        assert_eq!(number_to_string(1e17, CONV), b"100000000000000000".to_vec());
        assert_eq!(
            number_to_string(1e30, CONV),
            b"1000000000000000019884624838656".to_vec()
        );
    }

    #[test]
    fn fractions_use_the_given_format() {
        assert_eq!(number_to_string(0.1 + 0.2, CONV), b"0.3".to_vec());
        assert_eq!(number_to_string(2.5678, b"%.2f"), b"2.57".to_vec());
        assert_eq!(number_to_string(f64::INFINITY, CONV), b"inf".to_vec());
        assert_eq!(number_to_string(f64::NEG_INFINITY, CONV), b"-inf".to_vec());
    }

    #[test]
    fn comparison_follows_posix_rules() {
        let field = Value::from_input(b"10".to_vec());
        let text = Value::from_input(b"9x".to_vec());
        assert_eq!(compare(&field, &Value::Num(9.0), CONV), Ordering::Greater);
        // "9x" is not numeric, so the comparison is between strings.
        assert_eq!(compare(&text, &Value::Num(10.0), CONV), Ordering::Greater);
        assert_eq!(
            compare(&Value::Uninit, &Value::Num(0.0), CONV),
            Ordering::Equal
        );
        assert_eq!(
            compare(&Value::Uninit, &Value::Str(b"a".to_vec()), CONV),
            Ordering::Less
        );
    }

    #[test]
    fn strnum_is_numeric_and_truthiness() {
        let zero = Value::from_input(b"0.0".to_vec());
        assert!(!zero.truthy());
        assert!(Value::from_input(b" 1 ".to_vec()).truthy());
        assert!(Value::Str(b"0".to_vec()).truthy());
        assert!(!Value::Str(Vec::new()).truthy());
        assert!(!Value::Uninit.truthy());
        assert!(zero.is_numeric());
        assert!(!Value::Str(b"x".to_vec()).is_numeric());
    }

    #[test]
    fn output_conversion_uses_ofmt() {
        let value = Value::Num(2.5);
        assert_eq!(value.to_output(b"%.1f").as_ref(), b"2.5");
        assert_eq!(value.to_str(b"%.3f").as_ref(), b"2.500");
        assert_eq!(Value::Uninit.to_str(CONV).as_ref(), b"");
    }
}
