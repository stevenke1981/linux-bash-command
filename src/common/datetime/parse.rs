use super::timezone;
use jiff::{
    civil::{Date, DateTime, Time},
    Span, Timestamp, Zoned,
};

fn invalid(input: &str) -> String {
    format!("invalid date '{input}'")
}

pub fn parse(input: &str) -> Result<Zoned, String> {
    let now = Timestamp::now().to_zoned(timezone(false)?);
    parse_at(input, &now)
}

pub fn parse_at(input: &str, now: &Zoned) -> Result<Zoned, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err(invalid(input));
    }
    if let Some(rest) = input.strip_prefix("TZ=\"") {
        if let Some((tz, rest)) = rest.split_once('"') {
            let tz = jiff::tz::TimeZone::get(tz).map_err(|_| invalid(input))?;
            return parse_at(rest.trim_start(), &now.with_time_zone(tz))
                .map(|date| date.with_time_zone(now.time_zone().clone()));
        }
        return Err(invalid(input));
    }
    if let Some(epoch) = input.strip_prefix('@') {
        return parse_epoch(epoch)
            .map(|t| t.to_zoned(now.time_zone().clone()))
            .map_err(|_| invalid(input));
    }
    if let Some(result) = absolute(input, now) {
        return Ok(result);
    }
    let words: Vec<_> = input.split_whitespace().collect();
    for split in (0..=words.len()).rev() {
        let base = if split == 0 {
            Some(now.clone())
        } else {
            absolute(&words[..split].join(" "), now)
        };
        if let Some(base) = base {
            let explicit_date = split != 0
                && !(split == 1
                    && ["now", "today", "yesterday", "tomorrow"]
                        .iter()
                        .any(|word| words[0].eq_ignore_ascii_case(word)));
            if let Some(result) = relative(base, &words[split..], explicit_date) {
                return Ok(result);
            }
        }
    }
    Err(invalid(input))
}

fn parse_epoch(text: &str) -> Result<Timestamp, String> {
    let negative = text.starts_with('-');
    let unsigned = text
        .strip_prefix('-')
        .or_else(|| text.strip_prefix('+'))
        .unwrap_or(text);
    let (sec, frac) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if sec.is_empty()
        || !sec.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
        || (unsigned.contains('.') && frac.is_empty())
    {
        return Err("invalid epoch".into());
    }
    let whole = sec.parse::<i128>().map_err(|e| e.to_string())?;
    let leading = &frac[..frac.len().min(9)];
    let nanos = if leading.is_empty() {
        0
    } else {
        let truncated = leading.parse::<i128>().map_err(|e| e.to_string())?
            * 10_i128.pow(9 - leading.len() as u32);
        truncated
            + i128::from(
                negative
                    && frac
                        .as_bytes()
                        .get(9..)
                        .is_some_and(|rest| rest.iter().any(|b| *b != b'0')),
            )
    };
    let value = whole
        .checked_mul(1_000_000_000)
        .and_then(|x| x.checked_add(nanos))
        .ok_or("epoch overflow")?;
    Timestamp::from_nanosecond(if negative { -value } else { value }).map_err(|e| e.to_string())
}

fn absolute(input: &str, now: &Zoned) -> Option<Zoned> {
    if let Some((weekday, rest)) = input.split_once(", ") {
        if ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
            .iter()
            .any(|day| weekday.eq_ignore_ascii_case(day))
        {
            return absolute(rest, now);
        }
    }
    let lower = input.to_ascii_lowercase();
    let zone = now.time_zone().clone();
    if let Some(utc) = input.strip_suffix('Z') {
        if utc.contains('T') || utc.contains(' ') {
            let normalized = format!("{utc}+00:00");
            return absolute(&normalized, now);
        }
    }
    match lower.as_str() {
        "utc" | "gmt" | "z" => {
            let utc = now.with_time_zone(jiff::tz::TimeZone::UTC);
            return utc
                .date()
                .at(0, 0, 0, 0)
                .to_zoned(jiff::tz::TimeZone::UTC)
                .ok()
                .map(|z| z.with_time_zone(zone));
        }
        "now" => return Some(now.clone()),
        "today" => return Some(now.clone()),
        "yesterday" => return now.checked_sub(Span::new().days(1)).ok(),
        "tomorrow" => return now.checked_add(Span::new().days(1)).ok(),
        "noon" => return now.date().at(12, 0, 0, 0).to_zoned(zone).ok(),
        "midnight" => return now.date().at(0, 0, 0, 0).to_zoned(zone).ok(),
        _ => {}
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%.f %:z",
        "%Y-%m-%d %H:%M:%S%.f %z",
        "%Y-%m-%dT%H:%M%:z",
        "%Y-%m-%dT%H:%M%z",
        "%Y-%m-%d %H:%M%:z",
        "%Y-%m-%d %H:%M%z",
        "%Y-%m-%d %H:%M %:z",
        "%Y-%m-%d %H:%M %z",
        "%a, %d %b %Y %H:%M:%S %z",
        "%a, %d %b %Y %H:%M %z",
        "%d %b %Y %H:%M:%S %z",
        "%d %b %Y %H:%M %z",
        "%a %b %e %H:%M:%S %z %Y",
    ] {
        if let Ok(timestamp) = Timestamp::strptime(format, input) {
            return Some(timestamp.to_zoned(zone));
        }
    }
    let without_utc = input
        .strip_suffix(" UTC")
        .or_else(|| input.strip_suffix(" GMT"))
        .or_else(|| input.strip_suffix(" Z"));
    if let Some(text) = without_utc {
        let utc_now = now.with_time_zone(jiff::tz::TimeZone::UTC);
        return absolute(text, &utc_now).map(|z| z.with_time_zone(zone));
    }
    for suffix in [" UTC ", " GMT ", " Z "] {
        if let Some((left, right)) = input.split_once(suffix) {
            let unzoned = format!("{left} {right}");
            if let Ok(date) = DateTime::strptime("%a %b %e %H:%M:%S %Y", unzoned) {
                return date
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .ok()
                    .map(|z| z.with_time_zone(zone));
            }
        }
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
        "%m/%d/%Y %H:%M:%S",
        "%d %b %Y %H:%M:%S",
        "%b %d %Y %H:%M:%S",
    ] {
        if let Ok(date) = DateTime::strptime(format, input) {
            return date.to_zoned(zone).ok();
        }
    }
    for format in ["%Y-%m-%d", "%m/%d/%Y", "%d %b %Y", "%b %d %Y"] {
        if let Ok(date) = Date::strptime(format, input) {
            return date.at(0, 0, 0, 0).to_zoned(zone).ok();
        }
    }
    if input
        .split_whitespace()
        .next()
        .is_some_and(|first| first.contains(':') && !first.contains('-'))
    {
        let dated = format!("{} {input}", now.date());
        if let Some(date) = absolute(&dated, now) {
            return Some(date);
        }
    }
    for format in ["%H:%M:%S%.f", "%H:%M", "%I:%M:%S %p", "%I:%M %p"] {
        if let Ok(time) = Time::strptime(format, input) {
            return time.to_datetime(now.date()).to_zoned(zone).ok();
        }
    }
    None
}

fn shift_months(date: &Zoned, months: i64) -> Option<Zoned> {
    let index = i64::from(date.year())
        .checked_mul(12)?
        .checked_add(i64::from(date.month()) - 1)?
        .checked_add(months)?;
    let year = i16::try_from(index.div_euclid(12)).ok()?;
    let month = i8::try_from(index.rem_euclid(12) + 1).ok()?;
    let first = Date::new(year, month, 1).ok()?;
    let shifted = first
        .checked_add(Span::new().days(i64::from(date.day()) - 1))
        .ok()?;
    shifted
        .to_datetime(date.time())
        .to_zoned(date.time_zone().clone())
        .ok()
}

fn relative(mut date: Zoned, words: &[&str], had_absolute: bool) -> Option<Zoned> {
    if words.is_empty() {
        return None;
    }
    let mut pos = 0;
    let mut tokens = Vec::new();
    let mut total_months = 0i64;
    while pos < words.len() {
        let token = words[pos].to_ascii_lowercase();
        let (mut count, unit, used) = if matches!(token.as_str(), "next" | "last") {
            (
                if token == "next" { 1 } else { -1 },
                words.get(pos + 1)?.to_ascii_lowercase(),
                2,
            )
        } else if let Ok(n) = token.parse::<i64>() {
            (n, words.get(pos + 1)?.to_ascii_lowercase(), 2)
        } else {
            (1, token, 1)
        };
        pos += used;
        if words
            .get(pos)
            .is_some_and(|w| w.eq_ignore_ascii_case("ago"))
        {
            count = count.checked_neg()?;
            pos += 1;
        }
        let singular = unit.strip_suffix('s').unwrap_or(&unit);
        if singular.is_empty() {
            return None;
        }
        if singular == "year" || singular == "month" {
            let months = if singular == "year" {
                count.checked_mul(12)?
            } else {
                count
            };
            total_months = total_months.checked_add(months)?;
        } else {
            tokens.push((count, singular.to_owned(), used));
        }
    }
    if total_months != 0 {
        date = shift_months(&date, total_months)?;
    }
    for (count, unit, used) in tokens {
        let singular = unit.as_str();
        let span = match singular {
            "fortnight" => Span::new().try_weeks(count.checked_mul(2)?).ok()?,
            "week" => Span::new().try_weeks(count).ok()?,
            "day" => Span::new().try_days(count).ok()?,
            "hour" => Span::new().try_hours(count).ok()?,
            "minute" | "min" => Span::new().try_minutes(count).ok()?,
            "second" | "sec" => Span::new().try_seconds(count).ok()?,
            _ => {
                if singular.len() < 3 {
                    return None;
                }
                let day = [
                    "monday",
                    "tuesday",
                    "wednesday",
                    "thursday",
                    "friday",
                    "saturday",
                    "sunday",
                ]
                .iter()
                .position(|d| d.starts_with(singular))? as i64;
                if had_absolute {
                    continue;
                }
                date = date
                    .date()
                    .at(0, 0, 0, 0)
                    .to_zoned(date.time_zone().clone())
                    .ok()?;
                let current = i64::from(date.weekday().to_monday_zero_offset());
                let mut delta = (day - current).rem_euclid(7);
                if count < 0 {
                    delta = if delta == 0 { -7 } else { delta - 7 };
                } else if used == 2 && delta == 0 {
                    delta = 7;
                }
                Span::new().days(delta)
            }
        };
        date = date.checked_add(span).ok()?;
    }
    Some(date)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> Zoned {
        "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap()
    }
    #[test]
    fn epoch_fraction() {
        assert_eq!(
            parse_at("@-0.5", &now())
                .unwrap()
                .timestamp()
                .as_nanosecond(),
            -500_000_000
        );
    }
    #[test]
    fn iso_dates() {
        assert_eq!(
            parse_at("2026-10-30 05:10:00", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-30 05:10:00"
        );
    }
    #[test]
    fn relatives() {
        assert_eq!(
            parse_at("2026-01-01 +3 days 2 hours ago", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-01-03 22:00:00"
        );
    }
    #[test]
    fn words() {
        assert_eq!(
            parse_at("tomorrow", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-01-03 03:04:05"
        );
    }
    #[test]
    fn rejects_invalid() {
        assert!(parse_at("not a date", &now()).is_err());
    }
    #[test]
    fn parses_rfc2822() {
        assert_eq!(
            parse_at("Fri, 30 Oct 2026 05:10:00 +0000", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-30 05:10:00"
        );
    }
    #[test]
    fn parses_default_utc() {
        assert_eq!(
            parse_at("Fri Oct 30 05:10:00 UTC 2026", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-30 05:10:00"
        );
    }
    #[test]
    fn parses_z_and_offset() {
        assert_eq!(
            parse_at("2026-10-30T05:10:00Z", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-30 05:10:00"
        );
        assert_eq!(
            parse_at("2026-10-30T05:10:00+08:00", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-29 21:10:00"
        );
    }
    #[test]
    fn embedded_timezone_converts_to_output_zone() {
        let date = parse_at("TZ=\"Asia/Taipei\" 2026-01-01 00:00", &now()).unwrap();
        assert_eq!(
            date.strftime("%F %T %z").to_string(),
            "2025-12-31 16:00:00 +0000"
        );
    }
    #[test]
    fn rejects_multiple_epoch_signs() {
        assert!(parse_at("@--1", &now()).is_err());
    }
    #[test]
    fn weekday_defaults_to_midnight() {
        assert_eq!(
            parse_at("next monday", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-01-05 00:00:00"
        );
    }
    #[test]
    fn today_preserves_current_time() {
        assert_eq!(
            parse_at("today", &now())
                .unwrap()
                .strftime("%T")
                .to_string(),
            "03:04:05"
        );
    }
    #[test]
    fn parses_numeric_offsets_with_spaces() {
        assert_eq!(
            parse_at("2026-10-30 05:10:00 +0800", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-29 21:10:00"
        );
    }
    #[test]
    fn parses_month_and_time_names() {
        for value in ["10/30/2026", "30 Oct 2026", "Oct 30 2026"] {
            assert_eq!(
                parse_at(value, &now()).unwrap().strftime("%F").to_string(),
                "2026-10-30"
            );
        }
        assert_eq!(
            parse_at("11:30 pm", &now())
                .unwrap()
                .strftime("%T")
                .to_string(),
            "23:30:00"
        );
    }
    #[test]
    fn rejects_overflowing_relative_amount() {
        assert!(parse_at("999999999999 years", &now()).is_err());
        assert!(parse_at("-9223372036854775808 seconds ago", &now()).is_err());
    }
    #[test]
    fn weekday_after_now() {
        assert_eq!(
            parse_at("now next monday", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-01-05 00:00:00"
        );
    }
    #[test]
    fn calendar_month_overflow_rolls_forward() {
        assert_eq!(
            parse_at("2025-01-31 +1 month", &now())
                .unwrap()
                .strftime("%F")
                .to_string(),
            "2025-03-03"
        );
        assert_eq!(
            parse_at("2024-02-29 +1 year", &now())
                .unwrap()
                .strftime("%F")
                .to_string(),
            "2025-03-01"
        );
    }
    #[test]
    fn epoch_extra_precision_rounds_toward_negative_infinity() {
        assert_eq!(
            parse_at("@1.123456789123", &now())
                .unwrap()
                .timestamp()
                .as_nanosecond(),
            1_123_456_789
        );
        assert_eq!(
            parse_at("@-0.123456789123", &now())
                .unwrap()
                .timestamp()
                .as_nanosecond(),
            -123_456_790
        );
        assert!(parse_at("@1.", &now()).is_err());
    }
    #[test]
    fn rfc_without_weekday() {
        assert_eq!(
            parse_at("30 Oct 2026 05:10:00 +0000", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-30 05:10:00"
        );
    }
    #[test]
    fn time_with_numeric_offset() {
        assert_eq!(
            parse_at("12:30 +0800", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-01-02 04:30:00"
        );
    }
    #[test]
    fn iso_offset_without_colon() {
        assert_eq!(
            parse_at("2026-10-30T05:10:00+0800", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-29 21:10:00"
        );
    }
    #[test]
    fn rfc_weekday_does_not_override_date() {
        assert_eq!(
            parse_at("Thu, 30 Oct 2026 05:10:00 +0000", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2026-10-30 05:10:00"
        );
    }
    #[test]
    fn relative_months_are_combined_before_rollover() {
        assert_eq!(
            parse_at("2025-01-31 +1 month -1 month", &now())
                .unwrap()
                .strftime("%F")
                .to_string(),
            "2025-01-31"
        );
        assert_eq!(
            parse_at("2024-02-29 +1 year -1 year", &now())
                .unwrap()
                .strftime("%F")
                .to_string(),
            "2024-02-29"
        );
    }
}
