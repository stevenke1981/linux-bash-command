//! `tree`: lists directories as an indented tree, a subset of tree 2.1. Entries are shown in
//! byte order of their names unless a sort option is given. Symlinks are shown as
//! `name -> target` and not followed. The default connectors are box-drawing characters;
//! `--charset=ascii` switches to `|--` style (agentcmd does not read the locale).
//!
//! Submodules: `model` (scan and filters), `sort`, `glob` (`-I`/`-P` patterns), `render` (text)
//! and `json` (`-J`).

mod glob;
mod json;
mod model;
mod render;
mod sort;

use std::io::Write;

use self::glob::Patterns;
use self::model::{scan_root, tally, Filter};
use self::render::{render_text, summary_line, SizeStyle, View};
use self::sort::{Key, Order};
use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const DEFAULT_TIME_FORMAT: &str = "%b %e %H:%M";

const HELP: &str = r#"Usage: tree [OPTION]... [DIRECTORY]...
List the contents of directories in a tree-like format.

Listing options:
  -a                  all files are listed (hidden names included)
  -d                  list directories only
  -L LEVEL            descend only LEVEL directories deep
  -f                  print the path of each entry, not only its name
  -F                  append / to directories, * to executables, | to FIFOs, = to sockets
  -I PATTERN          do not list entries matching PATTERN (| separates patterns)
  -P PATTERN          list only files matching PATTERN (| separates patterns)
      --matchdirs     directory names are also matched against -P, and a matching
                      empty directory survives --prune
      --ignore-case   ignore case when matching -I and -P
      --prune         omit directories left empty after filtering
      --filelimit=N   do not descend into directories with more than N entries
      --gitignore     skip entries matched by .gitignore files
  -i                  do not print indentation lines
  -n                  accepted; output is never colored

File information:
  -p                  print the file mode
  -s                  print the size in bytes
  -h                  print the size in human-readable units (K, M, G)
      --si            like -h, with units of 1000
  -D                  print the modification time
      --timefmt=FMT   strftime format for -D (default "%b %e %H:%M")

Sorting:
  -t                  sort by modification time, newest first
  -v                  sort by natural version order
  -U                  do not sort
  -r                  reverse the order
      --sort=WORD     name, version, size, mtime or ctime
      --dirsfirst     list directories before files
      --filesfirst    list files before directories

Graphics and output:
      --charset=CS    ascii or utf-8 connectors (default utf-8)
      --noreport      omit the directory and file count at the end
  -J                  print the tree as JSON
      --help          display this help and exit
      --version       output version information and exit

Exit status: 0 on success, 1 for invalid options or values, 2 if a directory
operand does not exist.
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("all", Some('a'), None),
    OptSpec::flag("dirs", Some('d'), None),
    OptSpec::flag("full", Some('f'), None),
    OptSpec::flag("classify", Some('F'), None),
    OptSpec::req("level", Some('L'), None),
    OptSpec::req("ignore", Some('I'), None),
    OptSpec::req("include", Some('P'), None),
    OptSpec::flag("matchdirs", None, Some("matchdirs")),
    OptSpec::flag("ignore-case", None, Some("ignore-case")),
    OptSpec::flag("prune", None, Some("prune")),
    OptSpec::req("filelimit", None, Some("filelimit")),
    OptSpec::flag("gitignore", None, Some("gitignore")),
    OptSpec::flag("indent-off", Some('i'), None),
    OptSpec::flag("accept-color", Some('n'), None),
    OptSpec::flag("perms", Some('p'), None),
    OptSpec::flag("bytes", Some('s'), None),
    OptSpec::flag("human", Some('h'), None),
    OptSpec::flag("si", None, Some("si")),
    OptSpec::flag("date", Some('D'), None),
    OptSpec::req("timefmt", None, Some("timefmt")),
    OptSpec::flag("mtime-sort", Some('t'), None),
    OptSpec::flag("version-sort", Some('v'), None),
    OptSpec::flag("unsorted", Some('U'), None),
    OptSpec::flag("reverse", Some('r'), None),
    OptSpec::req("sort", None, Some("sort")),
    OptSpec::flag("dirsfirst", None, Some("dirsfirst")),
    OptSpec::flag("filesfirst", None, Some("filesfirst")),
    OptSpec::req("charset", None, Some("charset")),
    OptSpec::flag("noreport", None, Some("noreport")),
    OptSpec::flag("json", Some('J'), None),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Every setting the command line can change.
struct Settings {
    all: bool,
    dirs_only: bool,
    full_path: bool,
    classify: bool,
    max_depth: Option<usize>,
    ignore: Option<String>,
    include: Option<String>,
    matchdirs: bool,
    ignore_case: bool,
    prune: bool,
    filelimit: Option<usize>,
    gitignore: bool,
    indent: bool,
    perms: bool,
    bytes: bool,
    human: bool,
    si: bool,
    time_format: Option<String>,
    key: Key,
    reverse: bool,
    dirs_first: bool,
    files_first: bool,
    ascii: bool,
    noreport: bool,
    json: bool,
    roots: Vec<String>,
}

impl Settings {
    fn new() -> Self {
        Self {
            all: false,
            dirs_only: false,
            full_path: false,
            classify: false,
            max_depth: None,
            ignore: None,
            include: None,
            matchdirs: false,
            ignore_case: false,
            prune: false,
            filelimit: None,
            gitignore: false,
            indent: true,
            perms: false,
            bytes: false,
            human: false,
            si: false,
            time_format: None,
            key: Key::Name,
            reverse: false,
            dirs_first: false,
            files_first: false,
            ascii: false,
            noreport: false,
            json: false,
            roots: Vec::new(),
        }
    }

    fn size_style(&self) -> SizeStyle {
        if self.si {
            SizeStyle::HumanSi
        } else if self.human {
            SizeStyle::Human
        } else if self.bytes {
            SizeStyle::Bytes
        } else {
            SizeStyle::None
        }
    }

    fn filter(&self) -> Filter {
        Filter {
            all: self.all,
            dirs_only: self.dirs_only,
            max_depth: self.max_depth,
            ignore: self
                .ignore
                .as_ref()
                .map(|text| Patterns::new(text, self.ignore_case)),
            include: self
                .include
                .as_ref()
                .map(|text| Patterns::new(text, self.ignore_case)),
            matchdirs: self.matchdirs,
            prune: self.prune,
            filelimit: self.filelimit,
            gitignore: self.gitignore,
            order: Order {
                key: self.key,
                reverse: self.reverse,
                dirs_first: self.dirs_first,
                files_first: self.files_first,
            },
        }
    }

    fn view(&self) -> View {
        View {
            ascii: self.ascii,
            indent: self.indent,
            full_path: self.full_path,
            classify: self.classify,
            perms: self.perms,
            size: self.size_style(),
            time: self.time_format.clone(),
        }
    }
}

fn usage_error(message: &str) -> i32 {
    eprintln!("tree: {message}\nTry 'tree --help' for more information.");
    EXIT_FAIL
}

fn print_help() -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(HELP.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("tree: write error: {error}");
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output("tree", &mut out)
}

/// Appends a pattern; repeated `-I` or `-P` options are alternatives.
fn append_pattern(slot: &mut Option<String>, text: String) {
    match slot {
        Some(existing) => {
            existing.push('|');
            existing.push_str(&text);
        }
        None => *slot = Some(text),
    }
}

fn parse_sort_word(word: &str) -> Result<Key, i32> {
    match word {
        "name" => Ok(Key::Name),
        "version" => Ok(Key::Version),
        "size" => Ok(Key::Size),
        "mtime" => Ok(Key::Mtime),
        "ctime" => Ok(Key::Ctime),
        other => Err(usage_error(&format!("sort type '{other}' not valid"))),
    }
}

fn parse_positive(text: &str) -> Result<usize, i32> {
    match text.parse::<usize>() {
        Ok(value) if value > 0 => Ok(value),
        _ => {
            eprintln!("tree: Invalid level, must be greater than 0.");
            Err(EXIT_FAIL)
        }
    }
}

fn parse_count(text: &str) -> Result<usize, i32> {
    text.parse::<usize>()
        .map_err(|_| usage_error(&format!("invalid number of entries: '{text}'")))
}

/// Applies one parsed option to the settings. Returns an exit status on invalid values.
fn apply(settings: &mut Settings, id: &str, value: Option<String>) -> Result<(), i32> {
    let text = value.unwrap_or_default();
    match id {
        "all" => settings.all = true,
        "dirs" => settings.dirs_only = true,
        "full" => settings.full_path = true,
        "classify" => settings.classify = true,
        "level" => settings.max_depth = Some(parse_positive(&text)?),
        "ignore" => append_pattern(&mut settings.ignore, text),
        "include" => append_pattern(&mut settings.include, text),
        "matchdirs" => settings.matchdirs = true,
        "ignore-case" => settings.ignore_case = true,
        "prune" => settings.prune = true,
        "filelimit" => settings.filelimit = Some(parse_count(&text)?),
        "gitignore" => settings.gitignore = true,
        "indent-off" => settings.indent = false,
        "perms" => settings.perms = true,
        "bytes" => settings.bytes = true,
        "human" => settings.human = true,
        "si" => settings.si = true,
        "date" => settings.time_format = Some(DEFAULT_TIME_FORMAT.to_string()),
        "timefmt" => settings.time_format = Some(text),
        "mtime-sort" => settings.key = Key::Mtime,
        "version-sort" => settings.key = Key::Version,
        "unsorted" => settings.key = Key::Unsorted,
        "reverse" => settings.reverse = true,
        "sort" => settings.key = parse_sort_word(&text)?,
        "dirsfirst" => {
            settings.dirs_first = true;
            settings.files_first = false;
        }
        "filesfirst" => {
            settings.files_first = true;
            settings.dirs_first = false;
        }
        "charset" => settings.ascii = parse_charset(&text)?,
        "noreport" => settings.noreport = true,
        "json" => settings.json = true,
        _ => {}
    }
    Ok(())
}

/// `--charset`: returns true for ASCII connectors.
fn parse_charset(text: &str) -> Result<bool, i32> {
    match text {
        "ascii" => Ok(true),
        "utf-8" | "utf8" => Ok(false),
        other => Err(usage_error(&format!("invalid charset '{other}'"))),
    }
}

fn parse_settings(args: &[String]) -> Result<Result<Settings, i32>, i32> {
    let items = match Parser::new("tree", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return Err(report_usage("tree", &error, EXIT_FAIL)),
    };
    let mut settings = Settings::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return Ok(Err(print_help())),
            Item::Opt { id: "version", .. } => return Ok(Err(print_version("tree"))),
            Item::Opt { id, value } => apply(&mut settings, id, value)?,
            Item::Operand(text) => settings.roots.push(text),
        }
    }
    if settings.roots.is_empty() {
        settings.roots.push(".".to_string());
    }
    Ok(Ok(settings))
}

fn write_out(text: &str) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("tree: write error: {error}");
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output("tree", &mut out)
}

/// Builds the complete output for the roots, in the requested format.
fn produce(settings: &Settings, roots: &[model::Node]) -> String {
    let counts = tally(roots);
    if settings.json {
        let report = (!settings.noreport).then_some((counts, settings.dirs_only));
        return json::render_json(roots, settings.size_style() != SizeStyle::None, report);
    }
    let mut text = render_text(roots, &settings.view());
    if !settings.noreport {
        text.push('\n');
        text.push_str(&summary_line(counts, settings.dirs_only));
        text.push('\n');
    }
    text
}

pub fn run(args: &[String]) -> i32 {
    let settings = match parse_settings(args) {
        Ok(Ok(settings)) => settings,
        Ok(Err(code)) | Err(code) => return code,
    };
    let filter = settings.filter();
    let roots: Vec<model::Node> = settings
        .roots
        .iter()
        .map(|root| scan_root(root, &filter))
        .collect();
    let status = if roots.iter().any(|root| root.missing) {
        EXIT_TROUBLE
    } else {
        EXIT_OK
    };
    let text = produce(&settings, &roots);
    let written = write_out(&text);
    if written != EXIT_OK {
        return written;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn level_must_be_positive() {
        assert_eq!(parse_positive("3"), Ok(3));
        assert!(parse_positive("0").is_err());
        assert!(parse_positive("x").is_err());
    }

    #[test]
    fn charset_and_sort_words_are_checked() {
        assert_eq!(parse_charset("ascii"), Ok(true));
        assert_eq!(parse_charset("utf-8"), Ok(false));
        assert!(parse_charset("latin1").is_err());
        assert_eq!(parse_sort_word("size").ok(), Some(Key::Size));
        assert!(parse_sort_word("color").is_err());
    }

    #[test]
    fn repeated_patterns_are_alternatives() {
        let mut slot = None;
        append_pattern(&mut slot, "a".into());
        append_pattern(&mut slot, "b".into());
        assert_eq!(slot.as_deref(), Some("a|b"));
    }

    #[test]
    fn size_style_prefers_si_then_human_then_bytes() {
        let mut settings = Settings::new();
        assert_eq!(settings.size_style(), SizeStyle::None);
        settings.bytes = true;
        assert_eq!(settings.size_style(), SizeStyle::Bytes);
        settings.human = true;
        assert_eq!(settings.size_style(), SizeStyle::Human);
        settings.si = true;
        assert_eq!(settings.size_style(), SizeStyle::HumanSi);
    }

    #[test]
    fn defaults_list_the_current_directory() {
        let parsed = parse_settings(&args(&["-a"]))
            .ok()
            .and_then(Result::ok)
            .unwrap();
        assert_eq!(parsed.roots, vec![".".to_string()]);
        assert!(parsed.all);
        assert!(parsed.indent);
    }

    #[test]
    fn help_text_mentions_every_long_option() {
        for option in [
            "--prune",
            "--filelimit",
            "--matchdirs",
            "--gitignore",
            "--charset",
            "--sort",
        ] {
            assert!(HELP.contains(option), "{option}");
        }
    }
}
