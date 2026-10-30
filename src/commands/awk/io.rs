//! Input and output: records read with `RS`, the main input over `ARGV`, `getline` sources,
//! and output streams (`print > file`, `>>`, `/dev/stdout`, `/dev/stderr`).
//!
//! Pipes and `system()` need a shell and are not supported: they fail at run time.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::rc::Rc;

use regex::bytes::Regex;

use super::ast::{RedirKind, Redirect, ARGC, FILENAME, FNR, NR, RS};
use super::interp::{Fault, Interp};
use super::value::Value;
use crate::common::error::io_error_text;
use crate::common::io::{is_broken_pipe, stdout_writer};

/// How records are separated.
#[derive(Clone, Debug)]
pub enum RsMode {
    /// A single byte (the default newline included).
    Byte(u8),
    /// `RS = ""`: records are separated by blank lines.
    Paragraph,
    /// A regex (`RS` longer than one character).
    Regex(Rc<Regex>),
}

/// Reads records from a byte source, keeping a buffer between calls.
pub struct RecordReader {
    source: Box<dyn Read>,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl RecordReader {
    pub fn new(source: Box<dyn Read>) -> Self {
        Self {
            source,
            buf: Vec::new(),
            pos: 0,
            eof: false,
        }
    }

    /// The next record without its separator, or `None` at end of input.
    pub fn read(&mut self, mode: &RsMode) -> io::Result<Option<Vec<u8>>> {
        match mode {
            RsMode::Byte(separator) => self.read_byte(*separator),
            RsMode::Paragraph => self.read_paragraph(),
            RsMode::Regex(regex) => self.read_regex(regex),
        }
    }

    /// Reads more data into the buffer. Returns false when the input is exhausted.
    fn fill(&mut self) -> io::Result<bool> {
        if self.eof {
            return Ok(false);
        }
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            match self.source.read(&mut chunk) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(false);
                }
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(true);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
    }

    fn read_byte(&mut self, separator: u8) -> io::Result<Option<Vec<u8>>> {
        // `scanned` counts bytes after `pos` already searched, so refills do not rescan.
        let mut scanned = 0;
        loop {
            let from = self.pos + scanned;
            if let Some(offset) = self.buf[from..].iter().position(|&b| b == separator) {
                let end = from + offset;
                let record = self.buf[self.pos..end].to_vec();
                self.pos = end + 1;
                return Ok(Some(record));
            }
            scanned = self.buf.len() - self.pos;
            if !self.fill()? {
                return Ok(self.take_rest());
            }
        }
    }

    /// The unread bytes as a last record, or `None` when nothing is left.
    fn take_rest(&mut self) -> Option<Vec<u8>> {
        if self.pos < self.buf.len() {
            let record = self.buf[self.pos..].to_vec();
            self.pos = self.buf.len();
            Some(record)
        } else {
            None
        }
    }

    fn read_paragraph(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            while self.pos < self.buf.len() && self.buf[self.pos] == b'\n' {
                self.pos += 1;
            }
            if self.pos < self.buf.len() {
                break;
            }
            if !self.fill()? {
                return Ok(None);
            }
        }
        let mut scanned = 0;
        loop {
            let start = self.pos + scanned;
            let found = (start..self.buf.len().saturating_sub(1))
                .find(|&i| self.buf[i] == b'\n' && self.buf[i + 1] == b'\n');
            if let Some(end) = found {
                let record = self.buf[self.pos..end].to_vec();
                self.pos = end + 2;
                return Ok(Some(record));
            }
            scanned = self.buf.len() - self.pos;
            if !self.fill()? {
                let mut record = self.take_rest().unwrap_or_default();
                while record.last() == Some(&b'\n') {
                    record.pop();
                }
                return Ok(if record.is_empty() {
                    None
                } else {
                    Some(record)
                });
            }
        }
    }

    fn read_regex(&mut self, regex: &Regex) -> io::Result<Option<Vec<u8>>> {
        loop {
            let hay = &self.buf[self.pos..];
            if let Some(found) = regex.find(hay) {
                if !found.is_empty() && (found.end() < hay.len() || self.eof) {
                    let record = hay[..found.start()].to_vec();
                    self.pos += found.end();
                    return Ok(Some(record));
                }
            }
            if !self.fill()? {
                return Ok(self.take_rest());
            }
        }
    }
}

/// The main input: the files named in `ARGV`, read in turn.
#[derive(Default)]
pub struct MainInput {
    reader: Option<RecordReader>,
    next_arg: usize,
    used_file: bool,
}

/// All streams the program has open.
pub struct IoState {
    pub stdout: Box<dyn Write>,
    pub outputs: HashMap<Vec<u8>, Box<dyn Write>>,
    pub inputs: HashMap<Vec<u8>, RecordReader>,
    pub main: MainInput,
}

impl IoState {
    pub fn new() -> Self {
        Self {
            stdout: Box::new(stdout_writer()),
            outputs: HashMap::new(),
            inputs: HashMap::new(),
            main: MainInput {
                reader: None,
                // ARGV[0] is the program name, never a file.
                next_arg: 1,
                used_file: false,
            },
        }
    }
}

impl Default for IoState {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of reading from a named file for `getline < file`.
pub enum FileRead {
    Record(Vec<u8>),
    Eof,
    Failed,
}

/// Maps an I/O error from output to a run-time outcome: a closed pipe ends quietly.
pub fn write_failure(error: &io::Error) -> Fault {
    if is_broken_pipe(error) {
        Fault::Silent(0)
    } else {
        Fault::Fatal(format!("write error: {}", io_error_text(error)))
    }
}

fn is_stdin_name(name: &[u8]) -> bool {
    name == b"-" || name == b"/dev/stdin"
}

fn open_source(name: &[u8]) -> io::Result<Box<dyn Read>> {
    if is_stdin_name(name) {
        return Ok(Box::new(io::stdin()));
    }
    let path = String::from_utf8_lossy(name).into_owned();
    let target = if cfg!(windows) && path == "/dev/null" {
        "NUL".to_owned()
    } else {
        path
    };
    Ok(Box::new(File::open(target)?))
}

/// Splits `name=value` where `name` is an identifier.
pub fn split_assignment(arg: &[u8]) -> Option<(&str, &[u8])> {
    let eq = arg.iter().position(|&b| b == b'=')?;
    let name = &arg[..eq];
    let first = *name.first()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    if !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_') {
        return None;
    }
    let name = std::str::from_utf8(name).ok()?;
    Some((name, &arg[eq + 1..]))
}

impl Interp {
    /// The record separator in effect, compiling a regex when needed.
    pub(super) fn rs_mode(&mut self) -> Result<RsMode, Fault> {
        let rs = self.special_bytes(RS);
        if rs.len() == 1 {
            return Ok(RsMode::Byte(rs[0]));
        }
        if rs.is_empty() {
            return Ok(RsMode::Paragraph);
        }
        let regex = self.regex.get(&rs).map_err(Fault::Fatal)?;
        Ok(RsMode::Regex(regex))
    }

    /// The next record of the main input, updating `NR` and `FNR`.
    pub(super) fn next_main_record(&mut self) -> Result<Option<Vec<u8>>, Fault> {
        let mode = self.rs_mode()?;
        loop {
            if let Some(reader) = self.io.main.reader.as_mut() {
                let result = reader.read(&mode);
                match result {
                    Ok(Some(record)) => {
                        self.add_to_global(NR, 1.0);
                        self.add_to_global(FNR, 1.0);
                        return Ok(Some(record));
                    }
                    Ok(None) => self.io.main.reader = None,
                    Err(error) => {
                        return Err(Fault::Fatal(format!(
                            "read error: {}",
                            io_error_text(&error)
                        )))
                    }
                }
            }
            if !self.open_next_main()? {
                return Ok(None);
            }
        }
    }

    /// Opens the next file operand (applying `var=value` operands on the way). Returns false
    /// when there are no more.
    fn open_next_main(&mut self) -> Result<bool, Fault> {
        loop {
            let argc = self.global_number(ARGC) as usize;
            if self.io.main.next_arg >= argc {
                break;
            }
            let index = self.io.main.next_arg;
            self.io.main.next_arg += 1;
            let Some(arg) = self.argv_element(index) else {
                continue;
            };
            if arg.is_empty() {
                continue;
            }
            if let Some((name, value)) = split_assignment(&arg) {
                let (name, value) = (name.to_owned(), value.to_vec());
                self.assign_from_text(&name, &value)?;
                continue;
            }
            self.io.main.used_file = true;
            match open_source(&arg) {
                Ok(source) => {
                    self.start_main_file(source, &arg);
                    return Ok(true);
                }
                Err(error) => {
                    return Err(Fault::Fatal(format!(
                        "fatal: cannot open file `{}' for reading: {}",
                        String::from_utf8_lossy(&arg),
                        io_error_text(&error)
                    )));
                }
            }
        }
        if !self.io.main.used_file {
            self.io.main.used_file = true;
            let source: Box<dyn Read> = Box::new(io::stdin());
            self.start_main_file(source, b"");
            return Ok(true);
        }
        Ok(false)
    }

    fn start_main_file(&mut self, source: Box<dyn Read>, name: &[u8]) {
        self.io.main.reader = Some(RecordReader::new(source));
        self.set_global_value(FILENAME, Value::Str(name.to_vec()));
        self.set_global_value(FNR, Value::Num(0.0));
    }

    /// Ends the current main input file (for `nextfile`).
    pub(super) fn close_main_file(&mut self) {
        self.io.main.reader = None;
    }

    /// `getline < name`: the next record of a named file, opening it on first use.
    pub(super) fn read_named_file(&mut self, name: &[u8], mode: &RsMode) -> FileRead {
        if !self.io.inputs.contains_key(name) {
            match open_source(name) {
                Ok(source) => {
                    self.io
                        .inputs
                        .insert(name.to_vec(), RecordReader::new(source));
                }
                Err(_) => return FileRead::Failed,
            }
        }
        let Some(reader) = self.io.inputs.get_mut(name) else {
            return FileRead::Failed;
        };
        match reader.read(mode) {
            Ok(Some(record)) => FileRead::Record(record),
            Ok(None) => FileRead::Eof,
            Err(_) => FileRead::Failed,
        }
    }

    /// Writes `bytes` to standard output.
    pub(super) fn write_stdout(&mut self, bytes: &[u8]) -> Result<(), Fault> {
        self.io
            .stdout
            .write_all(bytes)
            .map_err(|e| write_failure(&e))
    }

    /// Sends printed output to its redirection target (or standard output).
    pub(super) fn emit(&mut self, redirect: Option<&Redirect>, bytes: &[u8]) -> Result<(), Fault> {
        let Some(redirect) = redirect else {
            return self.write_stdout(bytes);
        };
        if redirect.kind == RedirKind::Pipe {
            return Err(Fault::Fatal(
                "output pipes are not supported by agentcmd".into(),
            ));
        }
        let target = self.eval(&redirect.target)?;
        let name = self.to_str(&target).into_owned();
        self.write_named(redirect.kind, &name, bytes)
    }

    fn write_named(&mut self, kind: RedirKind, name: &[u8], bytes: &[u8]) -> Result<(), Fault> {
        if name == b"/dev/stdout" {
            return self.write_stdout(bytes);
        }
        if name == b"/dev/stderr" {
            return io::stderr().write_all(bytes).map_err(|e| write_failure(&e));
        }
        if !self.io.outputs.contains_key(name) {
            let path = String::from_utf8_lossy(name).into_owned();
            let file = match kind {
                RedirKind::Append => OpenOptions::new().append(true).create(true).open(&path),
                _ => File::create(&path),
            };
            let file = file.map_err(|error| {
                Fault::Fatal(format!(
                    "can't redirect to {path} ({})",
                    io_error_text(&error)
                ))
            })?;
            self.io
                .outputs
                .insert(name.to_vec(), Box::new(io::BufWriter::new(file)));
        }
        let stream = self.io.outputs.get_mut(name).map(|s| s.write_all(bytes));
        match stream {
            Some(Err(error)) => Err(write_failure(&error)),
            _ => Ok(()),
        }
    }

    /// `close(name)`: 0 when a stream was closed, -1 when none was open under that name.
    pub(super) fn close_stream(&mut self, name: &[u8]) -> f64 {
        if let Some(mut stream) = self.io.outputs.remove(name) {
            return if stream.flush().is_ok() { 0.0 } else { -1.0 };
        }
        if self.io.inputs.remove(name).is_some() {
            return 0.0;
        }
        -1.0
    }

    /// `fflush()`: flushes everything. `fflush(name)`: flushes one stream.
    pub(super) fn flush_stream(&mut self, name: Option<&[u8]>) -> f64 {
        match name {
            None => {
                let _ = self.io.stdout.flush();
                for stream in self.io.outputs.values_mut() {
                    let _ = stream.flush();
                }
                0.0
            }
            Some(b"/dev/stdout") => {
                let _ = self.io.stdout.flush();
                0.0
            }
            Some(name) => match self.io.outputs.get_mut(name) {
                Some(stream) => {
                    let _ = stream.flush();
                    0.0
                }
                None => -1.0,
            },
        }
    }

    /// Flushes and closes every stream at the end of the program.
    pub(super) fn close_all(&mut self) {
        let _ = self.io.stdout.flush();
        for (_, mut stream) in self.io.outputs.drain() {
            let _ = stream.flush();
        }
        self.io.inputs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(text: &'static [u8]) -> RecordReader {
        RecordReader::new(Box::new(text))
    }

    fn all(reader: &mut RecordReader, mode: &RsMode) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(record) = reader.read(mode).unwrap() {
            out.push(String::from_utf8(record).unwrap());
        }
        out
    }

    #[test]
    fn byte_separator_keeps_empty_records_and_last_line() {
        let mut r = reader(b"a\n\nb");
        assert_eq!(all(&mut r, &RsMode::Byte(b'\n')), vec!["a", "", "b"]);
    }

    #[test]
    fn paragraph_mode_skips_blank_lines() {
        let mut r = reader(b"\n\none\ntwo\n\n\nthree\n");
        assert_eq!(all(&mut r, &RsMode::Paragraph), vec!["one\ntwo", "three"]);
    }

    #[test]
    fn regex_separator_splits_on_matches() {
        let regex = Rc::new(Regex::new("[;,]+").unwrap());
        let mut r = reader(b"a;b,,c");
        assert_eq!(all(&mut r, &RsMode::Regex(regex)), vec!["a", "b", "c"]);
    }

    #[test]
    fn reads_across_buffer_refills() {
        let big: Vec<u8> = (0..200_000)
            .map(|i| if i % 7 == 6 { b'\n' } else { b'x' })
            .collect();
        let mut r = RecordReader::new(Box::new(io::Cursor::new(big.clone())));
        let mut count = 0;
        while let Some(record) = r.read(&RsMode::Byte(b'\n')).unwrap() {
            assert!(record.iter().all(|&b| b == b'x'));
            count += 1;
        }
        assert_eq!(count, big.iter().filter(|&&b| b == b'\n').count() + 1);
    }

    #[test]
    fn assignment_operands_need_an_identifier_name() {
        assert_eq!(split_assignment(b"x=1"), Some(("x", &b"1"[..])));
        assert_eq!(split_assignment(b"_a1=b=c"), Some(("_a1", &b"b=c"[..])));
        assert_eq!(split_assignment(b"1x=2"), None);
        assert_eq!(split_assignment(b"file.txt"), None);
        assert_eq!(split_assignment(b"a-b=1"), None);
    }
}
