//! The splitting algorithms: by lines, bytes, line-bounded bytes, and by chunk count.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use crate::common::error::io_error_text;
use crate::common::io::{open_input, read_record};

use super::output::{Dest, Piece};
use super::{Mode, Options, Strategy};

const BUFFER: usize = 64 * 1024;

/// Splits `input` (a path or `-`) according to `opts`. Returns a message on failure.
pub(super) fn split(opts: &Options, input: &str) -> Result<(), String> {
    let needs_total =
        matches!(opts.mode, Mode::Chunks { strategy, .. } if strategy != Strategy::RoundRobin);
    let source = Source::open(input, needs_total)?;
    let mut stdout = BufWriter::with_capacity(BUFFER, io::stdout());
    let mut reader = source.reader;
    let result = match opts.mode {
        Mode::Lines(count) => by_lines(opts, &mut reader, count),
        Mode::Bytes(size) => by_bytes(opts, &mut reader, size),
        Mode::LineBytes(size) => by_line_bytes(opts, &mut reader, size),
        Mode::Chunks {
            strategy: Strategy::Bytes,
            count,
            only,
        } => by_chunk_bytes(opts, &mut reader, source.total, count, only, &mut stdout),
        Mode::Chunks {
            strategy: Strategy::Lines,
            count,
            only,
        } => by_chunk_lines(opts, &mut reader, source.total, count, only, &mut stdout),
        Mode::Chunks {
            strategy: Strategy::RoundRobin,
            count,
            only,
        } => by_round_robin(opts, &mut reader, count, only, &mut stdout),
    };
    result?;
    stdout
        .flush()
        .map_err(|e| format!("write error: {}", io_error_text(&e)))
}

/// Opened input plus, when requested, its total size. Non-seekable input is spooled to a
/// temporary file so that its size is known.
struct Source {
    reader: Box<dyn BufRead>,
    total: u64,
    _spool: Option<Spool>,
}

impl Source {
    fn open(input: &str, need_total: bool) -> Result<Self, String> {
        if need_total && input != "-" {
            if let Ok(meta) = fs::metadata(input) {
                if meta.is_file() {
                    let reader = open_input(input).map_err(|e| open_error(input, &e))?;
                    return Ok(Self {
                        reader,
                        total: meta.len(),
                        _spool: None,
                    });
                }
            }
        }
        if !need_total {
            let reader = open_input(input).map_err(|e| open_error(input, &e))?;
            return Ok(Self {
                reader,
                total: 0,
                _spool: None,
            });
        }
        let spool = Spool::from_stdin_or_path(input)?;
        let total = spool.len;
        let file = File::open(&spool.path).map_err(|e| open_error(input, &e))?;
        Ok(Self {
            reader: Box::new(BufReader::with_capacity(BUFFER, file)),
            total,
            _spool: Some(spool),
        })
    }
}

fn open_error(input: &str, error: &io::Error) -> String {
    format!(
        "cannot open '{input}' for reading: {}",
        io_error_text(error)
    )
}

/// A temporary copy of the input, removed when dropped.
struct Spool {
    path: PathBuf,
    len: u64,
}

impl Spool {
    fn from_stdin_or_path(input: &str) -> Result<Self, String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("agentcmd-split-{}-{stamp}", std::process::id()));
        let mut file = File::create(&path).map_err(|e| open_error(&path.to_string_lossy(), &e))?;
        let mut source = open_input(input).map_err(|e| open_error(input, &e))?;
        let len = io::copy(&mut source, &mut file)
            .map_err(|e| format!("read error: {}", io_error_text(&e)))?;
        file.flush()
            .map_err(|e| format!("write error: {}", io_error_text(&e)))?;
        Ok(Self { path, len })
    }
}

impl Drop for Spool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn read_error(error: &io::Error) -> String {
    format!("read error: {}", io_error_text(error))
}

/// Starts the next piece, first finishing the current one if there is one.
fn rotate<'a>(
    current: &mut Option<Piece<'a>>,
    index: &mut u64,
    dest: impl Fn() -> Dest<'a>,
) -> Result<(), String> {
    if let Some(piece) = current.take() {
        piece.finish()?;
        *index += 1;
    }
    *current = Some(Piece::new(dest(), *index, false)?);
    Ok(())
}

fn file_dest(opts: &Options) -> Dest<'_> {
    Dest {
        namer: &opts.namer,
        verbose: opts.verbose,
    }
}

/// `-l N`: at most N lines per piece.
fn by_lines(opts: &Options, reader: &mut dyn BufRead, count: u64) -> Result<(), String> {
    let mut record = Vec::new();
    let mut current: Option<Piece> = None;
    let mut index = 0;
    let mut in_piece = 0;
    while read_record(reader, opts.separator, &mut record).map_err(|e| read_error(&e))? {
        if current.is_none() || in_piece == count {
            rotate(&mut current, &mut index, || file_dest(opts))?;
            in_piece = 0;
        }
        if let Some(piece) = current.as_mut() {
            piece.write(&record)?;
        }
        in_piece += 1;
    }
    if let Some(piece) = current {
        piece.finish()?;
    }
    Ok(())
}

/// `-b N`: exactly N bytes per piece, cutting lines where needed.
fn by_bytes(opts: &Options, reader: &mut dyn BufRead, size: u64) -> Result<(), String> {
    let mut buffer = vec![0u8; BUFFER];
    let mut current: Option<Piece> = None;
    let mut index = 0;
    let mut in_piece = 0;
    loop {
        let count = reader.read(&mut buffer).map_err(|e| read_error(&e))?;
        if count == 0 {
            break;
        }
        let mut data = &buffer[..count];
        while !data.is_empty() {
            if current.is_none() || in_piece == size {
                rotate(&mut current, &mut index, || file_dest(opts))?;
                in_piece = 0;
            }
            let take = (size - in_piece).min(data.len() as u64) as usize;
            if let Some(piece) = current.as_mut() {
                piece.write(&data[..take])?;
            }
            in_piece += take as u64;
            data = &data[take..];
        }
    }
    if let Some(piece) = current {
        piece.finish()?;
    }
    Ok(())
}

/// `-C N`: whole lines, at most N bytes per piece; a line longer than N starts a new piece
/// and is cut every N bytes.
fn by_line_bytes(opts: &Options, reader: &mut dyn BufRead, size: u64) -> Result<(), String> {
    let mut record = Vec::new();
    let mut current: Option<Piece> = None;
    let mut index = 0;
    let mut in_piece: u64 = 0;
    while read_record(reader, b'\n', &mut record).map_err(|e| read_error(&e))? {
        let mut data = record.as_slice();
        let whole = data.len() as u64;
        if in_piece > 0 && in_piece + whole > size {
            rotate(&mut current, &mut index, || file_dest(opts))?;
            in_piece = 0;
        }
        while !data.is_empty() {
            if current.is_none() || in_piece == size {
                rotate(&mut current, &mut index, || file_dest(opts))?;
                in_piece = 0;
            }
            let take = (size - in_piece).min(data.len() as u64) as usize;
            if let Some(piece) = current.as_mut() {
                piece.write(&data[..take])?;
            }
            in_piece += take as u64;
            data = &data[take..];
        }
    }
    if let Some(piece) = current {
        piece.finish()?;
    }
    Ok(())
}

/// The byte size of chunk `i` (0-based) when `total` bytes are divided into `count` chunks:
/// the first `total % count` chunks get one extra byte.
fn chunk_size(total: u64, count: u64, i: u64) -> u64 {
    total / count + u64::from(i < total % count)
}

fn skip_bytes(reader: &mut dyn BufRead, mut count: u64) -> Result<(), String> {
    let mut buffer = vec![0u8; BUFFER];
    while count > 0 {
        let want = count.min(BUFFER as u64) as usize;
        let got = reader
            .read(&mut buffer[..want])
            .map_err(|e| read_error(&e))?;
        if got == 0 {
            break;
        }
        count -= got as u64;
    }
    Ok(())
}

fn copy_bytes(reader: &mut dyn BufRead, mut count: u64, piece: &mut Piece) -> Result<(), String> {
    let mut buffer = vec![0u8; BUFFER];
    while count > 0 {
        let want = count.min(BUFFER as u64) as usize;
        let got = reader
            .read(&mut buffer[..want])
            .map_err(|e| read_error(&e))?;
        if got == 0 {
            break;
        }
        piece.write(&buffer[..got])?;
        count -= got as u64;
    }
    Ok(())
}

/// `-n N`: N pieces of equal byte size (the remainder spread over the first pieces).
fn by_chunk_bytes(
    opts: &Options,
    reader: &mut dyn BufRead,
    total: u64,
    count: u64,
    only: Option<u64>,
    stdout: &mut BufWriter<io::Stdout>,
) -> Result<(), String> {
    for i in 0..count {
        let size = chunk_size(total, count, i);
        if only.is_some_and(|k| k != i + 1) {
            skip_bytes(reader, size)?;
            continue;
        }
        if only.is_some() {
            copy_bytes_to_stdout(reader, size, stdout)?;
            continue;
        }
        let mut piece = Piece::new(file_dest(opts), i, !opts.elide)?;
        copy_bytes(reader, size, &mut piece)?;
        piece.finish()?;
    }
    Ok(())
}

fn copy_bytes_to_stdout(
    reader: &mut dyn BufRead,
    count: u64,
    stdout: &mut BufWriter<io::Stdout>,
) -> Result<(), String> {
    let mut buffer = vec![0u8; BUFFER];
    let mut left = count;
    while left > 0 {
        let want = left.min(BUFFER as u64) as usize;
        let got = reader
            .read(&mut buffer[..want])
            .map_err(|e| read_error(&e))?;
        if got == 0 {
            break;
        }
        stdout
            .write_all(&buffer[..got])
            .map_err(|e| format!("write error: {}", io_error_text(&e)))?;
        left -= got as u64;
    }
    Ok(())
}

/// `-n l/N`: N pieces that end on line boundaries near equal byte offsets. A line belongs to
/// the piece in which it ends.
fn by_chunk_lines(
    opts: &Options,
    reader: &mut dyn BufRead,
    total: u64,
    count: u64,
    only: Option<u64>,
    stdout: &mut BufWriter<io::Stdout>,
) -> Result<(), String> {
    let mut record = Vec::new();
    let mut position = 0u64;
    for i in 0..count {
        let boundary = if i + 1 == count {
            u64::MAX
        } else {
            (i + 1)
                .checked_mul(total)
                .map(|x| x.div_ceil(count))
                .unwrap_or(u64::MAX)
        };
        let to_stdout = only == Some(i + 1);
        let mut piece = if only.is_none() {
            Some(Piece::new(file_dest(opts), i, !opts.elide)?)
        } else {
            None
        };
        while position < boundary {
            if !read_record(reader, opts.separator, &mut record).map_err(|e| read_error(&e))? {
                break;
            }
            position += record.len() as u64;
            if to_stdout {
                stdout
                    .write_all(&record)
                    .map_err(|e| format!("write error: {}", io_error_text(&e)))?;
            } else if let Some(piece) = piece.as_mut() {
                piece.write(&record)?;
            }
        }
        if let Some(piece) = piece {
            piece.finish()?;
        }
    }
    Ok(())
}

/// `-n r/N`: line k goes to piece k mod N, so every piece gets whole lines in turn.
fn by_round_robin(
    opts: &Options,
    reader: &mut dyn BufRead,
    count: u64,
    only: Option<u64>,
    stdout: &mut BufWriter<io::Stdout>,
) -> Result<(), String> {
    let mut pieces: Vec<Piece> = Vec::new();
    if only.is_none() {
        for i in 0..count {
            pieces.push(Piece::new(file_dest(opts), i, !opts.elide)?);
        }
    }
    let mut record = Vec::new();
    let mut line = 0u64;
    while read_record(reader, opts.separator, &mut record).map_err(|e| read_error(&e))? {
        let target = line % count;
        line += 1;
        match only {
            Some(k) if target + 1 == k => {
                stdout
                    .write_all(&record)
                    .map_err(|e| format!("write error: {}", io_error_text(&e)))?;
            }
            Some(_) => {}
            None => {
                if let Some(piece) = pieces.get_mut(target as usize) {
                    piece.write(&record)?;
                }
            }
        }
    }
    for piece in pieces {
        piece.finish()?;
    }
    Ok(())
}
