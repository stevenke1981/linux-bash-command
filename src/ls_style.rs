//! Terminal styling for `ls`: name colors from `LS_COLORS` (over GNU's
//! defaults) and the choice between one-per-line, columns and across.
use crate::common::color;
use crate::common::meta::is_executable;
use std::fs;
use std::path::Path;

/// How short-format entries are arranged.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Layout {
    /// Columns on a terminal, one entry per line when piped.
    #[default]
    Auto,
    /// `-C`: fill columns top to bottom.
    Columns,
    /// `-x`: fill rows left to right.
    Across,
    /// `-1`: one entry per line.
    OneLine,
}

/// Type keys with their GNU default SGR. `fi` (plain files) has no default color.
const DEFAULT_KINDS: [(&str, &str); 9] = [
    ("di", "01;34"),
    ("ln", "01;36"),
    ("ex", "01;32"),
    ("fi", ""),
    ("pi", "40;33"),
    ("so", "01;35"),
    ("bd", "40;33;01"),
    ("cd", "40;33;01"),
    ("or", "40;31;01"),
];

const ARCHIVE_EXTENSIONS: [&str; 11] = [
    ".tar", ".tgz", ".gz", ".zip", ".xz", ".bz2", ".7z", ".zst", ".rar", ".deb", ".rpm",
];
const IMAGE_EXTENSIONS: [&str; 8] = [
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".svg", ".webp", ".ico",
];

/// Name colors for one listing. The default value colors nothing.
#[derive(Default)]
pub struct Style {
    on: bool,
    /// Type keys and their SGR: `LS_COLORS` entries replace the defaults.
    kinds: Vec<(String, String)>,
    /// `*.ext` patterns, `LS_COLORS` entries first, then the built-in ones.
    suffixes: Vec<(String, String)>,
}

impl Style {
    /// Builds the palette when `on`; a disabled style leaves names unchanged.
    pub fn new(on: bool) -> Self {
        if !on {
            return Self::default();
        }
        let mut style = Style {
            on,
            kinds: DEFAULT_KINDS
                .iter()
                .map(|(key, sgr)| (key.to_string(), sgr.to_string()))
                .collect(),
            suffixes: Vec::new(),
        };
        if let Some(spec) = std::env::var_os("LS_COLORS") {
            style.apply_ls_colors(&spec.to_string_lossy());
        }
        let builtin = ARCHIVE_EXTENSIONS
            .iter()
            .map(|ext| (*ext, "01;31"))
            .chain(IMAGE_EXTENSIONS.iter().map(|ext| (*ext, "01;35")));
        for (ext, sgr) in builtin {
            style.suffixes.push((ext.to_string(), sgr.to_string()));
        }
        style
    }

    /// Applies `key=sgr` pairs separated by `:`, as the `LS_COLORS` variable holds them.
    /// Unknown keys and values that are not SGR parameters are ignored.
    fn apply_ls_colors(&mut self, spec: &str) {
        for item in spec.split(':') {
            let Some((key, sgr)) = item.split_once('=') else {
                continue;
            };
            if !sgr.bytes().all(|b| b.is_ascii_digit() || b == b';') {
                continue;
            }
            if let Some(pattern) = key.strip_prefix('*') {
                if !pattern.is_empty() {
                    self.suffixes.push((pattern.to_string(), sgr.to_string()));
                }
            } else if let Some(slot) = self.kinds.iter_mut().find(|(name, _)| name == key) {
                slot.1 = sgr.to_string();
            }
        }
    }

    /// Returns `name` wrapped in its color when this style is on.
    pub fn name(&self, name: &str, path: &Path, metadata: &fs::Metadata) -> String {
        if !self.on {
            return name.to_owned();
        }
        color::paint(true, self.sgr_for(name, path, metadata), name)
    }

    fn sgr_for(&self, name: &str, path: &Path, metadata: &fs::Metadata) -> &str {
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            let key = if fs::metadata(path).is_ok() {
                "ln"
            } else {
                "or"
            };
            return self.kind(key);
        }
        if file_type.is_dir() {
            return self.kind("di");
        }
        if let Some(key) = special_kind(&file_type) {
            return self.kind(key);
        }
        if !file_type.is_file() {
            return "";
        }
        if let Some((_, sgr)) = self
            .suffixes
            .iter()
            .find(|(ext, _)| name.ends_with(ext.as_str()))
        {
            return sgr.as_str();
        }
        if is_executable(path, metadata) {
            self.kind("ex")
        } else {
            self.kind("fi")
        }
    }

    fn kind(&self, key: &str) -> &str {
        self.kinds
            .iter()
            .find(|(name, _)| name == key)
            .map_or("", |(_, sgr)| sgr.as_str())
    }
}

#[cfg(unix)]
fn special_kind(file_type: &fs::FileType) -> Option<&'static str> {
    use std::os::unix::fs::FileTypeExt;
    if file_type.is_fifo() {
        Some("pi")
    } else if file_type.is_socket() {
        Some("so")
    } else if file_type.is_block_device() {
        Some("bd")
    } else if file_type.is_char_device() {
        Some("cd")
    } else {
        None
    }
}

#[cfg(not(unix))]
fn special_kind(_file_type: &fs::FileType) -> Option<&'static str> {
    None
}

/// Parses the value of `--color[=WHEN]`; the bare form means `always`, as in GNU ls.
pub fn parse_color(value: Option<&str>) -> Result<color::When, i32> {
    color::parse_when(value, color::When::Always).map_err(|error| {
        eprintln!("ls: {error}");
        2
    })
}

/// Arranges short-format cells. Columns are used when forced by `-C`/`-x`, or on a
/// terminal by default. Piped output stays one entry per line unless forced.
pub fn arrange(cells: Vec<String>, layout: Layout) -> Vec<String> {
    match layout {
        Layout::OneLine => cells,
        Layout::Auto if !color::stdout_is_tty() => cells,
        Layout::Across => color::columns(&cells, width(), true),
        Layout::Auto | Layout::Columns => color::columns(&cells, width(), false),
    }
}

/// Column width: the terminal when stdout is one, else `COLUMNS`, else 80 (as GNU ls).
fn width() -> usize {
    if color::stdout_is_tty() {
        return color::terminal_width();
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|&columns| columns > 0)
        .unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ls_colors_overlay_defaults_and_add_suffixes() {
        let mut style = Style::new(false);
        style.kinds = DEFAULT_KINDS
            .iter()
            .map(|(key, sgr)| (key.to_string(), sgr.to_string()))
            .collect();
        style.apply_ls_colors("di=01;33:*.rs=32:ln=target:bogus=1:*.x=");
        assert_eq!(style.kind("di"), "01;33");
        assert_eq!(style.kind("ln"), "01;36");
        assert_eq!(style.suffixes[0], (".rs".to_string(), "32".to_string()));
        assert_eq!(style.suffixes[1], (".x".to_string(), String::new()));
    }

    #[test]
    fn disabled_style_leaves_names_plain() {
        let style = Style::new(false);
        let metadata = fs::metadata(".").unwrap();
        assert_eq!(style.name("src", Path::new("."), &metadata), "src");
    }
}
