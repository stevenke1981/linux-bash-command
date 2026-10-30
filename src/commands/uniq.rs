//! `uniq`: filters adjacent matching lines. Keys can skip leading fields and characters,
//! compare a limited width, and ignore case. Modes: plain, `-c` counts, `-d` duplicates,
//! `-u` unique lines, `-D`/`--all-repeated` every duplicated line, `--group` every line
//! with separators. Specification: docs/spec/coreutils-alignment/03-commands-core.md.

use std::fs::File;
use std::io::{self, BufRead, BufWriter, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const CMD: &str = "uniq";

const OPTS: &[OptSpec] = &[
    OptSpec::flag("c", Some('c'), Some("count")),
    OptSpec::flag("d", Some('d'), Some("repeated")),
    OptSpec::flag("D", Some('D'), None),
    OptSpec::opt("all-repeated", None, Some("all-repeated")),
    OptSpec::opt("group", None, Some("group")),
    OptSpec::req("f", Some('f'), Some("skip-fields")),
    OptSpec::flag("i", Some('i'), Some("ignore-case")),
    OptSpec::req("s", Some('s'), Some("skip-chars")),
    OptSpec::flag("u", Some('u'), Some("unique")),
    OptSpec::flag("z", Some('z'), Some("zero-terminated")),
    OptSpec::req("w", Some('w'), Some("check-chars")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Blank separators around the groups printed by `-D` and `--all-repeated`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AllRepeated {
    None,
    Prepend,
    Separate,
}

/// Blank separators around the groups printed by `--group`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupSep {
    Separate,
    Prepend,
    Append,
    Both,
}

#[derive(Debug)]
struct Settings {
    count: bool,
    repeated: bool,
    unique_only: bool,
    all_repeated: Option<AllRepeated>,
    group: Option<GroupSep>,
    ignore_case: bool,
    skip_fields: usize,
    skip_chars: usize,
    check_chars: Option<usize>,
    delim: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            count: false,
            repeated: false,
            unique_only: false,
            all_repeated: None,
            group: None,
            ignore_case: false,
            skip_fields: 0,
            skip_chars: 0,
            check_chars: None,
            delim: b'\n',
        }
    }
}

/// Entry point for `uniq` and the `agentcmd uniq` multi-call form.
pub fn run(args: &[String]) -> i32 {
    let args = expand_obsolete(args);
    let items = match Parser::new(CMD, OPTS).parse(&args) {
        Ok(items) => items,
        Err(err) => return report_usage(CMD, &err, EXIT_FAIL),
    };
    let mut operands = Vec::new();
    let mut settings = Settings::default();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => return write_help(),
                "version" => return print_version(CMD),
                _ => {
                    if let Err(code) = apply(&mut settings, id, value.as_deref()) {
                        return code;
                    }
                }
            },
        }
    }
    if let Some(code) = check_combinations(&settings) {
        return code;
    }
    if operands.len() > 2 {
        eprintln!(
            "{CMD}: extra operand '{}'\nTry '{CMD} --help' for more information.",
            operands[2]
        );
        return EXIT_FAIL;
    }
    run_files(&settings, &operands)
}

fn write_help() -> i32 {
    let mut out = stdout_writer();
    if out.write_all(HELP.as_bytes()).is_err() {
        return EXIT_OK;
    }
    finish_output(CMD, &mut out)
}

/// Turns the obsolete `-N` (skip N fields) and `+N` (skip N characters) forms into the
/// option forms. A value that follows `-f`, `-s` or `-w` is never reinterpreted.
fn expand_obsolete(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut after_double_dash = false;
    let mut expect_value = false;
    for arg in args {
        if after_double_dash || expect_value {
            expect_value = false;
            out.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_double_dash = true;
        } else if let Some(digits) = obsolete_digits(arg, '-') {
            out.extend(["-f".to_owned(), digits.to_owned()]);
            continue;
        } else if let Some(digits) = obsolete_digits(arg, '+') {
            out.extend(["-s".to_owned(), digits.to_owned()]);
            continue;
        }
        expect_value = matches!(arg.as_str(), "-f" | "-s" | "-w");
        out.push(arg.clone());
    }
    out
}

fn obsolete_digits(arg: &str, sign: char) -> Option<&str> {
    let digits = arg.strip_prefix(sign)?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

fn apply(settings: &mut Settings, id: &str, value: Option<&str>) -> Result<(), i32> {
    match id {
        "c" => settings.count = true,
        "d" => settings.repeated = true,
        "D" => settings.all_repeated = Some(AllRepeated::None),
        "all-repeated" => {
            settings.all_repeated = Some(match value {
                None | Some("none") => AllRepeated::None,
                Some("prepend") => AllRepeated::Prepend,
                Some("separate") => AllRepeated::Separate,
                Some(other) => {
                    return Err(invalid_choice(
                        other,
                        "--all-repeated",
                        &["none", "prepend", "separate"],
                    ))
                }
            });
        }
        "group" => {
            settings.group = Some(match value {
                None | Some("separate") => GroupSep::Separate,
                Some("prepend") => GroupSep::Prepend,
                Some("append") => GroupSep::Append,
                Some("both") => GroupSep::Both,
                Some(other) => {
                    return Err(invalid_choice(
                        other,
                        "--group",
                        &["separate", "prepend", "append", "both"],
                    ))
                }
            });
        }
        "f" => settings.skip_fields = parse_count(value, "fields to skip")?,
        "s" => settings.skip_chars = parse_count(value, "bytes to skip")?,
        "w" => settings.check_chars = Some(parse_count(value, "bytes to compare")?),
        "i" => settings.ignore_case = true,
        "u" => settings.unique_only = true,
        "z" => settings.delim = 0,
        _ => {}
    }
    Ok(())
}

fn invalid_choice(value: &str, opt: &str, valid: &[&str]) -> i32 {
    let mut message =
        format!("{CMD}: invalid argument '{value}' for '{opt}'\nValid arguments are:");
    for word in valid {
        message.push_str(&format!("\n  - '{word}'"));
    }
    eprintln!("{message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

fn parse_count(value: Option<&str>, what: &str) -> Result<usize, i32> {
    let text = value.unwrap_or_default();
    match text.parse::<usize>() {
        Ok(n) if text.bytes().all(|b| b.is_ascii_digit()) => Ok(n),
        _ => {
            eprintln!("{CMD}: {text}: invalid number of {what}");
            Err(EXIT_FAIL)
        }
    }
}

/// Mode combinations that GNU rejects.
fn check_combinations(settings: &Settings) -> Option<i32> {
    let message = if settings.group.is_some()
        && (settings.count
            || settings.repeated
            || settings.unique_only
            || settings.all_repeated.is_some())
    {
        "--group is mutually exclusive with -c/-d/-D/-u"
    } else if settings.all_repeated.is_some() && settings.count {
        "printing all duplicated lines and repeat counts is meaningless"
    } else {
        return None;
    };
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    Some(EXIT_FAIL)
}

fn run_files(settings: &Settings, operands: &[String]) -> i32 {
    let input = operands.first().map_or("-", String::as_str);
    let mut reader = match open_input(input) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("{CMD}: {input}: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
    };
    let mut out: Box<dyn Write> = match operands.get(1) {
        None => Box::new(stdout_writer()),
        Some(path) => match File::create(path) {
            Ok(file) => Box::new(BufWriter::new(file)),
            Err(error) => {
                eprintln!("{CMD}: {path}: {}", io_error_text(&error));
                return EXIT_FAIL;
            }
        },
    };
    match process(reader.as_mut(), input, settings, out.as_mut()) {
        Ok(()) => finish_output(CMD, &mut out),
        Err(Failure::Read(message)) => {
            eprintln!("{CMD}: {message}");
            EXIT_FAIL
        }
        Err(Failure::Write(error)) if is_broken_pipe(&error) => EXIT_OK,
        Err(Failure::Write(error)) => {
            eprintln!("{CMD}: write error: {}", io_error_text(&error));
            EXIT_FAIL
        }
    }
}

/// Why processing stopped: reading the input or writing the output.
enum Failure {
    Read(String),
    Write(io::Error),
}

/// Collects one group at a time (adjacent lines with equal keys) and prints it.
fn process(
    reader: &mut dyn BufRead,
    name: &str,
    settings: &Settings,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let keep_all = settings.group.is_some() || settings.all_repeated.is_some();
    let mut group = Group::default();
    let mut printed_groups = 0usize;
    let mut line = Vec::new();
    loop {
        match read_record(reader, settings.delim, &mut line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => {
                return Err(Failure::Read(format!(
                    "{name}: read error: {}",
                    io_error_text(&error)
                )))
            }
        }
        if line.last() == Some(&settings.delim) {
            line.pop();
        }
        if group
            .first
            .as_ref()
            .is_some_and(|first| same_key(first, &line, settings))
        {
            group.count += 1;
            if keep_all {
                group.lines.push(line.clone());
            }
        } else {
            if group.first.is_some() {
                emit_group(&group, settings, out, &mut printed_groups).map_err(Failure::Write)?;
            }
            group = Group {
                first: Some(line.clone()),
                count: 1,
                lines: if keep_all {
                    vec![line.clone()]
                } else {
                    Vec::new()
                },
            };
        }
    }
    if group.first.is_some() {
        emit_group(&group, settings, out, &mut printed_groups).map_err(Failure::Write)?;
    }
    Ok(())
}

/// The group being collected: its first line, its size, and (when needed) all its lines.
#[derive(Default)]
struct Group {
    first: Option<Vec<u8>>,
    count: u64,
    lines: Vec<Vec<u8>>,
}

/// True when two lines fall in the same group under the skip, width and case options.
fn same_key(a: &[u8], b: &[u8], settings: &Settings) -> bool {
    let (ka, kb) = (key_of(a, settings), key_of(b, settings));
    if ka.len() != kb.len() {
        return false;
    }
    if settings.ignore_case {
        ka.eq_ignore_ascii_case(kb)
    } else {
        ka == kb
    }
}

/// The part of `line` that is compared: skip whole fields, then characters, then keep at
/// most `-w` bytes. A field is a run of blanks followed by a run of non-blanks.
fn key_of<'a>(line: &'a [u8], settings: &Settings) -> &'a [u8] {
    let limit = line.len();
    let mut pos = 0;
    for _ in 0..settings.skip_fields {
        if pos >= limit {
            break;
        }
        while pos < limit && is_blank(line[pos]) {
            pos += 1;
        }
        while pos < limit && !is_blank(line[pos]) {
            pos += 1;
        }
    }
    if limit - pos < settings.skip_chars {
        return &line[limit..];
    }
    pos += settings.skip_chars;
    let rest = &line[pos..];
    match settings.check_chars {
        Some(width) => &rest[..width.min(rest.len())],
        None => rest,
    }
}

fn is_blank(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// Prints one finished group according to the selected mode.
fn emit_group(
    group: &Group,
    settings: &Settings,
    out: &mut dyn Write,
    printed: &mut usize,
) -> io::Result<()> {
    let delim = settings.delim;
    if let Some(mode) = settings.group {
        let before = match mode {
            GroupSep::Prepend => true,
            GroupSep::Both => *printed == 0,
            GroupSep::Separate => *printed > 0,
            GroupSep::Append => false,
        };
        if before {
            out.write_all(&[delim])?;
        }
        for line in &group.lines {
            write_line(out, line, delim)?;
        }
        if matches!(mode, GroupSep::Append | GroupSep::Both) {
            out.write_all(&[delim])?;
        }
        *printed += 1;
        return Ok(());
    }
    if let Some(mode) = settings.all_repeated {
        if group.count > 1 {
            if mode == AllRepeated::Prepend || (mode == AllRepeated::Separate && *printed > 0) {
                out.write_all(&[delim])?;
            }
            for line in &group.lines {
                write_line(out, line, delim)?;
            }
            *printed += 1;
        }
        return Ok(());
    }
    let show = if settings.repeated {
        group.count > 1
    } else if settings.unique_only {
        group.count == 1
    } else {
        true
    };
    if show {
        if settings.count {
            write!(out, "{:>7} ", group.count)?;
        }
        if let Some(first) = &group.first {
            write_line(out, first, delim)?;
        }
    }
    Ok(())
}

fn write_line(out: &mut dyn Write, line: &[u8], delim: u8) -> io::Result<()> {
    out.write_all(line)?;
    out.write_all(&[delim])
}

const HELP: &str = "\
Usage: uniq [OPTION]... [INPUT [OUTPUT]]
Filter adjacent matching lines from INPUT (or standard input),
writing to OUTPUT (or standard output).

With no options, matching lines are merged to the first occurrence.

Mandatory arguments to long options are mandatory for short options too.
  -c, --count           prefix lines by the number of occurrences
  -d, --repeated        only print duplicate lines, one for each group
  -D                    print all duplicate lines
      --all-repeated[=METHOD]  like -D, but allow separating groups
                                 with an empty line;
                                 METHOD={none(default),prepend,separate}
  -f, --skip-fields=N   avoid comparing the first N fields
      --group[=METHOD]  show all items, separating groups with an empty line;
                          METHOD={separate(default),prepend,append,both}
  -i, --ignore-case     ignore differences in case when comparing
  -s, --skip-chars=N    avoid comparing the first N characters
  -u, --unique          only print unique lines
  -z, --zero-terminated     line delimiter is NUL, not newline
  -w, --check-chars=N   compare no more than N characters in lines
      --help        display this help and exit
      --version     output version information and exit

A field is a run of blanks (usually spaces and/or TABs), then non-blank
characters.  Fields are skipped before chars.

Note: 'uniq' does not detect repeated lines unless they are adjacent.
You may want to sort the input first, or use 'sort -u' without 'uniq'.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with(f: impl FnOnce(&mut Settings)) -> Settings {
        let mut settings = Settings::default();
        f(&mut settings);
        settings
    }

    fn run_on(settings: &Settings, input: &[u8]) -> Vec<u8> {
        let mut reader: &[u8] = input;
        let mut out = Vec::new();
        process(&mut reader, "-", settings, &mut out).ok().unwrap();
        out
    }

    #[test]
    fn plain_mode_keeps_first_of_each_run() {
        let out = run_on(&Settings::default(), b"a\na\nb\nc\nc\n");
        assert_eq!(out, b"a\nb\nc\n");
    }

    #[test]
    fn counts_use_seven_wide_field() {
        let settings = settings_with(|s| s.count = true);
        assert_eq!(run_on(&settings, b"a\na\nb\n"), b"      2 a\n      1 b\n");
    }

    #[test]
    fn repeated_and_unique_select_groups() {
        let input = b"a\na\nb\nc\nc\nc\nd\n";
        let repeated = settings_with(|s| s.repeated = true);
        assert_eq!(run_on(&repeated, input), b"a\nc\n");
        let unique = settings_with(|s| s.unique_only = true);
        assert_eq!(run_on(&unique, input), b"b\nd\n");
    }

    #[test]
    fn all_repeated_separate_puts_blank_between_groups() {
        let settings = settings_with(|s| s.all_repeated = Some(AllRepeated::Separate));
        assert_eq!(run_on(&settings, b"a\na\nb\nc\nc\n"), b"a\na\n\nc\nc\n");
    }

    #[test]
    fn group_both_wraps_every_group() {
        let settings = settings_with(|s| s.group = Some(GroupSep::Both));
        assert_eq!(run_on(&settings, b"a\nb\n"), b"\na\n\nb\n\n");
    }

    #[test]
    fn skip_fields_chars_and_width_shape_the_key() {
        let fields = settings_with(|s| s.skip_fields = 1);
        assert_eq!(run_on(&fields, b"x a\ny a\nz b\n"), b"x a\nz b\n");
        let chars = settings_with(|s| {
            s.skip_chars = 1;
            s.check_chars = Some(2);
        });
        assert_eq!(run_on(&chars, b"abc\nxbc\nxyz\n"), b"abc\nxyz\n");
    }

    #[test]
    fn obsolete_forms_are_expanded() {
        let args: Vec<String> = ["-1", "+2", "-f", "-1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(expand_obsolete(&args), ["-f", "1", "-s", "2", "-f", "-1"]);
    }

    #[test]
    fn ignore_case_compares_ascii_letters() {
        let settings = settings_with(|s| {
            s.ignore_case = true;
            s.count = true;
        });
        assert_eq!(run_on(&settings, b"A\na\nb\n"), b"      2 A\n      1 b\n");
    }
}
