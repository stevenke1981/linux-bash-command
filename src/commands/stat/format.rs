//! Text produced by `stat` for one file: `-c`/`--printf` format expansion, the default
//! multi-line layout, and the `-t` terse layout.

use jiff::{tz::TimeZone, Timestamp};

use super::platform::{kind_word, mode_text, split_device, RawStat};
use crate::common::datetime::format_in;

/// Everything `stat` shows about one operand, gathered before formatting.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// The operand as the user wrote it (`%n`).
    pub name: String,
    /// Symbolic link target when the operand is a link that was not followed.
    pub link: Option<String>,
    pub stat: RawStat,
    /// Owner name, or the numeric uid when unknown.
    pub user: String,
    pub group: String,
    /// Mount point of the file.
    pub mount: String,
}

/// Human-readable time with nanoseconds, as GNU prints it.
fn human_time(time: (i64, u32), zone: &TimeZone) -> String {
    let ts = Timestamp::new(time.0, time.1 as i32).unwrap_or(Timestamp::UNIX_EPOCH);
    format_in("%Y-%m-%d %H:%M:%S.%N %z", ts, zone)
}

/// Birth time as text: `-` when the file system does not record it.
fn birth_text(info: &FileInfo, zone: &TimeZone) -> String {
    match info.stat.btime {
        Some(time) => human_time(time, zone),
        None => "-".to_string(),
    }
}

/// `%N`: the quoted name, with the link target when there is one.
pub fn quoted_name(info: &FileInfo) -> String {
    match &info.link {
        Some(target) => format!("{} -> {}", single_quote(&info.name), single_quote(target)),
        None => single_quote(&info.name),
    }
}

/// Always wraps in single quotes, escaping embedded quotes the shell way.
fn single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Flags and width of one directive.
#[derive(Debug, Default, PartialEq, Eq)]
struct Flags {
    left: bool,
    zero: bool,
    width: usize,
}

/// Expands a `-c` or `--printf` format for `info`. Unknown directives print `?`, as GNU does.
pub fn expand(format: &str, info: &FileInfo, zone: &TimeZone) -> String {
    expand_with(format, &|conv| conversion_value(conv, info, zone))
}

/// Walks `format`, replacing each directive with `value(conversion)` after applying its flags
/// and width. `value` receives one character, or `Hd`, `Ld`, `Hr`, `Lr`.
pub fn expand_with(format: &str, value: &dyn Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(format.len() + 32);
    let mut rest = format;
    while let Some(pos) = rest.find('%') {
        out.push_str(&rest[..pos]);
        let body = &rest[pos + 1..];
        let (flags, conversion, used) = parse_directive(body);
        match conversion {
            None => {
                out.push('%');
                return out;
            }
            Some(conv) => out.push_str(&pad(&value(conv), &flags)),
        }
        rest = &body[used..];
    }
    out.push_str(rest);
    out
}

/// Reads flags, width and the conversion after a `%`. Returns the flags, the conversion
/// (`None` at end of input) and the number of bytes consumed from `body`.
fn parse_directive(body: &str) -> (Flags, Option<&str>, usize) {
    let bytes = body.as_bytes();
    let mut flags = Flags::default();
    let mut index = 0;
    while index < bytes.len() && matches!(bytes[index], b'-' | b'0') {
        if bytes[index] == b'-' {
            flags.left = true;
        } else {
            flags.zero = true;
        }
        index += 1;
    }
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        flags.width = flags
            .width
            .saturating_mul(10)
            .saturating_add(usize::from(bytes[index] - b'0'));
        index += 1;
    }
    if index >= bytes.len() {
        return (flags, None, index);
    }
    let two_char = matches!(bytes[index], b'H' | b'L')
        && index + 1 < bytes.len()
        && matches!(bytes[index + 1], b'd' | b'r');
    let length = if two_char {
        2
    } else {
        body[index..].chars().next().map_or(1, char::len_utf8)
    };
    (flags, Some(&body[index..index + length]), index + length)
}

/// The value for one conversion. `conv` is one character, or `Hd`, `Ld`, `Hr`, `Lr`.
fn conversion_value(conv: &str, info: &FileInfo, zone: &TimeZone) -> String {
    let stat = &info.stat;
    // `%Hd`/`%Ld` describe the device the file lives on; `%Hr`/`%Lr` describe the device
    // number of a special file (`st_rdev`), as GNU does.
    let (dev_major, dev_minor) = split_device(stat.dev);
    let (rdev_major, rdev_minor) = split_device(stat.rdev);
    match conv {
        "Hd" => return dev_major.to_string(),
        "Ld" => return dev_minor.to_string(),
        "Hr" => return format!("{rdev_major:x}"),
        "Lr" => return format!("{rdev_minor:x}"),
        _ => {}
    }
    let (major, minor) = (rdev_major, rdev_minor);
    match conv {
        "a" => format!("{:o}", stat.mode & 0o7777),
        "A" => mode_text(stat.mode),
        "b" => stat.blocks.to_string(),
        "B" => "512".to_string(),
        "C" => "?".to_string(),
        "d" => stat.dev.to_string(),
        "D" => format!("{:x}", stat.dev),
        "f" => format!("{:x}", stat.mode),
        "F" => kind_word(stat.mode).to_string(),
        "g" => stat.gid.to_string(),
        "G" => info.group.clone(),
        "h" => stat.nlink.to_string(),
        "i" => stat.ino.to_string(),
        "m" => info.mount.clone(),
        "n" => info.name.clone(),
        "N" => quoted_name(info),
        "o" => stat.blksize.to_string(),
        "s" => stat.size.to_string(),
        "t" => format!("{major:x}"),
        "T" => format!("{minor:x}"),
        "u" => stat.uid.to_string(),
        "U" => info.user.clone(),
        "w" => birth_text(info, zone),
        "W" => stat.btime.map_or(0, |t| t.0).to_string(),
        "x" => human_time(stat.atime, zone),
        "X" => stat.atime.0.to_string(),
        "y" => human_time(stat.mtime, zone),
        "Y" => stat.mtime.0.to_string(),
        "z" => human_time(stat.ctime, zone),
        "Z" => stat.ctime.0.to_string(),
        "%" => "%".to_string(),
        _ => "?".to_string(),
    }
}

/// Applies `-` (left justify) and `0` (zero fill) and the width. Zero fill goes after any
/// sign; text is space-filled unless `0` is given.
fn pad(text: &str, flags: &Flags) -> String {
    let len = text.chars().count();
    if len >= flags.width {
        return text.to_string();
    }
    let fill = flags.width - len;
    if flags.left {
        format!("{text}{}", " ".repeat(fill))
    } else if flags.zero {
        let sign_len = usize::from(text.starts_with('-'));
        format!(
            "{}{}{}",
            &text[..sign_len],
            "0".repeat(fill),
            &text[sign_len..]
        )
    } else {
        format!("{}{text}", " ".repeat(fill))
    }
}

/// The default multi-line layout (GNU 9.4).
pub fn default_layout(info: &FileInfo, zone: &TimeZone) -> String {
    let stat = &info.stat;
    let (major, minor) = split_device(stat.dev);
    let shown_name = match &info.link {
        Some(target) => format!("{} -> {target}", info.name),
        None => info.name.clone(),
    };
    let mut out = String::new();
    out.push_str(&format!("  File: {shown_name}\n"));
    out.push_str(&format!(
        "  Size: {:<10}\tBlocks: {:<10} IO Block: {:<6} {}\n",
        stat.size,
        stat.blocks,
        stat.blksize,
        kind_word(stat.mode)
    ));
    out.push_str(&format!(
        "Device: {major},{minor}\tInode: {:<10}  Links: {}\n",
        stat.ino, stat.nlink
    ));
    out.push_str(&format!(
        "Access: ({:04o}/{})  Uid: ({:>5}/{:>8})   Gid: ({:>5}/{:>8})\n",
        stat.mode & 0o7777,
        mode_text(stat.mode),
        stat.uid,
        info.user,
        stat.gid,
        info.group
    ));
    out.push_str(&format!("Access: {}\n", human_time(stat.atime, zone)));
    out.push_str(&format!("Modify: {}\n", human_time(stat.mtime, zone)));
    out.push_str(&format!("Change: {}\n", human_time(stat.ctime, zone)));
    out.push_str(&format!(" Birth: {}\n", birth_text(info, zone)));
    out
}

/// The `-t` terse layout: one line of plain values.
pub fn terse_layout(info: &FileInfo) -> String {
    let stat = &info.stat;
    let (major, minor) = split_device(stat.rdev);
    format!(
        "{} {} {} {:x} {} {} {:x} {} {} {:x} {:x} {} {} {} {} {}\n",
        info.name,
        stat.size,
        stat.blocks,
        stat.mode,
        stat.uid,
        stat.gid,
        stat.dev,
        stat.ino,
        stat.nlink,
        major,
        minor,
        stat.atime.0,
        stat.mtime.0,
        stat.ctime.0,
        stat.btime.map_or(0, |t| t.0),
        stat.blksize
    )
}

#[cfg(test)]
mod tests;
