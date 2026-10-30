use crate::common::{args, escape, io};
use std::io::Write;

const HELP: &str = "Usage: printf FORMAT [ARGUMENT]...\nPrint FORMAT, reusing it for the remaining arguments.\nConversions: d i o u x X f F e E g G a A c s b q %\nFlags: - + space # 0; width and precision may use *.\n  --help     display this help (only as the sole argument)\n  --version  output version information\nExit status: 0 on success, 1 on invalid format or numeric arguments.\n";

pub fn run(args: &[String]) -> i32 {
    if let Some(code) = args::help_version("printf", args, HELP, true) {
        return code;
    }
    let args = if args.first().is_some_and(|s| s == "--") {
        &args[1..]
    } else {
        args
    };
    let Some(format) = args.first() else {
        return usage("printf", "missing operand");
    };
    let mut bad = false;
    let code = io::output("printf", |out| {
        match render(format.as_bytes(), &args[1..], out) {
            Ok(failed) => bad = failed,
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
                eprintln!("printf: {error}");
                bad = true;
            }
            Err(error) => return Err(error),
        }
        Ok(())
    });
    if code == 0 {
        i32::from(bad)
    } else {
        code
    }
}

fn invalid(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

fn number(text: &str, unsigned: bool) -> (u64, bool) {
    let s = text.trim_start();
    if s.starts_with(['\'', '"']) {
        return (s[1..].chars().next().map_or(0, u64::from), false);
    }
    let (negative, s) = if let Some(v) = s.strip_prefix('-') {
        (true, v)
    } else {
        (false, s.strip_prefix('+').unwrap_or(s))
    };
    let (radix, digits) = if s.starts_with("0x") || s.starts_with("0X") {
        (16, &s[2..])
    } else if s.starts_with('0') && s.len() > 1 {
        (8, s)
    } else {
        (10, s)
    };
    let end = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    let parsed = u128::from_str_radix(&digits[..end], radix);
    let limit = if unsigned {
        u128::from(u64::MAX)
    } else {
        i64::MAX as u128 + u128::from(negative)
    };
    let (value, range) = match parsed {
        Ok(v) if v <= limit => (v as u64, false),
        _ if end > 0 => (limit as u64, true),
        _ => (0, false),
    };
    let bad = end == 0 || end != digits.len() || range;
    if bad {
        eprintln!(
            "printf: '{text}': {}",
            if range {
                "Numerical result out of range"
            } else {
                "expected a numeric value"
            }
        );
    }
    (
        if negative {
            value.wrapping_neg()
        } else {
            value
        },
        bad,
    )
}

fn float_number(text: &str) -> (f64, bool) {
    if text.starts_with(['\'', '"']) {
        return (text[1..].chars().next().map_or(0, u32::from) as f64, false);
    }
    let s = text.trim_start();
    let negative = s.starts_with('-');
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        let bytes = unsigned.as_bytes();
        let (mut at, mut value, mut fractional, mut scale, mut digits) = (2, 0.0, false, 1.0, 0);
        while let Some(b) = bytes.get(at) {
            if *b == b'.' && !fractional {
                fractional = true;
                at += 1;
                continue;
            }
            let Some(digit) = char::from(*b).to_digit(16) else {
                break;
            };
            if fractional {
                scale /= 16.0;
                value += f64::from(digit) * scale;
            } else {
                value = value * 16.0 + f64::from(digit);
            }
            digits += 1;
            at += 1;
        }
        if matches!(bytes.get(at), Some(b'p' | b'P')) {
            let begin = at;
            at += 1;
            if matches!(bytes.get(at), Some(b'+' | b'-')) {
                at += 1;
            }
            let start = at;
            while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            if at > start {
                value *= 2.0f64.powi(unsigned[begin + 1..at].parse().unwrap_or(
                    if bytes.get(begin + 1) == Some(&b'-') {
                        i32::MIN
                    } else {
                        i32::MAX
                    },
                ));
            } else {
                at = begin;
            }
        }
        let bad = digits == 0 || at != bytes.len();
        if bad {
            eprintln!("printf: '{text}': expected a numeric value");
        }
        return (if negative { -value } else { value }, bad);
    }
    if let Ok(v) = s.parse::<f64>() {
        return (v, false);
    }
    for (end, _) in s.char_indices().rev().filter(|(i, _)| *i > 0) {
        if let Ok(v) = s[..end].parse::<f64>() {
            eprintln!("printf: '{text}': expected a numeric value");
            return (v, true);
        }
    }
    eprintln!("printf: '{text}': expected a numeric value");
    (0.0, true)
}

pub(crate) fn float_format(
    value: f64,
    conversion: u8,
    precision: Option<usize>,
    alternate: bool,
) -> String {
    let p = precision.unwrap_or(6);
    let upper = conversion.is_ascii_uppercase();
    let mut result = if value.is_nan() {
        "nan".into()
    } else if value.is_infinite() {
        "inf".into()
    } else {
        match conversion.to_ascii_lowercase() {
            b'e' => exponent(format!("{value:.p$e}")),
            b'g' => {
                let p = p.max(1);
                let raw = format!("{value:.prec$e}", prec = p - 1);
                let (mantissa, exp) = raw.split_once('e').unwrap();
                let exp: i32 = exp.parse().unwrap();
                if exp < -4 || exp >= p as i32 {
                    exponent(format!(
                        "{}e{exp}",
                        if alternate {
                            mantissa.to_string()
                        } else {
                            trim_fraction(mantissa)
                        }
                    ))
                } else {
                    let raw = format!(
                        "{value:.prec$}",
                        prec = (p as i32 - exp - 1).max(0) as usize
                    );
                    if alternate {
                        raw
                    } else {
                        trim_fraction(&raw)
                    }
                }
            }
            b'a' => hex_float(value, precision, alternate),
            _ => format!("{value:.p$}"),
        }
    };
    if alternate && value.is_finite() && !result.contains('.') {
        let at = result.find(['e', 'p']).unwrap_or(result.len());
        result.insert(at, '.');
    }
    if upper {
        result.make_ascii_uppercase();
    }
    result
}

fn trim_fraction(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

fn exponent(s: String) -> String {
    let (mantissa, power) = s.split_once('e').unwrap();
    let power: i32 = power.parse().unwrap();
    format!(
        "{mantissa}e{}{power:02}",
        if power < 0 { "-" } else { "+" },
        power = power.unsigned_abs()
    )
}

fn hex_float(value: f64, precision: Option<usize>, alternate: bool) -> String {
    if value == 0.0 {
        return if let Some(p) = precision {
            format!(
                "0x0{}{}p+0",
                if p > 0 || alternate { "." } else { "" },
                "0".repeat(p)
            )
        } else {
            "0x0p+0".into()
        };
    }
    let bits = value.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let significand = (bits & ((1u64 << 52) - 1)) | if raw_exp == 0 { 0 } else { 1u64 << 52 };
    let highest = 63 - significand.leading_zeros();
    let mut exp = if raw_exp == 0 {
        -1074
    } else {
        raw_exp - 1023 - 52
    } + highest as i32
        - 3;
    // GNU x86_64 formats long doubles with the explicit leading bit in the
    // high position of a hexadecimal digit. Preserve that representation
    // while using this command's specified f64 precision on every platform.
    let (mut whole, mut frac) = if highest >= 3 {
        let shift = highest - 3;
        (
            significand >> shift,
            (significand & ((1u64 << shift) - 1)) << (52 - shift),
        )
    } else {
        (significand << (3 - highest), 0)
    };
    let p = precision.unwrap_or(13);
    if p < 13 {
        let shift = 52 - 4 * p;
        let half = 1u64 << (shift - 1);
        let mask = (1u64 << shift) - 1;
        let remainder = frac & mask;
        frac >>= shift;
        if remainder > half || (remainder == half && (if p == 0 { whole } else { frac }) & 1 != 0) {
            frac += 1;
        }
        if frac >= 1u64 << (4 * p) {
            whole += 1;
            frac = 0;
        }
        if whole == 16 {
            whole = 1;
            exp += 4;
        }
    }
    let mut digits = if p == 0 {
        String::new()
    } else if p <= 13 {
        format!("{frac:0p$x}")
    } else {
        format!("{frac:013x}{}", "0".repeat(p - 13))
    };
    if precision.is_none() {
        digits = digits.trim_end_matches('0').to_string();
    }
    format!(
        "0x{whole:x}{}{digits}p{exp:+}",
        if !digits.is_empty() || alternate {
            "."
        } else {
            ""
        }
    )
}

pub(crate) fn render(
    format: &[u8],
    arguments: &[String],
    out: &mut dyn Write,
) -> std::io::Result<bool> {
    let mut consumed = 0;
    let mut failed = false;
    loop {
        let before = consumed;
        let mut i = 0;
        while i < format.len() {
            if format[i] == b'\\' {
                let start = i;
                i += 1;
                if i < format.len() {
                    let e = format[i];
                    i += 1;
                    let count = match e {
                        b'x' => 2,
                        b'u' => 4,
                        b'U' => 8,
                        b'0'..=b'7' => 2,
                        _ => 0,
                    };
                    let radix = if e.is_ascii_digit() { 8 } else { 16 };
                    for _ in 0..count {
                        if i < format.len() && char::from(format[i]).is_digit(radix) {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }
                let (bytes, stop) =
                    escape::decode(&format[start..i], escape::Mode::Printf).map_err(invalid)?;
                out.write_all(&bytes)?;
                if stop {
                    return Ok(failed);
                }
                continue;
            }
            if format[i] != b'%' {
                out.write_all(&format[i..i + 1])?;
                i += 1;
                continue;
            }
            i += 1;
            if format.get(i) == Some(&b'%') {
                out.write_all(b"%")?;
                i += 1;
                continue;
            }
            let mut flags = Vec::new();
            while i < format.len() && b"-+ #0".contains(&format[i]) {
                flags.push(format[i]);
                i += 1;
            }
            let mut next = || {
                let value = arguments.get(consumed).map(String::as_str).unwrap_or("");
                if consumed < arguments.len() {
                    consumed += 1;
                }
                value
            };
            let mut width = 0usize;
            if format.get(i) == Some(&b'*') {
                let s = next();
                let (v, bad) = if s.is_empty() {
                    (0, false)
                } else {
                    number(s, false)
                };
                failed |= bad;
                let v = v as i64;
                if v < 0 {
                    flags.push(b'-');
                }
                width = v.unsigned_abs().min(1_000_000) as usize;
                i += 1;
            } else {
                while i < format.len() && format[i].is_ascii_digit() {
                    width = width
                        .saturating_mul(10)
                        .saturating_add(usize::from(format[i] - b'0'))
                        .min(1_000_000);
                    i += 1;
                }
            }
            let mut precision = None;
            if format.get(i) == Some(&b'.') {
                i += 1;
                let mut p = 0usize;
                if format.get(i) == Some(&b'*') {
                    let s = next();
                    let (v, bad) = if s.is_empty() {
                        (0, false)
                    } else {
                        number(s, false)
                    };
                    failed |= bad;
                    if (v as i64) >= 0 {
                        precision = Some(v.min(1_000_000) as usize);
                    }
                    i += 1;
                } else {
                    while i < format.len() && format[i].is_ascii_digit() {
                        p = p
                            .saturating_mul(10)
                            .saturating_add(usize::from(format[i] - b'0'))
                            .min(1_000_000);
                        i += 1;
                    }
                    precision = Some(p);
                }
            }
            let conversion = *format
                .get(i)
                .ok_or_else(|| invalid("%: invalid conversion specification"))?;
            i += 1;
            let text = next();
            let mut sign = None;
            let mut prefix = Vec::new();
            let mut stop = false;
            let numeric = b"diouxXfFeEgGaA".contains(&conversion);
            let mut bytes = match conversion {
                b's' | b'c' | b'b' | b'q' => {
                    let mut bytes = match conversion {
                        b'c' => vec![text.as_bytes().first().copied().unwrap_or(0)],
                        b'b' => {
                            let result = escape::decode(text.as_bytes(), escape::Mode::String)
                                .map_err(invalid)?;
                            stop = result.1;
                            result.0
                        }
                        b'q' => escape::quote_name(text.as_bytes()),
                        _ => text.as_bytes().to_vec(),
                    };
                    if conversion == b's' {
                        if let Some(p) = precision {
                            bytes.truncate(p);
                        }
                    }
                    bytes
                }
                b'd' | b'i' | b'o' | b'u' | b'x' | b'X' => {
                    let signed = b"di".contains(&conversion);
                    let (v, bad) = if text.is_empty() {
                        (0, false)
                    } else {
                        number(text, !signed)
                    };
                    failed |= bad;
                    let magnitude = if signed && (v as i64) < 0 {
                        sign = Some(b'-');
                        (v as i64).unsigned_abs()
                    } else {
                        v
                    };
                    if signed && sign.is_none() {
                        if flags.contains(&b'+') {
                            sign = Some(b'+');
                        } else if flags.contains(&b' ') {
                            sign = Some(b' ');
                        }
                    }
                    let mut digits = match conversion {
                        b'o' => format!("{magnitude:o}"),
                        b'x' => format!("{magnitude:x}"),
                        b'X' => format!("{magnitude:X}"),
                        _ => magnitude.to_string(),
                    };
                    if precision == Some(0) && magnitude == 0 {
                        digits.clear();
                    }
                    if let Some(p) = precision {
                        if p > digits.len() {
                            digits = format!("{}{digits}", "0".repeat(p - digits.len()));
                        }
                    }
                    if flags.contains(&b'#') {
                        match conversion {
                            b'o' if !digits.starts_with('0') => prefix.push(b'0'),
                            b'x' | b'X' if magnitude != 0 => prefix
                                .extend_from_slice(if conversion == b'x' { b"0x" } else { b"0X" }),
                            _ => {}
                        }
                    }
                    digits.into_bytes()
                }
                b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                    let (value, bad) = if text.is_empty() {
                        (0.0, false)
                    } else {
                        float_number(text)
                    };
                    failed |= bad;
                    sign = if value.is_sign_negative() {
                        Some(b'-')
                    } else if flags.contains(&b'+') {
                        Some(b'+')
                    } else if flags.contains(&b' ') {
                        Some(b' ')
                    } else {
                        None
                    };
                    float_format(value.abs(), conversion, precision, flags.contains(&b'#'))
                        .into_bytes()
                }
                _ => {
                    return Err(invalid(format!(
                        "%{}: invalid conversion specification",
                        char::from(conversion)
                    )))
                }
            };
            let length = bytes.len() + prefix.len() + usize::from(sign.is_some());
            let padding = width.saturating_sub(length);
            let left = flags.contains(&b'-');
            let zero = flags.contains(&b'0')
                && !left
                && numeric
                && (!b"diouxX".contains(&conversion) || precision.is_none())
                && !text.to_ascii_lowercase().contains("inf")
                && !text.to_ascii_lowercase().contains("nan");
            if !left && !zero {
                out.write_all(&vec![b' '; padding])?;
            }
            if let Some(sign) = sign {
                out.write_all(&[sign])?;
            }
            out.write_all(&prefix)?;
            if zero {
                if bytes.starts_with(b"0x") || bytes.starts_with(b"0X") {
                    out.write_all(&bytes[..2])?;
                    bytes.drain(..2);
                }
                out.write_all(&vec![b'0'; padding])?;
            }
            out.write_all(&bytes)?;
            if left {
                out.write_all(&vec![b' '; padding])?;
            }
            if stop {
                return Ok(failed);
            }
        }
        if consumed >= arguments.len() || consumed == before {
            break;
        }
    }
    Ok(failed)
}

fn usage(command: &str, message: &str) -> i32 {
    crate::common::error::usage(command, message, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(f: &str, a: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        assert!(!render(
            f.as_bytes(),
            &a.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &mut out
        )
        .unwrap());
        out
    }
    #[test]
    fn repeated() {
        assert_eq!(p("%s:%d\n", &["a", "2", "b", "3"]), b"a:2\nb:3\n");
    }
    #[test]
    fn integer_bases() {
        assert_eq!(p("%#x %o %d", &["16", "010", "'A"]), b"0x10 10 65");
    }
    #[test]
    fn flags() {
        assert_eq!(p("%+06d|%-4s|%.3d", &["12", "x", "2"]), b"+00012|x   |002");
    }
    #[test]
    fn stars() {
        assert_eq!(p("%*.*f", &["8", "2", "1.25"]), b"    1.25");
    }
    #[test]
    fn escapes() {
        assert_eq!(p("\\101\\u03bb%bEND", &["x\\cy"]), b"A\\u03BBx");
    }
    #[test]
    fn float_forms() {
        assert_eq!(
            p("%.2e %.3g %a", &["12", "1.2", "1.5"]),
            b"1.20e+01 1.2 0xcp-3"
        );
    }
    #[test]
    fn missing_values() {
        assert_eq!(p("%d%s%c", &[]), b"0\0");
    }
    #[test]
    fn numeric_boundary() {
        assert_eq!(
            number("-9223372036854775808", false),
            (i64::MIN as u64, false)
        );
        assert_eq!(number("12bad", false), (12, true));
    }
    #[test]
    fn hexadecimal_float_input() {
        assert_eq!(p("%f", &["0x1.8p+0"]), b"1.500000");
    }
    #[test]
    fn hexadecimal_ties_even() {
        assert_eq!(p("%.0a", &["1.5"]), b"0xcp-3");
    }
    #[test]
    fn hexadecimal_gnu_leading_nibble() {
        assert_eq!(
            p("%a|%A|%#.0a", &["1.5", "2.5", "1"]),
            b"0xcp-3|0XAP-2|0x8.p-3"
        );
    }
    #[test]
    fn hexadecimal_zero_and_fraction() {
        assert_eq!(p("%a|%a|%a", &["0", "-0", "0.5"]), b"0x0p+0|-0x0p+0|0x8p-4");
    }
    #[test]
    fn hexadecimal_subnormal() {
        assert_eq!(hex_float(f64::from_bits(1), None, false), "0x8p-1077");
    }
    #[test]
    fn hexadecimal_tie_rounding() {
        assert_eq!(p("%.0a|%.0a", &["1.0625", "1.1875"]), b"0x8p-3|0xap-3");
    }
    #[test]
    fn hexadecimal_fraction_precision() {
        assert_eq!(p("%.2a|%#.0a", &["1.125", "1"]), b"0x9.00p-3|0x8.p-3");
    }
    #[test]
    fn hexadecimal_rounding_carry() {
        assert_eq!(p("%.0a|%.1a", &["1.99", "1.99609375"]), b"0x1p+1|0x1.0p+1");
    }
}
