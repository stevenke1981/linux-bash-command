//! Parsing of `-t` type strings, byte counts (`-j -N -S`) and traditional offsets.

use super::{Format, Kind, Radix};

pub(super) fn letter_format(letter: &str) -> Format {
    let (kind, size) = match letter {
        "a" => (Kind::Named, 1),
        "b" => (Kind::Octal, 1),
        "c" => (Kind::Char, 1),
        "d" => (Kind::Unsigned, 2),
        "f" => (Kind::Float, 4),
        "i" => (Kind::Signed, 4),
        "l" => (Kind::Signed, 8),
        "o" => (Kind::Octal, 2),
        "s" => (Kind::Signed, 2),
        _ => (Kind::Hex, 2),
    };
    Format {
        kind,
        size,
        ascii: false,
    }
}

pub(super) fn parse_radix(value: &str) -> Result<Radix, String> {
    match value {
        "o" => Ok(Radix::Octal),
        "d" => Ok(Radix::Decimal),
        "x" => Ok(Radix::Hex),
        "n" => Ok(Radix::None),
        _ => Err(format!(
            "invalid output address radix '{value}'; it must be one character from [doxn]"
        )),
    }
}

/// Parses a `-t` type string such as `x1`, `d2z`, `fD` or `a c`-style sequences.
pub(super) fn parse_formats(spec: &str) -> Result<Vec<Format>, String> {
    let bytes = spec.as_bytes();
    let mut formats = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let letter = bytes[index] as char;
        index += 1;
        let kind = match letter {
            'a' => Kind::Named,
            'c' => Kind::Char,
            'd' => Kind::Signed,
            'o' => Kind::Octal,
            'u' => Kind::Unsigned,
            'x' => Kind::Hex,
            'f' => Kind::Float,
            _ => {
                return Err(format!(
                    "invalid character '{letter}' in type string '{spec}'"
                ))
            }
        };
        let (size, used) = parse_type_size(kind, &spec[index..], spec)?;
        index += used;
        let mut ascii = false;
        if bytes.get(index) == Some(&b'z') {
            ascii = true;
            index += 1;
        }
        formats.push(Format { kind, size, ascii });
    }
    if formats.is_empty() {
        return Err(format!("invalid type string '{spec}'"));
    }
    Ok(formats)
}

/// Reads the optional size after a type letter. Returns the size and the bytes consumed.
pub(super) fn parse_type_size(
    kind: Kind,
    rest: &str,
    spec: &str,
) -> Result<(usize, usize), String> {
    let floating = kind == Kind::Float;
    let default = if floating { 8 } else { 4 };
    let first = rest.as_bytes().first().copied();
    match first {
        Some(b'0'..=b'9') => {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            let size: usize = rest[..digits].parse().unwrap_or(0);
            let valid = if floating {
                matches!(size, 4 | 8 | 16)
            } else {
                matches!(size, 1 | 2 | 4 | 8)
            };
            if !valid {
                return Err(format!(
                    "invalid type string '{spec}';\nthis system doesn't provide a {size}-byte {} type",
                    if floating { "floating-point" } else { "integral" }
                ));
            }
            Ok((size, digits))
        }
        Some(letter @ (b'C' | b'S' | b'I' | b'L' | b'F' | b'D'))
            if !matches!(kind, Kind::Named | Kind::Char) =>
        {
            let size = match (floating, letter) {
                (false, b'C') => 1,
                (false, b'S') => 2,
                (false, b'I') => 4,
                (false, b'L') => 8,
                (true, b'F') => 4,
                (true, b'D') => 8,
                (true, b'L') => 16,
                _ => {
                    return Err(format!(
                        "invalid character '{}' in type string '{spec}'",
                        letter as char
                    ))
                }
            };
            Ok((size, 1))
        }
        _ => Ok((default_size(kind, default), 0)),
    }
}

pub(super) fn default_size(kind: Kind, float_default: usize) -> usize {
    match kind {
        Kind::Float => float_default,
        Kind::Named | Kind::Char => 1,
        _ => 4,
    }
}

/// Parses a byte count: decimal or `0x` hex, with an optional GNU suffix (b, KB, K, MB, M, ...).
pub(super) fn parse_count(text: &str) -> Option<u64> {
    let (digits, radix) = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(rest) => (rest, 16),
        None => (text, 10),
    };
    let split = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    if split == 0 {
        return None;
    }
    let number = u64::from_str_radix(&digits[..split], radix).ok()?;
    let multiplier: u64 = match &digits[split..] {
        "" => 1,
        "b" => 512,
        "KB" => 1000,
        "K" | "KiB" => 1024,
        "MB" => 1_000_000,
        "M" | "MiB" => 1 << 20,
        "GB" => 1_000_000_000,
        "G" | "GiB" => 1 << 30,
        _ => return None,
    };
    number.checked_mul(multiplier)
}

/// Parses a traditional `[+]OFFSET[.][b]` operand: octal unless `0x` or a trailing `.`.
pub(super) fn parse_offset(text: &str) -> Option<u64> {
    let body = text.strip_prefix('+').unwrap_or(text);
    let (body, multiplier) = match body.strip_suffix('b') {
        Some(rest) => (rest, 512),
        None => (body, 1),
    };
    let number = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()?
    } else if let Some(decimal) = body.strip_suffix('.') {
        decimal.parse().ok()?
    } else {
        if body.is_empty() {
            return None;
        }
        u64::from_str_radix(body, 8).ok()?
    };
    number.checked_mul(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_type_strings_with_sizes_and_z() {
        let formats = parse_formats("x1z").unwrap();
        assert_eq!(
            formats,
            vec![Format {
                kind: Kind::Hex,
                size: 1,
                ascii: true
            }]
        );
        let formats = parse_formats("fF").unwrap();
        assert_eq!(formats[0].size, 4);
        assert_eq!(parse_formats("fD").unwrap()[0].size, 8);
        assert_eq!(parse_formats("dI").unwrap()[0].size, 4);
        assert_eq!(parse_formats("d").unwrap()[0].size, 4);
        assert_eq!(parse_formats("cx2").unwrap().len(), 2);
    }

    #[test]
    fn rejects_bad_type_strings_with_gnu_text() {
        let err = parse_formats("z").unwrap_err();
        assert_eq!(err, "invalid character 'z' in type string 'z'");
        let err = parse_formats("d3").unwrap_err();
        assert!(err.contains("3-byte integral type"), "{err}");
    }

    #[test]
    fn counts_accept_hex_and_suffixes() {
        assert_eq!(parse_count("16"), Some(16));
        assert_eq!(parse_count("0x10"), Some(16));
        assert_eq!(parse_count("1KB"), Some(1000));
        assert_eq!(parse_count("1K"), Some(1024));
        assert_eq!(parse_count("2b"), Some(1024));
        assert_eq!(parse_count("-1"), None);
        assert_eq!(parse_count("x"), None);
    }

    #[test]
    fn traditional_offsets_are_octal_unless_decimal_marked() {
        assert_eq!(parse_offset("10"), Some(8));
        assert_eq!(parse_offset("+10."), Some(10));
        assert_eq!(parse_offset("+1b"), Some(512));
        assert_eq!(parse_offset("0x10"), Some(16));
        assert_eq!(parse_offset("+"), None);
    }
}
