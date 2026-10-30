//! Line input for sed: records from the named files (or standard input), with one
//! line of lookahead so that `$` can be recognised.

use std::io::{self, BufRead, BufReader};

use crate::common::error::io_error_text;
use crate::common::io::{open_input, read_record};

/// Opens an input operand. Standard input is wrapped without holding its lock, so a
/// later `r /dev/stdin` or `R /dev/stdin` can still read it instead of deadlocking.
pub fn open_source(name: &str) -> io::Result<Box<dyn BufRead>> {
    if name == "-" || name == "/dev/stdin" {
        return Ok(Box::new(BufReader::new(io::stdin())));
    }
    open_input(name)
}

/// One input record. `chomped` is false only for a last line without a delimiter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub text: Vec<u8>,
    pub chomped: bool,
    /// Index of the file the line came from.
    pub file: usize,
}

struct Source {
    reader: Box<dyn BufRead>,
    index: usize,
}

/// Reads records file by file. Unreadable files are reported and skipped.
pub struct Input {
    names: Vec<String>,
    next: usize,
    current: Option<Source>,
    peeked: Option<Line>,
    delim: u8,
    /// Set when an operand could not be opened (exit status 2).
    pub failed: bool,
    /// A read error met while looking ahead, reported by the engine after the cycle.
    pub error: Option<io::Error>,
}

impl Input {
    pub fn new(names: Vec<String>, delim: u8) -> Self {
        Input {
            names,
            next: 0,
            current: None,
            peeked: None,
            delim,
            failed: false,
            error: None,
        }
    }

    /// The name of file `index` as given on the command line (`-` for stdin).
    pub fn name(&self, index: usize) -> &str {
        self.names.get(index).map_or("-", String::as_str)
    }

    /// Returns the next record, or `None` when all input is consumed.
    pub fn read(&mut self) -> io::Result<Option<Line>> {
        if let Some(line) = self.peeked.take() {
            return Ok(Some(line));
        }
        loop {
            if let Some(line) = self.read_current()? {
                return Ok(Some(line));
            }
            if !self.open_next() {
                return Ok(None);
            }
        }
    }

    /// True when another record exists. With `cross_files` the following files are
    /// opened to look for one; without it only the current file is considered.
    pub fn has_more(&mut self, cross_files: bool) -> bool {
        if self.peeked.is_some() {
            return true;
        }
        loop {
            match self.read_current() {
                Ok(Some(line)) => {
                    self.peeked = Some(line);
                    return true;
                }
                Ok(None) => {}
                Err(error) => {
                    self.error = Some(error);
                    return false;
                }
            }
            if !cross_files || !self.open_next() {
                return false;
            }
        }
    }

    /// Drops the current file, closing it before its output is renamed.
    pub fn close(&mut self) {
        self.current = None;
        self.peeked = None;
    }

    fn read_current(&mut self) -> io::Result<Option<Line>> {
        let delim = self.delim;
        let Some(source) = self.current.as_mut() else {
            return Ok(None);
        };
        let mut buffer = Vec::new();
        if read_record(source.reader.as_mut(), delim, &mut buffer)? {
            let chomped = buffer.last() == Some(&delim);
            if chomped {
                buffer.pop();
            }
            return Ok(Some(Line {
                text: buffer,
                chomped,
                file: source.index,
            }));
        }
        self.current = None;
        Ok(None)
    }

    /// Opens the next operand that can be read. Returns false when none is left.
    fn open_next(&mut self) -> bool {
        self.current = None;
        while self.next < self.names.len() {
            let index = self.next;
            self.next += 1;
            let name = &self.names[index];
            match open_source(name) {
                Ok(reader) => {
                    self.current = Some(Source { reader, index });
                    return true;
                }
                Err(error) => {
                    eprintln!("sed: can't read {name}: {}", io_error_text(&error));
                    self.failed = true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_from(names: &[&str]) -> Input {
        Input::new(names.iter().map(|n| n.to_string()).collect(), b'\n')
    }

    #[test]
    fn records_report_whether_they_were_terminated() {
        let mut input = input_from(&[]);
        input.current = Some(Source {
            reader: Box::new(&b"one\ntwo"[..]),
            index: 0,
        });
        let first = input.read().unwrap().unwrap();
        assert_eq!(first.text, b"one");
        assert!(first.chomped);
        let last = input.read().unwrap().unwrap();
        assert_eq!(last.text, b"two");
        assert!(!last.chomped);
        assert!(input.read().unwrap().is_none());
    }

    #[test]
    fn lookahead_does_not_consume_the_line() {
        let mut input = input_from(&[]);
        input.current = Some(Source {
            reader: Box::new(&b"a\nb\n"[..]),
            index: 0,
        });
        assert!(input.has_more(false));
        assert!(input.has_more(false));
        assert_eq!(input.read().unwrap().unwrap().text, b"a");
        assert!(input.has_more(false));
        assert_eq!(input.read().unwrap().unwrap().text, b"b");
        assert!(!input.has_more(false));
    }

    #[test]
    fn missing_operands_are_skipped_and_flagged() {
        let dir = std::env::temp_dir().join(format!("agentcmd-sed-input-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let present = dir.join("present.txt");
        std::fs::write(&present, b"hello\n").unwrap();
        let missing = dir.join("missing.txt");
        let mut input = input_from(&[&missing.to_string_lossy(), &present.to_string_lossy()]);
        let line = input.read().unwrap().unwrap();
        assert_eq!(line.text, b"hello");
        assert_eq!(line.file, 1);
        assert!(input.failed);
        assert!(input.read().unwrap().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cross_file_lookahead_finds_later_files() {
        let dir = std::env::temp_dir().join(format!("agentcmd-sed-cross-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.txt");
        let second = dir.join("second.txt");
        std::fs::write(&first, b"x\n").unwrap();
        std::fs::write(&second, b"y\n").unwrap();
        let mut input = input_from(&[&first.to_string_lossy(), &second.to_string_lossy()]);
        assert_eq!(input.read().unwrap().unwrap().text, b"x");
        assert!(!input.has_more(false));
        assert!(input.has_more(true));
        let line = input.read().unwrap().unwrap();
        assert_eq!((line.text.as_slice(), line.file), (&b"y"[..], 1));
        // Close the open files before removing them: Windows refuses to delete open files.
        drop(input);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_fall_back_to_dash() {
        let input = input_from(&["a.txt"]);
        assert_eq!(input.name(0), "a.txt");
        assert_eq!(input.name(5), "-");
    }
}
