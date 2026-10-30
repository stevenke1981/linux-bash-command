//! `-printf` format parsing and rendering, and the simple `-ls` long listing.

use super::expr::{file_name_text, kind_char, permission_bits, time_ns, Clock, Entry, NS_PER_SEC};
use crate::display_path;
use std::fs::Metadata;
use std::path::Path;

/// One element of a parsed `-printf` format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    /// `%p`: the path as printed.
    Path,
    /// `%P`: the path relative to its starting point.
    RelativePath,
    /// `%f`: the last path component.
    BaseName,
    /// `%h`: the leading directories.
    Dir,
    /// `%s`: size in bytes.
    Size,
    /// `%d`: depth below the starting point.
    Depth,
    /// `%y`: file type letter.
    Kind,
    /// `%m`: permission bits in octal.
    Mode,
    /// `%T@`: modification time in seconds since the epoch, with a fraction.
    ModifiedSeconds,
}

/// Parses a `-printf` format string, expanding `\` escapes and `%` directives.
pub fn parse_format(format: &str) -> Result<Vec<Piece>, String> {
    let chars: Vec<char> = format.chars().collect();
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => {
                let (escaped, next) = escape(&chars, index);
                text.push_str(&escaped);
                index = next;
            }
            '%' => {
                let (piece, next) = directive(&chars, index)?;
                if let Piece::Text(literal) = piece {
                    text.push_str(&literal);
                } else {
                    if !text.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut text)));
                    }
                    pieces.push(piece);
                }
                index = next;
            }
            other => {
                text.push(other);
                index += 1;
            }
        }
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Ok(pieces)
}

fn escape(chars: &[char], index: usize) -> (String, usize) {
    let Some(&next) = chars.get(index + 1) else {
        return ("\\".to_string(), index + 1);
    };
    let simple = match next {
        'a' => Some('\u{7}'),
        'b' => Some('\u{8}'),
        'f' => Some('\u{c}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'v' => Some('\u{b}'),
        '\\' => Some('\\'),
        '"' => Some('"'),
        _ => None,
    };
    if let Some(character) = simple {
        return (character.to_string(), index + 2);
    }
    if next.is_digit(8) {
        let mut value = 0u32;
        let mut end = index + 1;
        while end < chars.len() && end < index + 4 && chars[end].is_digit(8) {
            value = value * 8 + chars[end].to_digit(8).unwrap_or(0);
            end += 1;
        }
        let character = char::from_u32(value).unwrap_or('?');
        return (character.to_string(), end);
    }
    ("\\".to_string(), index + 1)
}

fn directive(chars: &[char], index: usize) -> Result<(Piece, usize), String> {
    let Some(&code) = chars.get(index + 1) else {
        return Err("incomplete -printf directive at end of format".to_string());
    };
    let piece = match code {
        '%' => Piece::Text("%".to_string()),
        'p' => Piece::Path,
        'P' => Piece::RelativePath,
        'f' => Piece::BaseName,
        'h' => Piece::Dir,
        's' => Piece::Size,
        'd' => Piece::Depth,
        'y' => Piece::Kind,
        'm' => Piece::Mode,
        'T' if chars.get(index + 2) == Some(&'@') => {
            return Ok((Piece::ModifiedSeconds, index + 3));
        }
        'T' => {
            let letter = chars.get(index + 2).map_or(String::new(), char::to_string);
            return Err(format!(
                "unsupported -printf directive '%T{letter}' (only %T@)"
            ));
        }
        other => return Err(format!("unsupported -printf directive '%{other}'")),
    };
    Ok((piece, index + 2))
}

/// Renders a parsed format for one entry. No trailing newline is added.
pub fn render(pieces: &[Piece], entry: &Entry) -> String {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Text(text) => out.push_str(text),
            Piece::Path => out.push_str(&display_path(entry.path)),
            Piece::RelativePath => out.push_str(&relative_path(entry)),
            Piece::BaseName => out.push_str(&file_name_text(entry.path)),
            Piece::Dir => out.push_str(&dir_text(entry.path)),
            Piece::Size => out.push_str(&entry.meta.len().to_string()),
            Piece::Depth => out.push_str(&entry.depth.to_string()),
            Piece::Kind => out.push(kind_char(entry.meta)),
            Piece::Mode => out.push_str(&format!("{:o}", permission_bits(entry.meta))),
            Piece::ModifiedSeconds => {
                out.push_str(&seconds_with_fraction(time_ns(entry.meta, Clock::Modified)))
            }
        }
    }
    out
}

fn relative_path(entry: &Entry) -> String {
    if entry.path == entry.root {
        return String::new();
    }
    match entry.path.strip_prefix(entry.root) {
        Ok(relative) => display_path(relative),
        Err(_) => display_path(entry.path),
    }
}

/// The leading directories of `path`, or `.` when there are none.
pub fn dir_text(path: &Path) -> String {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => display_path(parent),
        _ => ".".to_string(),
    }
}

fn seconds_with_fraction(time: Option<i128>) -> String {
    match time {
        Some(ns) => format!(
            "{}.{:09}0",
            ns.div_euclid(NS_PER_SEC),
            ns.rem_euclid(NS_PER_SEC)
        ),
        None => "?".to_string(),
    }
}

/// One `-ls` style line body (without newline): inode, blocks, mode, links, owner,
/// group, size, date (UTC) and path. Owner and group are numeric on Unix; on Windows
/// they are `?`.
pub fn long_listing(entry: &Entry) -> String {
    let meta = entry.meta;
    let (inode, blocks, links, owner, group) = identity(meta);
    let seconds = time_ns(meta, Clock::Modified).map_or(0, |ns| ns.div_euclid(NS_PER_SEC) as i64);
    format!(
        "{inode:>7} {blocks:>4} {mode} {links:>3} {owner:>8} {group:>8} {size:>8} {date} {path}",
        mode = mode_string(meta),
        size = meta.len(),
        date = date_text(seconds),
        path = display_path(entry.path),
    )
}

#[cfg(unix)]
fn identity(meta: &Metadata) -> (u64, u64, u64, String, String) {
    use std::os::unix::fs::MetadataExt;
    (
        meta.ino(),
        meta.blocks().div_ceil(2),
        meta.nlink(),
        meta.uid().to_string(),
        meta.gid().to_string(),
    )
}

#[cfg(not(unix))]
fn identity(meta: &Metadata) -> (u64, u64, u64, String, String) {
    (
        0,
        meta.len().div_ceil(1024),
        1,
        "?".to_string(),
        "?".to_string(),
    )
}

fn mode_string(meta: &Metadata) -> String {
    let kind = match kind_char(meta) {
        'f' => '-',
        other => other,
    };
    let bits = permission_bits(meta);
    let mut text = String::with_capacity(10);
    text.push(kind);
    for shift in [6, 3, 0] {
        let triple = (bits >> shift) & 0o7;
        text.push(if triple & 0o4 != 0 { 'r' } else { '-' });
        text.push(if triple & 0o2 != 0 { 'w' } else { '-' });
        text.push(if triple & 0o1 != 0 { 'x' } else { '-' });
    }
    text
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Formats seconds since the epoch as `Mon DD HH:MM` in UTC.
fn date_text(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (_year, month, day) = civil_from_days(days);
    format!(
        "{} {:>2} {:02}:{:02}",
        MONTHS[(month - 1) as usize],
        day,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60
    )
}

/// Converts days since 1970-01-01 to (year, month 1-12, day 1-31) in the proleptic
/// Gregorian calendar (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = (if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    }) as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, date_text, parse_format, Piece};

    #[test]
    fn parses_escapes_and_directives() {
        let pieces = parse_format(r"%P\t%s\n").unwrap();
        assert_eq!(
            pieces,
            vec![
                Piece::RelativePath,
                Piece::Text("\t".into()),
                Piece::Size,
                Piece::Text("\n".into()),
            ]
        );
        assert_eq!(
            parse_format("100%%").unwrap(),
            vec![Piece::Text("100%".into())]
        );
        assert_eq!(
            parse_format(r"\101").unwrap(),
            vec![Piece::Text("A".into())]
        );
    }

    #[test]
    fn rejects_unknown_directives() {
        assert!(parse_format("%u").is_err());
        assert!(parse_format("%TY").is_err());
        assert!(parse_format("%").is_err());
    }

    #[test]
    fn converts_epoch_days_to_calendar_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(date_text(0), "Jan  1 00:00");
    }
}
