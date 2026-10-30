//! Rendering of one line of input bytes in a given format, plus float and character text.

use super::{Format, Kind, MAX_ITEM};

/// Item columns for each format in a group. Every format's full line is made as wide as the
/// widest of them, so the `-t` columns line up as in GNU `od`.
pub(super) fn column_widths(formats: &[Format], width: usize) -> Vec<usize> {
    let per_line = |format: &Format| width / format.size;
    let widest = formats
        .iter()
        .map(|f| field_width(*f) * per_line(f))
        .max()
        .unwrap_or(0);
    formats.iter().map(|f| widest / per_line(f)).collect()
}

/// Appends the items of one line for `format`, each right-justified in `item_width` columns.
pub(super) fn format_items(
    text: &mut String,
    format: Format,
    item_width: usize,
    bytes: &[u8],
    width: usize,
    big: bool,
) {
    let mut count = 0;
    for chunk in bytes.chunks(format.size) {
        let mut item = [0u8; MAX_ITEM];
        item[..chunk.len()].copy_from_slice(chunk);
        text.push_str(&format!("{:>item_width$}", item_text(format, &item, big)));
        count += 1;
    }
    if format.ascii {
        let full = width / format.size;
        let missing = full.saturating_sub(count);
        text.push_str(&" ".repeat(missing * item_width));
        text.push_str("  >");
        text.extend(bytes.iter().map(|&b| {
            if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            }
        }));
        text.push('<');
    }
}

/// Width of one printed item including its leading separator.
pub(super) fn field_width(format: Format) -> usize {
    match format.kind {
        Kind::Named | Kind::Char => 4,
        Kind::Octal => 1 + (8 * format.size).div_ceil(3),
        Kind::Hex => 1 + 2 * format.size,
        Kind::Unsigned => 1 + [3, 5, 10, 20][size_index(format.size)],
        Kind::Signed => 1 + [4, 6, 11, 20][size_index(format.size)],
        Kind::Float => match format.size {
            4 => 16,
            8 => 25,
            _ => 37,
        },
    }
}

pub(super) fn size_index(size: usize) -> usize {
    match size {
        1 => 0,
        2 => 1,
        4 => 2,
        _ => 3,
    }
}

/// The text of one item without its column padding.
fn item_text(format: Format, item: &[u8; MAX_ITEM], big: bool) -> String {
    let size = format.size;
    let value = load_uint(&item[..size], big);
    match format.kind {
        Kind::Named => named(value as u8),
        Kind::Char => char_escape(value as u8),
        Kind::Octal => format!("{:0width$o}", value, width = (8 * size).div_ceil(3)),
        Kind::Hex => format!("{:0width$x}", value, width = 2 * size),
        Kind::Unsigned => value.to_string(),
        Kind::Signed => {
            let shift = 64 - 8 * size as u32;
            let signed = ((value << shift) as i64) >> shift;
            signed.to_string()
        }
        Kind::Float => {
            let number = match size {
                4 => f64::from(f32::from_bits(value as u32)),
                8 => f64::from_bits(value),
                _ => extended_to_f64(&item[..10]),
            };
            float_text(number, size == 4)
        }
    }
}

pub(super) fn load_uint(bytes: &[u8], big: bool) -> u64 {
    if big {
        bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
    } else {
        bytes
            .iter()
            .rev()
            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
    }
}

const NAMES: [&str; 33] = [
    "nul", "soh", "stx", "etx", "eot", "enq", "ack", "bel", "bs", "ht", "nl", "vt", "ff", "cr",
    "so", "si", "dle", "dc1", "dc2", "dc3", "dc4", "nak", "syn", "etb", "can", "em", "sub", "esc",
    "fs", "gs", "rs", "us", "sp",
];

/// `-t a` text: the low seven bits as a control name, `sp`, `del`, or the character itself.
pub(super) fn named(byte: u8) -> String {
    let low = byte & 0x7f;
    match low {
        0..=32 => NAMES[usize::from(low)].to_owned(),
        127 => "del".to_owned(),
        _ => (low as char).to_string(),
    }
}

/// `-t c` text: C-style escapes for the common control characters, graphic characters as
/// themselves, and other bytes as three octal digits.
pub(super) fn char_escape(byte: u8) -> String {
    match byte {
        0 => "\\0".to_owned(),
        7 => "\\a".to_owned(),
        8 => "\\b".to_owned(),
        9 => "\\t".to_owned(),
        10 => "\\n".to_owned(),
        11 => "\\v".to_owned(),
        12 => "\\f".to_owned(),
        13 => "\\r".to_owned(),
        0x20..=0x7e => (byte as char).to_string(),
        _ => format!("{byte:03o}"),
    }
}

/// Decodes an x87 80-bit extended value (little-endian, 10 bytes) to f64.
pub(super) fn extended_to_f64(bytes: &[u8]) -> f64 {
    let mut mantissa_bytes = [0u8; 8];
    mantissa_bytes.copy_from_slice(&bytes[..8]);
    let mantissa = u64::from_le_bytes(mantissa_bytes);
    let sign_exp = u16::from_le_bytes([bytes[8], bytes[9]]);
    let exponent = i32::from(sign_exp & 0x7fff);
    let value = if exponent == 0x7fff {
        if mantissa << 1 == 0 {
            f64::INFINITY
        } else {
            f64::NAN
        }
    } else if mantissa == 0 {
        0.0
    } else {
        mantissa as f64 * 2f64.powi(exponent - 16383 - 63)
    };
    if sign_exp & 0x8000 != 0 {
        -value
    } else {
        value
    }
}

/// Formats a float the way GNU `od` does: the shortest `%g` precision whose text parses back
/// to the same value (9 digits at most for single, 17 for double).
pub(super) fn float_text(value: f64, single: bool) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    let (low, high) = if single { (1, 9) } else { (1, 17) };
    for precision in low..=high {
        let text = format_g(value, precision);
        let same = if single {
            text.parse::<f32>().ok() == Some(value as f32)
        } else {
            text.parse::<f64>().ok() == Some(value)
        };
        if same {
            return text;
        }
    }
    format_g(value, high)
}

/// C `%.{precision}g` for a finite value.
pub(super) fn format_g(value: f64, precision: usize) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let scientific = format!("{:.*e}", precision - 1, value);
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if exponent < -4 || exponent >= precision as i32 {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{sign}{:02}",
            trim_fraction(mantissa),
            exponent.unsigned_abs()
        )
    } else {
        let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
        trim_fraction(&format!("{value:.decimals$}"))
    }
}

pub(super) fn trim_fraction(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_use_shortest_round_trip_g() {
        assert_eq!(
            float_text(f64::from(f32::from_bits(0x4443_4241)), true),
            "781.0352"
        );
        assert_eq!(float_text(f64::from(204_057.08_f32), true), "204057.08");
        assert_eq!(float_text(5.329_13e7, true), "5.32913e+07");
        assert_eq!(float_text(1.0, false), "1");
        assert_eq!(float_text(0.5, false), "0.5");
        assert_eq!(float_text(f64::INFINITY, false), "inf");
    }

    #[test]
    fn named_and_char_escapes_match_gnu() {
        assert_eq!(named(0), "nul");
        assert_eq!(named(0x80), "nul");
        assert_eq!(named(b' '), "sp");
        assert_eq!(named(0x7f), "del");
        assert_eq!(char_escape(0), "\\0");
        assert_eq!(char_escape(0x0a), "\\n");
        assert_eq!(char_escape(0x01), "001");
        assert_eq!(char_escape(0xff), "377");
    }
}
