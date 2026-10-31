use similar::{capture_diff_slices, Algorithm, DiffOp};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::common::color::{self, When};
use crate::common::error::{io_text, usage};
use crate::diff_color;
use crate::display_path;

const DEFAULT_CONTEXT: usize = 3;

const USAGE: &str = concat!(
    "Usage: diff [OPTIONS] LEFT RIGHT\n",
    "Compare two files, or two directories (-r recurses), and print a unified diff.\n",
    "\n",
    "Options:\n",
    "  -u, -U N, --unified[=N]       Unified format with N unchanged context lines (default: 3)\n",
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
    "      --context N               Same as -U N\n",
    "      --color[=WHEN]            Color hunks on a terminal: auto (bare --color), always, never\n",
    "  -h, --help                    Show this help\n",
    "\n",
    "Not supported: -c, -C, -y, -e (only unified output is available).\n",
    "Exit status: 0 when equal, 1 when different, 2 on error.\n",
);

/// Options that change how files are compared or how the result is reported.
#[derive(Default)]
struct Options {
    context: usize,
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
    /// The `--color[=WHEN]` value, if one was given.
    color_when: Option<When>,
    /// Whether this run colors its output, decided once in `run`.
    colored: bool,
}

/// Compare two files or directories. Returns 0 when equal, 1 when different, and 2 on error.
pub fn run(args: &[String]) -> i32 {
    let (mut options, paths) = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    options.colored = color::enabled(options.color_when);
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
            eprintln!("diff: cannot write output: {}", io_text(&error));
            2
        }
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), i32> {
    let mut options = Options {
        context: DEFAULT_CONTEXT,
        ..Options::default()
    };
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
        "unified" => {}
        "label" => {
            let label = next_value(args, index, "--label")?;
            push_label(options, label)?;
        }
        "context" => {
            let value = next_value(args, index, "--context")?;
            options.context = parse_context(value)?;
        }
        "side-by-side" | "ed" | "normal" | "context-format" => {
            eprintln!("diff: --{name} is not supported (only unified output is available)");
            return Err(2);
        }
        _ => parse_long_with_value(name, arg, options)?,
    }
    Ok(())
}

fn parse_long_with_value(name: &str, arg: &str, options: &mut Options) -> Result<(), i32> {
    if let Some(value) = name
        .strip_prefix("unified=")
        .or_else(|| name.strip_prefix("context="))
    {
        options.context = parse_context(value)?;
    } else if let Some(label) = name.strip_prefix("label=") {
        push_label(options, label)?;
    } else if let Some(value) = color::color_arg(arg) {
        options.color_when = Some(parse_when(value)?);
    } else {
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
            'u' => {}
            'q' => options.brief = true,
            's' => options.report_identical = true,
            'r' => options.recursive = true,
            'N' => options.new_file = true,
            'w' => options.ignore_all_space = true,
            'b' => options.ignore_space_change = true,
            'B' => options.ignore_blank_lines = true,
            'i' => options.ignore_case = true,
            'a' => options.text = true,
            'U' => {
                let rest = &flags[offset + flag.len_utf8()..];
                let value = if rest.is_empty() {
                    next_value(args, index, "-U")?
                } else {
                    rest
                };
                options.context = parse_context(value)?;
                return Ok(());
            }
            'h' => {
                print!("{USAGE}");
                return Err(0);
            }
            'c' | 'C' | 'y' | 'e' => {
                eprintln!("diff: -{flag} is not supported (only unified output is available)");
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

fn parse_when(value: Option<&str>) -> Result<When, i32> {
    color::parse_when(value, When::Auto).map_err(|message| usage("diff", message, 2))
}

fn parse_context(value: &str) -> Result<usize, i32> {
    value.parse::<usize>().map_err(|_| {
        eprintln!("diff: invalid context line count: {value}");
        2
    })
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
    Ok(report_pair(&left_bytes, &right_bytes, names, options))
}

fn load(path: &Path, new_file: bool) -> Result<Vec<u8>, String> {
    if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| format!("standard input: {}", io_text(&error)))?;
        return Ok(bytes);
    }
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if new_file && error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("{}: {}", display_path(path), io_text(&error))),
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
                let mut line = Vec::new();
                let text = format!("{command} {} {}", names.0, names.1);
                diff_color::header_line(&mut line, options.colored, &text);
                out.write_all(&line)?;
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
    let items =
        fs::read_dir(dir).map_err(|error| format!("{}: {}", display_path(dir), io_text(&error)))?;
    items
        .map(|item| {
            item.map(|item| item.file_name())
                .map_err(|error| format!("{}: {}", display_path(dir), io_text(&error)))
        })
        .collect()
}

/// Returns (exit status, text to print) for one pair of file contents.
fn report_pair(
    left: &[u8],
    right: &[u8],
    names: &(String, String),
    options: &Options,
) -> (i32, Vec<u8>) {
    match compare_contents(left, right, names, options) {
        Comparison::Same if options.report_identical => (
            0,
            format!("Files {} and {} are identical\n", names.0, names.1).into_bytes(),
        ),
        Comparison::Same => (0, Vec::new()),
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
    Same,
    Binary,
    Text(Vec<u8>),
}

fn compare_contents(
    left: &[u8],
    right: &[u8],
    names: &(String, String),
    options: &Options,
) -> Comparison {
    if left == right {
        return Comparison::Same;
    }
    if !options.text && (is_binary(left) || is_binary(right)) {
        return Comparison::Binary;
    }
    match unified_body(left, right, names, options) {
        Some(body) => Comparison::Text(body),
        None => Comparison::Same,
    }
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

/// Builds the unified diff, or returns None when the texts are equal under the ignore options.
fn unified_body(
    left: &[u8],
    right: &[u8],
    names: &(String, String),
    options: &Options,
) -> Option<Vec<u8>> {
    let old_lines: Vec<&[u8]> = split_lines(left);
    let new_lines: Vec<&[u8]> = split_lines(right);
    let old_keys: Vec<Vec<u8>> = old_lines
        .iter()
        .map(|line| line_key(line, options))
        .collect();
    let new_keys: Vec<Vec<u8>> = new_lines
        .iter()
        .map(|line| line_key(line, options))
        .collect();
    let ops = capture_diff_slices(Algorithm::Myers, &old_keys, &new_keys);
    let changes = collect_changes(&ops, &old_lines, &new_lines, options.ignore_blank_lines);
    let hunks = group_hunks(&changes, options.context, old_lines.len());
    if hunks.is_empty() {
        return None;
    }
    let mut text = Vec::new();
    diff_color::header_line(&mut text, options.colored, &format!("--- {}", names.0));
    diff_color::header_line(&mut text, options.colored, &format!("+++ {}", names.1));
    for hunk in &hunks {
        text.extend(render_hunk(
            hunk,
            &changes,
            &old_lines,
            &new_lines,
            options.colored,
        ));
    }
    Some(text)
}

/// Splits raw bytes into lines that keep their `\n`, without assuming any text encoding.
fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|byte| *byte == b'\n').collect()
}

/// ASCII whitespace as recognized by the C locale (`isspace`).
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r' | b'\n')
}

/// Normalizes one line for comparison. The trailing newline is kept as a marker so that a
/// missing final newline still counts as a difference.
fn line_key(line: &[u8], options: &Options) -> Vec<u8> {
    let mut text = line.strip_suffix(b"\n").unwrap_or(line);
    if options.strip_trailing_cr {
        text = text.strip_suffix(b"\r").unwrap_or(text);
    }
    let mut key = if options.ignore_all_space {
        text.iter()
            .copied()
            .filter(|byte| !is_space(*byte))
            .collect()
    } else if options.ignore_space_change {
        text.split(|byte| is_space(*byte))
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(&b' ')
    } else {
        text.to_vec()
    };
    if options.ignore_case {
        // ASCII-only folding, as GNU diff does under the C locale: other bytes compare exactly.
        key.make_ascii_lowercase();
    }
    key.push(if line.ends_with(b"\n") { b'\n' } else { 0 });
    key
}

/// A run of differing lines. `silent` changes are blank-only changes hidden by `-B`.
#[derive(Clone)]
struct Change {
    old: Range<usize>,
    new: Range<usize>,
    silent: bool,
}

fn collect_changes(
    ops: &[DiffOp],
    old_lines: &[&[u8]],
    new_lines: &[&[u8]],
    ignore_blank_lines: bool,
) -> Vec<Change> {
    let mut changes: Vec<Change> = Vec::new();
    for op in ops {
        let Some((old, new)) = op_ranges(op) else {
            continue;
        };
        let silent =
            ignore_blank_lines && is_blank_change(&old_lines[old.clone()], &new_lines[new.clone()]);
        match changes.last_mut() {
            Some(last)
                if last.silent == silent
                    && last.old.end == old.start
                    && last.new.end == new.start =>
            {
                last.old.end = old.end;
                last.new.end = new.end;
            }
            _ => changes.push(Change { old, new, silent }),
        }
    }
    changes
}

fn op_ranges(op: &DiffOp) -> Option<(Range<usize>, Range<usize>)> {
    match *op {
        DiffOp::Equal { .. } => None,
        DiffOp::Delete {
            old_index,
            old_len,
            new_index,
        } => Some((old_index..old_index + old_len, new_index..new_index)),
        DiffOp::Insert {
            old_index,
            new_index,
            new_len,
        } => Some((old_index..old_index, new_index..new_index + new_len)),
        DiffOp::Replace {
            old_index,
            old_len,
            new_index,
            new_len,
        } => Some((
            old_index..old_index + old_len,
            new_index..new_index + new_len,
        )),
    }
}

fn is_blank_change(old: &[&[u8]], new: &[&[u8]]) -> bool {
    old.iter()
        .chain(new.iter())
        .all(|line| line.iter().all(|byte| is_space(*byte)))
}

/// A hunk covers the changes in `changes` (index range) plus context lines on both sides.
struct Hunk {
    changes: Range<usize>,
    old_start: usize,
    old_end: usize,
    new_start: usize,
}

/// Groups visible changes into hunks. Context never reaches into a neighbouring change,
/// so hidden (`silent`) changes are never printed as context.
fn group_hunks(changes: &[Change], context: usize, old_len: usize) -> Vec<Hunk> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut span_start: Option<usize> = None;
    let mut last_visible = 0;
    for (index, change) in changes.iter().enumerate() {
        if change.silent {
            continue;
        }
        match span_start {
            Some(start)
                if change
                    .old
                    .start
                    .saturating_sub(changes[last_visible].old.end)
                    > context.saturating_mul(2) =>
            {
                spans.push(start..last_visible + 1);
                span_start = Some(index);
            }
            Some(_) => {}
            None => span_start = Some(index),
        }
        last_visible = index;
    }
    if let Some(start) = span_start {
        spans.push(start..last_visible + 1);
    }
    spans
        .into_iter()
        .map(|span| hunk_for(changes, span, context, old_len))
        .collect()
}

fn hunk_for(changes: &[Change], span: Range<usize>, context: usize, old_len: usize) -> Hunk {
    let first = &changes[span.start];
    let last = &changes[span.end - 1];
    let before_limit = if span.start == 0 {
        0
    } else {
        changes[span.start - 1].old.end
    };
    let old_start = first.old.start.saturating_sub(context).max(before_limit);
    let after_limit = changes.get(span.end).map_or(old_len, |next| next.old.start);
    let old_end = last.old.end.saturating_add(context).min(after_limit);
    let new_start = first.new.start - (first.old.start - old_start);
    Hunk {
        changes: span,
        old_start,
        old_end,
        new_start,
    }
}

fn render_hunk(
    hunk: &Hunk,
    changes: &[Change],
    old_lines: &[&[u8]],
    new_lines: &[&[u8]],
    colored: bool,
) -> Vec<u8> {
    let mut body = Vec::new();
    let (mut old_count, mut new_count) = (0, 0);
    let mut position = hunk.old_start;
    for change in &changes[hunk.changes.clone()] {
        let unchanged = &old_lines[position..change.old.start];
        for line in unchanged {
            diff_color::body_line(&mut body, colored, b' ', line);
        }
        old_count += unchanged.len();
        new_count += unchanged.len();
        position = change.old.end;
        if change.silent {
            continue;
        }
        for line in &old_lines[change.old.clone()] {
            diff_color::body_line(&mut body, colored, b'-', line);
        }
        for line in &new_lines[change.new.clone()] {
            diff_color::body_line(&mut body, colored, b'+', line);
        }
        old_count += change.old.len();
        new_count += change.new.len();
    }
    let trailing = &old_lines[position..hunk.old_end];
    for line in trailing {
        diff_color::body_line(&mut body, colored, b' ', line);
    }
    old_count += trailing.len();
    new_count += trailing.len();
    let mut text = Vec::new();
    diff_color::hunk_header(
        &mut text,
        colored,
        &format!(
            "@@ -{} +{} @@",
            range_text(hunk.old_start, old_count),
            range_text(hunk.new_start, new_count)
        ),
    );
    text.extend(body);
    text
}

fn range_text(start: usize, count: usize) -> String {
    match count {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        _ => format!("{},{count}", start + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::{report_pair, Options};

    fn names() -> (String, String) {
        ("a".to_string(), "b".to_string())
    }

    fn text_options() -> Options {
        Options {
            context: 3,
            ..Options::default()
        }
    }

    #[test]
    fn unchanged_bytes_produce_no_output() {
        let (status, text) = report_pair(b"x\n", b"x\n", &names(), &text_options());
        assert_eq!(status, 0);
        assert!(text.is_empty());
    }

    #[test]
    fn whitespace_only_change_is_equal_under_w() {
        let options = Options {
            ignore_all_space: true,
            ..text_options()
        };
        let (status, _) = report_pair(b"a  b\n", b"a b\n", &names(), &options);
        assert_eq!(status, 0);
    }

    #[test]
    fn strip_trailing_cr_treats_crlf_as_lf() {
        let options = Options {
            strip_trailing_cr: true,
            ..text_options()
        };
        let (status, _) = report_pair(b"x\r\n", b"x\n", &names(), &options);
        assert_eq!(status, 0);
        let (status, _) = report_pair(b"x\r\n", b"x\n", &names(), &text_options());
        assert_eq!(status, 1);
    }
}
