//! Date and time support shared by `date`, `touch`, `stat` and `ls`.
//!
//! - [`parse`] reads GNU-style date strings (`date -d`, `touch -d`).
//! - [`broken_down`] and [`strftime`] format an instant in a zone, using GNU conversions in
//!   the C locale.
//! - [`zone_from_env`] resolves the zone from `TZ`, falling back to the system zone.

use std::time::SystemTime;

use jiff::{tz::TimeZone, Timestamp};

pub mod format;
pub mod parse;

pub use format::{broken_down, strftime, BrokenDown};
pub use parse::{parse, InvalidDate};

/// The zone named by `TZ`, or the system zone when `TZ` is unset.
///
/// Follows glibc: a leading `:` is ignored, an empty value means UTC, and an unknown name
/// falls back to UTC rather than failing.
pub fn zone_from_env() -> TimeZone {
    match std::env::var_os("TZ") {
        None => TimeZone::system(),
        Some(value) => zone_from_name(&value.to_string_lossy()),
    }
}

/// Resolves a `TZ` value (IANA name, POSIX-style `NAME[+-]HH[:MM]`, or empty).
pub fn zone_from_name(value: &str) -> TimeZone {
    let name = value.strip_prefix(':').unwrap_or(value);
    if name.is_empty() {
        return TimeZone::UTC;
    }
    if let Ok(zone) = TimeZone::get(name) {
        return zone;
    }
    posix_fixed_zone(name).unwrap_or(TimeZone::UTC)
}

/// A POSIX zone such as `JST-9` or `EST+5`. The sign is inverted relative to ISO 8601: a
/// positive value means west of UTC.
fn posix_fixed_zone(text: &str) -> Option<TimeZone> {
    let name_end = text.find(|c: char| !c.is_ascii_alphabetic())?;
    if name_end < 3 {
        return None;
    }
    let rest = &text[name_end..];
    let (sign, body) = match rest.as_bytes().first()? {
        b'+' => (-1, &rest[1..]),
        b'-' => (1, &rest[1..]),
        b'0'..=b'9' => (-1, rest),
        _ => return None,
    };
    let (hours, minutes) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?),
        None => (body.parse::<i32>().ok()?, 0),
    };
    if !(0..=24).contains(&hours) || !(0..60).contains(&minutes) {
        return None;
    }
    let offset = sign * (hours * 3600 + minutes * 60);
    jiff::tz::Offset::from_seconds(offset)
        .ok()
        .map(TimeZone::fixed)
}

/// The current instant.
pub fn now() -> Timestamp {
    Timestamp::now()
}

/// Converts a file time from `std::fs::Metadata` into a timestamp. Times before the epoch or
/// out of range map to the epoch.
pub fn from_system_time(time: SystemTime) -> Timestamp {
    Timestamp::try_from(time).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Formats `ts` in `zone` with the GNU `strftime` rules.
pub fn format_in(format_text: &str, ts: Timestamp, zone: &TimeZone) -> String {
    strftime(format_text, &broken_down(ts, zone))
}

#[cfg(test)]
mod tests;
