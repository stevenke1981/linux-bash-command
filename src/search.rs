//! Text search engine shared by the `rg` (ripgrep-compatible subset) and `search` commands.
//!
//! `search` keeps its historical output and error behavior; `rg` accepts ripgrep's flags.

use crate::common::color::{self, When};
use crate::search_color::{self, Painter, Part, Styles};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::WalkBuilder;
use regex::bytes::{Regex, RegexBuilder};
use std::borrow::Cow;
use std::fmt::Display;
use std::fs;
use std::io::{self, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    Search,
    #[default]
    Rg,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Case {
    #[default]
    Sensitive,
    Insensitive,
    Smart,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Lines,
    OnlyMatching,
    Count,
    CountMatches,
    FilesWith,
    FilesWithout,
    Quiet,
    Json,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Path,
    Modified,
    Accessed,
    Created,
}

/// File type names and their globs (a subset of ripgrep's built-in table).
const FILE_TYPES: &[(&str, &[&str])] = &[
    ("c", &["*.c", "*.h"]),
    (
        "cpp",
        &[
            "*.cc", "*.cpp", "*.cxx", "*.h", "*.hh", "*.hpp", "*.hxx", "*.inl",
        ],
    ),
    ("cs", &["*.cs"]),
    ("css", &["*.css", "*.scss"]),
    ("go", &["*.go"]),
    ("html", &["*.htm", "*.html"]),
    ("java", &["*.java"]),
    ("js", &["*.cjs", "*.js", "*.jsx", "*.mjs"]),
    ("json", &["*.json", "*.jsonl"]),
    ("kotlin", &["*.kt", "*.kts"]),
    ("markdown", &["*.markdown", "*.md", "*.mdx"]),
    ("md", &["*.markdown", "*.md", "*.mdx"]),
    ("php", &["*.php", "*.phtml"]),
    ("py", &["*.py", "*.pyi"]),
    ("rb", &["*.gemspec", "*.rb"]),
    ("ruby", &["*.gemspec", "*.rb"]),
    ("rust", &["*.rs"]),
    ("sh", &["*.bash", "*.sh", "*.zsh"]),
    ("sql", &["*.sql"]),
    ("swift", &["*.swift"]),
    ("toml", &["*.toml"]),
    ("ts", &["*.cts", "*.mts", "*.ts", "*.tsx"]),
    ("txt", &["*.txt"]),
    ("xml", &["*.xml", "*.xsd", "*.xsl", "*.xslt"]),
    ("yaml", &["*.yaml", "*.yml"]),
];

#[derive(Default)]
struct Config {
    mode: Mode,
    patterns: Vec<String>,
    pattern_given: bool,
    paths: Vec<String>,
    fixed: bool,
    word: bool,
    line: bool,
    invert: bool,
    case: Case,
    multiline: bool,
    dotall: bool,
    pcre2: bool,
    quiet: bool,
    files_with: bool,
    files_without: bool,
    count: bool,
    count_matches: bool,
    only_matching: bool,
    json: bool,
    files_mode: bool,
    type_list: bool,
    before: Option<usize>,
    after: Option<usize>,
    context: Option<usize>,
    before_lines: usize,
    after_lines: usize,
    no_line_numbers: bool,
    with_filename: Option<bool>,
    column: bool,
    null: bool,
    heading: bool,
    trim: bool,
    max_count: Option<u64>,
    max_columns: Option<usize>,
    max_columns_preview: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
    follow: bool,
    sort: Option<(SortKey, bool)>,
    hidden: bool,
    no_ignore: bool,
    no_ignore_vcs: bool,
    no_ignore_parent: bool,
    unrestricted: u8,
    binary: bool,
    globs: Vec<String>,
    iglobs: Vec<String>,
    types: Vec<String>,
    types_not: Vec<String>,
    glob_case_insensitive: bool,
    no_messages: bool,
    /// `--color WHEN` as given; `None` defers to the environment and the terminal.
    color_when: Option<When>,
    styles: Styles,
    /// True when `--heading` or `--no-heading` was given explicitly.
    heading_set: bool,
}

impl Config {
    fn output(&self) -> Output {
        if self.quiet {
            Output::Quiet
        } else if self.files_without {
            Output::FilesWithout
        } else if self.files_with {
            Output::FilesWith
        } else if self.count_matches {
            Output::CountMatches
        } else if self.count {
            Output::Count
        } else if self.json {
            Output::Json
        } else if self.only_matching {
            Output::OnlyMatching
        } else {
            Output::Lines
        }
    }

    fn name(&self) -> &'static str {
        match self.mode {
            Mode::Search => "search",
            Mode::Rg => "rg",
        }
    }

    /// Every `-g`, `-t` and `-T` glob, in the order given to the override matcher.
    fn glob_list(&self) -> Result<Vec<String>, String> {
        let mut globs = self.globs.clone();
        for name in &self.types {
            globs.extend(type_patterns(name)?.iter().map(|glob| glob.to_string()));
        }
        for name in &self.types_not {
            globs.extend(type_patterns(name)?.iter().map(|glob| format!("!{glob}")));
        }
        Ok(globs)
    }
}

enum Parsed {
    Help,
    Version,
    Run(Box<Config>),
}

fn usage(mode: Mode) {
    if mode == Mode::Search {
        println!("Usage: search [OPTIONS] PATTERN [PATH...]");
        println!("Search text files recursively with a regular expression; use - as a path to search standard input.");
        println!("\nOptions:");
        println!("  -e, --regexp PATTERN  Set the search pattern");
        println!("  -i, --ignore-case     Match without case sensitivity");
        println!("  -n, --line-number     Print matching line numbers (default)");
        println!("      --no-line-number  Omit line numbers");
        println!("  -F, --fixed-strings   Treat the pattern as a literal string");
        println!("  -g, --glob GLOB       Include paths matching a glob (repeatable)");
        println!("      --hidden          Search hidden files and directories");
        println!("      --no-ignore       Do not honor ignore files");
        println!("      --json            Emit one JSON object per matching line");
        println!("  -h, --help            Show this help");
        println!("\nExit status: 0 when matches are found, 1 when none are found, 2 on error.");
        return;
    }
    print!("{RG_USAGE}");
}

const RG_USAGE: &str = "\
Usage: rg [OPTIONS] PATTERN [PATH...]
       rg [OPTIONS] -e PATTERN... [PATH...]
       rg [OPTIONS] --files [PATH...]
       rg --type-list

Search files for lines matching a regular expression (ripgrep-compatible subset).
Without PATH, the current directory is searched (or stdin when stdin is piped).

Matching:
  -e, --regexp PATTERN         Search for PATTERN (repeatable)
  -f, --file FILE              Read patterns from FILE, one per line
  -F, --fixed-strings          Treat patterns as literal strings
  -i, --ignore-case            Case-insensitive matching
  -s, --case-sensitive         Case-sensitive matching (default)
  -S, --smart-case             Insensitive unless the pattern has uppercase letters
  -w, --word-regexp            Match whole words only
  -x, --line-regexp            Match whole lines only
  -v, --invert-match           Select non-matching lines
  -U, --multiline              Let patterns match across lines
      --multiline-dotall       Multiline, and . also matches newlines
  -P, --pcre2                  Accepted; only the Rust regex syntax is available

Output:
  -l, --files-with-matches     Print only names of files with matches
      --files-without-match    Print only names of files without matches
  -c, --count                  Print the number of matching lines per file
      --count-matches          Print the number of matches per file
  -o, --only-matching          Print only the matched text
  -q, --quiet                  Print nothing; exit 0 on the first match
  -n, --line-number            Show line numbers (default)
  -N, --no-line-number         Hide line numbers
      --column                 Show the column of the first match
  -H, --with-filename          Show file names
  -I, --no-filename            Hide file names
      --heading                Group matches under the file name (default on a terminal)
      --no-heading             Print the file name before each line (default when piped)
  -0, --null                   Terminate file names with NUL
  -A, --after-context N        Show N lines after each match
  -B, --before-context N       Show N lines before each match
  -C, --context N              Show N lines before and after each match
      --trim                   Strip leading whitespace from output lines
  -M, --max-columns N          Replace lines longer than N bytes with a notice
      --max-columns-preview    Show a preview of long lines instead of a notice
  -m, --max-count N            Stop after N selected lines per file
      --color WHEN             Color output: never, auto (default), always or ansi
      --colors SPEC            Override a color: {path,line,column,match}:{fg,bg}:NAME,
                               {type}:style:{bold,nobold,underline,intense}, {type}:none
                               (repeatable; defaults: path=35, line=32, column=32, match=1;31)

Files:
      --files                  List the files that would be searched
      --type-list              List the built-in file types
  -t, --type TYPE              Only search files of TYPE (repeatable)
  -T, --type-not TYPE          Skip files of TYPE (repeatable)
  -g, --glob GLOB              Include paths matching GLOB; !GLOB excludes (repeatable)
      --iglob GLOB             Case-insensitive --glob
      --glob-case-insensitive  Make every --glob case-insensitive
  -d, --max-depth N            Descend at most N directory levels
      --max-filesize NUM[K|M|G]  Skip files larger than the limit
  -L, --follow                 Follow symbolic links
      --sort KEY               Sort by path, modified, accessed, created, or none
      --sortr KEY              Like --sort, in reverse order
  -.  --hidden                 Search hidden files and directories
  -u, -uu, -uuu                Disable ignore rules / also hidden / also binary files
      --no-ignore              Do not honor any ignore files
      --no-ignore-vcs          Do not honor .gitignore and related VCS files
      --no-ignore-parent       Do not honor ignore files in parent directories
  -a, --text                   Search binary files as if they were text
      --no-messages            Suppress file error messages
  -j, --threads N              Accepted and ignored
  -h, --help                   Show this help
  -V, --version                Show the version

Exit status: 0 when matches are found, 1 when none are found, 2 on error.
";

fn parse(args: &[String], mode: Mode) -> Result<Parsed, String> {
    let mut cfg = Config {
        mode,
        ..Config::default()
    };
    let mut positional = Vec::new();
    let mut only_positional = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if only_positional || arg == "-" || !arg.starts_with('-') {
            positional.push(arg.to_string());
        } else if arg == "--" {
            only_positional = true;
        } else if arg == "-h" || arg == "--help" {
            return Ok(Parsed::Help);
        } else if arg == "-V" || arg == "--version" {
            return Ok(Parsed::Version);
        } else if let Some(long) = arg.strip_prefix("--") {
            parse_long(&mut cfg, long, args, &mut index)?;
        } else {
            parse_short(&mut cfg, &arg[1..], args, &mut index)?;
        }
    }
    finish(cfg, positional).map(|cfg| Parsed::Run(Box::new(cfg)))
}

fn parse_long(
    cfg: &mut Config,
    long: &str,
    args: &[String],
    index: &mut usize,
) -> Result<(), String> {
    let (name, inline) = match long.split_once('=') {
        Some((name, value)) => (name, Some(value.to_string())),
        None => (long, None),
    };
    if takes_value(name) {
        let value = match inline {
            Some(value) => value,
            None => next_arg(args, index, &format!("--{name}"))?,
        };
        apply_value(cfg, name, value)
    } else if inline.is_some() {
        Err(format!("--{name} does not take a value"))
    } else {
        apply_flag(cfg, name)
    }
}

/// Handles a bundle such as `-inw`, `-C3`, `-uu` or `-A 3`.
fn parse_short(
    cfg: &mut Config,
    bundle: &str,
    args: &[String],
    index: &mut usize,
) -> Result<(), String> {
    for (offset, ch) in bundle.char_indices() {
        let Some((name, takes_value)) = short_option(ch) else {
            return Err(format!("unknown option: -{ch}"));
        };
        if takes_value {
            let rest = &bundle[offset + ch.len_utf8()..];
            let value = if rest.is_empty() {
                next_arg(args, index, &format!("-{ch}"))?
            } else {
                rest.to_string()
            };
            return apply_value(cfg, name, value);
        }
        apply_flag(cfg, name)?;
    }
    Ok(())
}

fn next_arg(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    let value = args
        .get(*index)
        .ok_or_else(|| format!("{flag} requires a value"))?;
    *index += 1;
    Ok(value.clone())
}

fn short_option(ch: char) -> Option<(&'static str, bool)> {
    let option = match ch {
        'e' => ("regexp", true),
        'f' => ("file", true),
        'g' => ("glob", true),
        't' => ("type", true),
        'T' => ("type-not", true),
        'A' => ("after-context", true),
        'B' => ("before-context", true),
        'C' => ("context", true),
        'm' => ("max-count", true),
        'd' => ("max-depth", true),
        'M' => ("max-columns", true),
        'j' => ("threads", true),
        'E' => ("encoding", true),
        'r' => ("replace", true),
        'i' => ("ignore-case", false),
        's' => ("case-sensitive", false),
        'S' => ("smart-case", false),
        'F' => ("fixed-strings", false),
        'w' => ("word-regexp", false),
        'x' => ("line-regexp", false),
        'v' => ("invert-match", false),
        'U' => ("multiline", false),
        'P' => ("pcre2", false),
        'o' => ("only-matching", false),
        'l' => ("files-with-matches", false),
        'c' => ("count", false),
        'q' => ("quiet", false),
        'n' => ("line-number", false),
        'N' => ("no-line-number", false),
        'H' => ("with-filename", false),
        'I' => ("no-filename", false),
        '0' => ("null", false),
        'L' => ("follow", false),
        'u' => ("unrestricted", false),
        'a' => ("text", false),
        '.' => ("hidden", false),
        'z' => ("search-zip", false),
        _ => return None,
    };
    Some(option)
}

fn takes_value(name: &str) -> bool {
    matches!(
        name,
        "regexp"
            | "file"
            | "glob"
            | "iglob"
            | "type"
            | "type-not"
            | "after-context"
            | "before-context"
            | "context"
            | "max-count"
            | "max-depth"
            | "maxdepth"
            | "max-columns"
            | "max-filesize"
            | "threads"
            | "color"
            | "colors"
            | "colour"
            | "sort"
            | "sortr"
            | "replace"
            | "encoding"
    )
}

fn apply_flag(cfg: &mut Config, name: &str) -> Result<(), String> {
    match name {
        "ignore-case" => cfg.case = Case::Insensitive,
        "case-sensitive" => cfg.case = Case::Sensitive,
        "smart-case" => cfg.case = Case::Smart,
        "fixed-strings" => cfg.fixed = true,
        "word-regexp" => cfg.word = true,
        "line-regexp" => cfg.line = true,
        "invert-match" => cfg.invert = true,
        "multiline" => cfg.multiline = true,
        "multiline-dotall" => (cfg.multiline, cfg.dotall) = (true, true),
        "pcre2" => cfg.pcre2 = true,
        "only-matching" => cfg.only_matching = true,
        "files-with-matches" => cfg.files_with = true,
        "files-without-match" => cfg.files_without = true,
        "count" => cfg.count = true,
        "count-matches" => cfg.count_matches = true,
        "quiet" => cfg.quiet = true,
        "json" => cfg.json = true,
        "line-number" => cfg.no_line_numbers = false,
        "no-line-number" => cfg.no_line_numbers = true,
        "with-filename" => cfg.with_filename = Some(true),
        "no-filename" => cfg.with_filename = Some(false),
        "heading" => {
            cfg.heading = true;
            cfg.heading_set = true;
        }
        "no-heading" => {
            cfg.heading = false;
            cfg.heading_set = true;
        }
        "column" => cfg.column = true,
        "null" => cfg.null = true,
        "trim" => cfg.trim = true,
        "max-columns-preview" => cfg.max_columns_preview = true,
        "no-messages" => cfg.no_messages = true,
        "files" => cfg.files_mode = true,
        "type-list" => cfg.type_list = true,
        "hidden" => cfg.hidden = true,
        "no-ignore" => cfg.no_ignore = true,
        "no-ignore-vcs" => cfg.no_ignore_vcs = true,
        "no-ignore-parent" => cfg.no_ignore_parent = true,
        "follow" => cfg.follow = true,
        "glob-case-insensitive" => cfg.glob_case_insensitive = true,
        "unrestricted" => cfg.unrestricted = cfg.unrestricted.saturating_add(1),
        "text" | "binary" => cfg.binary = true,
        "search-zip" => return Err("-z/--search-zip is not supported".into()),
        "debug" | "no-config" => {}
        _ => return Err(format!("unknown option: --{name}")),
    }
    Ok(())
}

fn apply_value(cfg: &mut Config, name: &str, value: String) -> Result<(), String> {
    match name {
        "regexp" => {
            cfg.patterns.push(value);
            cfg.pattern_given = true;
        }
        "file" => {
            cfg.patterns.extend(read_pattern_file(&value)?);
            cfg.pattern_given = true;
        }
        "glob" => cfg.globs.push(value),
        "iglob" => cfg.iglobs.push(value),
        "type" => cfg.types.push(value),
        "type-not" => cfg.types_not.push(value),
        "after-context" => cfg.after = Some(parse_num(name, &value)?),
        "before-context" => cfg.before = Some(parse_num(name, &value)?),
        "context" => cfg.context = Some(parse_num(name, &value)?),
        "max-count" => cfg.max_count = Some(parse_num(name, &value)?),
        "max-depth" | "maxdepth" => cfg.max_depth = Some(parse_num(name, &value)?),
        "max-columns" => cfg.max_columns = Some(parse_num(name, &value)?),
        "max-filesize" => cfg.max_filesize = Some(parse_size(&value)?),
        "sort" => cfg.sort = parse_sort(&value)?.map(|key| (key, false)),
        "sortr" => cfg.sort = parse_sort(&value)?.map(|key| (key, true)),
        "threads" => {
            parse_num::<usize>(name, &value)?;
        }
        "color" | "colour" => cfg.color_when = Some(search_color::parse_when(&value)?),
        "colors" => search_color::apply_spec(&mut cfg.styles, &value)?,
        "replace" | "encoding" => return Err(format!("--{name} is not supported")),
        _ => return Err(format!("unknown option: --{name}")),
    }
    Ok(())
}

fn parse_num<T: std::str::FromStr>(name: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for --{name}: {value}"))
}

/// Parses a byte count such as `500`, `64K`, `2M` or `1G` (powers of 1024).
fn parse_size(value: &str) -> Result<u64, String> {
    let (digits, multiplier) = crate::common::size::split_suffix(
        value,
        &[
            ("k", 1 << 10),
            ("K", 1 << 10),
            ("m", 1 << 20),
            ("M", 1 << 20),
            ("g", 1 << 30),
            ("G", 1 << 30),
        ],
        1,
    );
    digits
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(multiplier))
        .ok_or_else(|| format!("invalid value for --max-filesize: {value}"))
}

fn parse_sort(value: &str) -> Result<Option<SortKey>, String> {
    Ok(match value {
        "path" => Some(SortKey::Path),
        "modified" => Some(SortKey::Modified),
        "accessed" => Some(SortKey::Accessed),
        "created" => Some(SortKey::Created),
        "none" => None,
        _ => return Err(format!("invalid sort key: {value}")),
    })
}

fn read_pattern_file(path: &str) -> Result<Vec<String>, String> {
    let text = if path == "-" {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| format!("standard input: {error}"))?;
        text
    } else {
        fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?
    };
    Ok(text.lines().map(str::to_string).collect())
}

fn finish(mut cfg: Config, mut positional: Vec<String>) -> Result<Config, String> {
    if cfg.unrestricted >= 1 {
        cfg.no_ignore = true;
    }
    if cfg.unrestricted >= 2 {
        cfg.hidden = true;
    }
    if cfg.unrestricted >= 3 {
        cfg.binary = true;
    }
    if cfg.json && cfg.mode == Mode::Rg {
        return Err("--json output is not supported by rg; use agentcmd search --json".into());
    }
    if !cfg.pattern_given && !cfg.files_mode && !cfg.type_list {
        if positional.is_empty() {
            return Err("missing search pattern (try --help)".into());
        }
        cfg.patterns.push(positional.remove(0));
    }
    cfg.paths = positional;
    cfg.before_lines = cfg.before.or(cfg.context).unwrap_or(0);
    cfg.after_lines = cfg.after.or(cfg.context).unwrap_or(0);
    cfg.glob_list()?;
    Ok(cfg)
}

fn type_patterns(name: &str) -> Result<&'static [&'static str], String> {
    FILE_TYPES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, globs)| *globs)
        .ok_or_else(|| format!("unrecognized file type: {name}"))
}

fn print_type_list() {
    let mut types: Vec<_> = FILE_TYPES.to_vec();
    types.sort_by_key(|(name, _)| *name);
    for (name, globs) in types {
        println!("{name}: {}", globs.join(", "));
    }
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Compiled pattern plus the `-w` word-boundary rule, which the regex crate cannot express.
#[derive(Default)]
struct Matcher {
    regex: Option<Regex>,
    word: bool,
}

impl Matcher {
    fn find_at(&self, haystack: &[u8], from: usize) -> Option<(usize, usize)> {
        let regex = self.regex.as_ref()?;
        let mut position = from;
        while position <= haystack.len() {
            let found = regex.find_at(haystack, position)?;
            if !self.word || is_word_bounded(haystack, found.start(), found.end()) {
                return Some((found.start(), found.end()));
            }
            position = found.start() + utf8_width(haystack, found.start());
        }
        None
    }

    /// Every non-empty match, left to right and non-overlapping.
    fn all(&self, haystack: &[u8]) -> Vec<(usize, usize)> {
        let mut matches = Vec::new();
        let mut position = 0;
        while let Some((start, end)) = self.find_at(haystack, position) {
            if end > start {
                matches.push((start, end));
                position = end;
            } else {
                position = start + utf8_width(haystack, start);
            }
        }
        matches
    }

    fn is_match(&self, haystack: &[u8]) -> bool {
        self.find_at(haystack, 0).is_some()
    }
}

fn utf8_width(haystack: &[u8], index: usize) -> usize {
    match haystack.get(index) {
        Some(&byte) if byte >= 0xF0 => 4,
        Some(&byte) if byte >= 0xE0 => 3,
        Some(&byte) if byte >= 0xC0 => 2,
        _ => 1,
    }
}

fn is_word_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn is_word_bounded(haystack: &[u8], start: usize, end: usize) -> bool {
    !word_char_before(haystack, start) && !word_char_at(haystack, end)
}

fn word_char_before(haystack: &[u8], end: usize) -> bool {
    (end.saturating_sub(4)..end)
        .rev()
        .find(|&index| haystack[index] & 0xC0 != 0x80)
        .and_then(|start| std::str::from_utf8(&haystack[start..end]).ok())
        .and_then(|text| text.chars().next_back())
        .is_some_and(is_word_char)
}

fn word_char_at(haystack: &[u8], start: usize) -> bool {
    if start >= haystack.len() {
        return false;
    }
    let end = (start + utf8_width(haystack, start)).min(haystack.len());
    std::str::from_utf8(&haystack[start..end])
        .ok()
        .and_then(|text| text.chars().next())
        .is_some_and(is_word_char)
}

fn has_upper_literal(pattern: &str, fixed: bool) -> bool {
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && !fixed {
            escaped = true;
        } else if ch.is_uppercase() {
            return true;
        }
    }
    false
}

/// True when the pattern contains a newline, which can only match with `-U`.
fn has_newline_escape(pattern: &str, fixed: bool) -> bool {
    if fixed {
        return pattern.contains('\n');
    }
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if bytes.get(index + 1) == Some(&b'n') {
                return true;
            }
            index += 2;
        } else {
            if bytes[index] == b'\n' {
                return true;
            }
            index += 1;
        }
    }
    false
}

fn build_matcher(cfg: &Config) -> Result<Matcher, String> {
    let word = cfg.word && !cfg.line;
    if cfg.patterns.is_empty() {
        return Ok(Matcher { regex: None, word });
    }
    if !cfg.multiline && cfg.mode == Mode::Rg {
        if let Some(pattern) = cfg
            .patterns
            .iter()
            .find(|pattern| has_newline_escape(pattern, cfg.fixed))
        {
            return Err(format!(
                "the literal \"\\n\" in {pattern:?} can only match with -U/--multiline"
            ));
        }
    }
    let insensitive = match cfg.case {
        Case::Insensitive => true,
        Case::Sensitive => false,
        Case::Smart => !cfg
            .patterns
            .iter()
            .any(|pattern| has_upper_literal(pattern, cfg.fixed)),
    };
    let alternatives: Vec<String> = cfg
        .patterns
        .iter()
        .map(|pattern| {
            let body = if cfg.fixed {
                regex::escape(pattern)
            } else {
                pattern.clone()
            };
            let body = if cfg.line {
                format!("^(?:{body})$")
            } else {
                body
            };
            format!("(?:{body})")
        })
        .collect();
    let regex = RegexBuilder::new(&alternatives.join("|"))
        .case_insensitive(insensitive)
        .multi_line(cfg.multiline)
        .dot_matches_new_line(cfg.dotall)
        .build()
        .map_err(|error| {
            if cfg.pcre2 {
                format!("-P/--pcre2 pattern is not supported by this engine: {error}")
            } else {
                format!("invalid regular expression: {error}")
            }
        })?;
    Ok(Matcher {
        regex: Some(regex),
        word,
    })
}

/// Byte ranges of each line, without the trailing `\n`. `strip_cr` drops a trailing `\r`.
fn line_ranges(text: &[u8], strip_cr: bool) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, &byte) in text.iter().enumerate() {
        if byte == b'\n' {
            ranges.push((start, trim_cr(text, start, index, strip_cr)));
            start = index + 1;
        }
    }
    if start < text.len() {
        ranges.push((start, trim_cr(text, start, text.len(), strip_cr)));
    }
    ranges
}

fn trim_cr(text: &[u8], start: usize, end: usize, strip_cr: bool) -> usize {
    if strip_cr && end > start && text[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

/// Index of the line containing byte `position`.
fn line_index(lines: &[(usize, usize)], position: usize) -> usize {
    lines
        .partition_point(|&(start, _)| start <= position)
        .saturating_sub(1)
}

fn selected_indices(selected: &[bool]) -> impl Iterator<Item = usize> + '_ {
    selected
        .iter()
        .enumerate()
        .filter_map(|(index, &chosen)| chosen.then_some(index))
}

fn cap_selected(selected: &mut [bool], limit: Option<u64>) {
    let Some(limit) = limit else { return };
    let mut seen = 0u64;
    for chosen in selected.iter_mut().filter(|chosen| **chosen) {
        if seen >= limit {
            *chosen = false;
        } else {
            seen += 1;
        }
    }
}

fn trim_start(line: &[u8]) -> &[u8] {
    let count = line
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count();
    &line[count..]
}

fn sort_time(path: &Path, key: SortKey) -> Option<SystemTime> {
    let metadata = fs::metadata(path).ok()?;
    match key {
        SortKey::Path => None,
        SortKey::Modified => metadata.modified().ok(),
        SortKey::Accessed => metadata.accessed().ok(),
        SortKey::Created => metadata.created().ok(),
    }
}

fn sort_paths(paths: &mut Vec<PathBuf>, sort: Option<(SortKey, bool)>) {
    let Some((key, reverse)) = sort else { return };
    let mut keyed: Vec<(Option<SystemTime>, PathBuf)> = paths
        .drain(..)
        .map(|path| (sort_time(&path, key), path))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    if reverse {
        keyed.reverse();
    }
    paths.extend(keyed.into_iter().map(|(_, path)| path));
}

fn build_override(root: &Path, globs: &[String], insensitive: bool) -> Result<Override, String> {
    let mut builder = OverrideBuilder::new(root);
    builder
        .case_insensitive(insensitive)
        .map_err(|error| format!("invalid glob: {error}"))?;
    for glob in globs {
        builder
            .add(glob)
            .map_err(|error| format!("invalid glob {glob:?}: {error}"))?;
    }
    builder
        .build()
        .map_err(|error| format!("invalid glob: {error}"))
}

fn build_walker(root: &Path, cfg: &Config) -> Result<WalkBuilder, String> {
    let vcs_ignored = cfg.no_ignore || cfg.no_ignore_vcs;
    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(!cfg.hidden)
        .ignore(!cfg.no_ignore)
        .git_ignore(!vcs_ignored)
        .git_global(!vcs_ignored)
        .git_exclude(!vcs_ignored)
        .parents(!(cfg.no_ignore || cfg.no_ignore_parent))
        .follow_links(cfg.follow)
        .max_depth(cfg.max_depth);
    let globs = cfg.glob_list()?;
    if !globs.is_empty() {
        walker.overrides(build_override(root, &globs, cfg.glob_case_insensitive)?);
    }
    if !cfg.iglobs.is_empty() {
        let iglob_filter = build_override(root, &cfg.iglobs, true)?;
        walker.filter_entry(move |entry| {
            entry.file_type().is_some_and(|kind| kind.is_dir())
                || !iglob_filter.matched(entry.path(), false).is_ignore()
        });
    }
    Ok(walker)
}

fn search_roots(cfg: &Config) -> (Vec<String>, bool) {
    if !cfg.paths.is_empty() {
        return (cfg.paths.clone(), false);
    }
    if cfg.mode == Mode::Rg && !cfg.files_mode && grep_cli::is_readable_stdin() {
        return (vec!["-".to_string()], false);
    }
    (vec![".".to_string()], true)
}

struct Searcher {
    cfg: Config,
    matcher: Matcher,
    show_name: bool,
    painter: Painter,
    out: BufWriter<io::Stdout>,
    printed: bool,
    found: bool,
    had_error: bool,
}

impl Searcher {
    fn new(cfg: Config, matcher: Matcher, show_name: bool) -> Self {
        let painter = Painter::new(cfg.color_when, cfg.styles);
        Self {
            cfg,
            matcher,
            show_name,
            painter,
            out: BufWriter::new(io::stdout()),
            printed: false,
            found: false,
            had_error: false,
        }
    }

    fn status(&self) -> i32 {
        let quiet_hit = self.found && self.cfg.output() == Output::Quiet;
        if self.had_error && !quiet_hit {
            2
        } else if self.found {
            0
        } else {
            1
        }
    }

    fn stop(&self) -> bool {
        self.found && self.cfg.output() == Output::Quiet
    }

    fn record(&mut self, status: bool) {
        self.found |= status;
    }

    fn report(&mut self, message: impl Display) {
        self.had_error = true;
        if !self.cfg.no_messages {
            eprintln!("{}: {message}", self.cfg.name());
        }
    }

    fn label(&self, path: &Path, implicit: bool) -> String {
        match self.cfg.mode {
            Mode::Search => path.display().to_string(),
            Mode::Rg => {
                let path = if implicit {
                    path.strip_prefix(".").unwrap_or(path)
                } else {
                    path
                };
                crate::display_path(path)
            }
        }
    }

    fn run_root(&mut self, root: &str, implicit: bool) -> io::Result<()> {
        if root == "-" {
            return self.search_stdin();
        }
        let path = Path::new(root);
        if path.is_file() {
            let label = self.label(path, false);
            return self.handle_path(path, &label);
        }
        if !path.exists() {
            let message = match self.cfg.mode {
                Mode::Search => format!("{root}: path does not exist"),
                Mode::Rg => format!("{root}: No such file or directory (os error 2)"),
            };
            self.report(message);
            return Ok(());
        }
        for file in self.walk(path) {
            if self.stop() {
                break;
            }
            let label = self.label(&file, implicit);
            self.handle_path(&file, &label)?;
        }
        Ok(())
    }

    fn walk(&mut self, root: &Path) -> Vec<PathBuf> {
        let walker = match build_walker(root, &self.cfg) {
            Ok(walker) => walker,
            Err(error) => {
                self.report(error);
                return Vec::new();
            }
        };
        let mut files = Vec::new();
        for entry in walker.build() {
            match entry {
                Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                    files.push(entry.into_path());
                }
                Ok(_) => {}
                Err(error) => self.report(error),
            }
        }
        sort_paths(&mut files, self.cfg.sort);
        files
    }

    fn handle_path(&mut self, path: &Path, label: &str) -> io::Result<()> {
        if self.cfg.files_mode {
            self.record(true);
            return self.write_name(label);
        }
        self.search_path(path, label)
    }

    fn search_stdin(&mut self) -> io::Result<()> {
        let mut text = Vec::new();
        if let Err(error) = io::stdin().read_to_end(&mut text) {
            self.report(format!("standard input: {error}"));
            return Ok(());
        }
        let label = match self.cfg.mode {
            Mode::Search => "-",
            Mode::Rg => "(standard input)",
        };
        self.search_text(label, &text)
    }

    fn search_path(&mut self, path: &Path, label: &str) -> io::Result<()> {
        if let Some(limit) = self.cfg.max_filesize {
            if fs::metadata(path).is_ok_and(|metadata| metadata.len() > limit) {
                return Ok(());
            }
        }
        match fs::read(path) {
            Ok(text) => self.search_text(label, &text),
            Err(error) => {
                self.report(format!("{label}: {error}"));
                Ok(())
            }
        }
    }

    fn search_text(&mut self, label: &str, text: &[u8]) -> io::Result<()> {
        let binary_at = if self.cfg.binary {
            None
        } else {
            text.iter().position(|&byte| byte == 0)
        };
        if binary_at.is_some() && self.cfg.mode == Mode::Search {
            return Ok(());
        }
        if self.cfg.mode == Mode::Search {
            if let Err(error) = std::str::from_utf8(text) {
                self.report(format!("{label}: invalid UTF-8: {error}"));
                return Ok(());
            }
        }
        let lines = line_ranges(text, self.cfg.mode == Mode::Search);
        let selected = self.select(text, &lines);
        let found = selected.iter().any(|&chosen| chosen);
        let output = self.cfg.output();
        match output {
            Output::Quiet => {}
            Output::FilesWith => {
                if found {
                    self.write_name(label)?;
                }
            }
            Output::FilesWithout => {
                if !found {
                    self.write_name(label)?;
                }
            }
            Output::Count => {
                if found {
                    let count = selected_indices(&selected).count();
                    self.write_count(label, count)?;
                }
            }
            Output::CountMatches => {
                let count = self.count_matches(text, &lines, &selected);
                if count > 0 {
                    self.write_count(label, count)?;
                }
            }
            Output::Json => self.emit_json(label, text, &lines, &selected)?,
            Output::OnlyMatching => {
                self.header(label, found)?;
                self.emit_only(label, text, &lines, &selected)?;
            }
            Output::Lines => match binary_at {
                Some(offset) if found => self.write_binary(label, offset)?,
                Some(_) => {}
                None => {
                    self.header(label, found)?;
                    self.emit_lines(label, text, &lines, &selected)?;
                }
            },
        }
        let status = if output == Output::FilesWithout {
            !found
        } else {
            found
        };
        self.record(status);
        Ok(())
    }

    /// Marks the lines that are selected for output, honoring `-v`, `-U` and `-m`.
    fn select(&self, text: &[u8], lines: &[(usize, usize)]) -> Vec<bool> {
        if lines.is_empty() {
            return Vec::new();
        }
        let mut selected: Vec<bool> = if self.cfg.multiline {
            let mut covered = vec![false; lines.len()];
            for (start, end) in self.matcher.all(text) {
                let first = line_index(lines, start);
                let last = line_index(lines, end - 1);
                covered[first..=last]
                    .iter_mut()
                    .for_each(|flag| *flag = true);
            }
            covered
        } else {
            lines
                .iter()
                .map(|&(start, end)| self.matcher.is_match(&text[start..end]))
                .collect()
        };
        if self.cfg.invert {
            selected.iter_mut().for_each(|flag| *flag = !*flag);
        }
        cap_selected(&mut selected, self.cfg.max_count);
        selected
    }

    fn count_matches(&self, text: &[u8], lines: &[(usize, usize)], selected: &[bool]) -> usize {
        if self.cfg.multiline {
            return self
                .matcher
                .all(text)
                .iter()
                .filter(|(start, _)| selected[line_index(lines, *start)])
                .count();
        }
        selected_indices(selected)
            .map(|index| {
                let (start, end) = lines[index];
                self.matcher.all(&text[start..end]).len()
            })
            .sum()
    }

    fn write_name(&mut self, label: &str) -> io::Result<()> {
        let mut out = Vec::new();
        self.painter.paint(&mut out, Part::Path, label.as_bytes());
        out.push(if self.cfg.null { 0 } else { b'\n' });
        self.out.write_all(&out)
    }

    fn write_count(&mut self, label: &str, count: usize) -> io::Result<()> {
        let mut out = Vec::new();
        if self.show_name {
            self.painter.paint(&mut out, Part::Path, label.as_bytes());
            out.push(if self.cfg.null { 0 } else { b':' });
        }
        out.extend_from_slice(format!("{count}\n").as_bytes());
        self.out.write_all(&out)
    }

    fn write_binary(&mut self, label: &str, offset: usize) -> io::Result<()> {
        let mut out = Vec::new();
        if self.show_name {
            self.painter.paint(&mut out, Part::Path, label.as_bytes());
            out.extend_from_slice(b": ");
        }
        out.extend_from_slice(
            format!("binary file matches (found \"\\0\" byte around offset {offset})\n").as_bytes(),
        );
        self.out.write_all(&out)
    }

    /// With `--heading`, prints the file name once above that file's matches.
    fn header(&mut self, label: &str, found: bool) -> io::Result<()> {
        if found && self.cfg.heading && self.show_name {
            if self.printed {
                writeln!(self.out)?;
            }
            let mut out = Vec::new();
            self.painter.paint(&mut out, Part::Path, label.as_bytes());
            out.push(b'\n');
            self.out.write_all(&out)?;
        }
        Ok(())
    }

    fn prefix(&self, label: &str, line_no: usize, column: Option<usize>, sep: u8) -> Vec<u8> {
        let mut out = Vec::new();
        if self.show_name && !self.cfg.heading {
            self.painter.paint(&mut out, Part::Path, label.as_bytes());
            out.push(if self.cfg.null { 0 } else { sep });
        }
        if !self.cfg.no_line_numbers {
            self.painter
                .paint(&mut out, Part::Line, line_no.to_string().as_bytes());
            out.push(sep);
        }
        if let Some(column) = column {
            self.painter
                .paint(&mut out, Part::Column, column.to_string().as_bytes());
            out.push(sep);
        }
        out
    }

    /// True when `-M` replaces the line with a notice, which must not be highlighted.
    fn is_notice(&self, line: &[u8]) -> bool {
        let line = if self.cfg.trim {
            trim_start(line)
        } else {
            line
        };
        !self.cfg.max_columns_preview && self.cfg.max_columns.is_some_and(|max| line.len() > max)
    }

    /// Appends `body` with every match wrapped in the match style.
    fn push_highlighted(&self, out: &mut Vec<u8>, body: &[u8]) {
        let mut last = 0;
        for (start, end) in self.matcher.all(body) {
            out.extend_from_slice(&body[last..start]);
            self.painter.paint(out, Part::Match, &body[start..end]);
            last = end;
        }
        out.extend_from_slice(&body[last..]);
    }

    fn display_body<'a>(&self, line: &'a [u8], is_match: bool) -> Cow<'a, [u8]> {
        let line = if self.cfg.trim {
            trim_start(line)
        } else {
            line
        };
        let Some(max) = self.cfg.max_columns else {
            return Cow::Borrowed(line);
        };
        if line.len() <= max {
            return Cow::Borrowed(line);
        }
        if self.cfg.max_columns_preview {
            let mut cut = max;
            while cut > 0 && line[cut] & 0xC0 == 0x80 {
                cut -= 1;
            }
            let mut preview = line[..cut].to_vec();
            preview.extend_from_slice(b" [... omitted end of long line]");
            return Cow::Owned(preview);
        }
        if is_match {
            let count = self.matcher.all(line).len();
            Cow::Owned(format!("[Omitted long line with {count} matches]").into_bytes())
        } else {
            Cow::Borrowed(&b"[Omitted long context line]"[..])
        }
    }

    fn write_line(
        &mut self,
        label: &str,
        line: &[u8],
        index: usize,
        is_match: bool,
    ) -> io::Result<()> {
        let column = if is_match && self.cfg.column {
            Some(
                self.matcher
                    .find_at(line, 0)
                    .map_or(1, |(start, _)| start + 1),
            )
        } else {
            None
        };
        let body = self.display_body(line, is_match);
        let mut out = self.prefix(label, index + 1, column, if is_match { b':' } else { b'-' });
        if is_match && self.painter.enabled() && !self.is_notice(line) {
            self.push_highlighted(&mut out, &body);
        } else {
            out.extend_from_slice(&body);
        }
        out.push(b'\n');
        self.out.write_all(&out)?;
        self.printed = true;
        Ok(())
    }

    fn emit_lines(
        &mut self,
        label: &str,
        text: &[u8],
        lines: &[(usize, usize)],
        selected: &[bool],
    ) -> io::Result<()> {
        let before = self.cfg.before_lines;
        let after = self.cfg.after_lines;
        let context = before > 0 || after > 0;
        let mut last: Option<usize> = None;
        for index in selected_indices(selected) {
            let first = index
                .saturating_sub(before)
                .max(last.map_or(0, |line| line + 1));
            let end = (index + after).min(lines.len() - 1);
            if first > end {
                continue;
            }
            let gap = last.is_none_or(|line| first > line + 1);
            if context && self.printed && gap {
                self.out.write_all(b"--\n")?;
            }
            for line in first..=end {
                let (start, stop) = lines[line];
                self.write_line(label, &text[start..stop], line, selected[line])?;
                last = Some(line);
            }
        }
        Ok(())
    }

    /// Writes one `-o` output line. `matched` is false for notices, which are not colored.
    fn write_match(
        &mut self,
        label: &str,
        line_no: usize,
        column: usize,
        bytes: &[u8],
        matched: bool,
    ) -> io::Result<()> {
        let column = self.cfg.column.then_some(column);
        let mut out = self.prefix(label, line_no, column, b':');
        if matched {
            self.painter.paint(&mut out, Part::Match, bytes);
        } else {
            out.extend_from_slice(bytes);
        }
        out.push(b'\n');
        self.printed = true;
        self.out.write_all(&out)
    }

    fn emit_only(
        &mut self,
        label: &str,
        text: &[u8],
        lines: &[(usize, usize)],
        selected: &[bool],
    ) -> io::Result<()> {
        if self.cfg.multiline {
            for (start, end) in self.matcher.all(text) {
                let index = line_index(lines, start);
                if selected[index] {
                    let column = start - lines[index].0 + 1;
                    self.write_match(label, index + 1, column, &text[start..end], true)?;
                }
            }
            return Ok(());
        }
        for index in selected_indices(selected) {
            let (line_start, line_end) = lines[index];
            let line = &text[line_start..line_end];
            if self.cfg.max_columns.is_some_and(|max| line.len() > max) {
                let count = self.matcher.all(line).len();
                let notice = format!("[Omitted long line with {count} matches]");
                self.write_match(label, index + 1, 1, notice.as_bytes(), false)?;
                continue;
            }
            for (start, end) in self.matcher.all(line) {
                self.write_match(label, index + 1, start + 1, &line[start..end], true)?;
            }
        }
        Ok(())
    }

    fn emit_json(
        &mut self,
        label: &str,
        text: &[u8],
        lines: &[(usize, usize)],
        selected: &[bool],
    ) -> io::Result<()> {
        for index in selected_indices(selected) {
            let (start, end) = lines[index];
            let line = String::from_utf8_lossy(&text[start..end]);
            writeln!(
                self.out,
                "{{\"path\":{},\"line_number\":{},\"text\":{}}}",
                json_string(label),
                index + 1,
                json_string(&line)
            )?;
        }
        Ok(())
    }
}

fn execute(mut cfg: Config) -> i32 {
    // On a terminal, group by file (as ripgrep does) unless the user chose explicitly.
    if !cfg.heading_set && color::stdout_is_tty() {
        cfg.heading = true;
    }
    let name = cfg.name();
    if let Err(error) = build_walker(Path::new("."), &cfg) {
        eprintln!("{name}: {error}");
        return 2;
    }
    let matcher = match build_matcher(&cfg) {
        Ok(matcher) => matcher,
        Err(error) => {
            eprintln!("{name}: {error}");
            return 2;
        }
    };
    let (roots, implicit) = search_roots(&cfg);
    let show_name = cfg.with_filename.unwrap_or(
        cfg.mode == Mode::Search
            || roots.len() > 1
            || roots.iter().any(|root| Path::new(root).is_dir()),
    );
    let mut searcher = Searcher::new(cfg, matcher, show_name);
    let mut outcome = Ok(());
    for root in &roots {
        if searcher.stop() {
            break;
        }
        outcome = searcher.run_root(root, implicit);
        if outcome.is_err() {
            break;
        }
    }
    let flushed = searcher.out.flush();
    match outcome.err().or(flushed.err()) {
        Some(error) if error.kind() != ErrorKind::BrokenPipe => {
            eprintln!("{name}: write error: {error}");
            searcher.had_error = true;
        }
        _ => {}
    }
    searcher.status()
}

fn run_impl(args: &[String], mode: Mode) -> i32 {
    let cfg = match parse(args, mode) {
        Ok(Parsed::Help) => {
            usage(mode);
            return 0;
        }
        Ok(Parsed::Version) => {
            println!("agentcmd {} (rg subset)", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        Ok(Parsed::Run(cfg)) => *cfg,
        Err(error) => {
            let name = if mode == Mode::Search { "search" } else { "rg" };
            eprintln!("{name}: {error}");
            return 2;
        }
    };
    if cfg.type_list {
        print_type_list();
        return 0;
    }
    execute(cfg)
}

pub fn run(args: &[String]) -> i32 {
    run_impl(args, Mode::Search)
}

pub fn run_rg(args: &[String]) -> i32 {
    run_impl(args, Mode::Rg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher_for(args: &[&str]) -> Matcher {
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        match parse(&args, Mode::Rg) {
            Ok(Parsed::Run(cfg)) => build_matcher(&cfg).unwrap(),
            _ => panic!("parse failed"),
        }
    }

    #[test]
    fn fixed_string_escapes_regex_metacharacters() {
        let matcher = matcher_for(&["-F", "a.b"]);
        assert!(matcher.is_match(b"xa.by"));
        assert!(!matcher.is_match(b"axby"));
    }

    #[test]
    fn word_option_requires_word_boundaries() {
        let matcher = matcher_for(&["-w", "foo"]);
        assert!(matcher.is_match(b"a foo b"));
        assert!(!matcher.is_match(b"foobar"));
        assert_eq!(matcher.all(b"foo_x foo"), vec![(6, 9)]);
    }

    #[test]
    fn smart_case_is_insensitive_without_uppercase_literals() {
        let lower = matcher_for(&["-S", "abc"]);
        assert!(lower.is_match(b"ABC"));
        let mixed = matcher_for(&["-S", "Abc"]);
        assert!(!mixed.is_match(b"abc"));
        assert!(matcher_for(&["-S", r"\Sabc"]).is_match(b"xabc"));
    }

    #[test]
    fn bundled_short_options_and_attached_values_parse() {
        let args: Vec<String> = ["-inC3", "-m1", "pat"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let Ok(Parsed::Run(cfg)) = parse(&args, Mode::Rg) else {
            panic!("parse failed");
        };
        assert!(matches!(cfg.case, Case::Insensitive));
        assert!(cfg.before_lines == 3 && cfg.after_lines == 3);
        assert_eq!(cfg.max_count, Some(1));
        assert_eq!(cfg.patterns, vec!["pat".to_string()]);
    }

    #[test]
    fn line_ranges_split_on_newlines_only() {
        assert_eq!(line_ranges(b"a\nb", false), vec![(0, 1), (2, 3)]);
        assert_eq!(line_ranges(b"a\n", false), vec![(0, 1)]);
        assert_eq!(line_ranges(b"a\r\n", true), vec![(0, 1)]);
    }

    #[test]
    fn size_suffixes_use_powers_of_1024() {
        assert_eq!(parse_size("2K"), Ok(2048));
        assert_eq!(parse_size("1m"), Ok(1 << 20));
        assert!(parse_size("x").is_err());
    }

    #[test]
    fn json_escapes_control_characters() {
        assert_eq!(json_string("a\n\"b"), "\"a\\n\\\"b\"");
    }
}
