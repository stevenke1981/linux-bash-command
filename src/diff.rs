//! `diff`: compares two files, or two directories with `-r`, and prints the differences in
//! normal (default), unified, context or side-by-side form. The renderers live in the child
//! modules `hunks` (shared line comparison), `normal`, `unified`, `context` and `side`.

mod context;
mod hunks;
mod normal;
mod side;
mod unified;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::common::args::prepare_legacy;
use crate::common::datetime::{format_in, from_system_time, now, zone_from_env};
use crate::common::error::io_error_text;
use crate::display_path;
use crate::fs_commands::is_color_option;

const DEFAULT_CONTEXT: usize = 3;
const DEFAULT_WIDTH: usize = 130;

const USAGE: &str = concat!(
    "Usage: diff [OPTIONS] LEFT RIGHT\n",
    "Compare two files, or two directories (-r recurses), and print the differences.\n",
    "\n",
    "Output format (default: normal):\n",
    "      --normal                  Normal format\n",
    "  -u, -U N, --unified[=N]       Unified format with N unchanged context lines (default: 3)\n",
    "  -c, -C N, --context[=N]       Context format with N lines of context (default: 3)\n",
    "  -y, --side-by-side            Side-by-side format\n",
    "  -W N, --width=N               Output width for -y (default: 130)\n",
    "      --left-column             With -y, print the left column of common lines only\n",
    "      --suppress-common-lines   With -y, do not print common lines\n",
    "\n",
    "Options:\n",
    "  -q, --brief                   Report only whether the files differ\n",
    "  -s, --report-identical-files  Report when two files are the same\n",
    "  -r, --recursive               Compare directory trees recursively\n",
    "  -N, --new-file                Treat an absent file as empty\n",
    "  -w, --ignore-all-space        Ignore all white space\n",
    "  -b, --ignore-space-change     Ignore changes in the amount of white space\n",
    "  -B, --ignore-blank-lines      Ignore changes whose lines are all blank\n",
    "  -i, --ignore-case             Ignore case differences\n",
    "      --strip-trailing-cr       Ignore a trailing carriage return on each line\n",
    "  -a, --text                    Compare binary-looking files as text\n",
    "      --label LABEL             Use LABEL in headers instead of the file name (up to two)\n",
    "      --color[=WHEN]            Accepted; output is never colored\n",
    "  -h, --help                    Show this help\n",
    "\n",
    "Not supported: -e (ed scripts).\n",
    "Exit status: 0 when equal, 1 when different, 2 on error.\n",
);

/// The output format. Only one may be chosen per run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Normal,
    Unified,
    Context,
    SideBySide,
}

/// Options that change how files are compared or how the result is reported.
#[derive(Debug)]
struct Options {
    style: Style,
    style_set: bool,
    /// Context lines for unified and context formats.
    context: usize,
    /// Output width for side-by-side.
    width: usize,
    left_column: bool,
    suppress_common: bool,
    brief: bool,
    report_identical: bool,
    recursive: bool,
    new_file: bool,
    ignore_all_space: bool,
    ignore_space_change: bool,
    ignore_blank_lines: bool,
    ignore_case: bool,
    strip_trailing_cr: bool,
    text: bool,
    labels: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            style: Style::Normal,
            style_set: false,
            context: DEFAULT_CONTEXT,
            width: DEFAULT_WIDTH,
            left_column: false,
            suppress_common: false,
            brief: false,
            report_identical: false,
            recursive: false,
            new_file: false,
            ignore_all_space: false,
            ignore_space_change: false,
            ignore_blank_lines: false,
            ignore_case: false,
            strip_trailing_cr: false,
            text: false,
            labels: Vec::new(),
        }
    }
}

/// Long options that take a value, for prefix expansion and `--name VALUE` forms.
const DIFF_LONGS: &[&str] = &[
    "brief",
    "context",
    "ed",
    "help",
    "ignore-all-space",
    "ignore-blank-lines",
    "ignore-case",
    "ignore-space-change",
    "label",
    "left-column",
    "new-file",
    "normal",
    "recursive",
    "report-identical-files",
    "side-by-side",
    "strip-trailing-cr",
    "suppress-common-lines",
    "text",
    "unified",
    "width",
];

/// Compare two files or directories. Returns 0 when equal, 1 when different, and 2 on error.
pub fn run(args: &[String]) -> i32 {
    let expanded = match prepare_legacy("diff", args, DIFF_LONGS, &["label", "width"], "UCW", 2) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let (options, paths) = match parse_args(&expanded) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    if paths.len() != 2 {
        eprintln!("diff: expected LEFT and RIGHT file paths\n{USAGE}");
        return 2;
    }
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let result = compare_operands(&paths[0], &paths[1], &options, &mut out)
        .and_then(|status| out.flush().map(|()| status));
    match result {
        Ok(status) => status,
        Err(error) => {
            eprintln!("diff: cannot write output: {}", io_error_text(&error));
            2
        }
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), i32> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut end_of_options = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if end_of_options || arg == "-" || !arg.starts_with('-') {
            paths.push(arg.to_string());
        } else if arg == "--" {
            end_of_options = true;
        } else if let Some(name) = arg.strip_prefix("--") {
            parse_long(name, arg, args, &mut index, &mut options)?;
        } else {
            parse_short(&arg[1..], args, &mut index, &mut options)?;
        }
    }
    Ok((options, paths))
}

/// Selects an output format, rejecting a second, different one (GNU: conflicting options).
fn set_style(options: &mut Options, style: Style) -> Result<(), i32> {
    if options.style_set && options.style != style {
        eprintln!(
            "diff: conflicting output style options\nTry 'diff --help' for more information."
        );
        return Err(2);
    }
    options.style = style;
    options.style_set = true;
    Ok(())
}

fn parse_long(
    name: &str,
    arg: &str,
    args: &[String],
    index: &mut usize,
    options: &mut Options,
) -> Result<(), i32> {
    match name {
        "help" => {
            print!("{USAGE}");
            return Err(0);
        }
        "brief" => options.brief = true,
        "report-identical-files" => options.report_identical = true,
        "recursive" => options.recursive = true,
        "new-file" => options.new_file = true,
        "ignore-all-space" => options.ignore_all_space = true,
        "ignore-space-change" => options.ignore_space_change = true,
        "ignore-blank-lines" => options.ignore_blank_lines = true,
        "ignore-case" => options.ignore_case = true,
        "strip-trailing-cr" => options.strip_trailing_cr = true,
        "text" => options.text = true,
        "left-column" => options.left_column = true,
        "suppress-common-lines" => options.suppress_common = true,
        "normal" => set_style(options, Style::Normal)?,
        "side-by-side" => set_style(options, Style::SideBySide)?,
        "unified" => set_style(options, Style::Unified)?,
        "context" => set_style(options, Style::Context)?,
        "label" => {
            let label = next_value(args, index, "--label")?;
            push_label(options, label)?;
        }
        "width" => {
            let value = next_value(args, index, "--width")?;
            options.width = parse_width(value)?;
        }
        "ed" => {
            eprintln!("diff: --ed is not supported (only the normal, context, unified and side-by-side formats)");
            return Err(2);
        }
        _ => parse_long_with_value(name, arg, options)?,
    }
    Ok(())
}

fn parse_long_with_value(name: &str, arg: &str, options: &mut Options) -> Result<(), i32> {
    if let Some(value) = name.strip_prefix("unified=") {
        set_style(options, Style::Unified)?;
        options.context = parse_context(value)?;
    } else if let Some(value) = name.strip_prefix("context=") {
        set_style(options, Style::Context)?;
        options.context = parse_context(value)?;
    } else if let Some(label) = name.strip_prefix("label=") {
        push_label(options, label)?;
    } else if let Some(value) = name.strip_prefix("width=") {
        options.width = parse_width(value)?;
    } else if !is_color_option(arg) {
        eprintln!("diff: unknown option: {arg}\n{USAGE}");
        return Err(2);
    }
    Ok(())
}

fn parse_short(
    flags: &str,
    args: &[String],
    index: &mut usize,
    options: &mut Options,
) -> Result<(), i32> {
    for (offset, flag) in flags.char_indices() {
        match flag {
            'u' => set_style(options, Style::Unified)?,
            'c' => set_style(options, Style::Context)?,
            'y' => set_style(options, Style::SideBySide)?,
            'q' => options.brief = true,
            's' => options.report_identical = true,
            'r' => options.recursive = true,
            'N' => options.new_file = true,
            'w' => options.ignore_all_space = true,
            'b' => options.ignore_space_change = true,
            'B' => options.ignore_blank_lines = true,
            'i' => options.ignore_case = true,
            'a' => options.text = true,
            'U' | 'C' | 'W' => {
                let rest = &flags[offset + flag.len_utf8()..];
                let value = if rest.is_empty() {
                    next_value(args, index, &format!("-{flag}"))?
                } else {
                    rest
                };
                match flag {
                    'U' => {
                        set_style(options, Style::Unified)?;
                        options.context = parse_context(value)?;
                    }
                    'C' => {
                        set_style(options, Style::Context)?;
                        options.context = parse_context(value)?;
                    }
                    _ => options.width = parse_width(value)?,
                }
                return Ok(());
            }
            'h' => {
                print!("{USAGE}");
                return Err(0);
            }
            'e' => {
                eprintln!("diff: -e is not supported (only the normal, context, unified and side-by-side formats)");
                return Err(2);
            }
            _ => {
                eprintln!("diff: unknown option: -{flag}\n{USAGE}");
                return Err(2);
            }
        }
    }
    Ok(())
}

fn next_value<'a>(args: &'a [String], index: &mut usize, option: &str) -> Result<&'a str, i32> {
    match args.get(*index) {
        Some(value) => {
            *index += 1;
            Ok(value.as_str())
        }
        None => {
            eprintln!("diff: {option} requires a value\n{USAGE}");
            Err(2)
        }
    }
}

fn parse_context(value: &str) -> Result<usize, i32> {
    value.parse::<usize>().map_err(|_| {
        eprintln!("diff: invalid context line count: {value}");
        2
    })
}

fn parse_width(value: &str) -> Result<usize, i32> {
    match value.parse::<usize>() {
        Ok(width) if width >= 1 => Ok(width),
        _ => {
            eprintln!("diff: invalid width '{value}'\nTry 'diff --help' for more information.");
            Err(2)
        }
    }
}

fn push_label(options: &mut Options, label: &str) -> Result<(), i32> {
    if options.labels.len() == 2 {
        eprintln!("diff: at most two --label options are allowed");
        return Err(2);
    }
    options.labels.push(label.to_string());
    Ok(())
}

/// Display names for the two operands, replaced by `--label` values when given.
fn display_names(left: &str, right: &str, options: &Options) -> (String, String) {
    let label = |index: usize, fallback: &str| {
        options
            .labels
            .get(index)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    (label(0, left), label(1, right))
}

fn compare_operands(
    left: &str,
    right: &str,
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<i32> {
    let (left_path, right_path) = (Path::new(left), Path::new(right));
    let (left_is_dir, right_is_dir) = (is_directory(left_path), is_directory(right_path));
    if left_is_dir && right_is_dir {
        return compare_directories(left_path, right_path, options, out);
    }
    if left_is_dir || right_is_dir {
        return compare_against_directory(left, right, left_is_dir, options, out);
    }
    let names = display_names(left, right, options);
    compare_top_level_files(left_path, right_path, &names, options, out)
}

/// Compares a directory operand with a file operand by looking for the file's name inside the directory.
fn compare_against_directory(
    left: &str,
    right: &str,
    left_is_dir: bool,
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<i32> {
    if left == "-" || right == "-" {
        eprintln!("diff: cannot compare '-' to a directory");
        return Ok(2);
    }
    let (dir, file) = if left_is_dir {
        (Path::new(left), Path::new(right))
    } else {
        (Path::new(right), Path::new(left))
    };
    let Some(sibling) = file.file_name().map(|name| dir.join(name)) else {
        eprintln!(
            "diff: cannot compare {} with a directory",
            display_path(file)
        );
        return Ok(2);
    };
    let (left_file, right_file) = if left_is_dir {
        (sibling, PathBuf::from(right))
    } else {
        (PathBuf::from(left), sibling)
    };
    let names = display_names(
        &display_path(&left_file),
        &display_path(&right_file),
        options,
    );
    compare_top_level_files(&left_file, &right_file, &names, options, out)
}

fn compare_top_level_files(
    left: &Path,
    right: &Path,
    names: &(String, String),
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<i32> {
    match compare_loaded(left, right, names, options) {
        Ok((status, text)) => {
            out.write_all(&text)?;
            Ok(status)
        }
        Err(message) => {
            eprintln!("diff: {message}");
            Ok(2)
        }
    }
}

/// Loads both operands and reports the comparison; load failures are returned as messages.
fn compare_loaded(
    left: &Path,
    right: &Path,
    names: &(String, String),
    options: &Options,
) -> Result<(i32, Vec<u8>), String> {
    let left_bytes = load(left, options.new_file)?;
    let right_bytes = load(right, options.new_file)?;
    // A file given a --label is shown by that label alone, without a time.
    let times = if options.style == Style::Context {
        let stamp = |path: &Path, labelled: bool| {
            if labelled {
                String::new()
            } else {
                time_stamp(path)
            }
        };
        (
            stamp(left, !options.labels.is_empty()),
            stamp(right, options.labels.len() == 2),
        )
    } else {
        (String::new(), String::new())
    };
    Ok(report_pair(
        &left_bytes,
        &right_bytes,
        names,
        &times,
        options,
    ))
}

/// The modification time as the context format prints it, in the `TZ` zone. Standard input
/// has no file time, so it gets the current time, as in GNU diff.
fn time_stamp(path: &Path) -> String {
    let stamp = if path == Path::new("-") {
        now()
    } else {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .map(from_system_time)
            .unwrap_or_else(|_| now())
    };
    format_in("%a %b %e %H:%M:%S %Y", stamp, &zone_from_env())
}

fn load(path: &Path, new_file: bool) -> Result<Vec<u8>, String> {
    if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| format!("standard input: {}", io_error_text(&error)))?;
        return Ok(bytes);
    }
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if new_file && error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("{}: {}", display_path(path), io_error_text(&error))),
    }
}

fn is_directory(path: &Path) -> bool {
    path != Path::new("-")
        && fs::metadata(path)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
}

fn compare_directories(
    left: &Path,
    right: &Path,
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<i32> {
    let left_names = match entry_names(left) {
        Ok(names) => names,
        Err(message) => {
            eprintln!("diff: {message}");
            return Ok(2);
        }
    };
    let right_names = match entry_names(right) {
        Ok(names) => names,
        Err(message) => {
            eprintln!("diff: {message}");
            return Ok(2);
        }
    };
    let mut status = 0;
    for name in left_names.union(&right_names) {
        let left_child = left.join(name);
        let right_child = right.join(name);
        let child_status = match (left_names.contains(name), right_names.contains(name)) {
            (true, true) => compare_entries(&left_child, &right_child, options, out)?,
            (true, false) => {
                single_entry(&left_child, &right_child, left, name, options, out, true)?
            }
            (false, true) => {
                single_entry(&right_child, &left_child, right, name, options, out, false)?
            }
            (false, false) => 0,
        };
        status = status.max(child_status);
    }
    Ok(status)
}

/// Handles an entry present on only one side: `-N` compares it with an empty file, otherwise it is reported.
#[allow(clippy::too_many_arguments)]
fn single_entry(
    present: &Path,
    absent: &Path,
    present_dir: &Path,
    name: &OsString,
    options: &Options,
    out: &mut dyn Write,
    present_on_left: bool,
) -> io::Result<i32> {
    if options.new_file && !present.is_dir() {
        let (left, right) = if present_on_left {
            (present, absent)
        } else {
            (absent, present)
        };
        return compare_entries(left, right, options, out);
    }
    writeln!(
        out,
        "Only in {}: {}",
        display_path(present_dir),
        name.to_string_lossy()
    )?;
    Ok(1)
}

fn compare_entries(
    left: &Path,
    right: &Path,
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<i32> {
    match (left.is_dir(), right.is_dir()) {
        (true, true) if options.recursive => compare_directories(left, right, options, out),
        (true, true) => {
            writeln!(
                out,
                "Common subdirectories: {} and {}",
                display_path(left),
                display_path(right)
            )?;
            Ok(0)
        }
        (true, false) => {
            writeln!(
                out,
                "File {} is a directory while file {} is a regular file",
                display_path(left),
                display_path(right)
            )?;
            Ok(1)
        }
        (false, true) => {
            writeln!(
                out,
                "File {} is a regular file while file {} is a directory",
                display_path(left),
                display_path(right)
            )?;
            Ok(1)
        }
        (false, false) => compare_tree_files(left, right, options, out),
    }
}

/// Compares one file pair found while walking directories, with a `diff LEFT RIGHT` header
/// (`diff -r LEFT RIGHT` under -r) before any differences.
fn compare_tree_files(
    left: &Path,
    right: &Path,
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<i32> {
    let names = (display_path(left), display_path(right));
    match compare_loaded(left, right, &names, options) {
        Ok((status, text)) => {
            if status != 0 && !options.brief {
                let command = if options.recursive { "diff -r" } else { "diff" };
                writeln!(out, "{command} {} {}", names.0, names.1)?;
            }
            out.write_all(&text)?;
            Ok(status)
        }
        Err(message) => {
            eprintln!("diff: {message}");
            Ok(2)
        }
    }
}

fn entry_names(dir: &Path) -> Result<BTreeSet<OsString>, String> {
    let items = fs::read_dir(dir)
        .map_err(|error| format!("{}: {}", display_path(dir), io_error_text(&error)))?;
    items
        .map(|item| {
            item.map(|item| item.file_name())
                .map_err(|error| format!("{}: {}", display_path(dir), io_error_text(&error)))
        })
        .collect()
}

/// Returns (exit status, text to print) for one pair of file contents.
fn report_pair(
    left: &[u8],
    right: &[u8],
    names: &(String, String),
    times: &(String, String),
    options: &Options,
) -> (i32, Vec<u8>) {
    match compare_contents(left, right, names, times, options) {
        Comparison::Same if options.report_identical => (
            0,
            format!("Files {} and {} are identical\n", names.0, names.1).into_bytes(),
        ),
        Comparison::Same => (0, Vec::new()),
        Comparison::Identical(mut body) => {
            if options.report_identical {
                body.extend(
                    format!("Files {} and {} are identical\n", names.0, names.1).into_bytes(),
                );
            }
            (0, body)
        }
        Comparison::Binary if options.brief => (1, differ_message(names, "Files").into_bytes()),
        Comparison::Binary => (1, differ_message(names, "Binary files").into_bytes()),
        Comparison::Text(_) if options.brief => (1, differ_message(names, "Files").into_bytes()),
        Comparison::Text(body) => (1, body),
    }
}

fn differ_message(names: &(String, String), prefix: &str) -> String {
    format!("{prefix} {} and {} differ\n", names.0, names.1)
}

enum Comparison {
    /// Equal under the options, with nothing to print.
    Same,
    /// Equal under the options, but side-by-side output still lists the common lines.
    Identical(Vec<u8>),
    Binary,
    Text(Vec<u8>),
}

fn compare_contents(
    left: &[u8],
    right: &[u8],
    names: &(String, String),
    times: &(String, String),
    options: &Options,
) -> Comparison {
    // GNU `-y` lists the lines of equal files too, unless the output is only a verdict.
    let lists_common = options.style == Style::SideBySide && !options.brief;
    if left == right && !lists_common {
        return Comparison::Same;
    }
    if left != right && !options.text && (is_binary(left) || is_binary(right)) {
        return Comparison::Binary;
    }
    let diffed = hunks::diff_lines(left, right, options);
    if !hunks::has_visible_change(&diffed.changes) {
        return if lists_common {
            Comparison::Identical(side::render(&diffed, options))
        } else {
            Comparison::Same
        };
    }
    match render_body(&diffed, names, times, options) {
        Some(body) => Comparison::Text(body),
        None => Comparison::Same,
    }
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

/// Builds the body in the selected format for inputs with a visible change.
fn render_body(
    diffed: &hunks::Diffed<'_>,
    names: &(String, String),
    times: &(String, String),
    options: &Options,
) -> Option<Vec<u8>> {
    match options.style {
        Style::Normal => Some(normal::render(diffed)),
        Style::Unified => unified::render(names, diffed, options.context),
        Style::Context => context::render(names, times, diffed, options.context),
        Style::SideBySide => Some(side::render(diffed, options)),
    }
}

#[cfg(test)]
mod tests;
