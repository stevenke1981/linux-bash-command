use crate::common::{datetime, meta};
use crate::display_path;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn default_format(terse: bool) -> &'static str {
    if terse {
        "%n %s %b %f %u %g %D %i %h %t %T %X %Y %Z %W %o\n"
    } else {
        "  File: %N\n  Size: %-10s\tBlocks: %-10b IO Block: %-6o %F\nDevice: %Hd,%Ld\tInode: %-10i  Links: %h\nAccess: (%04a/%A)  Uid: (%5u/%8U)   Gid: (%5g/%8G)\nAccess: %x\nModify: %y\nChange: %z\n Birth: %w\n"
    }
}

pub fn epoch(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(value) => value.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    }
}

pub fn timestamp(time: Option<SystemTime>) -> String {
    time.and_then(|time| datetime::from_system_time(time).ok())
        .and_then(|time| datetime::format(&time, "%Y-%m-%d %H:%M:%S.%N %z").ok())
        .unwrap_or_else(|| "-".into())
}

pub fn kind(meta: &Metadata) -> &'static str {
    match meta::kind_char(meta) {
        'f' if meta.len() == 0 => "regular empty file",
        'f' => "regular file",
        'd' => "directory",
        'l' => "symbolic link",
        'b' => "block special file",
        'c' => "character special file",
        'p' => "fifo",
        's' => "socket",
        _ => "unknown",
    }
}

pub fn mount(path: &Path) -> PathBuf {
    let mut path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let Ok(metadata) = fs::metadata(&path) else {
        return path;
    };
    let device = meta::file_identity(&path, &metadata).dev;
    if !metadata.is_dir() {
        path.pop();
    }
    while let Some(parent) = path.parent() {
        if !parent.is_absolute() {
            break;
        }
        let Ok(metadata) = fs::metadata(parent) else {
            break;
        };
        if meta::file_identity(parent, &metadata).dev != device {
            break;
        }
        path = parent.to_owned();
    }
    path
}

pub fn device_parts(device: u64) -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        (
            ((device >> 8) & 0xfff) | ((device >> 32) & 0xfffff000),
            (device & 0xff) | ((device >> 12) & 0xffffff00),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        ((device >> 24) & 0xff, device & 0xffffff)
    }
}

pub fn render_bytes(path: &Path, metadata: &Metadata, template: &[u8]) -> Vec<u8> {
    let id = meta::file_identity(path, metadata);
    let (uid, gid) = meta::owner_ids(metadata);
    let mode = meta::permission_bits_at(path, metadata);
    let mut full_mode = mode;
    full_mode |= match meta::kind_char(metadata) {
        'f' => 0o100000,
        'd' => 0o040000,
        'l' => 0o120000,
        'b' => 0o060000,
        'c' => 0o020000,
        'p' => 0o010000,
        's' => 0o140000,
        _ => 0,
    };
    #[cfg(unix)]
    let (blocksize, rdev) = {
        use std::os::unix::fs::MetadataExt;
        (Some(metadata.blksize()), metadata.rdev())
    };
    #[cfg(not(unix))]
    let (blocksize, rdev) = (
        super::filesystem::query(path)
            .ok()
            .map(|info| info.transfer_size),
        0u64,
    );
    render_with_bytes(template, |directive| {
        let (number, radix) = match directive {
            "a" => (u128::from(mode), 8),
            "b" => (u128::from(meta::allocated_bytes(metadata) / 512), 10),
            "B" => (512, 10),
            "d" => (u128::from(id.dev), 10),
            "D" => (u128::from(id.dev), 16),
            "f" => (u128::from(full_mode), 16),
            "g" => (u128::from(gid), 10),
            "h" => (u128::from(id.nlink), 10),
            "i" => (u128::from(id.inode), 10),
            "o" => match blocksize {
                Some(size) => (u128::from(size), 10),
                None => return ("?".into(), None),
            },
            "s" => (u128::from(metadata.len()), 10),
            "u" => (u128::from(uid), 10),
            "t" | "Hr" => (
                u128::from(device_parts(rdev).0),
                if directive == "t" { 16 } else { 10 },
            ),
            "T" | "Lr" => (
                u128::from(device_parts(rdev).1),
                if directive == "T" { 16 } else { 10 },
            ),
            "Hd" => (u128::from(device_parts(id.dev).0), 10),
            "Ld" => (u128::from(device_parts(id.dev).1), 10),
            _ => {
                let text = match directive {
                    "A" => meta::mode_string(path, metadata),
                    "C" => "?".into(),
                    "F" => kind(metadata).into(),
                    "G" => meta::group_name(gid).unwrap_or_else(|| "UNKNOWN".into()),
                    "U" => meta::user_name(uid).unwrap_or_else(|| "UNKNOWN".into()),
                    "m" => display_path(&mount(path)),
                    "n" => display_path(path),
                    "N" => {
                        let mut text = quoted_name(&display_path(path));
                        if metadata.file_type().is_symlink() {
                            if let Ok(target) = fs::read_link(path) {
                                text.push_str(&format!(
                                    " -> {}",
                                    quoted_name(&display_path(&target))
                                ));
                            }
                        }
                        text
                    }
                    "w" => timestamp(metadata.created().ok()),
                    "x" => timestamp(metadata.accessed().ok()),
                    "y" => timestamp(metadata.modified().ok()),
                    "z" => timestamp(meta::change_time(path, metadata)),
                    "W" | "X" | "Y" | "Z" => {
                        let time = match directive {
                            "W" => metadata.created().ok(),
                            "X" => metadata.accessed().ok(),
                            "Y" => metadata.modified().ok(),
                            _ => meta::change_time(path, metadata),
                        };
                        time.map_or(0, |time| {
                            if directive == "W" {
                                epoch(time).div_euclid(1_000_000_000)
                            } else {
                                epoch(time)
                            }
                        })
                        .to_string()
                    }
                    "%" => "%".into(),
                    _ => "?".into(),
                };
                return (
                    text,
                    if matches!(directive, "X" | "Y" | "Z") {
                        Some(0) // Epoch nanoseconds, rendered as seconds with optional precision.
                    } else if directive == "W" {
                        Some(10)
                    } else {
                        None
                    },
                );
            }
        };
        let text = match radix {
            8 => format!("{number:o}"),
            16 => format!("{number:x}"),
            _ => number.to_string(),
        };
        (text, Some(radix))
    })
}

/// GNU printf-style width, precision and integer flags for stat directives.
#[cfg(test)]
pub fn render_with(template: &str, value: impl FnMut(&str) -> (String, Option<u8>)) -> String {
    String::from_utf8(render_with_bytes(template.as_bytes(), value)).unwrap()
}

pub fn render_with_bytes(
    template: &[u8],
    mut value: impl FnMut(&str) -> (String, Option<u8>),
) -> Vec<u8> {
    let chars = template;
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != b'%' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        index += 1;
        let mut flags = String::new();
        while index < chars.len() && b"#0- +'I".contains(&chars[index]) {
            flags.push(char::from(chars[index]));
            index += 1;
        }
        let mut width = String::new();
        while index < chars.len() && chars[index].is_ascii_digit() {
            width.push(char::from(chars[index]));
            index += 1;
        }
        let mut precision = None;
        if chars.get(index) == Some(&b'.') {
            index += 1;
            let mut text = String::new();
            while index < chars.len() && chars[index].is_ascii_digit() {
                text.push(char::from(chars[index]));
                index += 1;
            }
            precision = Some(text.parse::<usize>().unwrap_or(0));
        }
        let Some(letter) = chars.get(index) else {
            out.push(b'%');
            break;
        };
        index += 1;
        let mut directive = char::from(*letter).to_string();
        if matches!(letter, b'H' | b'L')
            && chars.get(index).is_some_and(|ch| matches!(ch, b'd' | b'r'))
        {
            directive.push(char::from(chars[index]));
            index += 1;
        }
        let (mut text, mut radix) = value(&directive);
        if radix == Some(0) {
            let nanos = text.parse::<i128>().unwrap_or(0);
            let digits = precision.unwrap_or(0);
            text = if digits == 0 {
                nanos.div_euclid(1_000_000_000).to_string()
            } else {
                let magnitude = nanos.unsigned_abs();
                let fraction = format!("{:09}", magnitude % 1_000_000_000);
                format!(
                    "{}{}.{}{}",
                    if nanos < 0 { "-" } else { "" },
                    magnitude / 1_000_000_000,
                    &fraction[..digits.min(9)],
                    "0".repeat(digits.saturating_sub(9))
                )
            };
            radix = Some(10);
            precision = None;
        }
        if radix.is_none() {
            let bytes = text.as_bytes();
            let bytes = &bytes[..precision.unwrap_or(bytes.len()).min(bytes.len())];
            let padding = width
                .parse::<usize>()
                .unwrap_or(0)
                .saturating_sub(bytes.len());
            if !flags.contains('-') {
                out.extend_from_slice(" ".repeat(padding).as_bytes());
            }
            out.extend_from_slice(bytes);
            if flags.contains('-') {
                out.extend_from_slice(" ".repeat(padding).as_bytes());
            }
            continue;
        }
        if let Some(precision) = precision {
            if radix.is_some() {
                let prefix = usize::from(text.starts_with(['+', '-', ' ']));
                text = format!(
                    "{}{}{}",
                    &text[..prefix],
                    "0".repeat(precision.saturating_sub(text.len() - prefix)),
                    &text[prefix..]
                );
            } else {
                text = text.chars().take(precision).collect();
            }
        }
        if radix.is_some() {
            if flags.contains('#') && text != "0" {
                match radix {
                    Some(8) if !text.starts_with('0') => text.insert(0, '0'),
                    Some(16) => text.insert_str(0, "0x"),
                    _ => {}
                }
            }
            if flags.contains('+') && !text.starts_with('-') {
                text.insert(0, '+');
            } else if flags.contains(' ') && !text.starts_with('-') {
                text.insert(0, ' ');
            }
        }
        let width = width.parse::<usize>().unwrap_or(0);
        let padding = width.saturating_sub(text.chars().count());
        if flags.contains('-') {
            out.extend_from_slice(text.as_bytes());
            out.extend_from_slice(" ".repeat(padding).as_bytes());
        } else if flags.contains('0') && radix.is_some() && precision.is_none() {
            let prefix = if text.starts_with("0x") {
                2
            } else if text.starts_with(['+', '-', ' ']) {
                1
            } else {
                0
            };
            out.extend_from_slice(&text.as_bytes()[..prefix]);
            out.extend_from_slice("0".repeat(padding).as_bytes());
            out.extend_from_slice(&text.as_bytes()[prefix..]);
        } else {
            out.extend_from_slice(" ".repeat(padding).as_bytes());
            out.extend_from_slice(text.as_bytes());
        }
    }
    out
}

fn quoted_name(name: &str) -> String {
    let quoted = crate::common::escape::quote_name(name.as_bytes());
    if quoted == name.as_bytes() {
        format!("'{name}'")
    } else {
        String::from_utf8_lossy(&quoted).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn width() {
        assert_eq!(render_with("%5s", |_| ("12".into(), Some(10))), "   12");
    }
    #[test]
    fn left() {
        assert_eq!(render_with("%-5s", |_| ("12".into(), Some(10))), "12   ");
    }
    #[test]
    fn zero() {
        assert_eq!(render_with("%04a", |_| ("644".into(), Some(8))), "0644");
    }
    #[test]
    fn alternate_hex() {
        assert_eq!(render_with("%#06i", |_| ("ab".into(), Some(16))), "0x00ab");
    }
    #[test]
    fn negative_epoch() {
        assert_eq!(
            epoch(UNIX_EPOCH - std::time::Duration::from_millis(500)),
            -500_000_000
        );
    }
    #[test]
    fn precision() {
        assert_eq!(render_with("%.3n", |_| ("abcdef".into(), None)), "abc");
    }
    #[test]
    fn string_width_counts_bytes_in_c_locale() {
        assert_eq!(
            render_with_bytes(b"%4n", |_| ("é".into(), None)),
            b"  \xc3\xa9"
        );
    }
    #[test]
    fn string_precision_preserves_partial_utf8_bytes() {
        assert_eq!(render_with_bytes(b"%.1n", |_| ("é".into(), None)), b"\xc3");
    }
    #[test]
    fn epoch_precision_formats_fractional_seconds() {
        assert_eq!(
            render_with("%.3Y", |_| ("1234567890".into(), Some(0))),
            "1.234"
        );
        assert_eq!(render_with("%Y", |_| ("-500000000".into(), Some(0))), "-1");
        assert_eq!(
            render_with("%.3Y", |_| ("-500000000".into(), Some(0))),
            "-0.500"
        );
    }
}
