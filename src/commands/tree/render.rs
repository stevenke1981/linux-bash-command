//! Text output for `tree`: the connector lines, the per-entry prefixes (`-p`, `-s`/`-h`/`--si`,
//! `-D`), the entry labels (`-f`, `-F`, symlink targets) and the closing summary.

use super::model::{Class, Counts, Node};
use crate::common::path::display_path;

/// Size column style, from `-s`, `-h` and `--si`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeStyle {
    None,
    Bytes,
    Human,
    HumanSi,
}

/// Everything that changes how one entry is printed.
pub struct View {
    pub ascii: bool,
    pub indent: bool,
    pub full_path: bool,
    pub classify: bool,
    pub perms: bool,
    pub size: SizeStyle,
    /// strftime format for the modification time (`-D`), if shown.
    pub time: Option<String>,
}

/// Renders the text form of every root, one after another.
pub fn render_text(roots: &[Node], view: &View) -> String {
    let mut out = String::new();
    for root in roots {
        out.push_str(&label(root, view));
        out.push('\n');
        walk(&mut out, &root.children, "", view);
    }
    out
}

fn walk(out: &mut String, children: &[Node], prefix: &str, view: &View) {
    let count = children.len();
    for (index, child) in children.iter().enumerate() {
        let last = index + 1 == count;
        if view.indent {
            out.push_str(prefix);
            out.push_str(connector(view.ascii, last));
        }
        out.push_str(&label(child, view));
        out.push('\n');
        if !child.children.is_empty() {
            let next = if view.indent {
                format!("{prefix}{}", continuation(view.ascii, last))
            } else {
                String::new()
            };
            walk(out, &child.children, &next, view);
        }
    }
}

fn connector(ascii: bool, last: bool) -> &'static str {
    match (ascii, last) {
        (true, true) => "`-- ",
        (true, false) => "|-- ",
        (false, true) => "└── ",
        (false, false) => "├── ",
    }
}

fn continuation(ascii: bool, last: bool) -> &'static str {
    match (ascii, last) {
        (_, true) => "    ",
        (true, false) => "|   ",
        (false, false) => "│   ",
    }
}

/// One entry's text: its prefix columns, its name and any trailing notes.
fn label(node: &Node, view: &View) -> String {
    let mut text = String::new();
    if view.perms {
        text.push_str(&format!("[{}]  ", node.stat.mode_text));
    }
    match view.size {
        SizeStyle::None => {}
        SizeStyle::Bytes => text.push_str(&format!("[{:>11}]  ", node.stat.size)),
        SizeStyle::Human => text.push_str(&format!("[{:>4}]  ", human_size(node.stat.size, 1024))),
        SizeStyle::HumanSi => {
            text.push_str(&format!("[{:>4}]  ", human_size(node.stat.size, 1000)))
        }
    }
    if let Some(format) = &view.time {
        text.push_str(&format!("[{}]  ", time_text(node.stat.mtime, format)));
    }
    if view.full_path {
        text.push_str(&display_path(&node.path));
    } else {
        text.push_str(&node.name);
    }
    match &node.class {
        Class::Link { target, .. } => {
            text.push_str(" -> ");
            text.push_str(target);
        }
        _ if view.classify => text.push_str(classify_suffix(node)),
        _ => {}
    }
    if let Some(count) = node.over_limit {
        text.push_str(&format!(
            "  [{count} entries exceeds filelimit, not opening dir]"
        ));
    } else if node.error {
        text.push_str("  [error opening dir]");
    }
    text
}

/// `-F` marker: `/` directories, `*` executables, `|` FIFOs, `=` sockets.
fn classify_suffix(node: &Node) -> &'static str {
    match &node.class {
        Class::Dir => "/",
        Class::Fifo => "|",
        Class::Socket => "=",
        Class::File if node.stat.executable => "*",
        _ => "",
    }
}

/// Formats a byte count the way `tree -h` does: plain below the base, otherwise one decimal
/// below 10 units and a whole number above, with K, M, G, T, P, E (lower-case `k` for base
/// 1000).
pub fn human_size(bytes: u64, base: u64) -> String {
    if bytes < base {
        return bytes.to_string();
    }
    let suffixes: [&str; 6] = if base == 1000 {
        ["k", "M", "G", "T", "P", "E"]
    } else {
        ["K", "M", "G", "T", "P", "E"]
    };
    // The first division selects the K (or k) unit; each further step moves up one unit.
    let mut value = bytes as f64 / base as f64;
    let mut unit = 0;
    while value >= base as f64 && unit < suffixes.len() - 1 {
        value /= base as f64;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1}{}", suffixes[unit])
    } else {
        format!("{value:.0}{}", suffixes[unit])
    }
}

/// Formats a modification time with a strftime pattern in the local time zone.
fn time_text(seconds: i64, format: &str) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_second(seconds) else {
        return "?".to_string();
    };
    let zoned = timestamp.to_zoned(jiff::tz::TimeZone::system());
    jiff::fmt::strtime::format(format, &zoned).unwrap_or_else(|_| "?".to_string())
}

/// The closing line, without its surrounding blank line: `N directories, M files`.
pub fn summary_line(counts: Counts, dirs_only: bool) -> String {
    let dirs = format!(
        "{} {}",
        counts.dirs,
        plural(counts.dirs, "directory", "directories")
    );
    if dirs_only {
        return dirs;
    }
    format!(
        "{dirs}, {} {}",
        counts.files,
        plural(counts.files, "file", "files")
    )
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 {
        one
    } else {
        many
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes_match_tree_rounding() {
        assert_eq!(human_size(999, 1024), "999");
        assert_eq!(human_size(1024, 1024), "1.0K");
        assert_eq!(human_size(1536, 1024), "1.5K");
        assert_eq!(human_size(10240, 1024), "10K");
        assert_eq!(human_size(1048576, 1024), "1.0M");
        assert_eq!(human_size(1000, 1000), "1.0k");
        assert_eq!(human_size(102400, 1000), "102k");
    }

    #[test]
    fn summary_uses_singular_and_plural_words() {
        assert_eq!(
            summary_line(Counts { dirs: 1, files: 1 }, false),
            "1 directory, 1 file"
        );
        assert_eq!(
            summary_line(Counts { dirs: 4, files: 0 }, false),
            "4 directories, 0 files"
        );
        assert_eq!(
            summary_line(Counts { dirs: 2, files: 9 }, true),
            "2 directories"
        );
    }

    #[test]
    fn connectors_switch_with_charset() {
        assert_eq!(connector(true, true), "`-- ");
        assert_eq!(connector(false, false), "├── ");
        assert_eq!(continuation(true, false), "|   ");
        assert_eq!(continuation(false, true), "    ");
    }
}
