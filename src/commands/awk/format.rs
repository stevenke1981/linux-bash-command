//! `printf` and `sprintf` formatting for awk.
//!
//! Conversions: `%c %d %i %o %x %X %u %e %E %f %F %g %G %a %A %s %%`, with the flags
//! `- + space # 0`, width and precision, and `*` taken from the arguments. Length modifiers
//! (`h l L q j z t`) are accepted and ignored. Floating conversions reuse the C-style
//! formatter of the `printf` command. Unknown conversions are copied to the output.

use super::value::Value;
use crate::commands::printf::float::{format_float, pad, push_repeat, Spec};

/// Formats `args` according to `fmt`. Missing arguments count as empty or zero.
/// `convfmt` converts numbers printed with `%s`.
pub fn sprintf(fmt: &[u8], args: &[Value], convfmt: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len() + 16);
    let mut args = Args {
        values: args,
        next: 0,
    };
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        if fmt.get(i + 1) == Some(&b'%') {
            out.push(b'%');
            i += 2;
            continue;
        }
        let end = directive(fmt, i, &mut args, convfmt, &mut out);
        i = end;
    }
    out
}

/// Argument cursor; past the end every argument is uninitialized.
struct Args<'a> {
    values: &'a [Value],
    next: usize,
}

impl Args<'_> {
    fn take(&mut self) -> Value {
        let value = self.values.get(self.next).cloned().unwrap_or(Value::Uninit);
        self.next += 1;
        value
    }
}

/// Parsed flags, width and precision of one conversion.
struct Directive {
    spec: Spec,
    conv: u8,
}

/// Formats the conversion starting at `start` (a `%`) and returns the index after it.
fn directive(
    fmt: &[u8],
    start: usize,
    args: &mut Args,
    convfmt: &[u8],
    out: &mut Vec<u8>,
) -> usize {
    let mut i = start + 1;
    let mut spec = Spec::default();
    while let Some(&flag) = fmt.get(i) {
        match flag {
            b'-' => spec.left = true,
            b'+' => spec.plus = true,
            b' ' => spec.space = true,
            b'#' => spec.alt = true,
            b'0' => spec.zero = true,
            _ => break,
        }
        i += 1;
    }
    if fmt.get(i) == Some(&b'*') {
        let width = args.take().to_num() as i64;
        if width < 0 {
            spec.left = true;
        }
        spec.width = width.unsigned_abs() as usize;
        i += 1;
    } else {
        let (width, next) = read_number(fmt, i);
        spec.width = width;
        i = next;
    }
    if fmt.get(i) == Some(&b'.') {
        i += 1;
        if fmt.get(i) == Some(&b'*') {
            let precision = args.take().to_num() as i64;
            spec.precision = usize::try_from(precision).ok();
            i += 1;
        } else {
            let (precision, next) = read_number(fmt, i);
            spec.precision = Some(precision);
            i = next;
        }
    }
    while matches!(
        fmt.get(i),
        Some(b'h' | b'l' | b'L' | b'q' | b'j' | b'z' | b't')
    ) {
        i += 1;
    }
    let Some(&conv) = fmt.get(i) else {
        out.extend_from_slice(&fmt[start..]);
        return fmt.len();
    };
    let directive = Directive { spec, conv };
    match conv {
        b'd' | b'i' => {
            let value = args.take().to_num();
            let magnitude = value.trunc().abs();
            let neg = value < 0.0;
            let mag = if magnitude >= u64::MAX as f64 {
                u64::MAX
            } else {
                magnitude as u64
            };
            push_integer(out, neg, mag, &directive);
        }
        b'o' | b'x' | b'X' | b'u' => {
            let value = args.take().to_num().trunc();
            let mag = if value < 0.0 {
                (value as i64) as u64
            } else if value >= u64::MAX as f64 {
                u64::MAX
            } else {
                value as u64
            };
            push_integer(out, false, mag, &directive);
        }
        b'c' => {
            let value = args.take();
            let body = match value {
                Value::Num(n) | Value::StrNum(_, n) => vec![(n as i64) as u8],
                Value::Str(bytes) => bytes.into_iter().take(1).collect(),
                Value::Uninit => Vec::new(),
            };
            pad_bytes(out, &body, &directive.spec, false);
        }
        b's' => {
            let value = args.take();
            let text = value.to_str(convfmt);
            let limit = directive
                .spec
                .precision
                .unwrap_or(text.len())
                .min(text.len());
            pad_bytes(out, &text[..limit], &directive.spec, false);
        }
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
            let value = args.take().to_num();
            format_float(out, value, conv, &directive.spec);
        }
        _ => out.extend_from_slice(&fmt[start..=i]),
    }
    i + 1
}

fn read_number(fmt: &[u8], mut i: usize) -> (usize, usize) {
    let mut value = 0usize;
    while let Some(&d) = fmt.get(i) {
        if !d.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(d - b'0'));
        i += 1;
    }
    (value, i)
}

/// Appends an integer conversion (`d i o u x X`) with sign, precision, prefix and padding.
fn push_integer(out: &mut Vec<u8>, neg: bool, mag: u64, directive: &Directive) {
    let spec = &directive.spec;
    let mut digits = match directive.conv {
        b'o' => format!("{mag:o}"),
        b'x' => format!("{mag:x}"),
        b'X' => format!("{mag:X}"),
        _ => mag.to_string(),
    };
    if let Some(precision) = spec.precision {
        if precision == 0 && mag == 0 {
            digits.clear();
        } else if digits.len() < precision {
            let zeros = "0".repeat(precision - digits.len());
            digits.insert_str(0, &zeros);
        }
    }
    let signed = matches!(directive.conv, b'd' | b'i');
    let mut prefix = String::new();
    if signed {
        if neg {
            prefix.push('-');
        } else if spec.plus {
            prefix.push('+');
        } else if spec.space {
            prefix.push(' ');
        }
    }
    if spec.alt {
        match directive.conv {
            b'o' if !digits.starts_with('0') => digits.insert(0, '0'),
            b'x' if mag != 0 => prefix.push_str("0x"),
            b'X' if mag != 0 => prefix.push_str("0X"),
            _ => {}
        }
    }
    pad(out, &prefix, &digits, spec, spec.precision.is_none());
}

/// Appends `body` padded with spaces to the width (`-` pads on the right).
fn pad_bytes(out: &mut Vec<u8>, body: &[u8], spec: &Spec, _zero: bool) {
    let fill = spec.width.saturating_sub(body.len());
    if spec.left {
        out.extend_from_slice(body);
        push_repeat(out, b' ', fill);
    } else {
        push_repeat(out, b' ', fill);
        out.extend_from_slice(body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONV: &[u8] = b"%.6g";

    fn fmt(format: &str, args: &[Value]) -> String {
        String::from_utf8(sprintf(format.as_bytes(), args, CONV)).unwrap()
    }

    fn num(n: f64) -> Value {
        Value::Num(n)
    }

    #[test]
    fn integer_conversions_with_flags_and_width() {
        assert_eq!(fmt("%d|%i", &[num(42.9), num(-7.0)]), "42|-7");
        assert_eq!(
            fmt("[%5d][%-5d][%05d]", &[num(42.0), num(42.0), num(-42.0)]),
            "[   42][42   ][-0042]"
        );
        assert_eq!(fmt("%+d % d", &[num(5.0), num(5.0)]), "+5  5");
        assert_eq!(fmt("[%.3d]", &[num(7.0)]), "[007]");
        assert_eq!(fmt("[%.0d]", &[num(0.0)]), "[]");
    }

    #[test]
    fn unsigned_octal_and_hex_conversions() {
        assert_eq!(
            fmt("%o %x %X %u", &[num(8.0), num(255.0), num(255.0), num(3.0)]),
            "10 ff FF 3"
        );
        assert_eq!(
            fmt("%#o %#x %#X", &[num(8.0), num(255.0), num(255.0)]),
            "010 0xff 0XFF"
        );
        assert_eq!(fmt("%x", &[num(-1.0)]), "ffffffffffffffff");
        assert_eq!(fmt("%#x", &[num(0.0)]), "0");
    }

    #[test]
    fn float_conversions_delegate_to_c_style_formatting() {
        assert_eq!(
            fmt("%f %e %g", &[num(1.23456), num(31415.9), num(0.0001)]),
            "1.234560 3.141590e+04 0.0001"
        );
        assert_eq!(fmt("%.2E|%G", &[num(1234.5), num(1e-10)]), "1.23E+03|1E-10");
        assert_eq!(fmt("[%8.3f]", &[num(2.5)]), "[   2.500]");
        assert_eq!(fmt("%F", &[num(1.5)]), "1.500000");
    }

    #[test]
    fn string_conversions() {
        assert_eq!(fmt("[%s]", &[Value::Str(b"abc".to_vec())]), "[abc]");
        assert_eq!(fmt("[%.2s]", &[Value::Str(b"abc".to_vec())]), "[ab]");
        assert_eq!(
            fmt(
                "[%5s][%-5s]",
                &[Value::Str(b"ab".to_vec()), Value::Str(b"ab".to_vec())]
            ),
            "[   ab][ab   ]"
        );
        assert_eq!(fmt("%s", &[num(0.5)]), "0.5");
        assert_eq!(fmt("%s", &[num(12.0)]), "12");
    }

    #[test]
    fn character_conversion() {
        assert_eq!(fmt("%c%c", &[num(65.0), Value::Str(b"hi".to_vec())]), "Ah");
        assert_eq!(fmt("[%3c]", &[num(66.0)]), "[  B]");
        assert_eq!(fmt("[%-3c]", &[Value::Str(b"z".to_vec())]), "[z  ]");
        assert_eq!(fmt("%c", &[Value::Str(Vec::new())]), "");
    }

    #[test]
    fn star_takes_width_and_precision_from_arguments() {
        assert_eq!(fmt("[%*d]", &[num(4.0), num(7.0)]), "[   7]");
        assert_eq!(fmt("[%-*d]", &[num(4.0), num(7.0)]), "[7   ]");
        assert_eq!(fmt("[%*d]", &[num(-4.0), num(7.0)]), "[7   ]");
        assert_eq!(fmt("[%.*f]", &[num(2.0), num(1.0)]), "[1.00]");
        assert_eq!(
            fmt("[%.*s]", &[num(2.0), Value::Str(b"hello".to_vec())]),
            "[he]"
        );
    }

    #[test]
    fn percent_literal_and_unknown_conversions() {
        assert_eq!(fmt("100%%", &[]), "100%");
        assert_eq!(fmt("%z", &[num(1.0)]), "%z");
        assert_eq!(fmt("trailing %", &[]), "trailing %");
        assert_eq!(fmt("%5", &[]), "%5");
    }

    #[test]
    fn missing_arguments_are_empty_or_zero() {
        assert_eq!(fmt("[%s][%d]", &[]), "[][0]");
        assert_eq!(fmt("%d %d", &[num(1.0)]), "1 0");
    }

    #[test]
    fn length_modifiers_are_ignored() {
        assert_eq!(fmt("%ld %lld", &[num(1.0), num(2.0)]), "1 2");
    }

    #[test]
    fn number_to_string_is_used_for_fractional_percent_s() {
        use super::super::value::number_to_string;
        let text = sprintf(b"%s", &[num(1.0 / 3.0)], b"%.3f");
        assert_eq!(text, b"0.333".to_vec());
        assert_eq!(number_to_string(12.0, CONV), b"12".to_vec());
    }
}
