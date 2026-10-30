//! `md5sum`, `sha1sum`, `sha224sum`, `sha256sum`, `sha384sum`, `sha512sum` and `b2sum`:
//! print or verify message digests in GNU and BSD line formats.

pub mod check;
pub mod hasher;

use std::io::Write;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::escape::quote_name;
use crate::common::io::{finish_output, open_input, stdout_writer};
use check::CheckOpts;
use hasher::{digest_reader, to_hex, Algo};

const MAX_BLAKE2B_BYTES: usize = 64;

/// One digest tool: its program name, algorithm and whether it takes `-l`.
#[derive(Clone, Copy)]
struct Tool {
    name: &'static str,
    algo: Algo,
    blake2: bool,
}

const OPTIONS: &[OptSpec] = &[
    OptSpec::flag("binary", Some('b'), Some("binary")),
    OptSpec::flag("check", Some('c'), Some("check")),
    OptSpec::flag("tag", None, Some("tag")),
    OptSpec::flag("text", Some('t'), Some("text")),
    OptSpec::flag("zero", Some('z'), Some("zero")),
    OptSpec::flag("ignore-missing", None, Some("ignore-missing")),
    OptSpec::flag("quiet", None, Some("quiet")),
    OptSpec::flag("status", None, Some("status")),
    OptSpec::flag("strict", None, Some("strict")),
    OptSpec::flag("warn", Some('w'), Some("warn")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// The flags of one invocation.
#[derive(Default)]
struct Flags {
    binary: bool,
    check: bool,
    tag: bool,
    zero: bool,
    ignore_missing: bool,
    quiet: bool,
    status: bool,
    strict: bool,
    warn: bool,
}

pub fn md5sum(args: &[String]) -> i32 {
    run(fixed("md5sum", Algo::Md5), args)
}

pub fn sha1sum(args: &[String]) -> i32 {
    run(fixed("sha1sum", Algo::Sha1), args)
}

pub fn sha224sum(args: &[String]) -> i32 {
    run(fixed("sha224sum", Algo::Sha224), args)
}

pub fn sha256sum(args: &[String]) -> i32 {
    run(fixed("sha256sum", Algo::Sha256), args)
}

pub fn sha384sum(args: &[String]) -> i32 {
    run(fixed("sha384sum", Algo::Sha384), args)
}

pub fn sha512sum(args: &[String]) -> i32 {
    run(fixed("sha512sum", Algo::Sha512), args)
}

pub fn b2sum(args: &[String]) -> i32 {
    let tool = Tool {
        name: "b2sum",
        algo: Algo::Blake2b(MAX_BLAKE2B_BYTES),
        blake2: true,
    };
    run(tool, args)
}

fn fixed(name: &'static str, algo: Algo) -> Tool {
    Tool {
        name,
        algo,
        blake2: false,
    }
}

/// Escapes a name the GNU way: a backslash and newline become `\\` and `\n`, and the
/// line then starts with a backslash. Returns whether the line needs that prefix.
pub(crate) fn escape_name(name: &str, zero: bool) -> (bool, String) {
    if zero || !(name.contains('\\') || name.contains('\n')) {
        return (false, name.to_owned());
    }
    let mut out = String::with_capacity(name.len() + 2);
    for c in name.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    (true, out)
}

fn run(tool: Tool, args: &[String]) -> i32 {
    let mut specs = OPTIONS.to_vec();
    if tool.blake2 {
        specs.push(OptSpec::req("length", Some('l'), Some("length")));
    }
    let items = match Parser::new(tool.name, &specs).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(tool.name, &error, EXIT_FAIL),
    };
    let mut flags = Flags::default();
    let mut algo = tool.algo;
    let mut files = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{}", help_text(&tool));
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(tool.name),
            Item::Opt {
                id: "length",
                value,
            } => match parse_length(tool.name, value.as_deref().unwrap_or("")) {
                Ok(bytes) => algo = Algo::Blake2b(bytes),
                Err(()) => return EXIT_FAIL,
            },
            Item::Opt { id, .. } => set_flag(&mut flags, id),
            Item::Operand(file) => files.push(file),
        }
    }
    if let Err(message) = validate(&flags) {
        return usage_failure(tool.name, &message);
    }
    if files.is_empty() {
        files.push("-".to_owned());
    }
    if flags.check {
        let opts = CheckOpts {
            cmd: tool.name,
            forced: Some(algo),
            variable_length: tool.blake2,
            zero: flags.zero,
            quiet: flags.quiet,
            status: flags.status,
            strict: flags.strict,
            warn: flags.warn,
            ignore_missing: flags.ignore_missing,
        };
        let mut out = stdout_writer();
        let code = check::run(&opts, &files, &mut out);
        return merge(code, finish_output(tool.name, &mut out));
    }
    generate(tool.name, algo, &flags, &files)
}

fn set_flag(flags: &mut Flags, id: &str) {
    match id {
        "binary" => flags.binary = true,
        "check" => flags.check = true,
        "tag" => flags.tag = true,
        "zero" => flags.zero = true,
        "ignore-missing" => flags.ignore_missing = true,
        "quiet" => flags.quiet = true,
        "status" => flags.status = true,
        "strict" => flags.strict = true,
        "warn" => flags.warn = true,
        _ => {}
    }
}

/// GNU rejects the options that only make sense with `--check`, and the reverse.
fn validate(flags: &Flags) -> Result<(), String> {
    if flags.check {
        if flags.tag {
            return Err("the --tag option is meaningless when verifying checksums".into());
        }
        if flags.zero {
            return Err("the --zero option is not supported when verifying checksums".into());
        }
        return Ok(());
    }
    let checks = [
        (flags.ignore_missing, "ignore-missing"),
        (flags.quiet, "quiet"),
        (flags.status, "status"),
        (flags.strict, "strict"),
        (flags.warn, "warn"),
    ];
    match checks.iter().find(|(set, _)| *set) {
        Some((_, name)) => Err(format!(
            "the --{name} option is meaningful only when verifying checksums"
        )),
        None => Ok(()),
    }
}

fn usage_failure(cmd: &str, message: &str) -> i32 {
    let error = UsageError {
        message: message.to_owned(),
    };
    report_usage(cmd, &error, EXIT_FAIL)
}

fn merge(first: i32, second: i32) -> i32 {
    if first != EXIT_OK {
        first
    } else {
        second
    }
}

/// Parses `-l BITS`. Errors print GNU's messages and return `Err(())`.
pub(crate) fn parse_length(cmd: &str, text: &str) -> Result<usize, ()> {
    let invalid = || {
        eprintln!("{cmd}: invalid length: '{text}'");
        Err(())
    };
    let Ok(bits) = text.parse::<u64>() else {
        return invalid();
    };
    if bits == 0 {
        return Ok(MAX_BLAKE2B_BYTES);
    }
    if bits > 512 {
        eprintln!("{cmd}: invalid length: '{text}'");
        eprintln!("{cmd}: maximum digest length for 'BLAKE2b' is 512 bits");
        return Err(());
    }
    if bits % 8 != 0 {
        eprintln!("{cmd}: invalid length: '{text}'");
        eprintln!("{cmd}: length is not a multiple of 8");
        return Err(());
    }
    Ok((bits / 8) as usize)
}

/// Prints `HASH  NAME` lines (or BSD-style with `--tag`) for every file.
fn generate(cmd: &str, algo: Algo, flags: &Flags, files: &[String]) -> i32 {
    let mut out = stdout_writer();
    let mut code = EXIT_OK;
    let terminator = if flags.zero { '\0' } else { '\n' };
    for file in files {
        let mut reader = match open_input(file) {
            Ok(reader) => reader,
            Err(error) => {
                eprintln!("{cmd}: {}: {}", quote_name(file), io_error_text(&error));
                code = EXIT_FAIL;
                continue;
            }
        };
        let digest = match digest_reader(algo, reader.as_mut()) {
            Ok((digest, _)) => to_hex(&digest),
            Err(error) => {
                eprintln!("{cmd}: {}: {}", quote_name(file), io_error_text(&error));
                code = EXIT_FAIL;
                continue;
            }
        };
        let (escaped, body) = escape_name(file, flags.zero);
        let prefix = if escaped { "\\" } else { "" };
        let line = if flags.tag {
            format!("{prefix}{} ({body}) = {digest}{terminator}", algo.tag())
        } else {
            let mark = if flags.binary { '*' } else { ' ' };
            format!("{prefix}{digest} {mark}{body}{terminator}")
        };
        if out.write_all(line.as_bytes()).is_err() {
            break;
        }
    }
    merge(code, finish_output(cmd, &mut out))
}

fn help_text(tool: &Tool) -> String {
    let (desc, rfc) = match tool.algo {
        Algo::Blake2b(_) => ("BLAKE2b (512-bit)", "RFC 7693"),
        Algo::Md5 => ("MD5 (128-bit)", "RFC 1321"),
        Algo::Sha1 => ("SHA1 (160-bit)", "FIPS 180-4"),
        Algo::Sha224 => ("SHA224 (224-bit)", "FIPS 180-4"),
        Algo::Sha256 => ("SHA256 (256-bit)", "FIPS 180-4"),
        Algo::Sha384 => ("SHA384 (384-bit)", "FIPS 180-4"),
        Algo::Sha512 => ("SHA512 (512-bit)", "FIPS 180-4"),
    };
    let name = tool.name;
    let length = if tool.blake2 {
        "  -l, --length=BITS     digest length in bits; must not exceed the max for\n\
         \x20                       the blake2 algorithm and must be a multiple of 8\n"
    } else {
        ""
    };
    format!(
        "Usage: {name} [OPTION]... [FILE]...\n\
         Print or check {desc} checksums.\n\
         \n\
         With no FILE, or when FILE is -, read standard input.\n\
         \x20 -b, --binary          read in binary mode\n\
         \x20 -c, --check           read checksums from the FILEs and check them\n\
         {length}\
         \x20     --tag             create a BSD-style checksum\n\
         \x20 -t, --text            read in text mode (default)\n\
         \x20 -z, --zero            end each output line with NUL, not newline,\n\
         \x20                         and disable file name escaping\n\
         \n\
         The following five options are useful only when verifying checksums:\n\
         \x20     --ignore-missing  don't fail or report status for missing files\n\
         \x20     --quiet           don't print OK for each successfully verified file\n\
         \x20     --status          don't output anything, status code shows success\n\
         \x20     --strict          exit non-zero for improperly formatted checksum lines\n\
         \x20 -w, --warn            warn about improperly formatted checksum lines\n\
         \n\
         \x20     --help        display this help and exit\n\
         \x20     --version     output version information and exit\n\
         \n\
         The sums are computed as described in {rfc}.\n\
         When checking, the input should be a former output of this program.\n\
         The default mode is to print a line with: checksum, a space,\n\
         a character indicating input mode ('*' for binary, ' ' for text\n\
         or where binary is insignificant), and name for each FILE.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_backslash_and_newline_only() {
        assert_eq!(escape_name("plain", false), (false, "plain".to_owned()));
        assert_eq!(escape_name("a\\b", false), (true, "a\\\\b".to_owned()));
        assert_eq!(escape_name("a\nb", false), (true, "a\\nb".to_owned()));
        assert_eq!(escape_name("a\nb", true), (false, "a\nb".to_owned()));
    }

    #[test]
    fn length_follows_gnu_rules() {
        assert_eq!(parse_length("b2sum", "0"), Ok(64));
        assert_eq!(parse_length("b2sum", "256"), Ok(32));
        assert_eq!(parse_length("b2sum", "8"), Ok(1));
        assert_eq!(parse_length("b2sum", "7"), Err(()));
        assert_eq!(parse_length("b2sum", "520"), Err(()));
        assert_eq!(parse_length("b2sum", "x"), Err(()));
    }

    #[test]
    fn validation_rejects_misplaced_flags() {
        let mut flags = Flags {
            status: true,
            ..Flags::default()
        };
        assert!(validate(&flags).is_err());
        flags.check = true;
        assert!(validate(&flags).is_ok());
        flags.zero = true;
        assert!(validate(&flags).is_err());
    }

    #[test]
    fn help_lists_length_only_for_b2sum() {
        let b2 = Tool {
            name: "b2sum",
            algo: Algo::Blake2b(64),
            blake2: true,
        };
        assert!(help_text(&b2).contains("--length=BITS"));
        assert!(!help_text(&fixed("md5sum", Algo::Md5)).contains("--length"));
    }
}
