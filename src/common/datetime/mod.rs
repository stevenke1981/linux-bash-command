//! Date parsing and C-locale formatting shared by filesystem utilities.
mod format;
mod parse;

use jiff::{tz::TimeZone, Timestamp, Zoned};
use std::time::SystemTime;

pub use format::format;
pub use parse::{parse, parse_at};

pub fn timezone(utc: bool) -> Result<TimeZone, String> {
    if utc {
        return Ok(TimeZone::UTC);
    }
    match std::env::var("TZ") {
        Ok(tz) if tz.is_empty() || tz == "UTC" || tz == "GMT" || tz == "UTC0" => Ok(TimeZone::UTC),
        Ok(tz) => TimeZone::get(tz.trim_start_matches(':')).map_err(|e| e.to_string()),
        Err(_) => Ok(TimeZone::system()),
    }
}

pub fn from_system_time(time: SystemTime) -> Result<Zoned, String> {
    Ok(Timestamp::try_from(time)
        .map_err(|e| e.to_string())?
        .to_zoned(timezone(false)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_utc_and_system_time_epoch() {
        assert_eq!(timezone(true).unwrap(), TimeZone::UTC);
        // Epoch conversion must preserve the instant, independently of local TZ.
        if let Ok(date) = from_system_time(SystemTime::UNIX_EPOCH) {
            assert_eq!(date.timestamp(), Timestamp::UNIX_EPOCH);
        }
    }
    #[test]
    fn system_time_beyond_supported_calendar_is_reported() {
        let future =
            SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(253_402_300_800));
        if let Some(time) = future {
            assert!(from_system_time(time).is_err());
        }
    }
}
