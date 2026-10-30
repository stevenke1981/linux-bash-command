//! Numeric arguments for `printf`, with C-locale `strtoimax`, `strtoumax` and `strtold`
//! behaviour: base prefixes `0x` and `0`, leading white space, a quote-prefixed character
//! constant (`'A` = 65), and an empty argument meaning zero.

/// A problem found while converting an argument. The value is still produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumError {
    /// No digits at all.
    Expected,
    /// Trailing characters after the number.
    Partial,
    /// The value does not fit and was clamped.
    Range,
}

impl NumError {
    /// The message for `printf: 'ARG': MESSAGE`.
    pub fn text(self) -> &'static str {
        match self {
            NumError::Expected => "expected a numeric value",
            NumError::Partial => "value not completely converted",
            NumError::Range => "Numerical result out of range",
        }
    }
}

/// Parses a signed integer argument (`%d %i`).
pub fn parse_signed(arg: &str) -> (i64, Option<NumError>) {
    if let Some(result) = char_constant(arg) {
        return match result {
            Ok(value) => (i64::from(value), None),
            Err(error) => (0, Some(error)),
        };
    }
    if arg.is_empty() {
        return (0, None);
    }
    let scan = scan_integer(arg);
    let (negative, magnitude) = (scan.negative, scan.magnitude);
    let (value, range) = if negative {
        if magnitude > 1u128 << 63 {
            (i64::MIN, true)
        } else {
            (((magnitude as i128).wrapping_neg()) as i64, false)
        }
    } else if magnitude > i64::MAX as u128 {
        (i64::MAX, true)
    } else {
        (magnitude as i64, false)
    };
    (value, classify(&scan, range))
}

/// Parses an unsigned integer argument (`%u %o %x %X`). A negative argument wraps, as
/// `strtoumax` does.
pub fn parse_unsigned(arg: &str) -> (u64, Option<NumError>) {
    if let Some(result) = char_constant(arg) {
        return match result {
            Ok(value) => (u64::from(value), None),
            Err(error) => (0, Some(error)),
        };
    }
    if arg.is_empty() {
        return (0, None);
    }
    let scan = scan_integer(arg);
    let range = scan.magnitude > u64::MAX as u128;
    let magnitude = if range {
        u64::MAX
    } else {
        scan.magnitude as u64
    };
    let value = if scan.negative {
        magnitude.wrapping_neg()
    } else {
        magnitude
    };
    (value, classify(&scan, range))
}

/// Parses a floating point argument (`%f %e %g %a`).
pub fn parse_float(arg: &str) -> (f64, Option<NumError>) {
    if let Some(result) = char_constant(arg) {
        return match result {
            Ok(value) => (f64::from(value), None),
            Err(error) => (0.0, Some(error)),
        };
    }
    if arg.is_empty() {
        return (0.0, None);
    }
    let trimmed = arg.trim_start_matches(is_c_space);
    let (negative, body) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let (magnitude, used) = match special_float(body) {
        Some((value, used)) => (value, used),
        None => match hex_float(body) {
            Some((value, used)) => (value, used),
            None => match decimal_float(body) {
                Some((value, used)) => (value, used),
                None => return (0.0, Some(NumError::Expected)),
            },
        },
    };
    let value = if negative { -magnitude } else { magnitude };
    let error = if body[used..].is_empty() {
        None
    } else {
        Some(NumError::Partial)
    };
    (value, error)
}

/// A quote-prefixed character constant (`"A` or `'A`): its value, or `Expected` when the
/// quote has nothing after it. `None` when `arg` is not a character constant.
fn char_constant(arg: &str) -> Option<Result<u8, NumError>> {
    match arg.as_bytes().first() {
        Some(b'"') | Some(b'\'') => Some(arg.as_bytes().get(1).copied().ok_or(NumError::Expected)),
        _ => None,
    }
}

fn is_c_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

struct IntegerScan {
    negative: bool,
    magnitude: u128,
    digits: usize,
    /// Bytes consumed, including sign, prefix and digits.
    end: usize,
    /// Length of the whole argument.
    total: usize,
}

const MAGNITUDE_CAP: u128 = 1 << 100;

fn scan_integer(arg: &str) -> IntegerScan {
    let bytes = arg.as_bytes();
    let mut index = 0;
    while index < bytes.len() && is_c_space(bytes[index] as char) {
        index += 1;
    }
    let mut negative = false;
    if index < bytes.len() && (bytes[index] == b'-' || bytes[index] == b'+') {
        negative = bytes[index] == b'-';
        index += 1;
    }
    let (radix, start) = integer_radix(bytes, index);
    let mut magnitude: u128 = 0;
    let mut digits = 0;
    let mut end = start;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        let Some(digit) = (*byte as char).to_digit(radix) else {
            break;
        };
        magnitude = (magnitude * u128::from(radix) + u128::from(digit)).min(MAGNITUDE_CAP);
        digits += 1;
        end = start + offset + 1;
    }
    IntegerScan {
        negative,
        magnitude,
        digits,
        end,
        total: bytes.len(),
    }
}

/// The radix and the index of the first digit: `0x` selects hexadecimal only when a hex
/// digit follows, and a leading `0` selects octal.
fn integer_radix(bytes: &[u8], index: usize) -> (u32, usize) {
    let hex_follows = bytes.len() > index + 2
        && bytes[index] == b'0'
        && (bytes[index + 1] == b'x' || bytes[index + 1] == b'X')
        && (bytes[index + 2] as char).is_ascii_hexdigit();
    if hex_follows {
        (16, index + 2)
    } else if bytes.get(index) == Some(&b'0') {
        (8, index)
    } else {
        (10, index)
    }
}

fn classify(scan: &IntegerScan, range: bool) -> Option<NumError> {
    if scan.digits == 0 {
        Some(NumError::Expected)
    } else if range {
        Some(NumError::Range)
    } else if scan.end < scan.total {
        Some(NumError::Partial)
    } else {
        None
    }
}

/// Parses `inf`, `infinity` and `nan` (any case), with the sign already removed.
fn special_float(body: &str) -> Option<(f64, usize)> {
    let lower = body.to_ascii_lowercase();
    if lower.starts_with("infinity") {
        Some((f64::INFINITY, 8))
    } else if lower.starts_with("inf") {
        Some((f64::INFINITY, 3))
    } else if lower.starts_with("nan") {
        Some((f64::NAN, 3))
    } else {
        None
    }
}

/// Parses `0x` hexadecimal floating point with an optional binary exponent.
fn hex_float(body: &str) -> Option<(f64, usize)> {
    let bytes = body.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'0' || (bytes[1] != b'x' && bytes[1] != b'X') {
        return None;
    }
    let mut index = 2;
    let mut mantissa = 0f64;
    let mut exponent = 0i32;
    let mut any = false;
    let mut after_point = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'.' && !after_point {
            after_point = true;
        } else if let Some(digit) = (byte as char).to_digit(16) {
            mantissa = mantissa * 16.0 + f64::from(digit);
            if after_point {
                exponent -= 4;
            }
            any = true;
        } else {
            break;
        }
        index += 1;
    }
    if !any {
        return None;
    }
    if index < bytes.len() && (bytes[index] == b'p' || bytes[index] == b'P') {
        if let Some((value, used)) = decimal_exponent(&body[index + 1..]) {
            exponent += value;
            index += 1 + used;
        }
    }
    Some((mantissa * 2f64.powi(exponent), index))
}

/// Parses a decimal floating point number, returning the value and the bytes used.
fn decimal_float(body: &str) -> Option<(f64, usize)> {
    let bytes = body.as_bytes();
    let mut index = 0;
    let int_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let int_part = &body[int_start..index];
    let mut frac_part = "";
    if index < bytes.len() && bytes[index] == b'.' {
        let frac_start = index + 1;
        let mut end = frac_start;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        frac_part = &body[frac_start..end];
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        index = end;
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut exponent_text = String::new();
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        if let Some((value, used)) = decimal_exponent(&body[index + 1..]) {
            exponent_text = value.to_string();
            index += 1 + used;
        }
    }
    let normalised = format!(
        "{}.{}e{}",
        if int_part.is_empty() { "0" } else { int_part },
        if frac_part.is_empty() { "0" } else { frac_part },
        if exponent_text.is_empty() {
            "0"
        } else {
            &exponent_text
        }
    );
    let value: f64 = normalised.parse().ok()?;
    Some((value, index))
}

/// Parses an optionally signed decimal exponent after `e` or `p`. Returns the value and the
/// bytes used, counting the sign.
fn decimal_exponent(text: &str) -> Option<(i32, usize)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let negative = match bytes.first() {
        Some(b'-') => {
            index += 1;
            true
        }
        Some(b'+') => {
            index += 1;
            false
        }
        _ => false,
    };
    let start = index;
    let mut value: i32 = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        value = value
            .saturating_mul(10)
            .saturating_add(i32::from(bytes[index] - b'0'));
        index += 1;
    }
    if index == start {
        return None;
    }
    Some((if negative { -value } else { value }, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_accepts_bases_and_space() {
        assert_eq!(parse_signed("0x1f"), (31, None));
        assert_eq!(parse_signed("010"), (8, None));
        assert_eq!(parse_signed("  -12"), (-12, None));
        assert_eq!(parse_signed("+7"), (7, None));
        assert_eq!(parse_signed(""), (0, None));
    }

    #[test]
    fn signed_reports_partial_and_range() {
        assert_eq!(parse_signed("1.5"), (1, Some(NumError::Partial)));
        assert_eq!(parse_signed("abc"), (0, Some(NumError::Expected)));
        assert_eq!(
            parse_signed("99999999999999999999"),
            (i64::MAX, Some(NumError::Range))
        );
        assert_eq!(parse_signed("-9223372036854775808"), (i64::MIN, None));
        assert_eq!(parse_signed("0x"), (0, Some(NumError::Partial)));
    }

    #[test]
    fn character_constants_use_the_next_byte() {
        assert_eq!(parse_signed("'A"), (65, None));
        assert_eq!(parse_signed("\"x"), (120, None));
        assert_eq!(parse_float("'A"), (65.0, None));
        assert_eq!(parse_signed("'"), (0, Some(NumError::Expected)));
    }

    #[test]
    fn unsigned_wraps_negative_values() {
        assert_eq!(parse_unsigned("-1"), (u64::MAX, None));
        assert_eq!(parse_unsigned("18446744073709551615"), (u64::MAX, None));
        assert_eq!(
            parse_unsigned("18446744073709551616").1,
            Some(NumError::Range)
        );
    }

    #[test]
    fn float_handles_decimal_special_and_hex() {
        assert_eq!(parse_float("1.5"), (1.5, None));
        assert_eq!(parse_float(".5e1"), (5.0, None));
        assert_eq!(parse_float("-inf").0, f64::NEG_INFINITY);
        assert!(parse_float("nan").0.is_nan());
        assert_eq!(parse_float("0x1p3"), (8.0, None));
        assert_eq!(parse_float("1.5x"), (1.5, Some(NumError::Partial)));
        assert_eq!(parse_float("abc"), (0.0, Some(NumError::Expected)));
    }
}
