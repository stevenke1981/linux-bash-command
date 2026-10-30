//! Lookup tables and small text helpers for the `-d` parser: weekday, month, unit, meridiem
//! and zone words, numeric offsets, and clock-time fields.

use super::{Clock, Unit};

/// Anchors a weekday and returns its index (0 = Sunday).
pub(super) fn weekday_index(lower: &str) -> Option<u8> {
    const NAMES: [(&str, u8); 18] = [
        ("sun", 0),
        ("sunday", 0),
        ("mon", 1),
        ("monday", 1),
        ("tue", 2),
        ("tues", 2),
        ("tuesday", 2),
        ("wed", 3),
        ("wednes", 3),
        ("wednesday", 3),
        ("thu", 4),
        ("thur", 4),
        ("thurs", 4),
        ("thursday", 4),
        ("fri", 5),
        ("friday", 5),
        ("sat", 6),
        ("saturday", 6),
    ];
    let trimmed = lower.strip_suffix('.').unwrap_or(lower);
    NAMES
        .iter()
        .find(|(name, _)| *name == trimmed)
        .map(|(_, index)| *index)
}

/// Month number (1 through 12) for a full or abbreviated English month name.
pub(super) fn month_number(lower: &str) -> Option<i8> {
    const MONTHS: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    if lower == "sept" {
        return Some(9);
    }
    if lower.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|name| *name == lower || (lower.len() == 3 && name.starts_with(lower)))
        .map(|index| index as i8 + 1)
}

/// Relative unit words, with the multiplier that converts the count to the unit.
pub(super) fn unit_word(lower: &str) -> Option<(Unit, i64)> {
    Some(match lower {
        "year" | "years" => (Unit::Year, 1),
        "month" | "months" => (Unit::Month, 1),
        "fortnight" | "fortnights" => (Unit::Day, 14),
        "week" | "weeks" => (Unit::Day, 7),
        "day" | "days" => (Unit::Day, 1),
        "hour" | "hours" => (Unit::Hour, 1),
        "minute" | "minutes" | "min" | "mins" => (Unit::Minute, 1),
        "second" | "seconds" | "sec" | "secs" => (Unit::Second, 1),
        _ => return None,
    })
}

/// `Some(true)` for pm, `Some(false)` for am.
pub(super) fn meridiem(lower: &str) -> Option<bool> {
    match lower {
        "am" | "a.m." => Some(false),
        "pm" | "p.m." => Some(true),
        _ => None,
    }
}

/// Zone abbreviations with a fixed offset in seconds.
pub(super) fn zone_word(lower: &str) -> Option<i32> {
    const HOUR: i32 = 3600;
    Some(match lower {
        "utc" | "gmt" | "ut" | "z" | "wet" => 0,
        "bst" | "cet" | "west" => HOUR,
        "cest" | "eet" => 2 * HOUR,
        "eest" | "msk" => 3 * HOUR,
        "hkt" => 8 * HOUR,
        "jst" | "kst" => 9 * HOUR,
        "est" => -5 * HOUR,
        "edt" | "cdt" => -4 * HOUR,
        "cst" => -6 * HOUR,
        "mst" | "mdt" => -6 * HOUR,
        "pst" => -8 * HOUR,
        "pdt" => -7 * HOUR,
        _ => return None,
    })
}

/// Offset text after a clock time: `Z`, `+HH`, `+HHMM`, `+HH:MM`.
pub(super) fn parse_offset_text(text: &str) -> Option<i32> {
    if text.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let body = &text[1..];
    let (hours, minutes) = match body.split_once(':') {
        Some((h, m)) if h.len() == 2 && m.len() == 2 => (h, m),
        Some(_) => return None,
        None if body.len() == 4 => body.split_at(2),
        None if matches!(body.len(), 1 | 2) => (body, "0"),
        None => return None,
    };
    let hours: i32 = all_digits(hours)?.parse().ok()?;
    let minutes: i32 = all_digits(minutes)?.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60))
}

/// `text` when it is non-empty and all ASCII digits.
pub(super) fn all_digits(text: &str) -> Option<&str> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())).then_some(text)
}

/// A field of at most four digits.
pub(super) fn parse_part<T: std::str::FromStr>(text: &str) -> Option<T> {
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// A one- or two-digit day (`2`, `02`) with an optional ordinal suffix.
pub(super) fn short_day(text: &str) -> Option<i64> {
    let digits = text
        .strip_suffix("st")
        .or_else(|| text.strip_suffix("nd"))
        .or_else(|| text.strip_suffix("rd"))
        .or_else(|| text.strip_suffix("th"))
        .unwrap_or(text);
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    all_digits(digits)?.parse().ok()
}

/// A four-digit year.
pub(super) fn four_digits(text: &str) -> Option<i64> {
    (text.len() == 4).then(|| all_digits(text)?.parse().ok())?
}

/// Applies a meridiem to an hour written as 1 through 12.
pub(super) fn apply_meridiem(clock: Clock, pm: bool) -> Option<Clock> {
    if !(1..=12).contains(&clock.hour) {
        return None;
    }
    Some(Clock {
        hour: clock.hour % 12 + if pm { 12 } else { 0 },
        ..clock
    })
}

/// `HH:MM[:SS[.FRACTION]]` with one or two digits per field. Hours are 0 through 23 and
/// seconds 0 through 59.
pub(super) fn parse_clock_core(core: &str) -> Option<Clock> {
    let mut parts = core.split(':');
    let hour = parts.next()?;
    let minute = parts.next()?;
    let seconds = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let clock = parse_clock_parts(hour, minute, seconds)?;
    (clock.hour <= 23).then_some(clock)
}

/// Builds a clock from its text fields; `seconds` may carry a `.fraction`.
pub(super) fn parse_clock_parts(hour: &str, minute: &str, seconds: Option<&str>) -> Option<Clock> {
    let hour: i8 = parse_part(hour)?;
    let minute: i8 = parse_part(minute)?;
    let (second, nanos) = match seconds {
        None => (0, 0),
        Some(text) => match text.split_once('.') {
            Some((whole, fraction)) => {
                all_digits(fraction)?;
                (parse_part::<i8>(whole)?, fraction_nanos(fraction))
            }
            None => (parse_part::<i8>(text)?, 0),
        },
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(Clock {
        hour,
        minute,
        second,
        nanos,
    })
}

/// The first nine digits of a fraction as nanoseconds (`5` is 500 000 000).
pub(super) fn fraction_nanos(fraction: &str) -> i32 {
    let mut digits: String = fraction.chars().take(9).collect();
    while digits.len() < 9 {
        digits.push('0');
    }
    digits.parse().unwrap_or(0)
}
