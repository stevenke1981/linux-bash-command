//! Option parsing for grep: GNU-style short clusters, long options and their values.

use super::files::{io_message, read_glob_file};
use super::pattern::glob_to_regex;
use super::{BinaryMode, Devices, Filter, Listing, Options, Recurse, Syntax, USAGE};
use std::fs;
use std::io::{self, Read};

/// Short letters whose option takes a value (`-e PAT`, `-d ACTION`).
pub(super) const VALUE_SHORTS: &str = "efmABCdD";

/// Long options whose value may be given as a separate argument.
pub(super) const VALUE_LONGS: &[&str] = &[
    "after-context",
    "before-context",
    "binary-files",
    "context",
    "devices",
    "directories",
    "exclude",
    "exclude-dir",
    "exclude-from",
    "file",
    "include",
    "label",
    "max-count",
    "regexp",
];

/// Every long option name, used to expand unique prefixes such as `--null-d`.
pub(super) const LONG_OPTIONS: &[&str] = &[
    "after-context",
    "basic-regexp",
    "before-context",
    "binary",
    "binary-files",
    "byte-offset",
    "color",
    "context",
    "count",
    "dereference-recursive",
    "devices",
    "directories",
    "exclude",
    "exclude-dir",
    "exclude-from",
    "extended-regexp",
    "file",
    "files-with-matches",
    "files-without-match",
    "fixed-strings",
    "help",
    "ignore-case",
    "include",
    "initial-tab",
    "invert-match",
    "label",
    "line-buffered",
    "line-number",
    "line-regexp",
    "max-count",
    "no-filename",
    "no-ignore-case",
    "no-messages",
    "null",
    "null-data",
    "only-matching",
    "perl-regexp",
    "quiet",
    "recursive",
    "regexp",
    "silent",
    "text",
    "with-filename",
    "word-regexp",
];

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

pub(super) fn parse(args: &[String]) -> Result<Option<Options>, String> {
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
        if VALUE_LONGS.contains(&name) {
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
            "exclude-from" => {
                for glob in read_glob_file(&value)? {
                    self.options.filters.push(Filter {
                        include: false,
                        glob: glob_to_regex(&glob)?,
                    });
                }
            }
            "binary-files" => self.options.binary = binary_mode(&value)?,
            "label" => self.options.label = Some(value),
            "directories" => self.set_directories(&value)?,
            "devices" => {
                self.options.devices = match value.as_str() {
                    "read" => Devices::Read,
                    "skip" => Devices::Skip,
                    _ => return Err("unknown devices method".to_string()),
                };
            }
            _ => return Err(format!("unknown option: --{name}")),
        }
        Ok(())
    }

    /// `-d ACTION`: `read` (default) and `skip` leave recursion off; `recurse` is `-r`
    /// unless `-R` already asked for symlinks to be followed.
    fn set_directories(&mut self, value: &str) -> Result<(), String> {
        let o = &mut self.options;
        match value {
            "read" => {
                o.recurse = Recurse::Off;
                o.skip_dirs = false;
            }
            "skip" => {
                o.recurse = Recurse::Off;
                o.skip_dirs = true;
            }
            "recurse" => {
                if o.recurse == Recurse::Off {
                    o.recurse = Recurse::Command;
                }
                o.skip_dirs = false;
            }
            _ => {
                return Err(format!(
                    "invalid argument '{value}' for '--directories'\nValid arguments are:\n  - 'read'\n  - 'recurse'\n  - 'skip'\nTry 'grep --help' for more information."
                ))
            }
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
            "null-data" => o.zero = true,
            "null" => o.null = true,
            "byte-offset" => o.byte_offset = true,
            "initial-tab" => o.initial_tab = true,
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
        'd' => Some("directories"),
        'D' => Some("devices"),
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
        'z' => Ok("null-data"),
        'Z' => Ok("null"),
        'b' => Ok("byte-offset"),
        'T' => Ok("initial-tab"),
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
