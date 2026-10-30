use crate::common::regex::bre_to_regex;
use regex::bytes::{Regex as ByteRegex, RegexBuilder as ByteRegexBuilder};
use regex::Regex as NameRegex;
use std::collections::{HashSet, VecDeque};
use std::ffi::OsStr;
use std::fs::{self, DirEntry, File};
use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};

const USAGE: &str = "Usage: grep [OPTIONS] PATTERN [FILE...]\n\nSearch lines in files or standard input (GNU grep compatible subset).\n\nPattern syntax:\n  -G, --basic-regexp        PATTERN is a basic regular expression (default)\n  -E, --extended-regexp     PATTERN is an extended regular expression\n  -F, --fixed-strings       PATTERN is a literal string\n  -P, --perl-regexp         PATTERN is a Perl-style regular expression (no lookaround)\n  -e, --regexp=PATTERN      Use PATTERN (repeatable; newlines separate patterns)\n  -f, --file=FILE           Read patterns from FILE, one per line\n  -i, --ignore-case         Ignore case distinctions (also -y)\n  -w, --word-regexp         Match whole words only\n  -x, --line-regexp         Match whole lines only\n  -v, --invert-match        Select non-matching lines\n\nOutput control:\n  -c, --count               Print a count of selected lines per file\n  -l, --files-with-matches  Print only the names of files with matches\n  -L, --files-without-match Print only the names of files without matches\n  -o, --only-matching       Print only the matched parts, one per line\n  -q, --quiet, --silent     Suppress output; exit 0 on the first match\n  -s, --no-messages         Suppress error messages about files\n  -m, --max-count=NUM       Stop after NUM selected lines per file\n  -n, --line-number         Prefix lines with their line number\n  -H, --with-filename       Always print the file name\n  -h, --no-filename         Never print the file name\n  -A, --after-context=NUM   Print NUM lines of trailing context\n  -B, --before-context=NUM  Print NUM lines of leading context\n  -C, --context=NUM        Print NUM lines of context (also -NUM)\n      --line-buffered      Flush output after each line\n      --color[=WHEN]        Accepted for compatibility; output is never colored\n\nFile selection:\n  -r, --recursive           Recurse into directories (symlinks are not followed)\n  -R, --dereference-recursive  Recurse into directories, following symlinks\n      --include=GLOB        Search only files whose base name matches GLOB\n      --exclude=GLOB        Skip files whose base name matches GLOB\n      --exclude-dir=GLOB    Skip directories whose base name matches GLOB\n  -a, --text               Treat binary files as text\n  -I                        Ignore binary files\n      --binary-files=TYPE   TYPE is binary, text or without-match\n  -U, --binary              Accepted; no effect on this platform\n      --                    End option parsing\n      --help                Show this help\n\nGlobs support *, ?, [...] and {a,b}. Binary matches print 'Binary file NAME matches'.\nExit status: 0 when any line is selected (with -L, when a file is listed), 1 when none are,\n2 on error. With -q, a match gives 0 even if an error occurred.\n";

const BINARY_PROBE_BYTES: u64 = 8192;
const STDIN_LABEL: &str = "(standard input)";
const STDOUT_LABEL: &str = "(standard output)";
const VALUE_OPTIONS: [&str; 10] = [
    "regexp",
    "file",
    "max-count",
    "after-context",
    "before-context",
    "context",
    "include",
    "exclude",
    "exclude-dir",
    "binary-files",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Syntax {
    #[default]
    Basic,
    Extended,
    Perl,
    Fixed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Recurse {
    #[default]
    Off,
    /// `-r`: symbolic links met during recursion are skipped.
    Command,
    /// `-R`: symbolic links are followed.
    All,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BinaryMode {
    #[default]
    Match,
    Text,
    Skip,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Listing {
    #[default]
    None,
    Matching,
    NonMatching,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Lines,
    Count,
    Quiet,
    FilesWith,
    FilesWithout,
}

struct Filter {
    include: bool,
    glob: NameRegex,
}

#[derive(Default)]
struct Options {
    patterns: Vec<String>,
    files: Vec<String>,
    syntax: Syntax,
    ignore_case: bool,
    invert: bool,
    word: bool,
    line: bool,
    only: bool,
    quiet: bool,
    no_messages: bool,
    count: bool,
    listing: Listing,
    line_number: bool,
    line_buffered: bool,
    with_filename: Option<bool>,
    max_count: Option<u64>,
    before: usize,
    after: usize,
    recurse: Recurse,
    binary: BinaryMode,
    filters: Vec<Filter>,
    exclude_dirs: Vec<NameRegex>,
}

impl Options {
    fn mode(&self) -> Mode {
        if self.quiet {
            Mode::Quiet
        } else if self.listing == Listing::Matching {
            Mode::FilesWith
        } else if self.listing == Listing::NonMatching {
            Mode::FilesWithout
        } else if self.count {
            Mode::Count
        } else {
            Mode::Lines
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    let Some(options) = (match parse(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("grep: {error}");
            return 2;
        }
    }) else {
        return 0;
    };
    let regex = match build_regex(&options) {
        Ok(regex) => regex,
        Err(error) => {
            eprintln!("grep: {error}");
            return 2;
        }
    };
    let names = show_names(&options);
    let stdout = io::stdout();
    let mut grep = Grep::new(&options, &regex, BufWriter::new(stdout.lock()), names);
    grep.search_all();
    grep.exit_code()
}

// ---------------------------------------------------------------- option parsing

#[derive(Default)]
struct Parser {
    options: Options,
    pattern_given: bool,
    positional: Vec<String>,
    context: Option<u64>,
    before: Option<u64>,
    after: Option<u64>,
}

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let mut parser = Parser::default();
    let mut index = 0;
    let mut options_open = true;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if !options_open {
            parser.positional.push(arg.to_string());
        } else if arg == "--" {
            options_open = false;
        } else if arg == "--help" {
            print!("{USAGE}");
            return Ok(None);
        } else if let Some(long) = arg.strip_prefix("--") {
            parser.long_option(long, args, &mut index)?;
        } else if arg.len() > 1 && arg.starts_with('-') {
            parser.short_options(&arg[1..], args, &mut index)?;
        } else {
            parser.positional.push(arg.to_string());
        }
    }
    parser.finish().map(Some)
}

impl Parser {
    fn finish(mut self) -> Result<Options, String> {
        if !self.pattern_given {
            if self.positional.is_empty() {
                return Err(format!("missing pattern\n{USAGE}"));
            }
            let pattern = self.positional.remove(0);
            self.add_pattern(&pattern);
        }
        let context = self.context.unwrap_or(0);
        self.options.before = to_usize(self.before.unwrap_or(context));
        self.options.after = to_usize(self.after.unwrap_or(context));
        self.options.files = self.positional;
        Ok(self.options)
    }

    fn long_option(
        &mut self,
        text: &str,
        args: &[String],
        index: &mut usize,
    ) -> Result<(), String> {
        let (name, inline) = match text.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (text, None),
        };
        if matches!(name, "color" | "colour") {
            return check_color(inline);
        }
        if VALUE_OPTIONS.contains(&name) {
            let value = match inline {
                Some(value) => value.to_string(),
                None => take_next(args, index, name)?,
            };
            return self.set_value(name, value);
        }
        if inline.is_some() {
            return Err(format!("option '--{name}' doesn't allow an argument"));
        }
        self.set_flag(name)
    }

    fn short_options(
        &mut self,
        cluster: &str,
        args: &[String],
        index: &mut usize,
    ) -> Result<(), String> {
        let mut position = 0;
        while let Some(flag) = cluster[position..].chars().next() {
            let start = position;
            position += flag.len_utf8();
            if flag.is_ascii_digit() {
                // -NUM is the same as -C NUM.
                let end = start
                    + cluster[start..]
                        .bytes()
                        .take_while(u8::is_ascii_digit)
                        .count();
                self.context = Some(number(&cluster[start..end])?);
                position = end;
            } else if let Some(name) = short_value_name(flag) {
                let rest = &cluster[position..];
                let value = if rest.is_empty() {
                    take_next(args, index, name)?
                } else {
                    rest.to_string()
                };
                return self.set_value(name, value);
            } else if flag == 'I' {
                self.options.binary = BinaryMode::Skip;
            } else {
                let name = short_flag_name(flag)?;
                self.set_flag(name)?;
            }
        }
        Ok(())
    }

    fn add_pattern(&mut self, text: &str) {
        self.pattern_given = true;
        self.options
            .patterns
            .extend(text.split('\n').map(str::to_string));
    }

    fn add_pattern_file(&mut self, path: &str) -> Result<(), String> {
        let bytes = if path == "-" {
            let mut buffer = Vec::new();
            io::stdin().read_to_end(&mut buffer).map(|_| buffer)
        } else {
            fs::read(path)
        }
        .map_err(|error| format!("{path}: {}", io_message(&error)))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("{path}: pattern file is not valid UTF-8"))?;
        self.pattern_given = true;
        if text.is_empty() {
            return Ok(());
        }
        let body = text.strip_suffix('\n').unwrap_or(text.as_str());
        self.options
            .patterns
            .extend(body.split('\n').map(str::to_string));
        Ok(())
    }

    fn set_value(&mut self, name: &str, value: String) -> Result<(), String> {
        match name {
            "regexp" => self.add_pattern(&value),
            "file" => self.add_pattern_file(&value)?,
            "max-count" => self.options.max_count = Some(number(&value)?),
            "after-context" => self.after = Some(number(&value)?),
            "before-context" => self.before = Some(number(&value)?),
            "context" => self.context = Some(number(&value)?),
            "include" | "exclude" => {
                let glob = glob_to_regex(&value)?;
                self.options.filters.push(Filter {
                    include: name == "include",
                    glob,
                });
            }
            "exclude-dir" => self.options.exclude_dirs.push(glob_to_regex(&value)?),
            "binary-files" => self.options.binary = binary_mode(&value)?,
            _ => return Err(format!("unknown option: --{name}")),
        }
        Ok(())
    }

    fn set_flag(&mut self, name: &str) -> Result<(), String> {
        let o = &mut self.options;
        match name {
            "ignore-case" => o.ignore_case = true,
            "no-ignore-case" => o.ignore_case = false,
            "invert-match" => o.invert = true,
            "word-regexp" => o.word = true,
            "line-regexp" => o.line = true,
            "only-matching" => o.only = true,
            "quiet" | "silent" => o.quiet = true,
            "no-messages" => o.no_messages = true,
            "count" => o.count = true,
            "files-with-matches" => o.listing = Listing::Matching,
            "files-without-match" => o.listing = Listing::NonMatching,
            "line-number" => o.line_number = true,
            "with-filename" => o.with_filename = Some(true),
            "no-filename" => o.with_filename = Some(false),
            "fixed-strings" => o.syntax = Syntax::Fixed,
            "basic-regexp" => o.syntax = Syntax::Basic,
            "extended-regexp" => o.syntax = Syntax::Extended,
            "perl-regexp" => o.syntax = Syntax::Perl,
            "recursive" => o.recurse = Recurse::Command,
            "dereference-recursive" => o.recurse = Recurse::All,
            "text" => o.binary = BinaryMode::Text,
            "binary" => {}
            "line-buffered" => o.line_buffered = true,
            _ => return Err(format!("unknown option: --{name}")),
        }
        Ok(())
    }
}

fn short_value_name(flag: char) -> Option<&'static str> {
    match flag {
        'e' => Some("regexp"),
        'f' => Some("file"),
        'm' => Some("max-count"),
        'A' => Some("after-context"),
        'B' => Some("before-context"),
        'C' => Some("context"),
        _ => None,
    }
}

fn short_flag_name(flag: char) -> Result<&'static str, String> {
    match flag {
        'i' | 'y' => Ok("ignore-case"),
        'v' => Ok("invert-match"),
        'w' => Ok("word-regexp"),
        'x' => Ok("line-regexp"),
        'o' => Ok("only-matching"),
        'q' => Ok("quiet"),
        's' => Ok("no-messages"),
        'c' => Ok("count"),
        'n' => Ok("line-number"),
        'l' => Ok("files-with-matches"),
        'L' => Ok("files-without-match"),
        'H' => Ok("with-filename"),
        'h' => Ok("no-filename"),
        'F' => Ok("fixed-strings"),
        'E' => Ok("extended-regexp"),
        'G' => Ok("basic-regexp"),
        'P' => Ok("perl-regexp"),
        'r' => Ok("recursive"),
        'R' => Ok("dereference-recursive"),
        'a' => Ok("text"),
        'U' => Ok("binary"),
        _ => Err(format!("unknown option: -{flag}")),
    }
}

fn take_next(args: &[String], index: &mut usize, name: &str) -> Result<String, String> {
    let value = args
        .get(*index)
        .ok_or_else(|| format!("option requires an argument -- '{name}'"))?;
    *index += 1;
    Ok(value.clone())
}

fn check_color(value: Option<&str>) -> Result<(), String> {
    match value {
        None
        | Some("always" | "yes" | "force" | "never" | "no" | "none" | "auto" | "tty" | "if-tty") => {
            Ok(())
        }
        Some(other) => Err(format!("invalid argument '{other}' for '--color'")),
    }
}

fn binary_mode(value: &str) -> Result<BinaryMode, String> {
    match value {
        "binary" => Ok(BinaryMode::Match),
        "text" => Ok(BinaryMode::Text),
        "without-match" => Ok(BinaryMode::Skip),
        _ => Err(format!("invalid argument '{value}' for '--binary-files'")),
    }
}

fn number(text: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|_| format!("invalid number: '{text}'"))
}

fn to_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

// ---------------------------------------------------------------- pattern translation

/// Builds the single matcher. Group 1 always holds the pattern span, which
/// `-o` relies on; `-w` and `-x` wrap that group with boundary checks.
fn build_regex(options: &Options) -> Result<ByteRegex, String> {
    let mut alternatives = Vec::with_capacity(options.patterns.len());
    for pattern in &options.patterns {
        let translated = match options.syntax {
            Syntax::Fixed => regex::escape(pattern),
            Syntax::Basic => bre_to_regex(pattern)
                .map_err(|error| format!("invalid regular expression: {error}"))?,
            Syntax::Extended | Syntax::Perl => pattern.clone(),
        };
        alternatives.push(format!("(?:{translated})"));
    }
    // An empty pattern list matches nothing (for example an empty -f file).
    let body = if alternatives.is_empty() {
        String::from(r"\b\B")
    } else {
        alternatives.join("|")
    };
    let wrapped = if options.line {
        format!("^({body})$")
    } else if options.word {
        format!(r"(?:^|\W)({body})(?:\W|$)")
    } else {
        format!("({body})")
    };
    ByteRegexBuilder::new(&wrapped)
        .case_insensitive(options.ignore_case)
        .build()
        .map_err(|error| match options.syntax {
            Syntax::Perl => format!(
                "invalid or unsupported -P pattern (lookaround and backreferences are not supported): {error}"
            ),
            _ => format!("invalid regular expression: {error}"),
        })
}

/// Translates a glob (`*`, `?`, `[...]`, `{a,b}`) into an anchored regex.
fn glob_to_regex(glob: &str) -> Result<NameRegex, String> {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::from("^");
    let mut in_braces = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => match glob_class_end(&chars, i) {
                Some(end) => {
                    out.push_str(&glob_class(&chars[i..end]));
                    i = end + 1;
                }
                None => out.push_str("\\["),
            },
            '{' if !in_braces => {
                in_braces = true;
                out.push_str("(?:");
            }
            '}' if in_braces => {
                in_braces = false;
                out.push(')');
            }
            ',' if in_braces => out.push('|'),
            '\\' if i < chars.len() => {
                out.push_str(&regex::escape(&chars[i].to_string()));
                i += 1;
            }
            _ => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    if in_braces {
        return Err(format!("invalid glob (unmatched '{{'): {glob}"));
    }
    out.push('$');
    NameRegex::new(&out).map_err(|error| format!("invalid glob {glob}: {error}"))
}

fn glob_class_end(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start;
    if chars.get(j) == Some(&'!') {
        j += 1;
    }
    if chars.get(j) == Some(&']') {
        j += 1;
    }
    chars[j..]
        .iter()
        .position(|&c| c == ']')
        .map(|offset| j + offset)
}

fn glob_class(body: &[char]) -> String {
    let mut class = String::from("[");
    let mut rest = body;
    if rest.first() == Some(&'!') {
        class.push('^');
        rest = &rest[1..];
    }
    for &c in rest {
        match c {
            '[' | ']' | '\\' | '^' | '&' | '~' => {
                class.push('\\');
                class.push(c);
            }
            _ => class.push(c),
        }
    }
    class.push(']');
    class
}

/// Returns each non-empty span matched by the pattern (group 1), scanning left to right.
fn spans(regex: &ByteRegex, line: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut position = 0;
    while position <= line.len() {
        let Some(captures) = regex.captures_at(line, position) else {
            break;
        };
        let Some(group) = captures.get(1) else {
            break;
        };
        if group.start() == group.end() {
            position = group.end() + 1;
        } else {
            found.push((group.start(), group.end()));
            position = group.end();
        }
    }
    found
}

// ---------------------------------------------------------------- searching

#[derive(Default)]
struct ScanState {
    lineno: u64,
    selected: u64,
    last_printed: Option<u64>,
    before: VecDeque<(u64, Vec<u8>)>,
    after_left: usize,
    limit_reached: bool,
}

enum EntryKind {
    Directory,
    File,
}

struct Grep<'a, W: Write> {
    opts: &'a Options,
    regex: &'a ByteRegex,
    mode: Mode,
    show_names: bool,
    out: W,
    group_printed: bool,
    any_selected: bool,
    any_listed: bool,
    had_error: bool,
    halted: bool,
    visited: HashSet<PathBuf>,
}

impl<'a, W: Write> Grep<'a, W> {
    fn new(opts: &'a Options, regex: &'a ByteRegex, out: W, show_names: bool) -> Self {
        Self {
            opts,
            regex,
            mode: opts.mode(),
            show_names,
            out,
            group_printed: false,
            any_selected: false,
            any_listed: false,
            had_error: false,
            halted: false,
            visited: HashSet::new(),
        }
    }

    fn search_all(&mut self) {
        let opts = self.opts;
        if opts.files.is_empty() {
            if opts.recurse == Recurse::Off {
                self.search_stdin();
            } else {
                // Without operands, names are shown relative to "." (no "./" prefix).
                self.walk(Path::new("."), Path::new(""));
            }
        } else {
            for operand in &opts.files {
                if self.halted {
                    break;
                }
                self.search_operand(operand);
            }
        }
        if let Err(error) = self.out.flush() {
            self.report(STDOUT_LABEL, &error);
        }
    }

    fn exit_code(&self) -> i32 {
        if self.opts.quiet && self.any_selected {
            return 0;
        }
        if self.had_error {
            return 2;
        }
        let success = if self.opts.listing == Listing::NonMatching {
            self.any_listed
        } else {
            self.any_selected
        };
        if success {
            0
        } else {
            1
        }
    }

    fn search_operand(&mut self, operand: &str) {
        if operand == "-" {
            self.search_stdin();
            return;
        }
        let path = Path::new(operand);
        let label = crate::display_path(path);
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                if self.opts.recurse == Recurse::Off {
                    self.report_message(&label, "Is a directory");
                } else if !self.dir_excluded(path) {
                    self.walk(path, path);
                }
            }
            Ok(_) => {
                if self.operand_allowed(path) {
                    self.search_file(path, &label);
                }
            }
            Err(error) => self.report(&label, &error),
        }
    }

    fn walk(&mut self, fs_dir: &Path, shown: &Path) {
        let label = crate::display_path(shown);
        if self.opts.recurse == Recurse::All && !self.visited.insert(canonical(fs_dir)) {
            return;
        }
        let mut entries = Vec::new();
        match fs::read_dir(fs_dir) {
            Ok(read) => {
                for item in read {
                    match item {
                        Ok(entry) => entries.push(entry),
                        Err(error) => self.report(&label, &error),
                    }
                }
            }
            Err(error) => {
                self.report(&label, &error);
                return;
            }
        }
        entries.sort_by_key(DirEntry::file_name);
        for entry in entries {
            if self.halted {
                return;
            }
            let name = entry.file_name();
            let child_shown = shown.join(&name);
            let child_label = crate::display_path(&child_shown);
            match self.entry_kind(&entry, &child_label) {
                Some(EntryKind::Directory) => {
                    if !glob_hit(&self.opts.exclude_dirs, &name) {
                        self.walk(&entry.path(), &child_shown);
                    }
                }
                Some(EntryKind::File) if self.name_allowed(&name) => {
                    self.search_file(&entry.path(), &child_label);
                }
                Some(EntryKind::File) | None => {}
            }
        }
    }

    fn entry_kind(&mut self, entry: &DirEntry, label: &str) -> Option<EntryKind> {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                self.report(label, &error);
                return None;
            }
        };
        if !file_type.is_symlink() {
            return classify(file_type);
        }
        if self.opts.recurse != Recurse::All {
            return None;
        }
        match fs::metadata(entry.path()) {
            Ok(meta) => classify(meta.file_type()),
            Err(error) => {
                self.report(label, &error);
                None
            }
        }
    }

    fn dir_excluded(&self, path: &Path) -> bool {
        match path.file_name() {
            Some(name) => glob_hit(&self.opts.exclude_dirs, name),
            None => false,
        }
    }

    fn operand_allowed(&self, path: &Path) -> bool {
        match path.file_name() {
            Some(name) => self.name_allowed(name),
            None => true,
        }
    }

    /// `--include`/`--exclude`: the last matching option wins; when none
    /// matches, a file is included unless the first option is `--include`.
    fn name_allowed(&self, name: &OsStr) -> bool {
        let text = name.to_string_lossy();
        let mut decision = None;
        for filter in &self.opts.filters {
            if filter.glob.is_match(&text) {
                decision = Some(filter.include);
            }
        }
        decision.unwrap_or(!matches!(
            self.opts.filters.first(),
            Some(filter) if filter.include
        ))
    }

    fn search_stdin(&mut self) {
        let stdin = io::stdin();
        let result = self.search_reader(stdin.lock(), STDIN_LABEL);
        if let Err(error) = result {
            self.report(STDIN_LABEL, &error);
        }
    }

    fn search_file(&mut self, path: &Path, label: &str) {
        let result = match File::open(path) {
            Ok(file) => self.search_reader(file, label),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.report(label, &error);
        }
    }

    fn search_reader<R: Read>(&mut self, mut source: R, label: &str) -> io::Result<()> {
        let mut probe = Vec::new();
        (&mut source)
            .take(BINARY_PROBE_BYTES)
            .read_to_end(&mut probe)?;
        let binary = probe.contains(&0);
        if binary && self.opts.binary == BinaryMode::Skip {
            return self.finish(label, 0);
        }
        let treat_binary = binary && self.opts.binary != BinaryMode::Text;
        let reader = BufReader::new(Cursor::new(probe).chain(source));
        let selected = self.scan(reader, label, treat_binary)?;
        self.finish(label, selected)
    }

    fn scan<R: BufRead>(&mut self, mut reader: R, label: &str, binary: bool) -> io::Result<u64> {
        let mut state = ScanState::default();
        if self.opts.max_count == Some(0) {
            return Ok(0);
        }
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            state.lineno += 1;
            if state.limit_reached {
                // After -m is reached, trailing context stops at the next selected line.
                if self.is_selected(&line) || state.after_left == 0 {
                    break;
                }
                state.after_left -= 1;
                self.emit(label, state.lineno, b'-', &line, &mut state.last_printed)?;
                continue;
            }
            if !self.is_selected(&line) {
                self.keep_context(label, &mut state, &line)?;
                continue;
            }
            state.selected += 1;
            if !self.on_selected(label, &line, binary, &mut state)? {
                break;
            }
            state.limit_reached = self.opts.max_count == Some(state.selected);
            if state.limit_reached && state.after_left == 0 {
                break;
            }
        }
        Ok(state.selected)
    }

    fn is_selected(&self, line: &[u8]) -> bool {
        self.regex.is_match(line) != self.opts.invert
    }

    fn on_selected(
        &mut self,
        label: &str,
        line: &[u8],
        binary: bool,
        state: &mut ScanState,
    ) -> io::Result<bool> {
        match self.mode {
            Mode::Count => Ok(true),
            Mode::Lines if binary => {
                writeln!(self.out, "Binary file {label} matches")?;
                Ok(false)
            }
            Mode::Lines => {
                self.print_match(label, line, state)?;
                Ok(true)
            }
            Mode::Quiet | Mode::FilesWith | Mode::FilesWithout => Ok(false),
        }
    }

    fn keep_context(&mut self, label: &str, state: &mut ScanState, line: &[u8]) -> io::Result<()> {
        if self.mode != Mode::Lines || !self.context_on() {
            return Ok(());
        }
        if state.after_left > 0 {
            state.after_left -= 1;
            return self.emit(label, state.lineno, b'-', line, &mut state.last_printed);
        }
        if self.opts.before > 0 {
            if state.before.len() == self.opts.before {
                state.before.pop_front();
            }
            state.before.push_back((state.lineno, line.to_vec()));
        }
        Ok(())
    }

    fn print_match(&mut self, label: &str, line: &[u8], state: &mut ScanState) -> io::Result<()> {
        if self.opts.only {
            for (start, end) in spans(self.regex, line) {
                self.prefix(label, state.lineno, b':')?;
                self.out.write_all(&line[start..end])?;
                self.end_line()?;
            }
            return Ok(());
        }
        let pending: Vec<(u64, Vec<u8>)> = state.before.drain(..).collect();
        for (number, text) in pending {
            self.emit(label, number, b'-', &text, &mut state.last_printed)?;
        }
        self.emit(label, state.lineno, b':', line, &mut state.last_printed)?;
        state.after_left = self.opts.after;
        Ok(())
    }

    fn context_on(&self) -> bool {
        !self.opts.only && (self.opts.before > 0 || self.opts.after > 0)
    }

    fn emit(
        &mut self,
        label: &str,
        number: u64,
        sep: u8,
        text: &[u8],
        last: &mut Option<u64>,
    ) -> io::Result<()> {
        if self.context_on() && self.group_printed {
            let gap = match last {
                None => true,
                Some(previous) => number > *previous + 1,
            };
            if gap {
                self.out.write_all(b"--\n")?;
            }
        }
        self.prefix(label, number, sep)?;
        self.out.write_all(text)?;
        self.end_line()?;
        *last = Some(number);
        self.group_printed = true;
        Ok(())
    }

    fn prefix(&mut self, label: &str, number: u64, sep: u8) -> io::Result<()> {
        if self.show_names {
            write!(self.out, "{label}")?;
            self.out.write_all(&[sep])?;
        }
        if self.opts.line_number {
            write!(self.out, "{number}")?;
            self.out.write_all(&[sep])?;
        }
        Ok(())
    }

    fn end_line(&mut self) -> io::Result<()> {
        self.out.write_all(b"\n")?;
        if self.opts.line_buffered {
            self.out.flush()?;
        }
        Ok(())
    }

    fn finish(&mut self, label: &str, selected: u64) -> io::Result<()> {
        self.any_selected |= selected > 0;
        match self.mode {
            Mode::Count => {
                if self.show_names {
                    write!(self.out, "{label}:")?;
                }
                writeln!(self.out, "{selected}")?;
            }
            Mode::FilesWith if selected > 0 => writeln!(self.out, "{label}")?,
            Mode::FilesWithout if selected == 0 => {
                writeln!(self.out, "{label}")?;
                self.any_listed = true;
            }
            Mode::Quiet if selected > 0 => self.halted = true,
            _ => {}
        }
        Ok(())
    }

    fn report(&mut self, label: &str, error: &io::Error) {
        if error.kind() == io::ErrorKind::BrokenPipe {
            // The reader went away; stop quietly.
            self.had_error = true;
            self.halted = true;
            return;
        }
        self.report_message(label, &io_message(error));
    }

    fn report_message(&mut self, label: &str, message: &str) {
        self.had_error = true;
        if !self.opts.no_messages {
            eprintln!("grep: {label}: {message}");
        }
    }
}

fn classify(file_type: fs::FileType) -> Option<EntryKind> {
    if file_type.is_dir() {
        Some(EntryKind::Directory)
    } else if file_type.is_file() {
        Some(EntryKind::File)
    } else {
        None
    }
}

fn glob_hit(globs: &[NameRegex], name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    globs.iter().any(|glob| glob.is_match(&text))
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn show_names(options: &Options) -> bool {
    options.with_filename.unwrap_or(
        options.files.len() > 1
            || (options.recurse != Recurse::Off
                && (options.files.is_empty()
                    || options.files.iter().any(|file| Path::new(file).is_dir()))),
    )
}

fn io_message(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "No such file or directory".into(),
        io::ErrorKind::PermissionDenied => "Permission denied".into(),
        _ => {
            let text = error.to_string();
            match text.find(" (os error") {
                Some(index) => text[..index].to_string(),
                None => text,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bre(pattern: &str) -> ByteRegex {
        let translated = bre_to_regex(pattern).unwrap();
        ByteRegex::new(&translated).unwrap()
    }

    #[test]
    fn bre_treats_bare_metacharacters_as_literals() {
        assert!(bre("a+b").is_match(b"a+b"));
        assert!(!bre("a+b").is_match(b"aab"));
        assert!(bre("f(x)|y").is_match(b"f(x)|y"));
        assert!(!bre("f(x)|y").is_match(b"y"));
    }

    #[test]
    fn bre_escaped_operators_are_special() {
        assert!(bre(r"a\|b").is_match(b"b"));
        assert!(bre(r"\(ab\)\{2\}").is_match(b"abab"));
        assert!(bre(r"ab\+").is_match(b"abb"));
        assert!(bre(r"ab\?c").is_match(b"ac"));
    }

    #[test]
    fn bre_anchors_and_leading_star_are_positional() {
        assert!(bre("*a").is_match(b"*a"));
        assert!(bre("a^b").is_match(b"a^b"));
        assert!(bre("a$b").is_match(b"a$b"));
        assert!(bre("^x$").is_match(b"x"));
        assert!(!bre("^x$").is_match(b"yx"));
    }

    #[test]
    fn bre_bracket_backslash_is_literal() {
        assert!(bre(r"[\]").is_match(b"\\"));
        assert!(bre("[[:space:]]").is_match(b" "));
    }

    #[test]
    fn bre_rejects_unsupported_constructs() {
        assert!(bre_to_regex(r"\(a\)\1").is_err());
        assert!(bre_to_regex("[a").is_err());
        assert!(bre_to_regex(r"a\<(b").is_err());
        assert!(bre_to_regex(r"\<word\>").is_ok());
    }

    #[test]
    fn glob_supports_braces_classes_and_wildcards() {
        let rust = glob_to_regex("*.{rs,toml}").unwrap();
        assert!(rust.is_match("main.rs"));
        assert!(rust.is_match("Cargo.toml"));
        assert!(!rust.is_match("main.rsx"));
        let class = glob_to_regex("file[!0-9].txt").unwrap();
        assert!(class.is_match("fileA.txt"));
        assert!(!class.is_match("file1.txt"));
        assert!(glob_to_regex("a?c").unwrap().is_match("abc"));
    }

    #[test]
    fn spans_skip_empty_matches() {
        let regex = ByteRegex::new("(a*)").unwrap();
        assert_eq!(spans(&regex, b"baab"), vec![(1, 3)]);
    }
}
