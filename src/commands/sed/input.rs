use crate::common::{error, path::display_path};
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
};

pub struct Record {
    pub bytes: Vec<u8>,
    pub terminated: bool,
    pub name: String,
    pub file: usize,
}
pub struct Input {
    names: Vec<String>,
    index: usize,
    reader: Option<Box<dyn BufRead>>,
    stdin: Option<Box<dyn BufRead>>,
    delimiter: u8,
    unbuffered: bool,
    pub lookahead: Option<Record>,
    pub failed: bool,
    exhausted: bool,
}

impl Input {
    pub fn new(names: &[String], delimiter: u8, unbuffered: bool) -> io::Result<Self> {
        let names = if names.is_empty() {
            vec!["-".into()]
        } else {
            names.to_vec()
        };
        Ok(Self {
            names,
            index: 0,
            reader: None,
            stdin: None,
            delimiter,
            unbuffered,
            lookahead: None,
            failed: false,
            exhausted: false,
        })
    }
    pub fn take(&mut self, current: Option<usize>, separate: bool) -> io::Result<Option<Record>> {
        self.ensure()?;
        if separate
            && current.is_some()
            && self
                .lookahead
                .as_ref()
                .is_some_and(|next| Some(next.file) != current)
        {
            return Ok(None);
        }
        Ok(self.lookahead.take())
    }
    pub fn last(&mut self, file: usize, separate: bool) -> io::Result<bool> {
        self.ensure()?;
        Ok(self
            .lookahead
            .as_ref()
            .is_none_or(|next| separate && next.file != file))
    }
    fn ensure(&mut self) -> io::Result<()> {
        if self.lookahead.is_none() && !self.exhausted {
            self.lookahead = self.read()?;
            self.exhausted = self.lookahead.is_none();
        }
        Ok(())
    }
    fn read(&mut self) -> io::Result<Option<Record>> {
        loop {
            if self.reader.is_none() && !self.open()? {
                return Ok(None);
            }
            let mut bytes = Vec::new();
            if self
                .reader
                .as_mut()
                .unwrap()
                .read_until(self.delimiter, &mut bytes)
                .map_err(|e| {
                    io::Error::other(format!(
                        "read error on {}: {}",
                        self.names[self.index - 1],
                        error::io_text(&e)
                    ))
                })?
                != 0
            {
                let terminated = bytes.last() == Some(&self.delimiter);
                if terminated {
                    bytes.pop();
                }
                return Ok(Some(Record {
                    bytes,
                    terminated,
                    name: self.names[self.index - 1].clone(),
                    file: self.index,
                }));
            }
            if matches!(self.names[self.index - 1].as_str(), "-" | "/dev/stdin") {
                self.stdin = self.reader.take();
            }
            self.reader = None;
        }
    }
    fn open(&mut self) -> io::Result<bool> {
        while let Some(name) = self.names.get(self.index) {
            self.index += 1;
            let capacity = if self.unbuffered {
                1
            } else {
                crate::common::io::BUFFER_SIZE
            };
            let reader: io::Result<Box<dyn BufRead>> = match name.as_str() {
                "-" | "/dev/stdin" => Ok(self
                    .stdin
                    .take()
                    .unwrap_or_else(|| Box::new(BufReader::with_capacity(capacity, io::stdin())))),
                "/dev/null" => Ok(Box::new(io::empty())),
                _ => File::open(name).map(|file| {
                    Box::new(BufReader::with_capacity(capacity, file)) as Box<dyn BufRead>
                }),
            };
            match reader {
                Ok(reader) => {
                    self.reader = Some(reader);
                    return Ok(true);
                }
                Err(e) => {
                    if std::fs::metadata(name).is_ok_and(|meta| meta.is_dir()) {
                        return Err(io::Error::other(format!(
                            "read error on {}: Is a directory",
                            display_path(Path::new(name))
                        )));
                    }
                    eprintln!(
                        "sed: can't read {}: {}",
                        display_path(Path::new(name)),
                        error::io_text(&e)
                    );
                    self.failed = true;
                }
            }
        }
        Ok(false)
    }
}
