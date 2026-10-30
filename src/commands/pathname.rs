//! `basename` and `dirname`: purely textual path operations (no file system access).
//! On Windows `\` also separates components and a drive prefix such as `C:` is kept.

use std::io::Write;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const BASENAME_HELP: &str = r#"Usage: basename NAME [SUFFIX]
  or:  basename OPTION... NAME...
Print NAME with any leading directory components removed.
If specified, also remove a trailing SUFFIX.

  -a, --multiple       support multiple arguments and treat each as a NAME
  -s, --suffix=SUFFIX  remove a trailing SUFFIX; implies -a
  -z, --zero           end each output line with NUL, not newline

      --help     display this help and exit
      --version  output version information and exit
"#;

const DIRNAME_HELP: &str = r#"Usage: dirname [OPTION] NAME...
Output each NAME with its last non-slash component and trailing slashes
removed; if NAME contains no /'s, output '.' (meaning the current directory).

  -z, --zero     end each output line with NUL, not newline

      --help     display this help and exit
      --version  output version information and exit
"#;

const BASENAME_SPECS: &[OptSpec] = &[
    OptSpec::flag("multiple", Some('a'), Some("multiple")),
    OptSpec::req("suffix", Some('s'), Some("suffix")),
    OptSpec::flag("zero", Some('z'), Some("zero")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const DIRNAME_SPECS: &[OptSpec] = &[
    OptSpec::flag("zero", Some('z'), Some("zero")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Path syntax used for the textual operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavor {
    Posix,
    Windows,
}

fn native_flavor() -> Flavor {
    if cfg!(windows) {
        Flavor::Windows
    } else {
        Flavor::Posix
    }
}

fn is_sep(c: char, flavor: Flavor) -> bool {
    c == '/' || (flavor == Flavor::Windows && c == '\\')
}

fn trim_separators(text: &str, flavor: Flavor) -> &str {
    text.trim_end_matches(|c: char| is_sep(c, flavor))
}

/// Splits a leading Windows drive designator (`C:`) from `path`. Other paths return an
/// empty drive.
fn split_drive(path: &str, flavor: Flavor) -> (&str, &str) {
    let bytes = path.as_bytes();
    if flavor == Flavor::Windows
        && bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
    {
        (&path[..2], &path[2..])
    } else {
        ("", path)
    }
}

/// Last component of `path` after trailing separators are dropped. A root (or drive root)
/// yields the root itself, as POSIX basename does.
pub(crate) fn base_name(path: &str, flavor: Flavor) -> String {
    let (drive, rest) = split_drive(path, flavor);
    if rest.is_empty() {
        return drive.to_owned();
    }
    let trimmed = trim_separators(rest, flavor);
    if trimmed.is_empty() {
        return format!("{drive}/");
    }
    trimmed
        .rsplit(|c: char| is_sep(c, flavor))
        .next()
        .unwrap_or(trimmed)
        .to_owned()
}

/// Everything before the last component, after trailing separators are dropped.
/// `.` when there is no directory part; `/` when the directory part is the root.
pub(crate) fn dir_name(path: &str, flavor: Flavor) -> String {
    let (drive, rest) = split_drive(path, flavor);
    let trimmed = trim_separators(rest, flavor);
    if trimmed.is_empty() {
        return if rest.is_empty() {
            format!("{drive}.")
        } else {
            format!("{drive}/")
        };
    }
    let Some(index) = trimmed.rfind(|c: char| is_sep(c, flavor)) else {
        return if drive.is_empty() {
            ".".to_owned()
        } else {
            drive.to_owned()
        };
    };
    let dir = trim_separators(&trimmed[..index], flavor);
    if dir.is_empty() {
        format!("{drive}/")
    } else {
        format!("{drive}{dir}")
    }
}

/// Removes `suffix` from `base` unless that would leave nothing or the suffix is empty.
fn strip_suffix<'a>(base: &'a str, suffix: &str) -> &'a str {
    if !suffix.is_empty() && base.len() > suffix.len() && base.ends_with(suffix) {
        &base[..base.len() - suffix.len()]
    } else {
        base
    }
}

/// Windows results print with `/` separators, as `display_path` does elsewhere.
fn display(text: String, flavor: Flavor) -> String {
    if flavor == Flavor::Windows {
        text.replace('\\', "/")
    } else {
        text
    }
}

/// Writes each record followed by NUL (`zero`) or newline. A closed pipe is silent success.
fn emit(cmd: &str, records: &[String], zero: bool) -> i32 {
    let delimiter = if zero { '\0' } else { '\n' };
    let mut out = stdout_writer();
    for record in records {
        let mut line = record.clone();
        line.push(delimiter);
        if let Err(error) = out.write_all(line.as_bytes()) {
            if is_broken_pipe(&error) {
                return EXIT_OK;
            }
            eprintln!("{cmd}: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
    }
    finish_output(cmd, &mut out)
}

fn print_help(text: &str, cmd: &str) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{cmd}: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    finish_output(cmd, &mut out)
}

/// Splits basename's operands into names and an optional suffix, or the usage error text.
fn basename_plan(
    operands: Vec<String>,
    multiple: bool,
    suffix: Option<String>,
) -> Result<(Vec<String>, Option<String>), String> {
    if multiple {
        return Ok((operands, suffix));
    }
    match operands.len() {
        1 => Ok((operands, None)),
        2 => {
            let suffix = operands.get(1).cloned();
            Ok((vec![operands[0].clone()], suffix))
        }
        _ => Err(format!("extra operand '{}'", operands[2])),
    }
}

pub fn basename(args: &[String]) -> i32 {
    let items = match Parser::new("basename", BASENAME_SPECS)
        .no_permute()
        .parse(args)
    {
        Ok(items) => items,
        Err(error) => return report_usage("basename", &error, EXIT_FAIL),
    };
    let (mut multiple, mut suffix, mut zero) = (false, None, false);
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return print_help(BASENAME_HELP, "basename"),
            Item::Opt { id: "version", .. } => return print_version("basename"),
            Item::Opt { id: "multiple", .. } => multiple = true,
            Item::Opt {
                id: "suffix",
                value,
            } => {
                multiple = true;
                suffix = value;
            }
            Item::Opt { id: "zero", .. } => zero = true,
            Item::Opt { .. } => {}
            Item::Operand(text) => operands.push(text),
        }
    }
    if operands.is_empty() {
        return report_usage(
            "basename",
            &UsageError {
                message: "missing operand".to_owned(),
            },
            EXIT_FAIL,
        );
    }
    let (names, suffix) = match basename_plan(operands, multiple, suffix) {
        Ok(plan) => plan,
        Err(message) => {
            return report_usage("basename", &UsageError { message }, EXIT_FAIL);
        }
    };
    let flavor = native_flavor();
    let records: Vec<String> = names
        .iter()
        .map(|name| {
            let base = base_name(name, flavor);
            let trimmed = match &suffix {
                Some(sfx) => strip_suffix(&base, sfx).to_owned(),
                None => base,
            };
            display(trimmed, flavor)
        })
        .collect();
    emit("basename", &records, zero)
}

pub fn dirname(args: &[String]) -> i32 {
    let items = match Parser::new("dirname", DIRNAME_SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("dirname", &error, EXIT_FAIL),
    };
    let mut zero = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return print_help(DIRNAME_HELP, "dirname"),
            Item::Opt { id: "version", .. } => return print_version("dirname"),
            Item::Opt { id: "zero", .. } => zero = true,
            Item::Opt { .. } => {}
            Item::Operand(text) => operands.push(text),
        }
    }
    if operands.is_empty() {
        return report_usage(
            "dirname",
            &UsageError {
                message: "missing operand".to_owned(),
            },
            EXIT_FAIL,
        );
    }
    let flavor = native_flavor();
    let records: Vec<String> = operands
        .iter()
        .map(|name| display(dir_name(name, flavor), flavor))
        .collect();
    emit("dirname", &records, zero)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Flavor = Flavor::Posix;
    const W: Flavor = Flavor::Windows;

    #[test]
    fn basename_drops_directories_and_trailing_slashes() {
        assert_eq!(base_name("/usr/lib/", P), "lib");
        assert_eq!(base_name("a/b", P), "b");
        assert_eq!(base_name("file", P), "file");
        assert_eq!(base_name("", P), "");
        assert_eq!(base_name("//", P), "/");
        assert_eq!(base_name("/", P), "/");
    }

    #[test]
    fn dirname_follows_posix_rules() {
        let cases = [
            ("a/b/", "a"),
            ("c", "."),
            ("", "."),
            ("//a", "/"),
            ("///", "/"),
            ("a//", "."),
            ("x/y/z//w", "x/y/z"),
            ("./", "."),
            ("/a", "/"),
            ("///x", "/"),
        ];
        for (input, expected) in cases {
            assert_eq!(dir_name(input, P), expected, "dirname {input:?}");
        }
    }

    #[test]
    fn suffix_is_kept_when_it_would_remove_the_whole_name() {
        assert_eq!(strip_suffix(".txt", ".txt"), ".txt");
        assert_eq!(strip_suffix("foo.txt", ".txt"), "foo");
        assert_eq!(strip_suffix("foo", ""), "foo");
        assert_eq!(strip_suffix("foo", "x"), "foo");
    }

    #[test]
    fn windows_drive_roots_and_backslashes() {
        assert_eq!(base_name("C:/", W), "C:/");
        assert_eq!(base_name(r"C:\", W), "C:/");
        assert_eq!(base_name(r"C:\dir\file.txt", W), "file.txt");
        assert_eq!(base_name("C:foo", W), "foo");
        assert_eq!(dir_name("C:foo", W), "C:");
        assert_eq!(dir_name(r"C:\dir\file", W), r"C:\dir");
        assert_eq!(display(dir_name(r"C:\dir\file", W), W), "C:/dir");
        assert_eq!(dir_name("C:/", W), "C:/");
        assert_eq!(dir_name(r"a\b", W), "a");
        assert_eq!(display(r"C:\x".to_owned(), W), "C:/x");
    }

    #[test]
    fn two_operand_form_uses_second_operand_as_suffix() {
        let ops = vec!["a.c".to_owned(), ".c".to_owned()];
        assert_eq!(
            basename_plan(ops, false, None),
            Ok((vec!["a.c".to_owned()], Some(".c".to_owned())))
        );
        let extra = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_eq!(
            basename_plan(extra, false, None),
            Err("extra operand 'c'".to_owned())
        );
    }

    #[test]
    fn multiple_mode_keeps_every_operand() {
        let ops = vec!["x/a".to_owned(), "y/b".to_owned()];
        assert_eq!(
            basename_plan(ops.clone(), true, Some(".b".to_owned())),
            Ok((ops, Some(".b".to_owned())))
        );
    }
}
