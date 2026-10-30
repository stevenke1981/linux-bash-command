//! `paste`: merge corresponding lines of files, or join the lines of each file (`-s`).
//!
//! Delimiters come from a list that cycles; an empty list inserts nothing. In parallel mode
//! every input is opened before any output is written, so a missing file produces no output.
//! In serial mode each file is opened in turn. A `-` operand reads standard input, which is
//! shared by all `-` operands.

use std::io::{self, BufRead, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const CMD: &str = "paste";

const SPECS: &[OptSpec] = &[
    OptSpec::req("delimiters", Some('d'), Some("delimiters")),
    OptSpec::flag("serial", Some('s'), Some("serial")),
    OptSpec::flag("zero", Some('z'), Some("zero-terminated")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: paste [OPTION]... [FILE]...\n\
Write lines consisting of the sequentially corresponding lines from each\n\
FILE, separated by TABs, to standard output. With no FILE, or when FILE is -,\n\
read standard input.\n\n\
  -d, --delimiters=LIST   reuse characters from LIST instead of TABs; LIST may\n\
                          contain the escapes \\n, \\t, \\\\, and \\0\n\
  -s, --serial            paste one file at a time instead of in parallel\n\
  -z, --zero-terminated   line delimiter is NUL, not newline\n\
      --help              display this help and exit\n\
      --version           output version information and exit\n\n\
Exit status: 0 on success, 1 if an input could not be read.\n";

#[derive(Debug)]
struct Options {
    delimiters: Vec<u8>,
    serial: bool,
    terminator: u8,
}

/// Where a source's records come from.
enum Input {
    /// Standard input, read through the shared lock.
    Stdin,
    File(Box<dyn BufRead>),
}

/// One input with its end-of-input state.
struct Source {
    name: String,
    input: Input,
    done: bool,
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    for item in &items {
        if let Item::Opt { id, .. } = item {
            match *id {
                "help" => {
                    print!("{HELP}");
                    return EXIT_OK;
                }
                "version" => return print_version(CMD),
                _ => {}
            }
        }
    }
    let options = match options_from(&items) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let mut files: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(name) => Some(name.clone()),
            Item::Opt { .. } => None,
        })
        .collect();
    if files.is_empty() {
        files.push("-".to_owned());
    }
    run_files(&options, &files)
}

fn options_from(items: &[Item]) -> Result<Options, i32> {
    let mut options = Options {
        delimiters: vec![b'\t'],
        serial: false,
        terminator: b'\n',
    };
    for item in items {
        if let Item::Opt { id, value } = item {
            match *id {
                "delimiters" => {
                    options.delimiters = parse_delimiters(value.as_deref().unwrap_or(""))?
                }
                "serial" => options.serial = true,
                "zero" => options.terminator = 0,
                _ => {}
            }
        }
    }
    Ok(options)
}

/// Expands the escapes of a `-d` list. A list that is empty yields no delimiter at all.
fn parse_delimiters(list: &str) -> Result<Vec<u8>, i32> {
    let bytes = list.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some(&code) = bytes.get(index + 1) else {
            eprintln!(
                "{CMD}: delimiter list ends with an unescaped backslash: {list}\nTry '{CMD} --help' for more information."
            );
            return Err(EXIT_FAIL);
        };
        out.push(match code {
            b'n' => b'\n',
            b't' => b'\t',
            b'0' => 0,
            other => other,
        });
        index += 2;
    }
    Ok(out)
}

fn run_files(options: &Options, files: &[String]) -> i32 {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    let result = if options.serial {
        serial_all(options, files, &mut stdin, &mut out, &mut status)
    } else {
        parallel_all(options, files, &mut stdin, &mut out, &mut status)
    };
    match result {
        Ok(()) => {
            let flushed = finish_output(CMD, &mut out);
            if flushed == EXIT_OK {
                status
            } else {
                flushed
            }
        }
        Err(error) => write_failed(&error),
    }
}

fn write_failed(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_FAIL
}

fn open_source(name: &str) -> Result<Source, io::Error> {
    let input = if name == "-" {
        Input::Stdin
    } else {
        Input::File(open_input(name)?)
    };
    Ok(Source {
        name: name.to_owned(),
        input,
        done: false,
    })
}

/// Reads the next record of `source` into `record` (terminator removed). Returns `false` at
/// the end of input; a read error is reported and also ends the source.
fn next_record(
    source: &mut Source,
    stdin: &mut dyn BufRead,
    terminator: u8,
    record: &mut Vec<u8>,
    status: &mut i32,
) -> bool {
    if source.done {
        return false;
    }
    let reader: &mut dyn BufRead = match &mut source.input {
        Input::Stdin => stdin,
        Input::File(reader) => reader.as_mut(),
    };
    match read_record(reader, terminator, record) {
        Ok(true) => {
            if record.last() == Some(&terminator) {
                record.pop();
            }
            true
        }
        Ok(false) => {
            source.done = true;
            false
        }
        Err(error) => {
            report(CMD, &source.name, &error);
            *status = EXIT_FAIL;
            source.done = true;
            false
        }
    }
}

/// Delimiter for the gap after position `index`, cycling through the list.
fn delimiter_at(options: &Options, index: usize) -> Option<u8> {
    if options.delimiters.is_empty() {
        None
    } else {
        Some(options.delimiters[index % options.delimiters.len()])
    }
}

fn parallel_all(
    options: &Options,
    files: &[String],
    stdin: &mut dyn BufRead,
    out: &mut impl Write,
    status: &mut i32,
) -> io::Result<()> {
    let mut sources = Vec::with_capacity(files.len());
    let mut opened = true;
    for name in files {
        match open_source(name) {
            Ok(source) => sources.push(source),
            Err(error) => {
                report(CMD, name, &error);
                opened = false;
            }
        }
    }
    if !opened {
        *status = EXIT_FAIL;
        return Ok(());
    }
    parallel(options, &mut sources, stdin, out, status)
}

fn parallel(
    options: &Options,
    sources: &mut [Source],
    stdin: &mut dyn BufRead,
    out: &mut impl Write,
    status: &mut i32,
) -> io::Result<()> {
    let mut fields: Vec<Vec<u8>> = vec![Vec::new(); sources.len()];
    loop {
        let mut any = false;
        for (index, source) in sources.iter_mut().enumerate() {
            fields[index].clear();
            if next_record(
                source,
                stdin,
                options.terminator,
                &mut fields[index],
                status,
            ) {
                any = true;
            }
        }
        if !any {
            return Ok(());
        }
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                if let Some(byte) = delimiter_at(options, index - 1) {
                    out.write_all(&[byte])?;
                }
            }
            out.write_all(field)?;
        }
        out.write_all(&[options.terminator])?;
    }
}

fn serial_all(
    options: &Options,
    files: &[String],
    stdin: &mut dyn BufRead,
    out: &mut impl Write,
    status: &mut i32,
) -> io::Result<()> {
    for name in files {
        match open_source(name) {
            Ok(mut source) => serial_one(options, &mut source, stdin, out, status)?,
            Err(error) => {
                report(CMD, name, &error);
                *status = EXIT_FAIL;
            }
        }
    }
    Ok(())
}

/// Joins all records of one source on a single output line.
fn serial_one(
    options: &Options,
    source: &mut Source,
    stdin: &mut dyn BufRead,
    out: &mut impl Write,
    status: &mut i32,
) -> io::Result<()> {
    let mut record = Vec::new();
    let mut gap = 0usize;
    let mut first = true;
    while next_record(source, stdin, options.terminator, &mut record, status) {
        if !first {
            if let Some(byte) = delimiter_at(options, gap) {
                out.write_all(&[byte])?;
            }
            gap += 1;
        }
        out.write_all(&record)?;
        first = false;
    }
    out.write_all(&[options.terminator])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_source(text: &'static [u8]) -> Source {
        Source {
            name: "t".to_owned(),
            input: Input::File(Box::new(text)),
            done: false,
        }
    }

    #[test]
    fn expands_delimiter_escapes() {
        assert_eq!(parse_delimiters("a\\tb\\n").unwrap(), b"a\tb\n");
        assert_eq!(parse_delimiters("\\\\\\0").unwrap(), b"\\\0");
        assert_eq!(parse_delimiters("\\q").unwrap(), b"q");
        assert_eq!(parse_delimiters("").unwrap(), b"");
    }

    #[test]
    fn rejects_trailing_backslash() {
        assert!(parse_delimiters("a\\").is_err());
    }

    #[test]
    fn delimiters_cycle_and_empty_list_inserts_nothing() {
        let options = Options {
            delimiters: b"ab".to_vec(),
            serial: false,
            terminator: b'\n',
        };
        assert_eq!(delimiter_at(&options, 0), Some(b'a'));
        assert_eq!(delimiter_at(&options, 3), Some(b'b'));
        let empty = Options {
            delimiters: Vec::new(),
            ..options
        };
        assert_eq!(delimiter_at(&empty, 0), None);
    }

    #[test]
    fn parallel_pads_short_inputs() {
        let options = Options {
            delimiters: b"\t".to_vec(),
            serial: false,
            terminator: b'\n',
        };
        let mut sources = vec![file_source(b"1\n2\n3\n"), file_source(b"x\n")];
        let mut out = Vec::new();
        let mut status = EXIT_OK;
        let mut stdin: &[u8] = b"";
        parallel(&options, &mut sources, &mut stdin, &mut out, &mut status).unwrap();
        assert_eq!(out, b"1\tx\n2\t\n3\t\n");
    }

    #[test]
    fn serial_joins_each_source_on_one_line() {
        let options = Options {
            delimiters: b",".to_vec(),
            serial: true,
            terminator: b'\n',
        };
        let mut source = file_source(b"1\n2\n3\n");
        let mut out = Vec::new();
        let mut status = EXIT_OK;
        let mut stdin: &[u8] = b"";
        serial_one(&options, &mut source, &mut stdin, &mut out, &mut status).unwrap();
        assert_eq!(out, b"1,2,3\n");
    }

    #[test]
    fn stdin_source_reads_from_the_shared_reader() {
        let options = Options {
            delimiters: b"\t".to_vec(),
            serial: false,
            terminator: b'\n',
        };
        let mut sources = vec![
            Source {
                name: "-".to_owned(),
                input: Input::Stdin,
                done: false,
            },
            Source {
                name: "-".to_owned(),
                input: Input::Stdin,
                done: false,
            },
        ];
        let mut stdin: &[u8] = b"l1\nl2\nl3\n";
        let mut out = Vec::new();
        let mut status = EXIT_OK;
        parallel(&options, &mut sources, &mut stdin, &mut out, &mut status).unwrap();
        assert_eq!(out, b"l1\tl2\nl3\t\n");
    }
}
