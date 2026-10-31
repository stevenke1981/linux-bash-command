//! Shared terminal color and layout policy for every utility.
//!
//! Resolution order for whether a command colors its stdout:
//! 1. An explicit `--color[=WHEN]` option on the command line.
//! 2. `AGENTCMD_COLOR=auto|always|never` in the environment.
//! 3. `auto`, unless `NO_COLOR` is set (then `never`).
//!
//! `auto` colors only when stdout is a terminal and `TERM` is not `dumb`, so
//! piped or captured output (what agents and scripts read) stays byte-for-byte
//! plain.
use std::io::{self, IsTerminal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Auto,
    Always,
    Never,
}

/// Parses a `WHEN` value as GNU does. `None` means the bare `--color` form,
/// whose meaning differs per utility (`ls`: always, `grep`/`diff`: auto).
pub fn parse_when(value: Option<&str>, bare: When) -> Result<When, String> {
    match value {
        None => Ok(bare),
        Some("auto" | "tty" | "if-tty") => Ok(When::Auto),
        Some("always" | "yes" | "force") => Ok(When::Always),
        Some("never" | "no" | "none") => Ok(When::Never),
        Some(other) => Err(format!("invalid argument '{other}' for '--color'")),
    }
}

/// Splits a `--color`/`--colour` argument into its optional value.
/// Returns `None` when `arg` is not a color option at all.
pub fn color_arg(arg: &str) -> Option<Option<&str>> {
    let rest = arg
        .strip_prefix("--colour")
        .or_else(|| arg.strip_prefix("--color"))?;
    match rest {
        "" => Some(None),
        _ => rest.strip_prefix('=').map(Some),
    }
}

/// Default when the command line has no `--color` option.
pub fn env_default() -> When {
    match std::env::var("AGENTCMD_COLOR").as_deref() {
        Ok("always") => return When::Always,
        Ok("never") => return When::Never,
        _ => {}
    }
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        When::Never
    } else {
        When::Auto
    }
}

/// Resolves the final on/off decision for stdout.
/// `explicit` is the parsed `--color` value, if the user gave one.
pub fn enabled(explicit: Option<When>) -> bool {
    match explicit.unwrap_or_else(env_default) {
        When::Always => {
            ansi_ready();
            true
        }
        When::Never => false,
        When::Auto => stdout_is_tty() && ansi_ready(),
    }
}

/// True when stdout is an interactive terminal (and not `TERM=dumb`).
/// Used for terminal-only formatting such as `ls` columns or `rg` headings.
pub fn stdout_is_tty() -> bool {
    io::stdout().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb")
}

/// Output width for column layouts: `COLUMNS`, then the terminal, then 80.
pub fn terminal_width() -> usize {
    if let Some(cols) = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&c| c > 0)
    {
        return cols;
    }
    match crossterm::terminal::size() {
        Ok((cols, _)) if cols > 0 => cols as usize,
        _ => 80,
    }
}

/// Wraps `text` in an SGR sequence when `on`; returns it unchanged otherwise.
pub fn paint(on: bool, sgr: &str, text: &str) -> String {
    if on && !sgr.is_empty() {
        format!("\x1b[{sgr}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

/// Byte-slice form of [`paint`], appending into `out`.
pub fn paint_bytes(out: &mut Vec<u8>, on: bool, sgr: &str, text: &[u8]) {
    if on && !sgr.is_empty() {
        out.extend_from_slice(b"\x1b[");
        out.extend_from_slice(sgr.as_bytes());
        out.push(b'm');
        out.extend_from_slice(text);
        out.extend_from_slice(b"\x1b[0m");
    } else {
        out.extend_from_slice(text);
    }
}

/// Visible width of `text`, ignoring ANSI SGR escape sequences.
pub fn visible_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if !c.is_control() {
            width += char_width(c);
        }
    }
    width
}

/// Approximate terminal cell width: CJK and fullwidth forms take two cells.
fn char_width(c: char) -> usize {
    let cp = c as u32;
    let wide = matches!(cp,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD);
    if wide {
        2
    } else {
        1
    }
}

/// Lays out `cells` in GNU `ls -C` (down columns) or `-x` (across) order,
/// fitting `width` with two spaces between columns. Cells may contain SGR codes.
pub fn columns(cells: &[String], width: usize, across: bool) -> Vec<String> {
    if cells.is_empty() {
        return Vec::new();
    }
    let widths: Vec<usize> = cells.iter().map(|c| visible_width(c)).collect();
    let n = cells.len();
    let mut chosen = (n, vec![widths.iter().copied().max().unwrap_or(0)]);
    for cols in (1..=n).rev() {
        let rows = n.div_ceil(cols);
        let cols = n.div_ceil(rows);
        let mut col_widths = vec![0; cols];
        for (i, w) in widths.iter().enumerate() {
            let col = if across { i % cols } else { i / rows };
            col_widths[col] = col_widths[col].max(*w);
        }
        let total: usize = col_widths.iter().sum::<usize>() + 2 * (cols - 1);
        if total <= width || cols == 1 {
            chosen = (rows, col_widths);
            break;
        }
    }
    let (rows, col_widths) = chosen;
    let cols = col_widths.len();
    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut line = String::new();
        for (col, col_width) in col_widths.iter().enumerate() {
            let index = if across {
                row * cols + col
            } else {
                col * rows + row
            };
            let Some(cell) = cells.get(index) else {
                continue;
            };
            let next = if across { index + 1 } else { index + rows };
            line.push_str(cell);
            if col + 1 < cols && next < n {
                line.push_str(&" ".repeat(col_width - widths[index] + 2));
            }
        }
        lines.push(line);
    }
    lines
}

/// Enables ANSI escapes on legacy Windows consoles; always true elsewhere.
#[cfg(windows)]
fn ansi_ready() -> bool {
    crossterm::ansi_support::supports_ansi()
}

#[cfg(not(windows))]
fn ansi_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_when_values() {
        assert_eq!(parse_when(None, When::Always), Ok(When::Always));
        assert_eq!(parse_when(Some("tty"), When::Never), Ok(When::Auto));
        assert_eq!(parse_when(Some("none"), When::Auto), Ok(When::Never));
        assert!(parse_when(Some("rainbow"), When::Auto).is_err());
    }

    #[test]
    fn splits_color_args() {
        assert_eq!(color_arg("--color"), Some(None));
        assert_eq!(color_arg("--colour=never"), Some(Some("never")));
        assert_eq!(color_arg("--colors"), None);
        assert_eq!(color_arg("--count"), None);
    }

    #[test]
    fn width_skips_escapes_and_counts_wide_chars() {
        assert_eq!(visible_width("\x1b[01;34mdir\x1b[0m"), 3);
        assert_eq!(visible_width("中文"), 4);
    }

    #[test]
    fn columns_fill_down_then_across() {
        let cells: Vec<String> = ["a", "bb", "c", "dd", "e"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(columns(&cells, 10, false), vec!["a   c   e", "bb  dd"]);
        assert_eq!(columns(&cells, 10, true), vec!["a   bb  c", "dd  e"]);
        assert_eq!(columns(&cells, 1, false).len(), 5);
    }
}
