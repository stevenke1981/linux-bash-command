//! Number and time formatting for `du`: block-size scaling, human-readable sizes, the
//! `--time-style` presets, and parsing of `-B`/`-t` sizes.

use jiff::{tz::TimeZone, Timestamp};

use crate::common::datetime::format_in;
use crate::common::size::human;

/// How byte counts are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// `-b`: plain bytes.
    Bytes,
    /// Divide by this unit, rounding up (the default 1024-byte blocks, `-k`, `-m`, `-B`).
    Unit(u64),
    /// `-h` (base 1024) or `--si` (base 1000).
    Human(u64),
}

/// Formats `bytes` for display under `scale`.
pub fn size_text(bytes: u64, scale: Scale) -> String {
    match scale {
        Scale::Bytes => bytes.to_string(),
        Scale::Unit(unit) => bytes.div_ceil(unit).to_string(),
        Scale::Human(base) => human(bytes, base),
    }
}

/// Parses a `-B` or `-t` size: digits followed by an optional unit. `K M G T P E` use
/// powers of 1024; a trailing `B` (`KB`) switches to powers of 1000, and `iB` (`KiB`) is
/// accepted as the binary form. Returns `None` for malformed or overflowing input.
pub fn parse_size(text: &str) -> Option<u64> {
    let digits_end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, suffix) = text.split_at(digits_end);
    let number: u64 = if digits.is_empty() {
        1
    } else {
        digits.parse().ok()?
    };
    let (unit_letter, rest) = match suffix.chars().next() {
        None => return Some(number),
        Some(letter) => (letter.to_ascii_uppercase(), &suffix[1..]),
    };
    let power = match unit_letter {
        'K' => 1,
        'M' => 2,
        'G' => 3,
        'T' => 4,
        'P' => 5,
        'E' => 6,
        _ => return None,
    };
    let base: u64 = match rest {
        "" | "iB" => 1024,
        "B" => 1000,
        _ => return None,
    };
    let factor = base.checked_pow(power)?;
    number.checked_mul(factor)
}

/// Named `--time-style` presets, or a custom `strftime` format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeStyle {
    /// `%Y-%m-%d %H:%M`, the `du --time` default.
    Long,
    /// `%Y-%m-%d %H:%M:%S.%N %z`.
    FullIso,
    Custom(String),
}

impl TimeStyle {
    /// Parses a `--time-style` argument. Returns `None` for an unknown preset.
    pub fn parse(text: &str) -> Option<TimeStyle> {
        Some(match text {
            "long-iso" | "iso" => TimeStyle::Long,
            "full-iso" => TimeStyle::FullIso,
            other => TimeStyle::Custom(other.strip_prefix('+')?.to_string()),
        })
    }

    fn pattern(&self) -> &str {
        match self {
            TimeStyle::Long => "%Y-%m-%d %H:%M",
            TimeStyle::FullIso => "%Y-%m-%d %H:%M:%S.%N %z",
            TimeStyle::Custom(format) => format,
        }
    }
}

/// Formats a file time `(seconds, nanoseconds)` for the `--time` column.
pub fn time_text(time: (i64, u32), style: &TimeStyle, zone: &TimeZone) -> String {
    let ts = Timestamp::new(time.0, time.1 as i32).unwrap_or(Timestamp::UNIX_EPOCH);
    format_in(style.pattern(), ts, zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_scaling_rounds_up() {
        assert_eq!(size_text(3001, Scale::Unit(1024)), "3");
        assert_eq!(size_text(4096, Scale::Unit(1024)), "4");
        assert_eq!(size_text(0, Scale::Unit(1024)), "0");
        assert_eq!(size_text(8192, Scale::Unit(1000)), "9");
        assert_eq!(size_text(1234, Scale::Bytes), "1234");
    }

    #[test]
    fn human_sizes_match_gnu_shapes() {
        assert_eq!(size_text(4096, Scale::Human(1024)), "4.0K");
        assert_eq!(size_text(16384, Scale::Human(1024)), "16K");
        assert_eq!(size_text(1, Scale::Human(1024)), "1");
        assert_eq!(size_text(1500, Scale::Human(1000)), "1.5k");
    }

    #[test]
    fn parses_block_sizes() {
        assert_eq!(parse_size("1000"), Some(1000));
        assert_eq!(parse_size("K"), Some(1024));
        assert_eq!(parse_size("2K"), Some(2048));
        assert_eq!(parse_size("2KB"), Some(2000));
        assert_eq!(parse_size("1MiB"), Some(1024 * 1024));
        assert_eq!(parse_size("1GB"), Some(1_000_000_000));
        assert_eq!(parse_size("3x"), None);
        assert_eq!(parse_size("1KX"), None);
        assert_eq!(parse_size("99999999999999999E"), None);
        assert_eq!(parse_size(""), Some(1));
    }

    #[test]
    fn time_style_presets() {
        assert_eq!(TimeStyle::parse("iso"), Some(TimeStyle::Long));
        assert_eq!(TimeStyle::parse("full-iso"), Some(TimeStyle::FullIso));
        assert_eq!(
            TimeStyle::parse("+%Y"),
            Some(TimeStyle::Custom("%Y".to_string()))
        );
        assert_eq!(TimeStyle::parse("bogus"), None);
    }

    #[test]
    fn time_text_uses_style() {
        let time = (1_704_207_845, 123_456_789);
        assert_eq!(
            time_text(time, &TimeStyle::Long, &TimeZone::UTC),
            "2024-01-02 15:04"
        );
        assert_eq!(
            time_text(time, &TimeStyle::FullIso, &TimeZone::UTC),
            "2024-01-02 15:04:05.123456789 +0000"
        );
        assert_eq!(
            time_text(time, &TimeStyle::Custom("%d".to_string()), &TimeZone::UTC),
            "02"
        );
    }
}
