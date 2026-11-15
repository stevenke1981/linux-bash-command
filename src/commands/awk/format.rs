use super::value::Scalar;
pub fn numeric(format: &[u8], value: f64) -> Result<Vec<u8>, String> {
    render(format, &[Scalar::Number(value)], b"%.6g")
}
pub fn render(format: &[u8], values: &[Scalar], convfmt: &[u8]) -> Result<Vec<u8>, String> {
    let (mut out, mut at, mut argument) = (Vec::new(), 0, 0);
    while at < format.len() {
        let byte = format[at];
        at += 1;
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        if format.get(at) == Some(&b'%') {
            out.push(b'%');
            at += 1;
            continue;
        }
        let mut flags = Vec::new();
        while format.get(at).is_some_and(|b| b"-+ #0'".contains(b)) {
            flags.push(format[at]);
            at += 1;
        }
        let width = if format.get(at) == Some(&b'*') {
            at += 1;
            take(values, &mut argument)?.number() as i64
        } else {
            i64::try_from(digits(format, &mut at)?).map_err(|_| "printf width too large")?
        };
        let precision = if format.get(at) == Some(&b'.') {
            at += 1;
            let n = if format.get(at) == Some(&b'*') {
                at += 1;
                take(values, &mut argument)?.number() as i64
            } else {
                i64::try_from(digits(format, &mut at)?).map_err(|_| "printf precision too large")?
            };
            (n >= 0).then_some(n as usize)
        } else {
            None
        };
        let conversion = *format
            .get(at)
            .ok_or("not enough arguments or incomplete printf format")?;
        at += 1;
        let reserve = usize::try_from(width.unsigned_abs())
            .ok()
            .and_then(|n| n.checked_add(precision.unwrap_or(0)))
            .ok_or("printf width/precision too large")?;
        out.try_reserve(reserve)
            .map_err(|_| "insufficient memory for printf format")?;
        let value = take(values, &mut argument)?;
        let (mut bytes, prefix) = conversion_bytes(value, conversion, precision, &flags, convfmt)?;
        let padding = (width.unsigned_abs() as usize).saturating_sub(bytes.len());
        if width < 0 || flags.contains(&b'-') {
            bytes.extend(std::iter::repeat_n(b' ', padding));
        } else if padding > 0 {
            let zero = flags.contains(&b'0')
                && conversion != b's'
                && conversion != b'c'
                && (!b"diouxX".contains(&conversion) || precision.is_none());
            let mut padded = Vec::with_capacity(bytes.len() + padding);
            let index = if zero { prefix } else { 0 };
            padded.extend_from_slice(&bytes[..index]);
            padded.extend(std::iter::repeat_n(if zero { b'0' } else { b' ' }, padding));
            padded.extend_from_slice(&bytes[index..]);
            bytes = padded;
        }
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}
fn take<'a>(values: &'a [Scalar], argument: &mut usize) -> Result<&'a Scalar, String> {
    let value = values
        .get(*argument)
        .ok_or("not enough arguments to satisfy printf format")?;
    *argument += 1;
    Ok(value)
}
fn digits(bytes: &[u8], at: &mut usize) -> Result<usize, String> {
    let mut number = 0usize;
    while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
        number = number
            .checked_mul(10)
            .and_then(|n| n.checked_add(usize::from(bytes[*at] - b'0')))
            .ok_or("printf width/precision too large")?;
        *at += 1;
    }
    Ok(number)
}
fn conversion_bytes(
    value: &Scalar,
    conversion: u8,
    precision: Option<usize>,
    flags: &[u8],
    convfmt: &[u8],
) -> Result<(Vec<u8>, usize), String> {
    if conversion == b's' {
        let mut bytes = value.bytes(convfmt)?;
        if let Some(length) = precision {
            bytes.truncate(length);
        }
        return Ok((bytes, 0));
    }
    if conversion == b'c' {
        let bytes = if value.numeric() {
            vec![value.number() as u8]
        } else {
            let bytes = value.bytes(convfmt)?;
            bytes.get(..1).unwrap_or(&[]).to_vec()
        };
        return Ok((if bytes.is_empty() { vec![0] } else { bytes }, 0));
    }
    if b"diouxX".contains(&conversion) {
        return integer(value.number(), conversion, precision, flags);
    }
    if !b"fFeEgGaA".contains(&conversion) {
        return Err(format!(
            "invalid printf conversion '%{}'",
            char::from(conversion)
        ));
    }
    let number = value.number();
    let sign = sign(number.is_sign_negative(), flags);
    let rendered = if matches!(conversion, b'a' | b'A') && number.is_finite() {
        let value = hex(number.abs(), precision, flags.contains(&b'#'));
        if conversion == b'A' {
            value.to_ascii_uppercase()
        } else {
            value
        }
    } else {
        crate::commands::printf::float_format(
            number.abs(),
            conversion,
            precision,
            flags.contains(&b'#'),
        )
    };
    let mut bytes = sign.as_bytes().to_vec();
    bytes.extend_from_slice(rendered.as_bytes());
    Ok((bytes, sign.len()))
}
fn integer(
    number: f64,
    conversion: u8,
    precision: Option<usize>,
    flags: &[u8],
) -> Result<(Vec<u8>, usize), String> {
    let signed = matches!(conversion, b'd' | b'i');
    let value = number.trunc();
    if !signed && (!value.is_finite() || value < i64::MIN as f64 || value >= u64::MAX as f64) {
        let prefix = sign(value.is_sign_negative(), flags);
        let text = crate::commands::printf::float_format(
            value.abs(),
            b'g',
            Some(6),
            flags.contains(&b'#'),
        );
        return Ok((format!("{prefix}{text}").into_bytes(), prefix.len()));
    }
    let magnitude = if signed {
        (value as i64).unsigned_abs()
    } else if value < 0.0 {
        (value as i64) as u64
    } else {
        value as u64
    };
    let mut digits = match conversion {
        b'o' => format!("{magnitude:o}"),
        b'x' => format!("{magnitude:x}"),
        b'X' => format!("{magnitude:X}"),
        b'u' => magnitude.to_string(),
        _ => crate::commands::printf::float_format(value.abs(), b'f', Some(0), false),
    };
    if let Some(precision) = precision {
        if precision == 0 && value == 0.0 {
            digits.clear();
        }
        if digits.len() < precision {
            digits = format!("{}{digits}", "0".repeat(precision - digits.len()));
        }
    }
    let mut prefix = if signed {
        sign(value < 0.0, flags).to_owned()
    } else {
        String::new()
    };
    if flags.contains(&b'#') {
        match conversion {
            b'o' if !digits.starts_with('0') => prefix.push('0'),
            b'x' if magnitude != 0 => prefix.push_str("0x"),
            b'X' if magnitude != 0 => prefix.push_str("0X"),
            _ => {}
        }
    }
    let length = prefix.len();
    Ok((format!("{prefix}{digits}").into_bytes(), length))
}
fn sign(negative: bool, flags: &[u8]) -> &'static str {
    if negative {
        "-"
    } else if flags.contains(&b'+') {
        "+"
    } else if flags.contains(&b' ') {
        " "
    } else {
        ""
    }
}
fn hex(value: f64, precision: Option<usize>, alternate: bool) -> String {
    let bits = value.to_bits();
    let raw = ((bits >> 52) & 0x7ff) as i32;
    let mut whole = u64::from(raw != 0);
    let mut fraction = bits & ((1u64 << 52) - 1);
    let exponent = if value == 0.0 {
        0
    } else if raw == 0 {
        -1022
    } else {
        raw - 1023
    };
    let p = precision.unwrap_or(13);
    if p < 13 {
        let shift = 52 - 4 * p;
        let half = 1u64 << (shift - 1);
        let remainder = fraction & ((1u64 << shift) - 1);
        fraction >>= shift;
        if remainder > half
            || (remainder == half && (if p == 0 { whole } else { fraction }) & 1 != 0)
        {
            fraction += 1;
        }
        if fraction >= 1u64 << (4 * p) {
            whole += 1;
            fraction = 0;
        }
    }
    let mut digits = if p == 0 {
        String::new()
    } else if p <= 13 {
        format!("{fraction:0p$x}")
    } else {
        format!("{fraction:013x}{}", "0".repeat(p - 13))
    };
    if precision.is_none() {
        digits.truncate(digits.trim_end_matches('0').len());
    }
    format!(
        "0x{whole:x}{}{digits}p{exponent:+}",
        if !digits.is_empty() || alternate {
            "."
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_formats() {
        assert_eq!(numeric(b"%.2f", 1.235).unwrap(), b"1.24");
        assert_eq!(numeric(b"%.3g", 12345.0).unwrap(), b"1.23e+04");
    }
    #[test]
    fn integer_flags_and_dynamic_width() {
        assert_eq!(
            render(
                b"%#08x %*.*f",
                &[
                    Scalar::Number(10.0),
                    Scalar::Number(6.0),
                    Scalar::Number(2.0),
                    Scalar::Number(1.5)
                ],
                b"%.6g"
            )
            .unwrap(),
            b"0x00000a   1.50"
        );
    }
    #[test]
    fn string_precision_counts_bytes() {
        assert_eq!(
            render(
                b"%.2s",
                &[Scalar::string("a\u{732b}b".as_bytes().to_vec())],
                b"%.6g"
            )
            .unwrap(),
            &"a\u{732b}".as_bytes()[..2]
        );
    }
    #[test]
    fn invalid_format_and_missing_values() {
        assert!(render(b"%q", &[Scalar::Uninit], b"%.6g").is_err());
        assert!(render(b"%d", &[], b"%.6g").is_err());
    }
    #[test]
    fn unsigned_and_large_decimal_conversions() {
        assert_eq!(
            render(b"%u", &[Scalar::Number(-1.0)], b"%.6g").unwrap(),
            b"18446744073709551615"
        );
        assert_eq!(
            render(
                b"%d %x",
                &[Scalar::Number(1e30), Scalar::Number(1e30)],
                b"%.6g"
            )
            .unwrap(),
            b"1000000000000000019884624838656 1e+30"
        );
    }
}
