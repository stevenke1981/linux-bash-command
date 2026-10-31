//! GNU grep compatible coloring: the `GREP_COLORS` palette and the escape
//! sequences used around file names, line numbers, separators and matches.
//!
//! Each colored span is written as `ESC[<sgr>m ESC[K <text> ESC[m ESC[K`,
//! matching GNU grep. An empty SGR value disables coloring for that element.
use std::io::{self, Write};

/// SGR strings for each element grep can color.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Matched text in selected lines (`ms`, also set by `mt`).
    pub matched: String,
    /// Whole selected-line text outside matches (`sl`).
    pub selected_line: String,
    /// Whole context-line text (`cx`).
    pub context_line: String,
    /// File names (`fn`).
    pub file: String,
    /// Line numbers (`ln`).
    pub line: String,
    /// Byte offsets (`bn`).
    pub byte: String,
    /// Field separators `:` and `-`, and the `--` group separator (`se`).
    pub separator: String,
}

impl Default for Palette {
    /// GNU grep's built-in defaults.
    fn default() -> Self {
        Self {
            matched: "01;31".into(),
            selected_line: String::new(),
            context_line: String::new(),
            file: "35".into(),
            line: "32".into(),
            byte: "32".into(),
            separator: "36".into(),
        }
    }
}

impl Palette {
    /// The palette for this process: defaults overridden by `GREP_COLORS`.
    pub fn from_env() -> Self {
        let spec = std::env::var("GREP_COLORS").unwrap_or_default();
        Self::parse(&spec)
    }

    /// Applies a `GREP_COLORS` value (`key=sgr:key=sgr:...`) over the defaults.
    /// Unknown keys and boolean capabilities such as `rv` or `ne` are ignored.
    pub fn parse(spec: &str) -> Self {
        let mut palette = Self::default();
        for item in spec.split(':') {
            let Some((key, value)) = item.split_once('=') else {
                continue;
            };
            let value = value.to_string();
            match key {
                "mt" => palette.matched = value,
                "ms" => palette.matched = value,
                "sl" => palette.selected_line = value,
                "cx" => palette.context_line = value,
                "fn" => palette.file = value,
                "ln" => palette.line = value,
                "bn" => palette.byte = value,
                "se" => palette.separator = value,
                _ => {}
            }
        }
        palette
    }
}

/// Writes `text` wrapped in GNU-style SGR escapes, or plainly when `sgr` is empty.
pub fn write_painted<W: Write>(out: &mut W, sgr: &str, text: &[u8]) -> io::Result<()> {
    if sgr.is_empty() {
        return out.write_all(text);
    }
    write!(out, "\x1b[{sgr}m\x1b[K")?;
    out.write_all(text)?;
    out.write_all(b"\x1b[m\x1b[K")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_gnu() {
        let p = Palette::default();
        assert_eq!(p.matched, "01;31");
        assert_eq!(p.file, "35");
        assert_eq!(p.line, "32");
        assert_eq!(p.byte, "32");
        assert_eq!(p.separator, "36");
        assert!(p.selected_line.is_empty() && p.context_line.is_empty());
    }

    #[test]
    fn grep_colors_overrides_known_keys_only() {
        let p = Palette::parse("ms=01;32:fn=34:rv:ne:zz=1:mt=7");
        assert_eq!(p.matched, "7");
        assert_eq!(p.file, "34");
        assert_eq!(p.line, "32");
    }

    #[test]
    fn painted_span_has_gnu_layout() {
        let mut out = Vec::new();
        write_painted(&mut out, "32", b"7").unwrap();
        assert_eq!(out, b"\x1b[32m\x1b[K7\x1b[m\x1b[K");
        let mut plain = Vec::new();
        write_painted(&mut plain, "", b"7").unwrap();
        assert_eq!(plain, b"7");
    }
}
