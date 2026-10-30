//! C-style formatting of `f64` for the `%f %e %g %a` conversions. Shared by `printf`
//! and `seq -f`.
//!
//! Values are `f64`, not the `long double` GNU uses, so results can differ in the last
//! digit for some decimal inputs. This is recorded in the known-differences table.

/// Flags, width and precision of one floating or integer conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    pub left: bool,
    pub plus: bool,
    pub space: bool,
    pub alt: bool,
    pub zero: bool,
    pub width: usize,
    pub precision: Option<usize>,
}

/// Formats `value` for conversion `conv` (`f F e E g G a A`) and appends it to `out`.
pub fn format_float(out: &mut Vec<u8>, value: f64, conv: u8, spec: &Spec) {
    let upper = conv.is_ascii_uppercase();
    let sign = if value.is_sign_negative() {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    };
    if !value.is_finite() {
        let text = if value.is_nan() { "nan" } else { "inf" };
        let text = if upper {
            text.to_ascii_uppercase()
        } else {
            text.to_owned()
        };
        pad(out, sign, &text, spec, false);
        return;
    }
    let magnitude = value.abs();
    let (prefix, body) = match conv.to_ascii_lowercase() {
        b'f' => ("", fixed(magnitude, spec)),
        b'e' => ("", exponential(magnitude, spec)),
        b'g' => ("", general(magnitude, spec)),
        _ => ("0x", hex_float(magnitude, spec)),
    };
    let (prefix, body) = if upper {
        (prefix.to_ascii_uppercase(), body.to_ascii_uppercase())
    } else {
        (prefix.to_owned(), body)
    };
    pad(out, &format!("{sign}{prefix}"), &body, spec, true);
}

/// Appends `prefix` and `body` padded to `spec.width`. Zero padding goes between the prefix
/// and the body when `zero_ok` and the `0` flag is set and the `-` flag is not.
pub fn pad(out: &mut Vec<u8>, prefix: &str, body: &str, spec: &Spec, zero_ok: bool) {
    let fill = spec.width.saturating_sub(prefix.len() + body.len());
    if spec.left {
        out.extend_from_slice(prefix.as_bytes());
        out.extend_from_slice(body.as_bytes());
        push_repeat(out, b' ', fill);
    } else if spec.zero && zero_ok {
        out.extend_from_slice(prefix.as_bytes());
        push_repeat(out, b'0', fill);
        out.extend_from_slice(body.as_bytes());
    } else {
        push_repeat(out, b' ', fill);
        out.extend_from_slice(prefix.as_bytes());
        out.extend_from_slice(body.as_bytes());
    }
}

/// Appends `byte` `count` times.
pub fn push_repeat(out: &mut Vec<u8>, byte: u8, count: usize) {
    out.extend(std::iter::repeat_n(byte, count));
}

fn fixed(magnitude: f64, spec: &Spec) -> String {
    let precision = spec.precision.unwrap_or(6);
    let mut text = format!("{magnitude:.precision$}");
    if spec.alt && precision == 0 {
        text.push('.');
    }
    text
}

/// Splits `{:.p$e}` output into a mantissa and a decimal exponent.
fn scientific_parts(magnitude: f64, precision: usize) -> (String, i32) {
    let text = format!("{magnitude:.precision$e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => (mantissa.to_owned(), exponent.parse().unwrap_or(0)),
        None => (text, 0),
    }
}

fn exponent_suffix(exponent: i32) -> String {
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("e{sign}{:02}", exponent.unsigned_abs())
}

fn exponential(magnitude: f64, spec: &Spec) -> String {
    let precision = spec.precision.unwrap_or(6);
    let (mantissa, exponent) = scientific_parts(magnitude, precision);
    let mut text = mantissa;
    if spec.alt && precision == 0 {
        text.push('.');
    }
    text.push_str(&exponent_suffix(exponent));
    text
}

fn general(magnitude: f64, spec: &Spec) -> String {
    let precision = match spec.precision.unwrap_or(6) {
        0 => 1,
        p => p,
    };
    let (mantissa, exponent) = scientific_parts(magnitude, precision - 1);
    let keep_zeros = spec.alt;
    if exponent < -4 || exponent >= precision as i32 {
        let mut text = if keep_zeros {
            mantissa
        } else {
            strip_fraction_zeros(&mantissa)
        };
        if keep_zeros && !text.contains('.') {
            text.push('.');
        }
        text.push_str(&exponent_suffix(exponent));
        text
    } else {
        let decimals = (precision as i32 - 1 - exponent) as usize;
        let text = format!("{magnitude:.decimals$}");
        if keep_zeros {
            if text.contains('.') {
                text
            } else {
                format!("{text}.")
            }
        } else {
            strip_fraction_zeros(&text)
        }
    }
}

/// Removes trailing zeros after a decimal point, and the point itself when nothing remains.
fn strip_fraction_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_owned();
    }
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `%a`: hexadecimal floating point for an `f64` (glibc style, normalised leading digit).
/// A precision truncates the fraction; it is not rounded.
fn hex_float(magnitude: f64, spec: &Spec) -> String {
    if magnitude == 0.0 {
        return match spec.precision {
            Some(p) if p > 0 => format!("0.{}p+0", "0".repeat(p)),
            _ => "0p+0".to_owned(),
        };
    }
    let bits = magnitude.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (lead, exponent) = if biased == 0 {
        (0, -1022)
    } else {
        (1, biased - 1023)
    };
    let mut digits = format!("{fraction:013x}");
    match spec.precision {
        None => digits = digits.trim_end_matches('0').to_owned(),
        Some(p) if p < digits.len() => digits.truncate(p),
        Some(p) => digits.push_str(&"0".repeat(p - digits.len())),
    }
    let mut text = format!("{lead}");
    if !digits.is_empty() || spec.alt {
        text.push('.');
        text.push_str(&digits);
    }
    text.push_str(&format!(
        "p{}{}",
        if exponent < 0 { '-' } else { '+' },
        exponent.abs()
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(value: f64, conv: u8, spec: Spec) -> String {
        let mut out = Vec::new();
        format_float(&mut out, value, conv, &spec);
        String::from_utf8(out).unwrap()
    }

    fn plain() -> Spec {
        Spec::default()
    }

    #[test]
    fn fixed_and_exponential_match_c() {
        assert_eq!(fmt(1.25, b'f', plain()), "1.250000");
        assert_eq!(
            fmt(
                2.5,
                b'f',
                Spec {
                    precision: Some(0),
                    ..plain()
                }
            ),
            "2"
        );
        assert_eq!(fmt(12345.678, b'e', plain()), "1.234568e+04");
        assert_eq!(fmt(0.000123, b'E', plain()), "1.230000E-04");
        assert_eq!(
            fmt(
                5.0,
                b'e',
                Spec {
                    precision: Some(0),
                    alt: true,
                    ..plain()
                }
            ),
            "5.e+00"
        );
    }

    #[test]
    fn general_chooses_style_like_c() {
        assert_eq!(fmt(0.0001, b'g', plain()), "0.0001");
        assert_eq!(fmt(0.00001, b'g', plain()), "1e-05");
        assert_eq!(fmt(123456.0, b'g', plain()), "123456");
        assert_eq!(fmt(1234567.0, b'g', plain()), "1.23457e+06");
        assert_eq!(
            fmt(
                1.0,
                b'g',
                Spec {
                    alt: true,
                    ..plain()
                }
            ),
            "1.00000"
        );
        assert_eq!(fmt(0.000001, b'G', plain()), "1E-06");
    }

    #[test]
    fn flags_width_and_zero_padding() {
        let spec = Spec {
            plus: true,
            width: 9,
            precision: Some(2),
            ..plain()
        };
        assert_eq!(fmt(1.5, b'f', spec), "    +1.50");
        let spec = Spec {
            zero: true,
            width: 8,
            precision: Some(2),
            ..plain()
        };
        assert_eq!(fmt(-1.5, b'f', spec), "-0001.50");
        let spec = Spec {
            left: true,
            width: 8,
            precision: Some(2),
            ..plain()
        };
        assert_eq!(fmt(1.5, b'f', spec), "1.50    ");
    }

    #[test]
    fn non_finite_values_ignore_zero_flag() {
        let spec = Spec {
            zero: true,
            width: 5,
            ..plain()
        };
        assert_eq!(fmt(f64::INFINITY, b'f', spec), "  inf");
        assert_eq!(fmt(f64::NAN, b'F', plain()), "NAN");
        assert_eq!(fmt(-f64::INFINITY, b'f', plain()), "-inf");
    }

    #[test]
    fn hex_float_uses_normalised_leading_digit() {
        assert_eq!(fmt(1.0, b'a', plain()), "0x1p+0");
        assert_eq!(fmt(0.5, b'a', plain()), "0x1p-1");
        assert_eq!(fmt(2.5, b'a', plain()), "0x1.4p+1");
        assert_eq!(fmt(0.0, b'A', plain()), "0X0P+0");
    }
}
