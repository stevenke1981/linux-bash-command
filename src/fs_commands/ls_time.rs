//! Times and sizes of entries for `ls`: which timestamp is used (`--time`, `-c`, `-u`), how
//! it is printed (`--time-style`), and how many bytes an entry allocates (`-s`).

use super::ls_options::{SizeFormat, TimeField, TimeStyle};
use crate::common::datetime;
use jiff::{tz::TimeZone, Timestamp};
use std::fs::Metadata;
use std::time::{SystemTime, UNIX_EPOCH};

/// Half a Gregorian year in seconds: times within it count as "recent" for `--time-style=iso`.
const RECENT_SECONDS: i64 = 15_778_476;

/// Seconds and nanoseconds since the Unix epoch.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Stamp {
    pub secs: i64,
    pub nanos: u32,
}

impl Stamp {
    fn from_system(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(after) => Self {
                secs: i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
                nanos: after.subsec_nanos(),
            },
            Err(before) => {
                let back = before.duration();
                let secs = i64::try_from(back.as_secs()).unwrap_or(i64::MAX);
                if back.subsec_nanos() == 0 {
                    Self {
                        secs: -secs,
                        nanos: 0,
                    }
                } else {
                    Self {
                        secs: -secs - 1,
                        nanos: 1_000_000_000 - back.subsec_nanos(),
                    }
                }
            }
        }
    }

    fn timestamp(self) -> Timestamp {
        Timestamp::new(self.secs, self.nanos as i32).unwrap_or(Timestamp::UNIX_EPOCH)
    }
}

/// The timestamp `field` of an entry. Missing times count as the epoch.
pub fn stamp(meta: &Metadata, field: TimeField) -> Stamp {
    let system = match field {
        TimeField::Modified => meta.modified().ok(),
        TimeField::Accessed => meta.accessed().ok(),
        TimeField::Birth => meta.created().ok(),
        TimeField::Changed => return changed(meta),
    };
    system.map(Stamp::from_system).unwrap_or_default()
}

#[cfg(unix)]
fn changed(meta: &Metadata) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    Stamp {
        secs: meta.ctime(),
        nanos: u32::try_from(meta.ctime_nsec()).unwrap_or(0),
    }
}

/// Windows has no inode change time; the modification time stands in for it.
#[cfg(not(unix))]
fn changed(meta: &Metadata) -> Stamp {
    meta.modified().map(Stamp::from_system).unwrap_or_default()
}

/// Prints a timestamp with a `--time-style`. `now` decides whether `iso` shows the time.
pub fn format(stamp: Stamp, style: &TimeStyle, now: Timestamp, zone: &TimeZone) -> String {
    let when = stamp.timestamp();
    let text = match style {
        TimeStyle::LongIso => "%Y-%m-%d %H:%M".to_string(),
        TimeStyle::FullIso => "%Y-%m-%d %H:%M:%S.%N %z".to_string(),
        TimeStyle::Iso => {
            let recent = when.as_second() + RECENT_SECONDS >= now.as_second()
                && when.as_second() <= now.as_second();
            if recent {
                "%m-%d %H:%M".to_string()
            } else {
                // GNU pads the older form to the width of the recent one.
                "%Y-%m-%d ".to_string()
            }
        }
        TimeStyle::Custom(format) => format.clone(),
    };
    datetime::format_in(&text, when, zone)
}

/// Bytes an entry occupies on disk: 512-byte units on Unix, and the size rounded up to 4 KiB
/// on Windows, as `du` reports it.
#[cfg(unix)]
pub fn allocated_bytes(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks().saturating_mul(512)
}

/// See [`allocated_bytes`].
#[cfg(not(unix))]
pub fn allocated_bytes(meta: &Metadata) -> u64 {
    meta.len().div_ceil(4096).saturating_mul(4096)
}

/// A count of `bytes` in the units of `format`: rounded up for scaled sizes.
pub fn units(bytes: u64, format: &SizeFormat) -> u64 {
    match format {
        SizeFormat::Scaled { unit, .. } => bytes.div_ceil(*unit),
        SizeFormat::Bytes | SizeFormat::Human(_) => bytes,
    }
}

/// Prints `bytes` in `format`, for sizes and block counts.
pub fn format_size(bytes: u64, format: &SizeFormat) -> String {
    match format {
        SizeFormat::Bytes => bytes.to_string(),
        SizeFormat::Scaled { suffix, .. } => format!("{}{suffix}", units(bytes, format)),
        SizeFormat::Human(base) => crate::common::size::human(bytes, *base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> Stamp {
        Stamp { secs, nanos: 0 }
    }

    #[test]
    fn styles_print_gnu_layouts_in_utc() {
        let zone = TimeZone::UTC;
        let now = Timestamp::from_second(1_700_000_000 + 60).unwrap();
        let when = at(1_700_000_000);
        assert_eq!(
            format(when, &TimeStyle::LongIso, now, &zone),
            "2023-11-14 22:13"
        );
        assert_eq!(
            format(when, &TimeStyle::FullIso, now, &zone),
            "2023-11-14 22:13:20.000000000 +0000"
        );
        assert_eq!(format(when, &TimeStyle::Iso, now, &zone), "11-14 22:13");
        let old = at(1_600_000_000);
        assert_eq!(format(old, &TimeStyle::Iso, now, &zone), "2020-09-13 ");
        assert_eq!(
            format(when, &TimeStyle::Custom("%Y-%j".to_string()), now, &zone),
            "2023-318"
        );
    }

    #[test]
    fn sizes_scale_and_round_up() {
        let kib = SizeFormat::Scaled {
            unit: 1024,
            suffix: "",
        };
        assert_eq!(format_size(4096, &kib), "4");
        assert_eq!(format_size(1, &kib), "1");
        assert_eq!(format_size(4096, &SizeFormat::Human(1024)), "4.0K");
        assert_eq!(format_size(4096, &SizeFormat::Human(1000)), "4.1k");
        assert_eq!(
            format_size(
                4096,
                &SizeFormat::Scaled {
                    unit: 1000,
                    suffix: "kB"
                }
            ),
            "5kB"
        );
        assert_eq!(format_size(3000, &SizeFormat::Bytes), "3000");
    }

    #[test]
    fn stamps_before_the_epoch_keep_the_fraction_positive() {
        let before = UNIX_EPOCH - std::time::Duration::from_millis(1500);
        assert_eq!(
            Stamp::from_system(before),
            Stamp {
                secs: -2,
                nanos: 500_000_000
            }
        );
        assert_eq!(Stamp::from_system(UNIX_EPOCH), Stamp::default());
    }
}
