//! Time parsing for `touch`. Local to this command until `common::datetime` exists.
//!
//! - `-t [[CC]YY]MMDDhhmm[.ss]` (POSIX), read in the local time zone.
//! - `-d` accepts `@EPOCH` (with an optional sign and fraction), `now`, and
//!   `YYYY-MM-DD[( |T)HH:MM[:SS[.frac]]]`, read in the local time zone.

use filetime::FileTime;
use jiff::{civil, tz::TimeZone, Zoned};

/// Parses a `-t` stamp such as `200102030405.06` or `0102030405`.
pub fn parse_stamp(text: &str) -> Option<FileTime> {
    stamp(text).ok()
}

fn stamp(text: &str) -> Result<FileTime, ()> {
    let (main, seconds) = match text.split_once('.') {
        Some((main, seconds)) => (main, Some(seconds)),
        None => (text, None),
    };
    if !main.bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    let (year, rest) = match main.len() {
        8 => (current_year()?, main),
        10 => (yy_to_year(digits(&main[..2])?), &main[2..]),
        12 => (digits(&main[..4])? as i16, &main[4..]),
        _ => return Err(()),
    };
    let second = match seconds {
        None => 0,
        Some(part) if part.len() == 2 => digits(part)?,
        Some(_) => return Err(()),
    };
    civil_time(
        year,
        digits(&rest[..2])? as i8,
        digits(&rest[2..4])? as i8,
        digits(&rest[4..6])? as i8,
        digits(&rest[6..8])? as i8,
        second as i8,
        0,
    )
}

/// Parses a `-d` date string.
pub fn parse_date(text: &str) -> Option<FileTime> {
    date(text).ok()
}

fn date(text: &str) -> Result<FileTime, ()> {
    if let Some(epoch) = text.strip_prefix('@') {
        return parse_epoch(epoch);
    }
    if text == "now" {
        return Ok(FileTime::now());
    }
    let split = text.find([' ', 'T']);
    let (date, time) = match split {
        Some(index) => (&text[..index], Some(&text[index + 1..])),
        None => (text, None),
    };
    let (year, month, day) = parse_ymd(date)?;
    let (hour, minute, second, nanos) = match time {
        Some(time) => parse_hms(time)?,
        None => (0, 0, 0, 0),
    };
    civil_time(year, month, day, hour, minute, second, nanos)
}

fn digits(text: &str) -> Result<u32, ()> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    text.parse().map_err(|_| ())
}

/// POSIX two-digit years: 69 to 99 are 1969 to 1999, 00 to 68 are 2000 to 2068.
fn yy_to_year(yy: u32) -> i16 {
    if yy < 69 {
        2000 + yy as i16
    } else {
        1900 + yy as i16
    }
}

fn current_year() -> Result<i16, ()> {
    Ok(Zoned::now().year())
}

fn parse_ymd(text: &str) -> Result<(i16, i8, i8), ()> {
    let mut parts = text.split('-');
    let (Some(y), Some(m), Some(d), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(());
    };
    if y.len() != 4 {
        return Err(());
    }
    Ok((digits(y)? as i16, digits(m)? as i8, digits(d)? as i8))
}

/// `HH:MM`, `HH:MM:SS` or `HH:MM:SS.frac`.
fn parse_hms(text: &str) -> Result<(i8, i8, i8, i32), ()> {
    let (clock, fraction) = match text.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (text, None),
    };
    let mut parts = clock.split(':');
    let (Some(h), Some(mi)) = (parts.next(), parts.next()) else {
        return Err(());
    };
    let second = match parts.next() {
        Some(s) => digits(s)? as i8,
        None => 0,
    };
    if parts.next().is_some() || h.len() != 2 || mi.len() != 2 {
        return Err(());
    }
    let nanos = match fraction {
        None => 0,
        Some(frac) => fraction_nanos(frac)?,
    };
    Ok((digits(h)? as i8, digits(mi)? as i8, second, nanos))
}

/// Up to nine decimal digits, scaled to nanoseconds.
fn fraction_nanos(text: &str) -> Result<i32, ()> {
    if text.is_empty() || text.len() > 9 {
        return Err(());
    }
    let value = digits(text)? as i64;
    Ok((value * 10i64.pow(9 - text.len() as u32)) as i32)
}

/// `@SECONDS` with an optional sign and fractional part.
fn parse_epoch(text: &str) -> Result<FileTime, ()> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (body, None),
    };
    let seconds = whole.parse::<i64>().map_err(|_| ())?;
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    let nanos = match fraction {
        None => 0,
        Some(frac) => fraction_nanos(frac)? as u32,
    };
    if !negative {
        return Ok(FileTime::from_unix_time(seconds, nanos));
    }
    if nanos == 0 {
        Ok(FileTime::from_unix_time(-seconds, 0))
    } else {
        Ok(FileTime::from_unix_time(
            -seconds - 1,
            1_000_000_000 - nanos,
        ))
    }
}

/// Converts a civil date and time in the local zone to a file time.
fn civil_time(
    year: i16,
    month: i8,
    day: i8,
    hour: i8,
    minute: i8,
    second: i8,
    nanos: i32,
) -> Result<FileTime, ()> {
    let datetime =
        civil::DateTime::new(year, month, day, hour, minute, second, nanos).map_err(|_| ())?;
    let zoned = datetime.to_zoned(TimeZone::system()).map_err(|_| ())?;
    let timestamp = zoned.timestamp();
    Ok(FileTime::from_unix_time(
        timestamp.as_second(),
        timestamp.subsec_nanosecond() as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_dates_are_exact() {
        assert_eq!(
            parse_date("@1000000"),
            Some(FileTime::from_unix_time(1_000_000, 0))
        );
        assert_eq!(
            parse_date("@1.5"),
            Some(FileTime::from_unix_time(1, 500_000_000))
        );
        assert_eq!(
            parse_date("@-1.5"),
            Some(FileTime::from_unix_time(-2, 500_000_000))
        );
    }

    #[test]
    fn invalid_epochs_are_rejected() {
        assert!(parse_date("@").is_none());
        assert!(parse_date("@12x").is_none());
        assert!(parse_date("@1.1234567890").is_none());
    }

    #[test]
    fn stamp_and_date_agree_on_the_same_wall_time() {
        let stamp = parse_stamp("200102030405.06").unwrap();
        let date = parse_date("2001-02-03 04:05:06").unwrap();
        assert_eq!(stamp.unix_seconds(), date.unix_seconds());
        assert_eq!(parse_date("2001-02-03T04:05:06"), Some(date));
    }

    #[test]
    fn two_digit_years_follow_posix() {
        assert_eq!(yy_to_year(68), 2068);
        assert_eq!(yy_to_year(69), 1969);
        assert_eq!(yy_to_year(0), 2000);
    }

    #[test]
    fn stamp_rejects_malformed_input() {
        for text in [
            "12",
            "1234567",
            "20011399999",
            "200113031200",
            "0101000069",
            "abcdefgh",
            "20010203040.5",
        ] {
            assert!(parse_stamp(text).is_none(), "{text}");
        }
    }

    #[test]
    fn date_rejects_malformed_input() {
        for text in [
            "bogus",
            "2001-13-03",
            "2001-02-03 25:00",
            "01-02-03",
            "2001-02-03 04:05:06:07",
        ] {
            assert!(parse_date(text).is_none(), "{text}");
        }
    }

    #[test]
    fn fractions_scale_to_nanoseconds() {
        assert_eq!(fraction_nanos("5"), Ok(500_000_000));
        assert_eq!(fraction_nanos("000000001"), Ok(1));
    }
}
