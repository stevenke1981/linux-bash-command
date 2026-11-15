use super::{indicator, long_name, Entry, LsOptions, TimeStyle};
use crate::common::{color, datetime, meta};
use std::time::SystemTime;

#[derive(Clone, Copy, Default)]
pub(super) enum Quoting {
    #[default]
    Literal,
    Shell,
    Always,
    C,
    Escape,
}
impl Quoting {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "literal" => Ok(Self::Literal),
            "shell" | "shell-escape" => Ok(Self::Shell),
            "shell-always" => Ok(Self::Always),
            "c" => Ok(Self::C),
            "escape" => Ok(Self::Escape),
            _ => Err(format!("invalid argument '{value}' for 'quoting-style'")),
        }
    }
}

pub(super) fn quote(name: &str, style: Quoting) -> String {
    match style {
        Quoting::Literal => name.into(),
        Quoting::Shell => {
            String::from_utf8_lossy(&crate::common::escape::quote_name(name.as_bytes()))
                .into_owned()
        }
        Quoting::Always => format!("'{}'", name.replace('\'', "'\\''")),
        Quoting::C | Quoting::Escape => {
            let c = matches!(style, Quoting::C);
            let mut text = String::new();
            if c {
                text.push('"');
            }
            for byte in name.bytes() {
                match byte {
                    b'\n' => text.push_str("\\n"),
                    b'\r' => text.push_str("\\r"),
                    b'\t' => text.push_str("\\t"),
                    b'\\' => text.push_str("\\\\"),
                    b'"' if c => text.push_str("\\\""),
                    b' ' if !c => text.push_str("\\ "),
                    7 => text.push_str("\\a"),
                    8 => text.push_str("\\b"),
                    11 => text.push_str("\\v"),
                    12 => text.push_str("\\f"),
                    byte if !(32..127).contains(&byte) => text.push_str(&format!("\\{byte:03o}")),
                    byte => text.push(char::from(byte)),
                }
            }
            if c {
                text.push('"');
            }
            text
        }
    }
}

pub(super) fn visible(name: &str, options: &LsOptions) -> bool {
    !options.ignore.iter().any(|glob| glob.is_match(name))
        && !(options.backups && name.ends_with('~'))
        && (options.all || !options.hide.iter().any(|glob| glob.is_match(name)))
}

pub(super) fn time(entry: &Entry, clock: super::ls_options::Clock) -> Option<SystemTime> {
    match clock {
        super::ls_options::Clock::Modified => entry.metadata.modified().ok(),
        super::ls_options::Clock::Accessed => entry.metadata.accessed().ok(),
        super::ls_options::Clock::Changed => meta::change_time(&entry.path, &entry.metadata),
        super::ls_options::Clock::Birth => entry.metadata.created().ok(),
    }
}

fn size(bytes: u64, options: &LsOptions, allocated: bool) -> String {
    if allocated {
        if let Some(block) = options.allocated_block {
            return bytes.div_ceil(block).to_string();
        }
    }
    if options.human {
        crate::common::size::human(bytes, if options.si { 1000 } else { 1024 })
    } else {
        bytes
            .div_ceil(
                options
                    .block_size
                    .unwrap_or(if allocated { 1024 } else { 1 }),
            )
            .to_string()
    }
}

pub(super) fn allocated_size(bytes: u64, options: &LsOptions) -> String {
    size(bytes, options, true)
}

fn prefix(entry: &Entry, options: &LsOptions) -> String {
    let mut prefix = String::new();
    if options.inode {
        prefix.push_str(&format!(
            "{} ",
            meta::file_identity(&entry.path, &entry.metadata).inode
        ));
    }
    if options.blocks {
        prefix.push_str(&format!(
            "{} ",
            size(meta::allocated_bytes(&entry.metadata), options, true)
        ));
    }
    prefix
}

pub(super) fn entry_text(entry: &Entry, options: &LsOptions) -> String {
    let shown = quote(&entry.name, options.quote);
    let name = options
        .style
        .quoted_name(&entry.name, &shown, &entry.path, &entry.metadata);
    format!(
        "{}{name}{}",
        prefix(entry, options),
        indicator(entry, options)
    )
}

pub(super) fn long_lines(entries: &[Entry], options: &LsOptions) -> Vec<String> {
    let sizes: Vec<_> = entries
        .iter()
        .map(|entry| size(entry.metadata.len(), options, false))
        .collect();
    let size_width = sizes.iter().map(String::len).max().unwrap_or(0);
    let identities: Vec<_> = entries
        .iter()
        .map(|entry| meta::file_identity(&entry.path, &entry.metadata))
        .collect();
    let link_width = identities
        .iter()
        .map(|identity| identity.nlink.to_string().len())
        .max()
        .unwrap_or(1);
    entries
        .iter()
        .zip(&sizes)
        .zip(&identities)
        .map(|((entry, size), identity)| {
            let timestamp = time(entry, options.clock);
            let recent = timestamp.is_some_and(|time| recent(time, SystemTime::now()));
            let format = options
                .time_format
                .as_deref()
                .unwrap_or(match options.time_style {
                    TimeStyle::Minutes => "%Y-%m-%d %H:%M",
                    TimeStyle::Seconds => "%Y-%m-%d %H:%M:%S.%N %z",
                    TimeStyle::Iso if recent => "%m-%d %H:%M",
                    TimeStyle::Iso => "%Y-%m-%d ",
                    TimeStyle::Locale if recent => "%b %e %H:%M",
                    TimeStyle::Locale => "%b %e  %Y",
                });
            let date = timestamp
                .and_then(|time| datetime::from_system_time(time).ok())
                .and_then(|time| datetime::format(&time, format).ok())
                .unwrap_or_else(|| "-".into());
            format!(
                "{}{} {:>link_width$} - - {:>size_width$} {} {}",
                prefix(entry, options),
                meta::mode_string(&entry.path, &entry.metadata),
                identity.nlink,
                size,
                date,
                long_name_without_prefix(entry, options)
            )
        })
        .collect()
}

fn recent(time: SystemTime, now: SystemTime) -> bool {
    // GNU's recent window is half an average Gregorian year, excluding future times.
    now.duration_since(time)
        .is_ok_and(|age| age.as_secs() < 15_778_476)
}

fn long_name_without_prefix(entry: &Entry, options: &LsOptions) -> String {
    let text = long_name(entry, options);
    text[prefix(entry, options).len()..].into()
}

pub(super) fn arrange(cells: Vec<String>, options: &LsOptions) -> Vec<String> {
    let width = options.width.unwrap_or_else(|| {
        if color::stdout_is_tty() {
            color::terminal_width()
        } else {
            std::env::var("COLUMNS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(80)
        }
    });
    if options.comma {
        let mut lines = Vec::new();
        let mut line = String::new();
        for (index, cell) in cells.iter().enumerate() {
            let suffix = if index + 1 < cells.len() { "," } else { "" };
            if !line.is_empty()
                && width > 0
                && color::visible_width(&line) + color::visible_width(cell) + suffix.len() + 1
                    > width
            {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(cell);
            line.push_str(suffix);
        }
        if !line.is_empty() {
            lines.push(line);
        }
        return lines;
    }
    if options.width == Some(0) {
        return match options.layout {
            super::Layout::OneLine => cells,
            _ => vec![cells.join("  ")],
        };
    }
    match options.layout {
        super::Layout::OneLine => cells,
        super::Layout::Auto if !color::stdout_is_tty() => cells,
        super::Layout::Across => color::columns(&cells, width, true),
        _ => color::columns(&cells, width, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_c() {
        assert_eq!(quote("a\nb", Quoting::C), "\"a\\nb\"");
    }
    #[test]
    fn quotes_escape() {
        assert_eq!(quote("a b", Quoting::Escape), "a\\ b");
    }
    #[test]
    fn quotes_shell() {
        assert_eq!(quote("a b", Quoting::Shell), "'a b'");
    }
    #[test]
    fn literal() {
        assert_eq!(quote("a b", Quoting::Literal), "a b");
    }
    #[test]
    fn always() {
        assert_eq!(quote("abc", Quoting::Always), "'abc'");
    }
}
