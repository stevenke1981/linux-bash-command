//! `comm`: compares two sorted files line by line, printing lines unique to the first file,
//! unique to the second, and common to both in three columns (suppressible with `-1`,
//! `-2`, `-3`). Specification: docs/spec/coreutils-alignment/03-commands-core.md.

use std::cmp::Ordering;
use std::io::{self, BufRead, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const CMD: &str = "comm";

const OPTS: &[OptSpec] = &[
    OptSpec::flag("1", Some('1'), None),
    OptSpec::flag("2", Some('2'), None),
    OptSpec::flag("3", Some('3'), None),
    OptSpec::flag("check-order", None, Some("check-order")),
    OptSpec::flag("nocheck-order", None, Some("nocheck-order")),
    OptSpec::req("output-delimiter", None, Some("output-delimiter")),
    OptSpec::flag("total", None, Some("total")),
    OptSpec::flag("z", Some('z'), Some("zero-terminated")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// How strictly the input order is verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OrderCheck {
    /// Only when an unpaired line is printed (the default).
    Unpaired,
    /// Every line as it is read; the first disorder ends the run.
    Always,
    Never,
}

#[derive(Debug)]
struct Options {
    suppress: [bool; 3],
    order: OrderCheck,
    delimiter: Vec<u8>,
    total: bool,
    delim: u8,
}

/// One input file and its lookahead.
struct Input {
    /// 1 for FILE1, 2 for FILE2 (used in messages).
    number: u8,
    reader: Box<dyn BufRead>,
    /// The current line, `None` at end of input.
    current: Option<Vec<u8>>,
    /// The line before `current`, for order checks.
    previous: Option<Vec<u8>>,
    /// Whether the disorder of this input has been reported.
    reported: bool,
    /// Whether any disorder was seen (for the final status).
    disordered: bool,
}

/// Entry point for `comm` and the `agentcmd comm` multi-call form.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, OPTS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage(CMD, &err, EXIT_FAIL),
    };
    let mut options = Options {
        suppress: [false; 3],
        order: OrderCheck::Unpaired,
        delimiter: b"\t".to_vec(),
        total: false,
        delim: b'\n',
    };
    let mut operands = Vec::new();
    let mut delimiter_given: Option<String> = None;
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => return write_help(),
                "version" => return print_version(CMD),
                "output-delimiter" => {
                    let text = value.unwrap_or_default();
                    if delimiter_given.as_ref().is_some_and(|old| *old != text) {
                        return fail("multiple output delimiters specified");
                    }
                    delimiter_given = Some(text);
                }
                "check-order" => options.order = OrderCheck::Always,
                "nocheck-order" => options.order = OrderCheck::Never,
                "total" => options.total = true,
                "z" => options.delim = 0,
                "1" => options.suppress[0] = true,
                "2" => options.suppress[1] = true,
                "3" => options.suppress[2] = true,
                _ => {}
            },
        }
    }
    if let Some(text) = delimiter_given {
        options.delimiter = if text.is_empty() {
            vec![0]
        } else {
            text.into_bytes()
        };
    }
    match operands.len() {
        0 => return fail_with_try("missing operand"),
        1 => return fail_with_try(&format!("missing operand after '{}'", operands[0])),
        2 => {}
        _ => return fail_with_try(&format!("extra operand '{}'", operands[2])),
    }
    compare_files(&options, &operands[0], &operands[1])
}

fn write_help() -> i32 {
    let mut out = stdout_writer();
    if out.write_all(HELP.as_bytes()).is_err() {
        return EXIT_OK;
    }
    finish_output(CMD, &mut out)
}

fn fail(message: &str) -> i32 {
    eprintln!("{CMD}: {message}");
    EXIT_FAIL
}

fn fail_with_try(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

/// Opens both inputs. Standard input may be named once; a second `-` reads as empty and
/// is reported, as GNU does when stdin is already consumed.
fn compare_files(options: &Options, first: &str, second: &str) -> i32 {
    let mut status = EXIT_OK;
    let open = |name: &str| -> Result<Box<dyn BufRead>, i32> {
        open_input(name).map_err(|error| {
            eprintln!("{CMD}: {name}: {}", io_error_text(&error));
            EXIT_FAIL
        })
    };
    let first_reader = match open(first) {
        Ok(reader) => reader,
        Err(code) => return code,
    };
    let second_reader: Box<dyn BufRead> = if first == "-" && second == "-" {
        eprintln!("{CMD}: -: Bad file descriptor");
        status = EXIT_FAIL;
        Box::new(io::empty())
    } else {
        match open(second) {
            Ok(reader) => reader,
            Err(code) => return code,
        }
    };
    let mut inputs = [Input::new(1, first_reader), Input::new(2, second_reader)];
    let mut out = stdout_writer();
    let result = merge(&mut inputs, options, &mut out);
    let flushed = finish_output(CMD, &mut out);
    match result {
        Ok(Some(code)) => code,
        Ok(None) => {
            if inputs.iter().any(|input| input.disordered) {
                eprintln!("{CMD}: input is not in sorted order");
                return EXIT_FAIL;
            }
            if flushed != EXIT_OK {
                return flushed;
            }
            status
        }
        Err(error) if is_broken_pipe(&error) => EXIT_OK,
        Err(error) => {
            eprintln!("{CMD}: {}", io_error_text(&error));
            EXIT_FAIL
        }
    }
}

impl Input {
    fn new(number: u8, reader: Box<dyn BufRead>) -> Self {
        Self {
            number,
            reader,
            current: None,
            previous: None,
            reported: false,
            disordered: false,
        }
    }

    /// Reads the next line. Returns `Ok(false)` at end of input.
    fn advance(&mut self, delim: u8) -> io::Result<bool> {
        self.previous = self.current.take();
        let mut buf = Vec::new();
        if !read_record(self.reader.as_mut(), delim, &mut buf)? {
            return Ok(false);
        }
        if buf.last() == Some(&delim) {
            buf.pop();
        }
        self.current = Some(buf);
        Ok(true)
    }

    /// True when the current line sorts before the previous one.
    fn out_of_order(&self) -> bool {
        match (&self.previous, &self.current) {
            (Some(prev), Some(cur)) => cur < prev,
            _ => false,
        }
    }

    /// Reports a disorder once for this input, the first time it matters.
    fn report_if_disordered(&mut self) {
        if !self.reported && self.out_of_order() {
            self.reported = true;
            self.disordered = true;
            eprintln!("{CMD}: file {} is not in sorted order", self.number);
        }
    }
}

/// Merges the two sorted inputs. `Ok(Some(code))` ends the run early (`--check-order`).
fn merge(
    inputs: &mut [Input; 2],
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<Option<i32>> {
    let delim = options.delim;
    let mut totals = [0u64; 3];
    for input in inputs.iter_mut() {
        input.advance(delim)?;
    }
    loop {
        let ordering = match (&inputs[0].current, &inputs[1].current) {
            (None, None) => break,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        };
        match ordering {
            Ordering::Less | Ordering::Greater => {
                let index = usize::from(ordering == Ordering::Greater);
                let input = &mut inputs[index];
                if options.order == OrderCheck::Unpaired {
                    input.report_if_disordered();
                }
                totals[index] += 1;
                if !options.suppress[index] {
                    let line = input.current.clone().unwrap_or_default();
                    write_column(out, options, index, &line)?;
                }
                if let Some(code) = advance_checked(input, options)? {
                    return Ok(Some(code));
                }
            }
            Ordering::Equal => {
                if !options.suppress[2] {
                    write_column(
                        out,
                        options,
                        2,
                        inputs[0].current.as_deref().unwrap_or_default(),
                    )?;
                }
                totals[2] += 1;
                for input in inputs.iter_mut() {
                    if let Some(code) = advance_checked(input, options)? {
                        return Ok(Some(code));
                    }
                }
            }
        }
    }
    if options.total {
        write_total(out, options, totals)?;
    }
    Ok(None)
}

/// Advances `input`. With `--check-order` a disorder found while reading ends the run.
fn advance_checked(input: &mut Input, options: &Options) -> io::Result<Option<i32>> {
    let more = input.advance(options.delim)?;
    if options.order == OrderCheck::Always && more && input.out_of_order() {
        eprintln!("{CMD}: file {} is not in sorted order", input.number);
        return Ok(Some(EXIT_FAIL));
    }
    Ok(None)
}

/// Writes one line in column `column` (0, 1 or 2) with the separators GNU uses: one per
/// column printed before it.
fn write_column(
    out: &mut dyn Write,
    options: &Options,
    column: usize,
    line: &[u8],
) -> io::Result<()> {
    let printed_before = options.suppress[..column].iter().filter(|s| !**s).count();
    for _ in 0..printed_before {
        out.write_all(&options.delimiter)?;
    }
    out.write_all(line)?;
    out.write_all(&[options.delim])
}

fn write_total(out: &mut dyn Write, options: &Options, totals: [u64; 3]) -> io::Result<()> {
    let text = format!(
        "{}{d}{}{d}{}{d}total",
        totals[0],
        totals[1],
        totals[2],
        d = String::from_utf8_lossy(&options.delimiter)
    );
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")
}

const HELP: &str = "\
Usage: comm [OPTION]... FILE1 FILE2
Compare sorted files FILE1 and FILE2 line by line.

When FILE1 or FILE2 (not both) is -, read standard input.

With no options, produce three-column output.  Column one contains
lines unique to FILE1, column two contains lines unique to FILE2,
and column three contains lines common to both files.

  -1                      suppress column 1 (lines unique to FILE1)
  -2                      suppress column 2 (lines unique to FILE2)
  -3                      suppress column 3 (lines that appear in both files)

      --check-order       check that the input is correctly sorted, even
                            if all input lines are pairable
      --nocheck-order     do not check that the input is correctly sorted
      --output-delimiter=STR  separate columns with STR
      --total             output a summary
  -z, --zero-terminated   line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit
";

#[cfg(test)]
mod tests {
    use super::*;

    fn options(suppress: [bool; 3]) -> Options {
        Options {
            suppress,
            order: OrderCheck::Never,
            delimiter: b"\t".to_vec(),
            total: false,
            delim: b'\n',
        }
    }

    fn run_merge(a: &[u8], b: &[u8], options: &Options) -> Vec<u8> {
        let mut inputs = [
            Input::new(1, Box::new(io::Cursor::new(a.to_vec()))),
            Input::new(2, Box::new(io::Cursor::new(b.to_vec()))),
        ];
        let mut out = Vec::new();
        merge(&mut inputs, options, &mut out).unwrap();
        out
    }

    #[test]
    fn three_columns_use_leading_delimiters() {
        let out = run_merge(b"a\nb\nc\n", b"b\nc\nd\n", &options([false; 3]));
        assert_eq!(out, b"a\n\t\tb\n\t\tc\n\td\n");
    }

    #[test]
    fn suppressed_columns_drop_their_delimiters() {
        let out = run_merge(b"a\nb\nc\n", b"b\nc\nd\n", &options([true, false, false]));
        assert_eq!(out, b"\tb\n\tc\nd\n");
        let common = run_merge(b"a\nb\nc\n", b"b\nc\nd\n", &options([true, true, false]));
        assert_eq!(common, b"b\nc\n");
    }

    #[test]
    fn empty_delimiter_means_nul() {
        let mut opts = options([false; 3]);
        opts.delimiter = vec![0];
        let out = run_merge(b"a\n", b"a\nb\n", &opts);
        assert_eq!(out, b"\0\0a\n\0b\n");
    }

    #[test]
    fn totals_count_every_column() {
        let mut opts = options([false; 3]);
        opts.total = true;
        let out = run_merge(b"a\nb\n", b"b\nc\n", &opts);
        assert!(out.ends_with(b"1\t1\t1\ttotal\n"), "{out:?}");
    }

    #[test]
    fn check_order_reports_disorder_of_the_second_input() {
        let mut opts = options([false; 3]);
        opts.order = OrderCheck::Always;
        let mut inputs = [
            Input::new(1, Box::new(io::Cursor::new(b"a\nb\nc\n".to_vec()))),
            Input::new(2, Box::new(io::Cursor::new(b"b\na\n".to_vec()))),
        ];
        let mut out = Vec::new();
        let result = merge(&mut inputs, &opts, &mut out).unwrap();
        assert_eq!(result, Some(EXIT_FAIL));
        assert_eq!(out, b"a\n\t\tb\n");
    }

    #[test]
    fn default_order_check_only_looks_at_unpaired_lines() {
        let opts = options([false; 3]);
        let mut inputs = [
            Input::new(1, Box::new(io::Cursor::new(b"b\na\n".to_vec()))),
            Input::new(2, Box::new(io::Cursor::new(b"b\na\n".to_vec()))),
        ];
        let mut out = Vec::new();
        assert_eq!(merge(&mut inputs, &opts, &mut out).unwrap(), None);
        assert!(!inputs[0].disordered && !inputs[1].disordered);
    }
}
