//! `truncate`: shrink or extend files to a given size. Sizes may be relative (`+N`, `-N`),
//! bounds (`<N`, `>N`) or rounding (`/N`, `%N`), and may carry K/M/G… suffixes in GNU's
//! 1024-based (`K`, `KiB`) or 1000-based (`KB`) forms.

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

use crate::commands::fsops::helpers::{os_text, print_help};

const HELP: &str = r#"Usage: truncate [OPTION]... FILE...
Shrink or extend the size of each FILE to the specified size

A FILE argument that does not exist is created.

If a FILE is larger than the specified size, the extra data is lost.
If a FILE is shorter, it is extended and the extended part reads as zero bytes.

A SIZE argument is an integer and optional unit (example: 10K is 10*1024).
Units are K,M,G,T,P,E,Z,Y,R,Q (powers of 1024) or KB,MB,... (powers of 1000).
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.

SIZE may also be prefixed by one of the following modifying characters:
'+' extend by, '-' reduce by, '<' at most, '>' at least,
'/' round down to multiple of, '%' round up to multiple of.

  -c, --no-create        do not create any files
  -o, --io-blocks        treat SIZE as number of IO blocks instead of bytes
  -r, --reference=RFILE  base size on RFILE
  -s, --size=SIZE        set or adjust the file size by SIZE which is
                           interpreted as bytes unless suffixed
      --help             display this help and exit
      --version          output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("no-create", Some('c'), Some("no-create")),
    OptSpec::flag("io-blocks", Some('o'), Some("io-blocks")),
    OptSpec::req("reference", Some('r'), Some("reference")),
    OptSpec::req("size", Some('s'), Some("size")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// How a size argument changes the current size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Set,
    Plus,
    Minus,
    AtMost,
    AtLeast,
    RoundDown,
    RoundUp,
}

/// A parsed `--size` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub op: Op,
    pub value: u64,
}

/// Parses `[+-<>/%]NUMBER[SUFFIX]`. Errors carry GNU's text without the `truncate: ` prefix.
pub fn parse_size(text: &str) -> Result<Size, String> {
    let invalid = || format!("Invalid number: '{text}'");
    let (op, body) = match text.chars().next() {
        Some('+') => (Op::Plus, &text[1..]),
        Some('-') => (Op::Minus, &text[1..]),
        Some('<') => (Op::AtMost, &text[1..]),
        Some('>') => (Op::AtLeast, &text[1..]),
        Some('/') => (Op::RoundDown, &text[1..]),
        Some('%') => (Op::RoundUp, &text[1..]),
        _ => (Op::Set, text),
    };
    let digit_end = body
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(body.len());
    if digit_end == 0 {
        return Err(invalid());
    }
    let number = &body[..digit_end];
    let multiplier = suffix_multiplier(&body[digit_end..]).ok_or_else(invalid)?;
    let overflow = || format!("Invalid number: '{text}': Value too large for defined data type");
    let base: u128 = number.parse().map_err(|_| overflow())?;
    let value = base.checked_mul(multiplier).ok_or_else(overflow)?;
    let value = u64::try_from(value).map_err(|_| overflow())?;
    if matches!(op, Op::RoundDown | Op::RoundUp) && value == 0 {
        return Err(invalid());
    }
    Ok(Size { op, value })
}

/// The factor a unit suffix stands for. `None` for an unknown suffix.
fn suffix_multiplier(suffix: &str) -> Option<u128> {
    if suffix.is_empty() {
        return Some(1);
    }
    if suffix == "B" {
        return Some(1);
    }
    let mut chars = suffix.chars();
    let letter = chars.next()?;
    let power = match letter {
        'K' => 1,
        'M' => 2,
        'G' => 3,
        'T' => 4,
        'P' => 5,
        'E' => 6,
        'Z' => 7,
        'Y' => 8,
        'R' => 9,
        'Q' => 10,
        _ => return None,
    };
    match chars.as_str() {
        "" | "iB" => Some(1024u128.pow(power)),
        "B" => Some(1000u128.pow(power)),
        _ => None,
    }
}

/// The size a file should have after applying `size`. `block` scales the value for `-o`.
pub fn new_size(current: u64, size: Size, block: u64) -> u64 {
    let value = size.value.saturating_mul(block);
    match size.op {
        Op::Set => value,
        Op::Plus => current.saturating_add(value),
        Op::Minus => current.saturating_sub(value),
        Op::AtMost => current.min(value),
        Op::AtLeast => current.max(value),
        Op::RoundDown => current - current % value,
        Op::RoundUp => current.div_ceil(value).saturating_mul(value),
    }
}

/// Parsed command line.
struct Cli {
    no_create: bool,
    io_blocks: bool,
    reference: Option<String>,
    size: Option<String>,
    files: Vec<String>,
}

fn collect(items: Vec<Item>) -> Result<Cli, i32> {
    let mut cli = Cli {
        no_create: false,
        io_blocks: false,
        reference: None,
        size: None,
        files: Vec::new(),
    };
    for item in items {
        match item {
            Item::Operand(text) => cli.files.push(text),
            Item::Opt { id, value } => match id {
                "help" => return Err(print_help("truncate", HELP)),
                "version" => return Err(print_version("truncate")),
                "no-create" => cli.no_create = true,
                "io-blocks" => cli.io_blocks = true,
                "reference" => cli.reference = value,
                "size" => cli.size = value,
                _ => {}
            },
        }
    }
    Ok(cli)
}

fn usage(message: String) -> i32 {
    report_usage("truncate", &UsageError { message }, EXIT_FAIL)
}

/// Block size used by `-o`: the file's preferred I/O size where the platform reports one.
fn block_size(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let blksize = meta.blksize();
        if blksize > 0 {
            return blksize;
        }
    }
    let _ = meta;
    4096
}

/// Resizes one file. Returns false on failure (already reported).
fn truncate_one(name: &str, cli: &Cli, size: Size, reference_len: Option<u64>) -> bool {
    let path = Path::new(name);
    let file = match OpenOptions::new()
        .write(true)
        .create(!cli.no_create)
        .truncate(false)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound && cli.no_create => return true,
        Err(error) => {
            eprintln!(
                "truncate: cannot open '{name}' for writing: {}",
                os_text(&error)
            );
            return false;
        }
    };
    let meta = match file.metadata() {
        Ok(meta) => meta,
        Err(error) => {
            eprintln!("truncate: cannot stat '{name}': {}", os_text(&error));
            return false;
        }
    };
    let current = reference_len.unwrap_or(meta.len());
    let block = if cli.io_blocks { block_size(&meta) } else { 1 };
    let target = new_size(current, size, block);
    match file.set_len(target) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "truncate: failed to truncate '{name}' at {target} bytes: {}",
                os_text(&error)
            );
            false
        }
    }
}

/// Entry point for `truncate`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("truncate", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("truncate", &error, EXIT_FAIL),
    };
    let cli = match collect(items) {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    if cli.files.is_empty() {
        return usage("missing file operand".to_string());
    }
    if cli.size.is_none() && cli.reference.is_none() {
        return usage("you must specify either '--size' or '--reference'".to_string());
    }
    let size = match cli.size.as_deref().map(parse_size) {
        Some(Ok(size)) => Some(size),
        Some(Err(message)) => {
            eprintln!("truncate: {message}");
            return EXIT_FAIL;
        }
        None => None,
    };
    let mut reference_len = None;
    if let Some(reference) = &cli.reference {
        match fs::metadata(reference) {
            Ok(meta) => reference_len = Some(meta.len()),
            Err(error) => {
                eprintln!("truncate: cannot stat '{reference}': {}", os_text(&error));
                return EXIT_FAIL;
            }
        }
    }
    let size = match size {
        Some(size) => size,
        None => Size {
            op: Op::Set,
            value: reference_len.unwrap_or(0),
        },
    };
    if cli.reference.is_some() && cli.size.is_some() && size.op == Op::Set {
        return usage("you must specify a relative '--size' with '--reference'".to_string());
    }
    let mut status = EXIT_OK;
    for file in &cli.files {
        if !truncate_one(file, &cli, size, reference_len) {
            status = EXIT_FAIL;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_prefixed_sizes() {
        assert_eq!(
            parse_size("10"),
            Ok(Size {
                op: Op::Set,
                value: 10
            })
        );
        assert_eq!(
            parse_size("+3"),
            Ok(Size {
                op: Op::Plus,
                value: 3
            })
        );
        assert_eq!(
            parse_size("-4"),
            Ok(Size {
                op: Op::Minus,
                value: 4
            })
        );
        assert_eq!(
            parse_size("<5"),
            Ok(Size {
                op: Op::AtMost,
                value: 5
            })
        );
        assert_eq!(
            parse_size(">6"),
            Ok(Size {
                op: Op::AtLeast,
                value: 6
            })
        );
        assert_eq!(
            parse_size("/7"),
            Ok(Size {
                op: Op::RoundDown,
                value: 7
            })
        );
        assert_eq!(
            parse_size("%8"),
            Ok(Size {
                op: Op::RoundUp,
                value: 8
            })
        );
    }

    #[test]
    fn suffixes_use_1024_or_1000_bases() {
        assert_eq!(parse_size("1K").unwrap().value, 1024);
        assert_eq!(parse_size("1KB").unwrap().value, 1000);
        assert_eq!(parse_size("1MiB").unwrap().value, 1 << 20);
        assert_eq!(parse_size("2MB").unwrap().value, 2_000_000);
    }

    #[test]
    fn invalid_and_overflowing_sizes_are_reported() {
        assert_eq!(parse_size("x"), Err("Invalid number: 'x'".to_string()));
        assert_eq!(parse_size(""), Err("Invalid number: ''".to_string()));
        assert_eq!(parse_size("1X"), Err("Invalid number: '1X'".to_string()));
        assert!(parse_size("1Q").unwrap_err().contains("Value too large"));
        assert!(parse_size("99999999999999999999999")
            .unwrap_err()
            .contains("Value too large"));
        assert!(parse_size("/0").is_err());
    }

    #[test]
    fn new_size_applies_each_operator() {
        let s = |op, value| Size { op, value };
        assert_eq!(new_size(10, s(Op::Set, 4), 1), 4);
        assert_eq!(new_size(10, s(Op::Plus, 4), 1), 14);
        assert_eq!(new_size(10, s(Op::Minus, 40), 1), 0);
        assert_eq!(new_size(10, s(Op::AtMost, 4), 1), 4);
        assert_eq!(new_size(10, s(Op::AtLeast, 40), 1), 40);
        assert_eq!(new_size(10, s(Op::RoundDown, 4), 1), 8);
        assert_eq!(new_size(10, s(Op::RoundUp, 4), 1), 12);
        assert_eq!(new_size(10, s(Op::Set, 2), 512), 1024);
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    fn cli(no_create: bool) -> Cli {
        Cli {
            no_create,
            io_blocks: false,
            reference: None,
            size: None,
            files: Vec::new(),
        }
    }

    #[test]
    fn shrinks_and_extends_real_files() {
        let t = TempDir::new("truncate-files");
        let file = t.write("f", "hello");
        let name = file.to_str().unwrap();
        let shrink = Size {
            op: Op::Set,
            value: 2,
        };
        assert!(truncate_one(name, &cli(false), shrink, None));
        assert_eq!(fs::read(&file).unwrap(), b"he");
        let grow = Size {
            op: Op::Plus,
            value: 3,
        };
        assert!(truncate_one(name, &cli(false), grow, None));
        assert_eq!(fs::metadata(&file).unwrap().len(), 5);
    }

    #[test]
    fn creates_missing_files_unless_no_create() {
        let t = TempDir::new("truncate-create");
        let set = Size {
            op: Op::Set,
            value: 4,
        };
        assert!(truncate_one(
            t.at("new").to_str().unwrap(),
            &cli(true),
            set,
            None
        ));
        assert!(!t.at("new").exists());
        assert!(truncate_one(
            t.at("new").to_str().unwrap(),
            &cli(false),
            set,
            None
        ));
        assert_eq!(fs::metadata(t.at("new")).unwrap().len(), 4);
    }

    #[test]
    fn reference_length_overrides_the_file_size() {
        let t = TempDir::new("truncate-ref");
        let file = t.write("f", "x");
        let plus = Size {
            op: Op::Plus,
            value: 1,
        };
        assert!(truncate_one(
            file.to_str().unwrap(),
            &cli(false),
            plus,
            Some(10)
        ));
        assert_eq!(fs::metadata(&file).unwrap().len(), 11);
    }
}
