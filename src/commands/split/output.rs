//! One output piece: a named file created on first use (or up front), or standard output.

use std::fs::File;
use std::io::{self, BufWriter, Write};

use super::names::Namer;

/// Where a piece's bytes go: a file named by the namer, created when the piece is opened.
pub(super) struct Dest<'a> {
    pub(super) namer: &'a Namer,
    pub(super) verbose: bool,
}

/// A piece being written. A file piece is created lazily unless `eager` is set, so pieces
/// without data never leave an empty file behind (unless the caller asks for them).
pub(super) struct Piece<'a> {
    dest: Dest<'a>,
    index: u64,
    file: Option<BufWriter<File>>,
}

impl<'a> Piece<'a> {
    pub(super) fn new(dest: Dest<'a>, index: u64, eager: bool) -> Result<Self, String> {
        let mut piece = Self {
            dest,
            index,
            file: None,
        };
        if eager {
            piece.open()?;
        }
        Ok(piece)
    }

    fn open(&mut self) -> Result<(), String> {
        if self.file.is_some() {
            return Ok(());
        }
        let name = self
            .dest
            .namer
            .name(self.index)
            .ok_or_else(|| "output file suffixes exhausted".to_owned())?;
        if self.dest.verbose {
            let _ = writeln!(io::stdout(), "creating file '{name}'");
        }
        let file = File::create(&name).map_err(|error| {
            format!(
                "cannot open '{name}' for writing: {}",
                crate::common::error::io_error_text(&error)
            )
        })?;
        self.file = Some(BufWriter::with_capacity(64 * 1024, file));
        Ok(())
    }

    /// Appends `data` to the piece, creating the file first if needed.
    pub(super) fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.open()?;
        match self.file.as_mut() {
            Some(file) => file.write_all(data).map_err(|e| write_error(&e)),
            None => Ok(()),
        }
    }

    /// Flushes the file. Errors are reported for the piece's name.
    pub(super) fn finish(mut self) -> Result<(), String> {
        match self.file.as_mut() {
            Some(file) => file.flush().map_err(|e| write_error(&e)),
            None => Ok(()),
        }
    }
}

fn write_error(error: &io::Error) -> String {
    format!(
        "write error: {}",
        crate::common::error::io_error_text(error)
    )
}
