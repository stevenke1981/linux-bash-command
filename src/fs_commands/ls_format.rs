//! Text of `ls` entries: the long format (`-l`), one name per line, columns, rows and commas,
//! with the `-i` and `-s` prefixes, quoting, indicators and the `total` line.

use super::ls::Entry;
use super::ls_layout;
use super::ls_options::{Format, Indicator, LsOptions, Quoting};
use super::ls_quote;
use super::ls_time::{self, format_size, stamp, units};
use crate::commands::stat::platform::{group_name, user_name};
use crate::common::meta::{inode_nlink, mode_string, uid_gid};
use crate::display_path;
use jiff::{tz::TimeZone, Timestamp};
use std::fs;

/// What formatting needs besides the entries themselves.
pub struct Ctx {
    pub now: Timestamp,
    pub zone: TimeZone,
    /// Standard output is a terminal: `auto` formats use columns and `auto` classify applies.
    pub tty: bool,
    /// Line width for columns and commas.
    pub width: usize,
    /// The quoting style in effect (never `Unset`).
    pub quoting: Quoting,
}

/// Output for a directory listing, one line per entry (or columns, or commas), ending with a
/// newline after each line.
pub fn body(entries: &[Entry], options: &LsOptions, ctx: &Ctx) -> String {
    let format = match options.format {
        Format::Auto if ctx.tty => Format::Columns,
        Format::Auto => Format::Single,
        other => other,
    };
    if format == Format::Long {
        return long_lines(entries, options, ctx);
    }
    let widths = short_prefix_widths(entries, options);
    let items: Vec<String> = entries
        .iter()
        .map(|entry| prefixed(entry, options, ctx, &widths))
        .collect();
    match format {
        Format::Columns => ls_layout::columns(&items, ctx.width, options.tabsize, false),
        Format::Across => ls_layout::columns(&items, ctx.width, options.tabsize, true),
        Format::Commas => ls_layout::commas(&items, ctx.width),
        _ => items.iter().map(|item| format!("{item}\n")).collect(),
    }
}

/// The `total` line for `-l` and `-s`: the sum of block counts, in the block format.
pub fn total_line(entries: &[Entry], options: &LsOptions) -> String {
    let sum: u64 = entries
        .iter()
        .map(|entry| units(entry.allocated, &options.block_format))
        .sum();
    let text = match &options.block_format {
        super::ls_options::SizeFormat::Scaled { suffix, .. } => format!("{sum}{suffix}"),
        other => format_size(entries.iter().map(|entry| entry.allocated).sum(), other),
    };
    format!("total {text}\n")
}

/// Widths of the `-i` and `-s` columns, computed over the listed entries.
struct PrefixWidths {
    inode: usize,
    blocks: usize,
}

fn short_prefix_widths(entries: &[Entry], options: &LsOptions) -> PrefixWidths {
    PrefixWidths {
        inode: entries
            .iter()
            .map(|entry| inode_nlink(&entry.meta).0.to_string().len())
            .max()
            .unwrap_or(1),
        blocks: if options.blocks {
            entries
                .iter()
                .map(|entry| block_text(entry, options).len())
                .max()
                .unwrap_or(1)
        } else {
            0
        },
    }
}

fn block_text(entry: &Entry, options: &LsOptions) -> String {
    format_size(entry.allocated, &options.block_format)
}

/// The inode and block columns (when requested) followed by the name cell.
fn prefixed(entry: &Entry, options: &LsOptions, ctx: &Ctx, widths: &PrefixWidths) -> String {
    let mut text = String::new();
    if options.inode {
        let inode = inode_nlink(&entry.meta).0;
        text.push_str(&format!("{inode:>width$} ", width = widths.inode));
    }
    if options.blocks {
        text.push_str(&format!(
            "{:>width$} ",
            block_text(entry, options),
            width = widths.blocks
        ));
    }
    text.push_str(&cell(entry, options, ctx));
    text
}

/// The quoted name with its indicator, or with ` -> target` added for long listings.
fn cell(entry: &Entry, options: &LsOptions, ctx: &Ctx) -> String {
    let mut text = format!(
        "{}{}",
        ls_quote::quote(&entry.name, ctx.quoting),
        indicator(entry, options, ctx)
    );
    if entry.is_link && options.format == Format::Long {
        if let Ok(target) = fs::read_link(&entry.path) {
            text.push_str(" -> ");
            text.push_str(&ls_quote::quote(&display_path(&target), ctx.quoting));
        }
    }
    text
}

/// `-p`, `-F` and `--file-type` marks: `/` directories, `@` links, `|` FIFOs, `=` sockets,
/// and `*` executables (classify only). Entries are judged by the metadata in use.
fn indicator(entry: &Entry, options: &LsOptions, ctx: &Ctx) -> &'static str {
    let kind = match options.indicator {
        Indicator::None => return "",
        Indicator::ClassifyAuto if !ctx.tty => return "",
        Indicator::ClassifyAuto => Indicator::Classify,
        other => other,
    };
    let file_type = entry.meta.file_type();
    if file_type.is_dir() {
        return "/";
    }
    if kind == Indicator::Slash {
        return "";
    }
    if file_type.is_symlink() {
        return "@";
    }
    if let Some(mark) = special_mark(&file_type) {
        return mark;
    }
    if kind == Indicator::Classify && is_executable(entry) {
        "*"
    } else {
        ""
    }
}

#[cfg(unix)]
fn special_mark(file_type: &fs::FileType) -> Option<&'static str> {
    use std::os::unix::fs::FileTypeExt;
    if file_type.is_fifo() {
        Some("|")
    } else if file_type.is_socket() {
        Some("=")
    } else {
        None
    }
}

#[cfg(not(unix))]
fn special_mark(_file_type: &fs::FileType) -> Option<&'static str> {
    None
}

#[cfg(unix)]
fn is_executable(entry: &Entry) -> bool {
    use std::os::unix::fs::PermissionsExt;
    entry.meta.is_file() && entry.meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(entry: &Entry) -> bool {
    const EXECUTABLE_EXTENSIONS: [&str; 5] = ["exe", "bat", "cmd", "com", "ps1"];
    let Some(extension) = entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
    else {
        return false;
    };
    entry.meta.is_file()
        && EXECUTABLE_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
}

/// Owner or group text: the account name, or the number when there is none (or with `-n`).
/// Windows has no account database and shows `-`, as `ls -l` always has.
fn account(id: u32, numeric: bool, lookup: fn(u32) -> Option<String>) -> String {
    if numeric {
        return id.to_string();
    }
    lookup(id).unwrap_or_else(|| {
        if cfg!(windows) {
            "-".to_string()
        } else {
            id.to_string()
        }
    })
}

/// One line per entry: `[inode] [blocks] mode links owner group size time name`.
fn long_lines(entries: &[Entry], options: &LsOptions, ctx: &Ctx) -> String {
    let rows: Vec<LongRow> = entries
        .iter()
        .map(|entry| LongRow::new(entry, options, ctx))
        .collect();
    let link_width = width_of(rows.iter().map(|row| row.links.as_str()));
    let owner_width = width_of(rows.iter().map(|row| row.owner.as_str()));
    let group_width = width_of(rows.iter().map(|row| row.group.as_str()));
    let size_width = width_of(rows.iter().map(|row| row.size.as_str()));
    let time_width = width_of(rows.iter().map(|row| row.time.as_str()));
    let inode_width = width_of(rows.iter().map(|row| row.inode.as_str()));
    let block_width = width_of(rows.iter().map(|row| row.blocks.as_str()));
    let mut out = String::new();
    for row in &rows {
        let mut fields: Vec<String> = Vec::new();
        if options.inode {
            fields.push(format!("{:>inode_width$}", row.inode));
        }
        if options.blocks {
            fields.push(format!("{:>block_width$}", row.blocks));
        }
        fields.push(row.mode.clone());
        fields.push(format!("{:>link_width$}", row.links));
        if options.owner {
            fields.push(format!("{:<owner_width$}", row.owner));
        }
        if options.group {
            fields.push(format!("{:<group_width$}", row.group));
        }
        fields.push(format!("{:>size_width$}", row.size));
        fields.push(format!("{:<time_width$}", row.time));
        fields.push(row.cell.clone());
        out.push_str(&fields.join(" "));
        out.push('\n');
    }
    out
}

fn width_of<'a>(texts: impl Iterator<Item = &'a str>) -> usize {
    texts.map(|text| text.chars().count()).max().unwrap_or(0)
}

/// The fields of one long-format line before padding.
struct LongRow {
    inode: String,
    blocks: String,
    mode: String,
    links: String,
    owner: String,
    group: String,
    size: String,
    time: String,
    cell: String,
}

impl LongRow {
    fn new(entry: &Entry, options: &LsOptions, ctx: &Ctx) -> Self {
        let (uid, gid) = uid_gid(&entry.meta);
        let links = inode_nlink(&entry.meta).1;
        Self {
            inode: inode_nlink(&entry.meta).0.to_string(),
            blocks: block_text(entry, options),
            mode: mode_string(&entry.path, &entry.meta),
            links: links.to_string(),
            owner: account(uid, options.numeric_ids, user_name),
            group: account(gid, options.numeric_ids, group_name),
            size: format_size(entry.meta.len(), &options.size_format),
            time: ls_time::format(
                stamp(&entry.meta, options.time),
                &options.time_style,
                ctx.now,
                &ctx.zone,
            ),
            cell: cell(entry, options, ctx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ls_options::SizeFormat;
    use super::*;

    #[test]
    fn accounts_fall_back_to_numbers() {
        assert_eq!(account(0, true, |_| Some("root".to_string())), "0");
        assert_eq!(
            account(7, false, |_| None),
            if cfg!(windows) { "-" } else { "7" }
        );
        assert_eq!(account(7, false, |_| Some("ops".to_string())), "ops");
    }

    #[test]
    fn total_sums_block_units_per_entry() {
        let options = LsOptions {
            block_format: SizeFormat::Scaled {
                unit: 1024,
                suffix: "",
            },
            ..LsOptions::default()
        };
        assert_eq!(super::super::ls_time::units(512, &options.block_format), 1);
        assert_eq!(
            super::super::ls_time::format_size(4096, &SizeFormat::Human(1024)),
            "4.0K"
        );
    }
}
