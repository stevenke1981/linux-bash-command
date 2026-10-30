//! Parser for GNU-style date strings (`date -d`, `touch -d`).
//!
//! The string is split into whitespace tokens and scanned left to right. Each token fills
//! one slot of a [`Spec`]: an absolute date, a clock time, a zone, a weekday, or relative
//! amounts. [`resolve`] then combines the slots with the current time. Military zone letters
//! (`x`) are rejected, as the project specification requires.

use std::fmt;

use jiff::{
    civil::{Date, DateTime},
    tz::{Offset, TimeZone},
    Timestamp,
};

mod words;
use words::*;

/// The input could not be parsed. Displays as `invalid date 'INPUT'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDate(pub String);

impl fmt::Display for InvalidDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid date '{}'", self.0)
    }
}

/// Parses `input` relative to `now`. `zone` is the zone used when the string names none
/// (normally the `TZ` zone); `TZ="Zone" ...` prefixes and explicit offsets override it.
pub fn parse(input: &str, now: Timestamp, zone: &TimeZone) -> Result<Timestamp, InvalidDate> {
    let fail = || InvalidDate(input.to_string());
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix('@') {
        return parse_epoch(rest.trim_start()).ok_or_else(fail);
    }
    let (prefix_zone, body) = split_tz_prefix(trimmed).ok_or_else(fail)?;
    let spec = scan(body).ok_or_else(fail)?;
    let zone = spec
        .zone
        .clone()
        .or(prefix_zone)
        .unwrap_or_else(|| zone.clone());
    resolve(&spec, now, &zone).ok_or_else(fail)
}

/// `@SECONDS[.FRACTION]`, where SECONDS may be negative.
fn parse_epoch(text: &str) -> Option<Timestamp> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds: i128 = whole.parse().ok()?;
    let nanos = fraction_nanos(fraction);
    let magnitude = seconds
        .checked_mul(1_000_000_000)?
        .checked_add(i128::from(nanos))?;
    let total = if negative { -magnitude } else { magnitude };
    Timestamp::from_nanosecond(total).ok()
}

/// Consumes a leading `TZ=NAME` or `TZ="NAME"` and returns the zone and the remainder.
fn split_tz_prefix(text: &str) -> Option<(Option<TimeZone>, &str)> {
    let Some(rest) = text.strip_prefix("TZ=") else {
        return Some((None, text));
    };
    let (name, remainder) = if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], &quoted[end + 1..])
    } else {
        match rest.find(char::is_whitespace) {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, ""),
        }
    };
    let zone = TimeZone::get(name).ok()?;
    Some((Some(zone), remainder.trim_start()))
}

/// An explicit calendar date. `year` is `None` when the string omits it (`Jan 2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CivilDate {
    year: Option<i16>,
    month: i8,
    day: i8,
}

/// A wall-clock time with nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Clock {
    hour: i8,
    minute: i8,
    second: i8,
    nanos: i32,
}

impl Clock {
    const MIDNIGHT: Clock = Clock {
        hour: 0,
        minute: 0,
        second: 0,
        nanos: 0,
    };
}

/// A relative unit. Weeks and fortnights are days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

/// How a weekday name is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    /// `monday`, `this monday`: today or the next such day.
    Plain,
    /// `next monday`: strictly after today.
    Next,
    /// `last monday`: strictly before today.
    Last,
}

/// Everything the string said, before it is resolved against the current time.
#[derive(Debug, Default)]
struct Spec {
    date: Option<CivilDate>,
    time: Option<Clock>,
    zone: Option<TimeZone>,
    weekday: Option<(u8, Anchor)>,
    /// A date keyword or weekday name was given, so an unspecified time means midnight.
    reset_time: bool,
    /// Relative amounts in the order written. `ago` negates the last one.
    rel: Vec<(Unit, i64)>,
}

/// Splits `body` into whitespace tokens. A comma ends a token, and an ISO `T` between a
/// date and a time becomes a separator.
fn tokenize(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in body.split_whitespace() {
        let raw = raw.trim_end_matches(',');
        if raw.is_empty() {
            continue;
        }
        match iso_t_position(raw) {
            Some(index) => {
                out.push(raw[..index].to_string());
                out.push(raw[index + 1..].to_string());
            }
            None => out.push(raw.to_string()),
        }
    }
    out
}

/// Position of the `T` in `YYYY-MM-DDTHH...`.
fn iso_t_position(raw: &str) -> Option<usize> {
    let index = raw.find('T')?;
    let date = &raw[..index];
    let looks_like_date =
        date.contains('-') && date.bytes().all(|b| b.is_ascii_digit() || b == b'-') && index > 0;
    looks_like_date.then_some(index)
}

/// Scans the token list into a [`Spec`].
fn scan(body: &str) -> Option<Spec> {
    let mut scanner = Scanner {
        tokens: tokenize(body),
        pos: 0,
        spec: Spec::default(),
    };
    while scanner.pos < scanner.tokens.len() {
        let token = scanner.tokens[scanner.pos].clone();
        scanner.pos += 1;
        scanner.token(&token)?;
    }
    Some(scanner.spec)
}

struct Scanner {
    tokens: Vec<String>,
    pos: usize,
    spec: Spec,
}

impl Scanner {
    fn peek(&self) -> Option<String> {
        self.tokens.get(self.pos).map(|t| t.to_ascii_lowercase())
    }

    fn take_if<T>(&mut self, probe: impl Fn(&str) -> Option<T>) -> Option<T> {
        let value = probe(&self.tokens.get(self.pos)?.to_ascii_lowercase())?;
        self.pos += 1;
        Some(value)
    }

    fn token(&mut self, token: &str) -> Option<()> {
        let lower = token.to_ascii_lowercase();
        if lower.bytes().all(|b| b.is_ascii_digit()) {
            return self.number(token);
        }
        if let Some(first) = lower.chars().next() {
            if (first == '+' || first == '-') && token.len() > 1 {
                return self.signed(token);
            }
        }
        if lower.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return self.numeric_word(token);
        }
        self.word(&lower)
    }

    /// A token that starts with a digit but is not all digits: dates, times, ordinals and
    /// times with attached meridiem or zone.
    fn numeric_word(&mut self, token: &str) -> Option<()> {
        if token.contains(':') {
            return self.clock_token(token);
        }
        if token.contains('-') && token.matches('-').count() == 2 {
            return self.iso_date(token);
        }
        if token.contains('/') {
            return self.slash_date(token);
        }
        let digits_end = token.find(|c: char| !c.is_ascii_digit())?;
        let (digits, suffix) = token.split_at(digits_end);
        if matches!(
            suffix.to_ascii_lowercase().as_str(),
            "st" | "nd" | "rd" | "th"
        ) {
            return self.number(digits);
        }
        if let Some(pm) = meridiem(&suffix.to_ascii_lowercase()) {
            return self.hour_with_meridiem(digits, pm);
        }
        None
    }

    fn number(&mut self, digits: &str) -> Option<()> {
        if digits.len() > 14 {
            return None;
        }
        let value: i64 = digits.parse().ok()?;
        if let Some(pm) = self.take_if(meridiem) {
            return self.hour_with_meridiem(digits, pm);
        }
        if let Some((unit, multiplier)) = self.take_if(unit_word) {
            self.spec.rel.push((unit, value * multiplier));
            return Some(());
        }
        if let Some(month) = self.take_if(month_number) {
            if digits.len() <= 2 {
                return self.month_tail(month, Some(value));
            }
            return None;
        }
        if digits.len() == 4 {
            if let Some(date) = self.spec.date.as_mut().filter(|d| d.year.is_none()) {
                // `Tue Jan 2 10:00 UTC 2024`: a year that follows the time and zone.
                date.year = Some(i16::try_from(value).ok()?);
                return Some(());
            }
            if self.peek().is_some_and(|t| month_number(&t).is_some()) {
                return None;
            }
        }
        match digits.len() {
            8 => self.set_date(
                Some(parse_part(&digits[..4])?),
                parse_part(&digits[4..6])?,
                parse_part(&digits[6..])?,
            ),
            14 => {
                self.set_date(
                    Some(parse_part(&digits[..4])?),
                    parse_part(&digits[4..6])?,
                    parse_part(&digits[6..8])?,
                )?;
                self.set_clock(parse_clock_parts(
                    &digits[8..10],
                    &digits[10..12],
                    Some(&digits[12..14]),
                )?)
            }
            4 => self.set_clock(parse_clock_parts(&digits[..2], &digits[2..], None)?),
            _ => None,
        }
    }

    fn hour_with_meridiem(&mut self, digits: &str, pm: bool) -> Option<()> {
        if digits.len() > 2 {
            return None;
        }
        let hour: i8 = digits.parse().ok()?;
        if !(1..=12).contains(&hour) {
            return None;
        }
        let hour24 = hour % 12 + if pm { 12 } else { 0 };
        self.set_clock(Clock {
            hour: hour24,
            minute: 0,
            second: 0,
            nanos: 0,
        })
    }

    /// `+N unit`, `-N unit`, or a numeric zone such as `+08`, `+0800`, `-05:30`.
    fn signed(&mut self, token: &str) -> Option<()> {
        let negative = token.starts_with('-');
        let body = &token[1..];
        if body.contains(':') {
            return self.set_zone_text(token);
        }
        if !body.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let value: i64 = body.parse().ok()?;
        let signed_value = if negative { -value } else { value };
        if let Some((unit, multiplier)) = self.take_if(unit_word) {
            self.spec.rel.push((unit, signed_value * multiplier));
            return Some(());
        }
        // A bare numeric zone is only meaningful after a clock time.
        if self.spec.time.is_some() && matches!(body.len(), 1 | 2 | 4) {
            return self.set_zone_text(token);
        }
        None
    }

    fn clock_token(&mut self, token: &str) -> Option<()> {
        let split = token
            .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '.'))
            .unwrap_or(token.len());
        let (core, suffix) = token.split_at(split);
        let mut clock = parse_clock_core(core)?;
        let mut zone_suffix = String::new();
        if !suffix.is_empty() {
            match meridiem(&suffix.to_ascii_lowercase()) {
                Some(pm) => clock = apply_meridiem(clock, pm)?,
                None => zone_suffix = suffix.to_string(),
            }
        } else if let Some(pm) = self.take_if(meridiem) {
            clock = apply_meridiem(clock, pm)?;
        }
        self.set_clock(clock)?;
        if zone_suffix.is_empty() {
            return Some(());
        }
        self.set_zone_text(&zone_suffix)
    }

    /// `YYYY-MM-DD` (month and day may have one digit).
    fn iso_date(&mut self, token: &str) -> Option<()> {
        let parts: Vec<&str> = token.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 {
            return None;
        }
        self.set_date(
            Some(parse_part(parts[0])?),
            parse_part(parts[1])?,
            parse_part(parts[2])?,
        )
    }

    /// `YYYY/MM/DD`, `MM/DD/YYYY`, `MM/DD/YY` or `MM/DD`.
    fn slash_date(&mut self, token: &str) -> Option<()> {
        let parts: Vec<&str> = token.split('/').collect();
        match parts.as_slice() {
            [y, m, d] if y.len() == 4 => {
                self.set_date(Some(parse_part(y)?), parse_part(m)?, parse_part(d)?)
            }
            [m, d, y] if y.len() == 4 => {
                self.set_date(Some(parse_part(y)?), parse_part(m)?, parse_part(d)?)
            }
            [m, d, y] if y.len() == 2 => {
                let short: i16 = parse_part(y)?;
                let year = if short < 69 {
                    2000 + short
                } else {
                    1900 + short
                };
                self.set_date(Some(year), parse_part(m)?, parse_part(d)?)
            }
            [m, d] => self.set_date(None, parse_part(m)?, parse_part(d)?),
            _ => None,
        }
    }

    /// A month name followed by an optional day and year. `day_before` is a day already read
    /// before the name (`2 Jan 2024`).
    fn month_tail(&mut self, month: i8, day_before: Option<i64>) -> Option<()> {
        let mut day = day_before;
        if day.is_none() {
            day = self.take_if(short_day);
        }
        let day = i8::try_from(day?).ok()?;
        let mut year = None;
        if let Some(value) = self.take_if(four_digits) {
            year = Some(i16::try_from(value).ok()?);
        }
        self.set_date(year, month, day)
    }

    fn word(&mut self, lower: &str) -> Option<()> {
        match lower {
            "now" => Some(()),
            "today" => {
                self.spec.reset_time = true;
                Some(())
            }
            "yesterday" => self.add_day_keyword(-1),
            "tomorrow" => self.add_day_keyword(1),
            "noon" => self.set_clock(Clock {
                hour: 12,
                ..Clock::MIDNIGHT
            }),
            "midnight" => self.set_clock(Clock::MIDNIGHT),
            "this" | "next" | "last" => self.anchored(lower),
            "ago" => {
                let last = self.spec.rel.last_mut()?;
                last.1 = -last.1;
                Some(())
            }
            _ => self.word_class(lower),
        }
    }

    /// Weekday, month, zone or meridiem words that are not keywords.
    fn word_class(&mut self, lower: &str) -> Option<()> {
        if let Some(index) = weekday_index(lower) {
            return self.set_weekday(index, Anchor::Plain);
        }
        if let Some(month) = month_number(lower) {
            return self.month_tail(month, None);
        }
        if let Some(offset) = zone_word(lower) {
            return self.set_zone(TimeZone::fixed(Offset::from_seconds(offset).ok()?));
        }
        None
    }

    fn anchored(&mut self, keyword: &str) -> Option<()> {
        if let Some((unit, multiplier)) = self.take_if(unit_word) {
            let count = match keyword {
                "next" => 1,
                "last" => -1,
                _ => 0,
            };
            self.spec.rel.push((unit, count * multiplier));
            return Some(());
        }
        let index = self.take_if(weekday_index)?;
        let anchor = match keyword {
            "next" => Anchor::Next,
            "last" => Anchor::Last,
            _ => Anchor::Plain,
        };
        self.set_weekday(index, anchor)
    }

    fn add_day_keyword(&mut self, days: i64) -> Option<()> {
        self.spec.rel.push((Unit::Day, days));
        self.spec.reset_time = true;
        Some(())
    }

    fn set_weekday(&mut self, index: u8, anchor: Anchor) -> Option<()> {
        if self.spec.weekday.is_some() {
            return None;
        }
        self.spec.weekday = Some((index, anchor));
        self.spec.reset_time = true;
        Some(())
    }

    fn set_date(&mut self, year: Option<i16>, month: i8, day: i8) -> Option<()> {
        if self.spec.date.is_some() {
            return None;
        }
        self.spec.date = Some(CivilDate { year, month, day });
        Some(())
    }

    fn set_clock(&mut self, clock: Clock) -> Option<()> {
        if self.spec.time.is_some() {
            return None;
        }
        self.spec.time = Some(clock);
        Some(())
    }

    fn set_zone(&mut self, zone: TimeZone) -> Option<()> {
        if self.spec.zone.is_some() {
            return None;
        }
        self.spec.zone = Some(zone);
        Some(())
    }

    fn set_zone_text(&mut self, text: &str) -> Option<()> {
        let offset = parse_offset_text(text)?;
        self.set_zone(TimeZone::fixed(Offset::from_seconds(offset).ok()?))
    }
}

/// Combines the parsed pieces with `now`, interpreted in `zone`.
fn resolve(spec: &Spec, now: Timestamp, zone: &TimeZone) -> Option<Timestamp> {
    let base = now.to_zoned(zone.clone());
    let (mut year, mut month, mut day) = match spec.date {
        Some(explicit) => {
            let year = explicit.year.unwrap_or(base.year());
            Date::new(year, explicit.month, explicit.day).ok()?;
            (year, explicit.month, explicit.day)
        }
        None => (base.year(), base.month(), base.day()),
    };
    let (years, months, days, hours, minutes, seconds) = totals(&spec.rel);
    let total_months = i64::from(year) * 12 + i64::from(month - 1) + years * 12 + months;
    year = i16::try_from(total_months.div_euclid(12)).ok()?;
    month = (total_months.rem_euclid(12) + 1) as i8;
    let first = Date::new(year, month, 1).ok()?;
    let mut date = first
        .checked_add(jiff::Span::new().days(i64::from(day) - 1 + days))
        .ok()?;
    if spec.date.is_none() {
        if let Some((index, anchor)) = spec.weekday {
            date = apply_weekday(date, index, anchor)?;
        }
    }
    year = date.year();
    month = date.month();
    day = date.day();
    let clock = match spec.time {
        Some(clock) => clock,
        None if spec.date.is_some() || spec.reset_time => Clock::MIDNIGHT,
        None => Clock {
            hour: base.hour(),
            minute: base.minute(),
            second: base.second(),
            nanos: base.subsec_nanosecond(),
        },
    };
    let civil = DateTime::new(
        year,
        month,
        day,
        clock.hour,
        clock.minute,
        clock.second,
        clock.nanos,
    )
    .ok()?;
    let start = zone
        .to_ambiguous_zoned(civil)
        .compatible()
        .ok()?
        .timestamp();
    let delta = i128::from(hours * 3600 + minutes * 60 + seconds) * 1_000_000_000;
    Timestamp::from_nanosecond(start.as_nanosecond().checked_add(delta)?).ok()
}

/// Sums relative amounts into (years, months, days, hours, minutes, seconds).
fn totals(rel: &[(Unit, i64)]) -> (i64, i64, i64, i64, i64, i64) {
    let mut sums = [0i64; 6];
    for (unit, amount) in rel {
        let slot = match unit {
            Unit::Year => 0,
            Unit::Month => 1,
            Unit::Day => 2,
            Unit::Hour => 3,
            Unit::Minute => 4,
            Unit::Second => 5,
        };
        sums[slot] += amount;
    }
    (sums[0], sums[1], sums[2], sums[3], sums[4], sums[5])
}

/// Moves `date` to the requested weekday according to `anchor`.
fn apply_weekday(date: Date, index: u8, anchor: Anchor) -> Option<Date> {
    let current = i64::from(date.weekday().to_sunday_zero_offset());
    let target = i64::from(index);
    let forward = (target - current).rem_euclid(7);
    let shift = match anchor {
        Anchor::Plain => forward,
        Anchor::Next => {
            if forward == 0 {
                7
            } else {
                forward
            }
        }
        Anchor::Last => {
            let back = (current - target).rem_euclid(7);
            -(if back == 0 { 7 } else { back })
        }
    };
    date.checked_add(jiff::Span::new().days(shift)).ok()
}

#[cfg(test)]
mod tests;
