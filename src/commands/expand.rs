//! `expand` and `unexpand`: convert tabs to spaces and runs of spaces back to tabs.
//!
//! Tab stops are given with `-t`: a single number N means every N columns, a list
//! `a,b,c` gives explicit stops (a tab past the last stop becomes one space), `+N` in a list
//! adds N to the previous stop and repeats, and `/N` is the same as a single `N`.

use std::io::{self, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const DEFAULT_TAB: usize = 8;

const EXPAND_SPECS: &[OptSpec] = &[
    OptSpec::flag("initial", Some('i'), Some("initial")),
    OptSpec::req("tabs", Some('t'), Some("tabs")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const UNEXPAND_SPECS: &[OptSpec] = &[
    OptSpec::flag("all", Some('a'), Some("all")),
    OptSpec::flag("first", None, Some("first-only")),
    OptSpec::req("tabs", Some('t'), Some("tabs")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const EXPAND_HELP: &str = "Usage: expand [OPTION]... [FILE]...\n\
Convert tabs in each FILE to spaces, writing to standard output. With no FILE,\n\
or when FILE is -, read standard input.\n\n\
  -i, --initial     do not convert tabs after non-blanks\n\
  -t, --tabs=LIST   have tabs N characters apart instead of 8 (enables -i);\n\
                    LIST is N, or a comma-separated list of positions,\n\
                    where +N adds N to the previous position\n\
      --help        display this help and exit\n\
      --version     output version information and exit\n\n\
A legacy -N option is the same as -t N.\n\
Exit status: 0 on success, 1 if an input could not be read.\n";

const UNEXPAND_HELP: &str = "Usage: unexpand [OPTION]... [FILE]...\n\
Convert blanks in each FILE to tabs, writing to standard output. With no FILE,\n\
or when FILE is -, read standard input.\n\n\
  -a, --all        convert all blanks, instead of just initial blanks\n\
      --first-only  convert only leading sequences of blanks (overrides -a)\n\
  -t, --tabs=LIST  use comma-separated list of tab positions (implies -a);\n\
                   LIST is as for expand\n\
      --help       display this help and exit\n\
      --version    output version information and exit\n\n\
A legacy -N option is the same as -t N.\n\
Exit status: 0 on success, 1 if an input could not be read.\n";

/// Tab stop positions. Columns are 0-based.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TabStops {
    stops: Vec<usize>,
    /// `(base, step)`: after the last explicit stop, stops continue every `step` columns.
    repeat: Option<(usize, usize)>,
}

impl TabStops {
    fn every(step: usize) -> TabStops {
        TabStops {
            stops: Vec::new(),
            repeat: Some((0, step)),
        }
    }

    /// The first stop strictly after `col`, or `None` past the last explicit stop of a list.
    fn next_stop(&self, col: usize) -> Option<usize> {
        if let Some(&stop) = self.stops.iter().find(|&&stop| stop > col) {
            return Some(stop);
        }
        let (base, step) = self.repeat?;
        Some(base + step * ((col - base) / step + 1))
    }
}

pub fn run_expand(args: &[String]) -> i32 {
    run_tabs(args, Tool::Expand)
}

pub fn run_unexpand(args: &[String]) -> i32 {
    run_tabs(args, Tool::Unexpand)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Expand,
    Unexpand,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Expand => "expand",
            Tool::Unexpand => "unexpand",
        }
    }

    fn specs(self) -> &'static [OptSpec] {
        match self {
            Tool::Expand => EXPAND_SPECS,
            Tool::Unexpand => UNEXPAND_SPECS,
        }
    }

    fn help(self) -> &'static str {
        match self {
            Tool::Expand => EXPAND_HELP,
            Tool::Unexpand => UNEXPAND_HELP,
        }
    }
}

/// What the run converts.
#[derive(Debug)]
struct Settings {
    tabs: TabStops,
    /// expand: only convert blanks before the first non-blank.
    initial: bool,
    /// unexpand: convert every blank run, not just the leading ones.
    all: bool,
}

fn run_tabs(args: &[String], tool: Tool) -> i32 {
    let name = tool.name();
    let args = expand_legacy_tabs(args);
    let items = match Parser::new(name, tool.specs()).parse(&args) {
        Ok(items) => items,
        Err(error) => return report_usage(name, &error, EXIT_FAIL),
    };
    if let Some(code) = early_option(name, tool, &items) {
        return code;
    }
    let settings = match settings_from(tool, &items) {
        Ok(settings) => settings,
        Err(code) => return code,
    };
    let files = operands(&items);
    run_files(tool, &settings, &files)
}

/// Rewrites a legacy `-N` argument (digits only) to `-t N`.
fn expand_legacy_tabs(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut options_open = true;
    for arg in args {
        if arg == "--" {
            options_open = false;
        }
        let legacy = options_open
            && arg.len() > 1
            && arg.starts_with('-')
            && arg[1..].bytes().all(|b| b.is_ascii_digit());
        if legacy {
            out.push("-t".to_owned());
            out.push(arg[1..].to_owned());
        } else {
            out.push(arg.clone());
        }
    }
    out
}

fn early_option(name: &str, tool: Tool, items: &[Item]) -> Option<i32> {
    for item in items {
        if let Item::Opt { id, .. } = item {
            match *id {
                "help" => {
                    print!("{}", tool.help());
                    return Some(EXIT_OK);
                }
                "version" => return Some(print_version(name)),
                _ => {}
            }
        }
    }
    None
}

fn settings_from(tool: Tool, items: &[Item]) -> Result<Settings, i32> {
    let mut tabs = TabStops::every(DEFAULT_TAB);
    let mut initial = false;
    let mut all = false;
    let mut first_only = false;
    for item in items {
        if let Item::Opt { id, value } = item {
            match *id {
                "tabs" => {
                    tabs = parse_tabs(value.as_deref().unwrap_or(""))
                        .map_err(|message| tab_error(tool, &message))?;
                    all = true;
                }
                "initial" => initial = true,
                "all" => all = true,
                "first" => first_only = true,
                _ => {}
            }
        }
    }
    if first_only {
        all = false;
    }
    Ok(Settings { tabs, initial, all })
}

fn tab_error(tool: Tool, message: &str) -> i32 {
    eprintln!(
        "{}: {message}\nTry '{} --help' for more information.",
        tool.name(),
        tool.name()
    );
    EXIT_FAIL
}

fn operands(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(name) => Some(name.clone()),
            Item::Opt { .. } => None,
        })
        .collect()
}

/// Parses a `-t` argument into tab stops.
fn parse_tabs(text: &str) -> Result<TabStops, String> {
    let items: Vec<&str> = text.split(',').collect();
    if items.len() == 1 && !text.starts_with('+') && !text.starts_with('/') {
        return Ok(TabStops::every(parse_tab_size(text)?));
    }
    let mut stops = Vec::new();
    let mut prev = 0usize;
    let mut repeat = None;
    for (index, item) in items.iter().enumerate() {
        let last = index + 1 == items.len();
        let (increment, digits) = match item.as_bytes().first() {
            Some(b'+') | Some(b'/') => (true, &item[1..]),
            _ => (false, *item),
        };
        let size = parse_tab_size(digits)?;
        if increment {
            if last {
                repeat = Some((prev, size));
                break;
            }
            prev += size;
            stops.push(prev);
        } else {
            if size <= prev && index > 0 {
                return Err("tab sizes must be ascending".to_owned());
            }
            prev = size;
            stops.push(size);
        }
    }
    Ok(TabStops { stops, repeat })
}

fn parse_tab_size(text: &str) -> Result<usize, String> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("tab size contains invalid character(s): '{text}'"));
    }
    match text.parse::<usize>() {
        Ok(0) => Err("tab size cannot be 0".to_owned()),
        Ok(size) => Ok(size),
        Err(_) => Err("tab size too large".to_owned()),
    }
}

fn run_files(tool: Tool, settings: &Settings, files: &[String]) -> i32 {
    let default = ["-".to_owned()];
    let names: &[String] = if files.is_empty() { &default } else { files };
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    for name in names {
        match convert_file(tool, settings, name, &mut out) {
            Ok(()) => {}
            Err(Failure::Input(error)) => {
                report(tool.name(), name, &error);
                status = EXIT_FAIL;
            }
            Err(Failure::Output(error)) => return output_failed(tool, &error),
        }
    }
    let flushed = finish_output(tool.name(), &mut out);
    if flushed == EXIT_OK {
        status
    } else {
        flushed
    }
}

enum Failure {
    Input(io::Error),
    Output(io::Error),
}

fn convert_file(
    tool: Tool,
    settings: &Settings,
    name: &str,
    out: &mut impl Write,
) -> Result<(), Failure> {
    let mut reader = open_input(name).map_err(Failure::Input)?;
    let mut record = Vec::new();
    let mut converted = Vec::new();
    while read_record(reader.as_mut(), b'\n', &mut record).map_err(Failure::Input)? {
        converted.clear();
        match tool {
            Tool::Expand => expand_record(settings, &record, &mut converted),
            Tool::Unexpand => unexpand_record(settings, &record, &mut converted),
        }
        out.write_all(&converted).map_err(Failure::Output)?;
    }
    Ok(())
}

fn output_failed(tool: Tool, error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{}: write error: {}", tool.name(), io_error_text(error));
    EXIT_FAIL
}

/// Column after a non-tab byte: backspace moves back one, everything else advances one.
fn step_column(col: usize, byte: u8) -> usize {
    if byte == 0x08 {
        col.saturating_sub(1)
    } else {
        col + 1
    }
}

fn expand_record(settings: &Settings, record: &[u8], out: &mut Vec<u8>) {
    let mut col = 0usize;
    let mut initial = true;
    for &byte in record {
        match byte {
            b'\n' => {
                out.push(byte);
                col = 0;
                initial = true;
            }
            b'\t' => {
                let next = settings.tabs.next_stop(col);
                if settings.initial && !initial {
                    out.push(byte);
                    col = next.unwrap_or(col + 1);
                } else {
                    let stop = next.unwrap_or(col + 1);
                    out.extend(std::iter::repeat_n(b' ', stop - col));
                    col = stop;
                }
            }
            _ => {
                out.push(byte);
                if byte != b' ' {
                    initial = false;
                }
                col = step_column(col, byte);
            }
        }
    }
}

/// A run of blanks (spaces and tabs) being collected by `unexpand`.
#[derive(Default)]
struct Run {
    raw: Vec<u8>,
    start: usize,
    end: usize,
}

fn unexpand_record(settings: &Settings, record: &[u8], out: &mut Vec<u8>) {
    let mut col = 0usize;
    let mut initial = true;
    let mut run = Run::default();
    for &byte in record {
        if byte == b' ' || byte == b'\t' {
            if run.raw.is_empty() {
                run.start = col;
            }
            run.raw.push(byte);
            col = if byte == b'\t' {
                settings.tabs.next_stop(col).unwrap_or(col + 1)
            } else {
                col + 1
            };
            run.end = col;
            continue;
        }
        flush_run(settings, &mut run, initial, out);
        out.push(byte);
        if byte == b'\n' {
            col = 0;
            initial = true;
        } else {
            initial = false;
            col = step_column(col, byte);
        }
    }
    flush_run(settings, &mut run, initial, out);
}

/// Writes a collected blank run, converting it when the mode allows. `at_start` tells whether
/// the run began the line (for leading-blank-only conversion).
fn flush_run(settings: &Settings, run: &mut Run, at_start: bool, out: &mut Vec<u8>) {
    if run.raw.is_empty() {
        return;
    }
    if settings.all || at_start {
        write_converted(settings, run.start, run.end, out);
    } else {
        out.extend_from_slice(&run.raw);
    }
    run.raw.clear();
}

/// Emits the blanks covering columns `start..end`. A tab stop reached inside a run of at least
/// two blanks becomes a tab; a lone blank is never turned into a tab.
fn write_converted(settings: &Settings, start: usize, end: usize, out: &mut Vec<u8>) {
    let mut col = start;
    while col < end {
        match settings.tabs.next_stop(col) {
            Some(stop) if stop <= end && end - col >= 2 => {
                out.push(b'\t');
                col = stop;
            }
            _ => {
                out.push(b' ');
                col += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(tabs: &str, initial: bool, all: bool) -> Settings {
        Settings {
            tabs: parse_tabs(tabs).unwrap(),
            initial,
            all,
        }
    }

    fn expand(input: &[u8], settings: &Settings) -> Vec<u8> {
        let mut out = Vec::new();
        expand_record(settings, input, &mut out);
        out
    }

    fn unexpand(input: &[u8], settings: &Settings) -> Vec<u8> {
        let mut out = Vec::new();
        unexpand_record(settings, input, &mut out);
        out
    }

    #[test]
    fn parses_tab_lists() {
        assert_eq!(parse_tabs("4").unwrap().next_stop(1), Some(4));
        assert_eq!(parse_tabs("3,6").unwrap().next_stop(6), None);
        assert_eq!(parse_tabs("2,+3").unwrap().next_stop(2), Some(5));
        assert_eq!(parse_tabs("/3").unwrap().next_stop(3), Some(6));
        assert!(parse_tabs("0").is_err());
        assert!(parse_tabs("5,3").is_err());
        assert!(parse_tabs("x").is_err());
    }

    #[test]
    fn expands_tabs_to_stops() {
        assert_eq!(expand(b"a\tb\n", &settings("4", false, false)), b"a   b\n");
        assert_eq!(
            expand(b"ab\tc\n", &settings("3,6", false, false)),
            b"ab c\n"
        );
    }

    #[test]
    fn initial_mode_keeps_inner_tabs() {
        assert_eq!(
            expand(b"a\tb\n  \tc\n", &settings("8", true, false)),
            b"a\tb\n        c\n"
        );
        assert_eq!(
            expand(b"a\tb\n", &settings("8", false, false))[..8].to_vec(),
            b"a       "
        );
    }

    #[test]
    fn backspace_moves_back_one_column() {
        assert_eq!(
            expand(b"ab\x08\tc\n", &settings("8", false, false)),
            b"ab\x08       c\n"
        );
    }

    #[test]
    fn unexpands_leading_blanks_only_by_default() {
        assert_eq!(
            unexpand(b"        a       b\n", &settings("8", false, false)),
            b"\ta       b\n"
        );
    }

    #[test]
    fn unexpands_all_blanks_with_all() {
        assert_eq!(
            unexpand(b"        a       b\n", &settings("8", false, true)),
            b"\ta\tb\n"
        );
    }

    #[test]
    fn single_blank_is_not_converted() {
        assert_eq!(
            unexpand(b"1234567 8\n", &settings("8", false, true)),
            b"1234567 8\n"
        );
        assert_eq!(
            unexpand(b"ab  c\n", &settings("4", false, true)),
            b"ab\tc\n"
        );
    }

    #[test]
    fn legacy_tabs_are_rewritten() {
        let args = vec!["-4".to_owned(), "file".to_owned()];
        assert_eq!(expand_legacy_tabs(&args), vec!["-t", "4", "file"]);
    }
}
