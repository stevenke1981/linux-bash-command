use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io,
};
use std::io::{BufRead, Write};

const HELP:&str="Usage: comm [OPTION]... FILE1 FILE2\nCompare sorted files line by line.\n  -1                       suppress lines unique to FILE1\n  -2                       suppress lines unique to FILE2\n  -3                       suppress common lines\n      --check-order        fail immediately on disorder\n      --nocheck-order      do not check input order\n      --output-delimiter=STR  separate columns with STR\n      --total              append three counts and total\n  -z, --zero-terminated    use NUL records\n      --help               display this help\n      --version            output version information\nExit status: 0 success, 1 on failure or disorder.\n";

struct Records {
    reader: Box<dyn BufRead>,
    previous: Option<Vec<u8>>,
    line: Option<Vec<u8>>,
    disorder: bool,
}
impl Records {
    fn advance(&mut self, delimiter: u8) -> std::io::Result<()> {
        let mut line = Vec::new();
        let n = self.reader.read_until(delimiter, &mut line)?;
        if n == 0 {
            self.line = None;
            return Ok(());
        }
        if line.last() == Some(&delimiter) {
            line.pop();
        }
        self.disorder |= self.previous.as_ref().is_some_and(|p| p > &line);
        self.previous = Some(line.clone());
        self.line = Some(line);
        Ok(())
    }
}

struct Options<'a> {
    suppress: [bool; 3],
    separator: &'a [u8],
    delimiter: u8,
    check: Option<bool>,
    total: bool,
}

fn compare(
    left: &mut Records,
    right: &mut Records,
    out: &mut dyn Write,
    options: Options<'_>,
) -> std::io::Result<bool> {
    let Options {
        suppress,
        separator,
        delimiter,
        check,
        total,
    } = options;
    left.advance(delimiter)?;
    right.advance(delimiter)?;
    let (mut counts, mut unpaired, mut warned) = ([0u64; 3], false, [false; 2]);
    loop {
        if check == Some(true) || check.is_none() && unpaired {
            for (i, bad) in [left.disorder, right.disorder].into_iter().enumerate() {
                if bad && !warned[i] {
                    eprintln!("comm: file {} is not in sorted order", i + 1);
                    warned[i] = true;
                    if check == Some(true) {
                        return Ok(true);
                    }
                }
            }
        }
        let (column, line) = match (&left.line, &right.line) {
            (None, None) => break,
            (Some(a), None) => (0, a),
            (None, Some(b)) => (1, b),
            (Some(a), Some(b)) => match a.cmp(b) {
                std::cmp::Ordering::Less => (0, a),
                std::cmp::Ordering::Greater => (1, b),
                std::cmp::Ordering::Equal => (2, a),
            },
        };
        counts[column] += 1;
        unpaired |= column != 2;
        if !suppress[column] {
            for suppressed in &suppress[..column] {
                if !suppressed {
                    out.write_all(separator)?;
                }
            }
            out.write_all(line)?;
            out.write_all(&[delimiter])?;
        }
        if column != 1 {
            left.advance(delimiter)?;
        }
        if column != 0 {
            right.advance(delimiter)?;
        }
    }
    if total {
        write!(out, "{}", counts[0])?;
        out.write_all(separator)?;
        write!(out, "{}", counts[1])?;
        out.write_all(separator)?;
        write!(out, "{}", counts[2])?;
        out.write_all(separator)?;
        out.write_all(b"total")?;
        out.write_all(&[delimiter])?;
    }
    if warned.into_iter().any(|v| v) {
        eprintln!("comm: input is not in sorted order");
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn run(arguments: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
        OptSpec {
            id: "suppress-first",
            long: "",
            short: '1',
            has_arg: HasArg::No,
        },
        OptSpec {
            id: "suppress-second",
            long: "",
            short: '2',
            has_arg: HasArg::No,
        },
        OptSpec {
            id: "suppress-common",
            long: "",
            short: '3',
            has_arg: HasArg::No,
        },
        OptSpec::flag("check-order", '\0'),
        OptSpec::flag("nocheck-order", '\0'),
        OptSpec::value("output-delimiter", '\0'),
        OptSpec::flag("total", '\0'),
        OptSpec::flag("zero-terminated", 'z'),
    ];
    let parsed = match args::parse(arguments, &specs, true) {
        Ok(v) => v,
        Err(e) => return usage("comm", &e),
    };
    let (mut files, mut suppress, mut check, mut separator, mut delimiter, mut total) =
        (Vec::new(), [false; 3], None, b"\t".to_vec(), b'\n', false);
    for item in parsed {
        match item {
            Item::Operand(s) => files.push(s),
            Item::Opt(id, value) => match id {
                "help" | "version" => {
                    return args::help_version("comm", &[format!("--{id}")], HELP, true).unwrap()
                }
                "suppress-first" => suppress[0] = true,
                "suppress-second" => suppress[1] = true,
                "suppress-common" => suppress[2] = true,
                "check-order" => check = Some(true),
                "nocheck-order" => check = Some(false),
                "output-delimiter" => {
                    let v = value.unwrap();
                    separator = if v.is_empty() {
                        vec![0]
                    } else {
                        v.into_bytes()
                    };
                }
                "zero-terminated" => delimiter = 0,
                "total" => total = true,
                _ => unreachable!(),
            },
        }
    }
    if files.len() != 2 {
        return usage(
            "comm",
            if files.len() < 2 {
                "missing operand"
            } else {
                "extra operand"
            },
        );
    }
    let mut inputs = Vec::new();
    for name in &files {
        match io::input(name) {
            Ok(reader) => inputs.push(Records {
                reader,
                previous: None,
                line: None,
                disorder: false,
            }),
            Err(e) => {
                eprintln!("comm: {name}: {}", error::io_text(&e));
                return 1;
            }
        }
    }
    let mut right = inputs.pop().unwrap();
    let mut left = inputs.pop().unwrap();
    let mut failed = false;
    let code = io::output("comm", |out| {
        failed = compare(
            &mut left,
            &mut right,
            out,
            Options {
                suppress,
                separator: &separator,
                delimiter,
                check,
                total,
            },
        )?;
        Ok(())
    });
    if code == 0 {
        i32::from(failed)
    } else {
        code
    }
}

fn usage(command: &str, message: &str) -> i32 {
    error::usage(command, message, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(
        a: &[u8],
        b: &[u8],
        s: [bool; 3],
        d: &[u8],
        z: u8,
        check: Option<bool>,
        total: bool,
    ) -> (Vec<u8>, bool) {
        let input = |s: &[u8]| Records {
            reader: Box::new(std::io::Cursor::new(s.to_vec())),
            previous: None,
            line: None,
            disorder: false,
        };
        let mut out = Vec::new();
        let failed = compare(
            &mut input(a),
            &mut input(b),
            &mut out,
            Options {
                suppress: s,
                separator: d,
                delimiter: z,
                check,
                total,
            },
        )
        .unwrap();
        (out, failed)
    }
    #[test]
    fn columns() {
        assert_eq!(
            c(b"a\nb\n", b"b\nc\n", [false; 3], b"\t", b'\n', None, false).0,
            b"a\n\t\tb\n\tc\n"
        );
    }
    #[test]
    fn suppress() {
        assert_eq!(
            c(
                b"a\nb\n",
                b"b\nc\n",
                [true, false, true],
                b"\t",
                b'\n',
                None,
                false
            )
            .0,
            b"c\n"
        );
    }
    #[test]
    fn total() {
        assert_eq!(
            c(b"a\n", b"a\n", [true; 3], b":", b'\n', None, true).0,
            b"0:0:1:total\n"
        );
    }
    #[test]
    fn nul() {
        assert_eq!(
            c(b"a\0", b"b\0", [false; 3], b"\t", 0, None, false).0,
            b"a\0\tb\0"
        );
    }
    #[test]
    fn strict() {
        assert!(c(b"b\na\n", b"", [false; 3], b"\t", b'\n', Some(true), false).1);
    }
    #[test]
    fn nocheck() {
        assert!(!c(b"b\na\n", b"", [false; 3], b"\t", b'\n', Some(false), false).1);
    }
    #[test]
    fn identical_unordered() {
        assert!(!c(b"b\na\n", b"b\na\n", [false; 3], b"\t", b'\n', None, false).1);
    }
}
