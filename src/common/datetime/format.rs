//! GNU-compatible `strftime` in the C locale.
//!
//! Supports every GNU conversion, the `-`, `_`, `0`, `^` and `#` flags, field widths, and
//! the `E`/`O` modifiers (ignored). Unknown directives are copied to the output unchanged,
//! as GNU does.

use jiff::{tz::TimeZone, Timestamp};

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Calendar fields of one instant in one time zone, ready for [`strftime`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenDown {
    pub year: i32,
    /// 1 through 12.
    pub month: u8,
    /// 1 through 31.
    pub day: u8,
    /// 0 through 23.
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanosecond: u32,
    /// 0 = Sunday.
    pub weekday: u8,
    /// 1 through 366.
    pub yday: u16,
    /// UTC offset of the zone at this instant, in seconds east of UTC.
    pub offset_seconds: i32,
    /// Zone abbreviation such as `UTC` or `CET`.
    pub abbreviation: String,
    /// Seconds since the Unix epoch (`%s`).
    pub epoch: i64,
    pub iso_year: i32,
    pub iso_week: u8,
}

/// Computes the calendar fields of `ts` as seen in `zone`.
pub fn broken_down(ts: Timestamp, zone: &TimeZone) -> BrokenDown {
    let zoned = ts.to_zoned(zone.clone());
    let iso = zoned.clone().iso_week_date();
    BrokenDown {
        year: i32::from(zoned.year()),
        month: zoned.month() as u8,
        day: zoned.day() as u8,
        hour: zoned.hour() as u8,
        minute: zoned.minute() as u8,
        second: zoned.second() as u8,
        nanosecond: zoned.subsec_nanosecond() as u32,
        weekday: zoned.weekday().to_sunday_zero_offset() as u8,
        yday: zoned.day_of_year() as u16,
        offset_seconds: zoned.offset().seconds(),
        abbreviation: zone.to_offset_info(ts).abbreviation().to_string(),
        epoch: ts.as_second(),
        iso_year: i32::from(iso.year()),
        iso_week: iso.week() as u8,
    }
}

/// Flags and width of one directive.
#[derive(Debug, Default, Clone, Copy)]
struct Spec {
    /// `-`: no padding at all.
    no_pad: bool,
    /// `_` (space) or `0` (zero) padding override.
    pad: Option<char>,
    /// `^`: upper-case the text.
    upper: bool,
    /// `#`: swap case (upper for day and month names, lower for `%p` and `%Z`).
    swap: bool,
    width: Option<usize>,
}

/// How the `#` flag treats a text conversion.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Swap {
    Names,
    Lower,
}

/// Expands `format` for `bd`.
pub fn strftime(format: &str, bd: &BrokenDown) -> String {
    let mut out = String::with_capacity(format.len() + 16);
    let mut rest = format;
    while let Some(pos) = rest.find('%') {
        out.push_str(&rest[..pos]);
        let (used, text) = expand_at(&rest[pos..], bd);
        out.push_str(&text);
        rest = &rest[pos + used..];
    }
    out.push_str(rest);
    out
}

/// Expands the directive at the start of `text` (which begins with `%`). Returns the number
/// of bytes consumed and the replacement text.
fn expand_at(text: &str, bd: &BrokenDown) -> (usize, String) {
    let body = &text.as_bytes()[1..];
    let mut spec = Spec::default();
    let mut idx = 0;
    while idx < body.len() {
        match body[idx] {
            b'-' => spec.no_pad = true,
            b'_' => spec.pad = Some(' '),
            b'0' => spec.pad = Some('0'),
            b'^' => spec.upper = true,
            b'#' => spec.swap = true,
            _ => break,
        }
        idx += 1;
    }
    let width_start = idx;
    while idx < body.len() && body[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx > width_start {
        spec.width = std::str::from_utf8(&body[width_start..idx])
            .ok()
            .and_then(|digits| digits.parse().ok());
    }
    let colons_start = idx;
    while idx < body.len() && body[idx] == b':' {
        idx += 1;
    }
    let colons = idx - colons_start;
    let modifier_follows = idx + 1 < body.len() && is_conversion_byte(body[idx + 1]);
    if colons == 0 && modifier_follows && matches!(body[idx], b'E' | b'O') {
        idx += 1;
    }
    let Some(&conv_byte) = body.get(idx) else {
        return (text.len(), text.to_string());
    };
    if !conv_byte.is_ascii() {
        return (1, "%".to_string());
    }
    let used = 1 + idx + 1;
    let expansion = if colons > 0 {
        zone_offset(colons, conv_byte, bd)
    } else {
        conversion(conv_byte, &spec, bd)
    };
    match expansion {
        Some(value) => (used, value),
        None => (used, text[..used].to_string()),
    }
}

fn is_conversion_byte(byte: u8) -> bool {
    b"aAbBcCdDeFgGhHIjklmMnNpPqrRsStTuUVwWxXyYzZ%".contains(&byte)
}

/// `%:z`, `%::z`, `%:::z`. Only valid after one to three colons and before `z`.
fn zone_offset(colons: usize, conv: u8, bd: &BrokenDown) -> Option<String> {
    if conv != b'z' || colons > 3 {
        return None;
    }
    let total = bd.offset_seconds;
    let sign = if total < 0 { '-' } else { '+' };
    let abs = total.unsigned_abs();
    let (hours, minutes, seconds) = (abs / 3600, (abs % 3600) / 60, abs % 60);
    Some(match colons {
        1 => format!("{sign}{hours:02}:{minutes:02}"),
        2 => format!("{sign}{hours:02}:{minutes:02}:{seconds:02}"),
        _ => {
            let mut text = format!("{sign}{hours:02}");
            if seconds != 0 {
                text.push_str(&format!(":{minutes:02}:{seconds:02}"));
            } else if minutes != 0 {
                text.push_str(&format!(":{minutes:02}"));
            }
            text
        }
    })
}

/// Expands one conversion character. `None` means unknown (copied literally).
fn conversion(conv: u8, spec: &Spec, bd: &BrokenDown) -> Option<String> {
    let hour12 = if bd.hour.is_multiple_of(12) {
        12
    } else {
        bd.hour % 12
    };
    let century = bd.year.div_euclid(100);
    let wday = i64::from(bd.weekday);
    let yday0 = i64::from(bd.yday) - 1;
    let out = match conv {
        b'a' => {
            return Some(text(
                &WEEKDAYS[usize::from(bd.weekday)][..3],
                spec,
                Swap::Names,
            ))
        }
        b'A' => return Some(text(WEEKDAYS[usize::from(bd.weekday)], spec, Swap::Names)),
        b'b' | b'h' => {
            return Some(text(
                &MONTHS[usize::from(bd.month) - 1][..3],
                spec,
                Swap::Names,
            ))
        }
        b'B' => return Some(text(MONTHS[usize::from(bd.month) - 1], spec, Swap::Names)),
        b'c' => return Some(text(&date_time_text(bd), spec, Swap::Lower)),
        b'C' => num(i64::from(century), 2, '0', spec),
        b'd' => num(i64::from(bd.day), 2, '0', spec),
        b'D' => return Some(text(&strftime("%m/%d/%y", bd), spec, Swap::Lower)),
        b'e' => num(i64::from(bd.day), 2, ' ', spec),
        b'F' => return Some(strftime("%Y-%m-%d", bd)),
        b'g' => num(i64::from(bd.iso_year.rem_euclid(100)), 2, '0', spec),
        b'G' => num(i64::from(bd.iso_year), 4, '0', spec),
        b'H' => num(i64::from(bd.hour), 2, '0', spec),
        b'I' => num(i64::from(hour12), 2, '0', spec),
        b'j' => num(i64::from(bd.yday), 3, '0', spec),
        b'k' => num(i64::from(bd.hour), 2, ' ', spec),
        b'l' => num(i64::from(hour12), 2, ' ', spec),
        b'm' => num(i64::from(bd.month), 2, '0', spec),
        b'M' => num(i64::from(bd.minute), 2, '0', spec),
        b'n' => "\n".to_string(),
        b'N' => fraction(bd.nanosecond, spec),
        b'p' => return Some(text(meridiem(bd), spec, Swap::Lower)),
        b'P' => return Some(text(&meridiem(bd).to_ascii_lowercase(), spec, Swap::Names)),
        b'q' => num(i64::from(bd.month - 1) / 3 + 1, 1, '0', spec),
        b'r' => return Some(text(&strftime("%I:%M:%S %p", bd), spec, Swap::Lower)),
        b'R' => return Some(text(&strftime("%H:%M", bd), spec, Swap::Lower)),
        b's' => num(bd.epoch, 1, '0', spec),
        b'S' => num(i64::from(bd.second), 2, '0', spec),
        b't' => "\t".to_string(),
        b'T' => return Some(text(&strftime("%H:%M:%S", bd), spec, Swap::Lower)),
        b'u' => num(if wday == 0 { 7 } else { wday }, 1, '0', spec),
        b'U' => num((yday0 + 7 - wday) / 7, 2, '0', spec),
        b'V' => num(i64::from(bd.iso_week), 2, '0', spec),
        b'w' => num(wday, 1, '0', spec),
        b'W' => num((yday0 + 7 - (wday + 6) % 7) / 7, 2, '0', spec),
        b'x' => return Some(text(&strftime("%m/%d/%y", bd), spec, Swap::Lower)),
        b'X' => return Some(text(&strftime("%H:%M:%S", bd), spec, Swap::Lower)),
        b'y' => num(i64::from(bd.year.rem_euclid(100)), 2, '0', spec),
        b'Y' => num(i64::from(bd.year), 4, '0', spec),
        b'z' => numeric_zone(bd.offset_seconds),
        b'Z' => return Some(text(&bd.abbreviation, spec, Swap::Lower)),
        b'%' => return Some(text("%", spec, Swap::Names)),
        _ => return None,
    };
    Some(out)
}

/// `%c` in the C locale. The year is not zero-padded, which matches GNU for years below 1000.
fn date_time_text(bd: &BrokenDown) -> String {
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {}",
        &WEEKDAYS[usize::from(bd.weekday)][..3],
        &MONTHS[usize::from(bd.month) - 1][..3],
        bd.day,
        bd.hour,
        bd.minute,
        bd.second,
        bd.year
    )
}

fn meridiem(bd: &BrokenDown) -> &'static str {
    if bd.hour < 12 {
        "AM"
    } else {
        "PM"
    }
}

/// `%z`: `+hhmm`.
fn numeric_zone(total: i32) -> String {
    let sign = if total < 0 { '-' } else { '+' };
    let abs = total.unsigned_abs();
    format!("{sign}{:02}{:02}", abs / 3600, (abs % 3600) / 60)
}

/// `%N`: nanoseconds. The width counts digits: fewer truncates, more pads with zeros.
fn fraction(nanosecond: u32, spec: &Spec) -> String {
    let digits = format!("{nanosecond:09}");
    let width = spec.width.unwrap_or(9);
    if width <= 9 {
        digits[..width].to_string()
    } else {
        format!("{digits}{}", "0".repeat(width - 9))
    }
}

/// A numeric field. The default width and pad character apply unless a flag or width
/// overrides them. A negative value gets its sign in front of any padding.
fn num(value: i64, default_width: usize, default_pad: char, spec: &Spec) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let digits = value.unsigned_abs().to_string();
    let width = spec.width.unwrap_or(default_width);
    let pad = if spec.no_pad {
        None
    } else {
        Some(spec.pad.unwrap_or(default_pad))
    };
    let body_len = sign.len() + digits.len();
    match pad {
        Some(fill) if body_len < width => {
            let fill_text: String = std::iter::repeat_n(fill, width - body_len).collect();
            if fill == '0' {
                format!("{sign}{fill_text}{digits}")
            } else {
                format!("{fill_text}{sign}{digits}")
            }
        }
        _ => format!("{sign}{digits}"),
    }
}

/// A text field: case flags first, then space padding (or zero padding with `0`) to the width.
fn text(value: &str, spec: &Spec, swap: Swap) -> String {
    let mut out = value.to_string();
    if spec.upper || (spec.swap && swap == Swap::Names) {
        out = out.to_uppercase();
    } else if spec.swap && swap == Swap::Lower {
        out = out.to_lowercase();
    }
    let len = out.chars().count();
    let width = spec.width.unwrap_or(0);
    if spec.no_pad || len >= width {
        return out;
    }
    let fill = spec.pad.unwrap_or(' ');
    let padding: String = std::iter::repeat_n(fill, width - len).collect();
    format!("{padding}{out}")
}

#[cfg(test)]
mod tests;
