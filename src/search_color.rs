//! Color policy for the `rg` and `search` commands, modeled on ripgrep.
//!
//! Defaults when coloring is on: paths magenta (`35`), line and column numbers
//! green (`32`), matches bold red (`1;31`). `--colors` replaces individual
//! attributes; `{type}:none` clears a type. Output is only colored when
//! [`common::color::enabled`] says so, so piped output stays plain.

use crate::common::color::{self, When};

/// The four parts of a search result that can be colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Path,
    Line,
    Column,
    Match,
}

impl Part {
    fn index(self) -> usize {
        match self {
            Part::Path => 0,
            Part::Line => 1,
            Part::Column => 2,
            Part::Match => 3,
        }
    }

    fn parse(name: &str) -> Option<Part> {
        Some(match name {
            "path" => Part::Path,
            "line" => Part::Line,
            "column" => Part::Column,
            "match" => Part::Match,
            _ => return None,
        })
    }
}

const COLOR_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// SGR attributes for one part. The default value emits no escape codes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    /// ANSI color index 0..=7 (black..white).
    fg: Option<u8>,
    bg: Option<u8>,
    bold: bool,
    underline: bool,
    intense: bool,
}

impl Style {
    /// The SGR parameter list (without `\x1b[` and `m`), or empty for no style.
    fn sgr(&self) -> String {
        let mut codes: Vec<String> = Vec::new();
        if self.bold {
            codes.push("1".into());
        }
        if self.underline {
            codes.push("4".into());
        }
        let base = if self.intense { 100 } else { 40 };
        if let Some(bg) = self.bg {
            codes.push((base + u16::from(bg)).to_string());
        }
        let base = if self.intense { 90 } else { 30 };
        if let Some(fg) = self.fg {
            codes.push((base + u16::from(fg)).to_string());
        }
        codes.join(";")
    }
}

/// Styles for every [`Part`], indexed by [`Part::index`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Styles([Style; 4]);

impl Default for Styles {
    fn default() -> Self {
        let fg = |color: u8| Style {
            fg: Some(color),
            ..Style::default()
        };
        Styles([
            fg(5),
            fg(2),
            fg(2),
            Style {
                fg: Some(1),
                bold: true,
                ..Style::default()
            },
        ])
    }
}

/// Resolved coloring for one run: whether it is on, and what each part looks like.
#[derive(Clone, Debug)]
pub struct Painter {
    on: bool,
    sgr: [String; 4],
}

impl Painter {
    /// `when` is the parsed `--color` value, or `None` to use the environment default.
    pub fn new(when: Option<When>, styles: Styles) -> Self {
        let on = color::enabled(when);
        let sgr = styles.0.map(|style| style.sgr());
        Self { on, sgr }
    }

    pub fn enabled(&self) -> bool {
        self.on
    }

    /// Appends `text`, wrapped in the part's SGR sequence when coloring is on.
    pub fn paint(&self, out: &mut Vec<u8>, part: Part, text: &[u8]) {
        let sgr = &self.sgr[part.index()];
        color::paint_bytes(out, self.on, sgr, text);
    }
}

/// Parses a `--color WHEN` value. `ansi` is an alias for `always`.
pub fn parse_when(value: &str) -> Result<When, String> {
    match value {
        "never" | "auto" | "always" | "ansi" => {}
        _ => {
            return Err(format!(
                "invalid value '{value}' for --color (expected never, auto, always or ansi)"
            ))
        }
    }
    let normalized = if value == "ansi" { "always" } else { value };
    color::parse_when(Some(normalized), When::Auto)
}

/// Applies one `--colors` specification to `styles`.
///
/// Accepted forms: `{path,line,column,match}:{fg,bg}:{color}`,
/// `{path,line,column,match}:style:{bold,nobold,underline,intense}` and
/// `{path,line,column,match}:none`.
pub fn apply_spec(styles: &mut Styles, spec: &str) -> Result<(), String> {
    let invalid = || format!("invalid color specification: {spec}");
    let parts: Vec<&str> = spec.split(':').collect();
    let part = parts
        .first()
        .and_then(|name| Part::parse(name))
        .ok_or_else(&invalid)?;
    let style = &mut styles.0[part.index()];
    match parts.as_slice() {
        [_, "none"] => *style = Style::default(),
        [_, "fg", color] => style.fg = Some(color_index(color).ok_or_else(&invalid)?),
        [_, "bg", color] => style.bg = Some(color_index(color).ok_or_else(&invalid)?),
        [_, "style", attr] => match *attr {
            "bold" => style.bold = true,
            "nobold" => style.bold = false,
            "underline" => style.underline = true,
            "intense" => style.intense = true,
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    }
    Ok(())
}

fn color_index(name: &str) -> Option<u8> {
    COLOR_NAMES
        .iter()
        .position(|known| *known == name)
        .map(|index| index as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_styles_match_ripgrep() {
        let styles = Styles::default();
        assert_eq!(styles.0[Part::Path.index()].sgr(), "35");
        assert_eq!(styles.0[Part::Line.index()].sgr(), "32");
        assert_eq!(styles.0[Part::Column.index()].sgr(), "32");
        assert_eq!(styles.0[Part::Match.index()].sgr(), "1;31");
    }

    #[test]
    fn spec_replaces_color_and_keeps_other_attributes() {
        let mut styles = Styles::default();
        apply_spec(&mut styles, "match:fg:green").unwrap();
        assert_eq!(styles.0[Part::Match.index()].sgr(), "1;32");
        apply_spec(&mut styles, "match:style:nobold").unwrap();
        assert_eq!(styles.0[Part::Match.index()].sgr(), "32");
        apply_spec(&mut styles, "path:bg:blue").unwrap();
        assert_eq!(styles.0[Part::Path.index()].sgr(), "44;35");
        apply_spec(&mut styles, "path:style:intense").unwrap();
        assert_eq!(styles.0[Part::Path.index()].sgr(), "104;95");
    }

    #[test]
    fn none_clears_a_part() {
        let mut styles = Styles::default();
        apply_spec(&mut styles, "line:none").unwrap();
        assert_eq!(styles.0[Part::Line.index()].sgr(), "");
    }

    #[test]
    fn invalid_specs_are_rejected() {
        let mut styles = Styles::default();
        for spec in [
            "bogus",
            "match:fg:purple",
            "match:style:italic",
            "other:fg:red",
            "match",
            "match:fg",
        ] {
            assert!(apply_spec(&mut styles, spec).is_err(), "{spec}");
        }
    }

    #[test]
    fn when_accepts_ripgrep_values_only() {
        assert_eq!(parse_when("ansi"), Ok(When::Always));
        assert_eq!(parse_when("never"), Ok(When::Never));
        assert!(parse_when("tty").is_err());
        assert!(parse_when("sometimes").is_err());
    }
}
