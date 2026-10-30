//! Reverse conversion: hexdump text (or plain hex with `-p`) back to bytes.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Seek as _, SeekFrom, Stdout, Write};

use super::{open_input, Failure, Opts, SeekPos};

/// Reverse conversion: a hexdump (or plain hex with `-p`) back to bytes, written at the
/// addresses found in the dump plus the `-s` shift.
pub(super) fn reverse(opts: &Opts) -> Result<(), Failure> {
    let name = opts.infile.clone().unwrap_or_else(|| "-".to_string());
    let (reader, _) = open_input(&name, None)?;
    let mut reader = BufReader::new(reader);
    let shift = match opts.seek {
        Some(SeekPos::Start(n)) | Some(SeekPos::End(n)) => n,
        None => 0,
    };
    let target_name = opts.outfile.clone().unwrap_or_else(|| "-".to_string());
    let mut sink = Sink::open(&target_name).map_err(|e| Failure::Io(target_name.clone(), e))?;
    reverse_lines(&mut reader, &mut sink, shift, opts.plain)
        .and_then(|()| sink.finish())
        .map_err(|e| Failure::Io(name, e))
}

/// Decoded bytes are written at tracked positions: gaps are zero-filled, and a backward
/// move needs a seekable output file.
struct Sink {
    pos: u64,
    inner: SinkInner,
}

enum SinkInner {
    Stdout(BufWriter<Stdout>),
    File(BufWriter<File>),
}

impl Sink {
    fn open(name: &str) -> io::Result<Self> {
        let inner = if name == "-" {
            SinkInner::Stdout(BufWriter::with_capacity(64 * 1024, io::stdout()))
        } else {
            SinkInner::File(BufWriter::new(File::create(name)?))
        };
        Ok(Self { pos: 0, inner })
    }

    fn write_at(&mut self, at: u64, bytes: &[u8]) -> io::Result<()> {
        if at > self.pos {
            let zeros = [0u8; 4096];
            let mut gap = at - self.pos;
            while gap > 0 {
                let n = gap.min(zeros.len() as u64) as usize;
                self.raw().write_all(&zeros[..n])?;
                gap -= n as u64;
            }
            self.pos = at;
        } else if at < self.pos {
            match &mut self.inner {
                SinkInner::File(file) => {
                    file.flush()?;
                    file.seek(SeekFrom::Start(at))?;
                }
                SinkInner::Stdout(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "cannot seek backwards on standard output",
                    ));
                }
            }
            self.pos = at;
        }
        self.raw().write_all(bytes)?;
        self.pos += bytes.len() as u64;
        Ok(())
    }

    fn raw(&mut self) -> &mut dyn Write {
        match &mut self.inner {
            SinkInner::Stdout(w) => w,
            SinkInner::File(w) => w,
        }
    }

    fn finish(mut self) -> io::Result<()> {
        self.raw().flush()
    }
}

/// Decodes dump lines (`address: hex  ascii`), or plain hex when `plain` is set, and
/// writes the bytes. Lines without an address continue from the previous line.
fn reverse_lines(
    reader: &mut dyn BufRead,
    sink: &mut Sink,
    shift: u64,
    plain: bool,
) -> io::Result<()> {
    let mut line = Vec::new();
    let mut running = 0u64;
    let mut pending: Option<u8> = None;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let (address, bytes) = if plain {
            (None, decode_plain(&line, &mut pending))
        } else {
            decode_dump_line(&line)
        };
        let base = address.unwrap_or(running);
        if !bytes.is_empty() {
            sink.write_at(base.wrapping_add(shift), &bytes)?;
        }
        running = base.wrapping_add(bytes.len() as u64);
    }
    Ok(())
}

/// Hex digit pairs in plain text; other bytes are ignored. A lone digit waits in `pending`
/// for its partner, which may be on a later line.
pub(super) fn decode_plain(line: &[u8], pending: &mut Option<u8>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &c in line {
        let Some(value) = hex_value(c) else { continue };
        match pending.take() {
            None => *pending = Some(value),
            Some(high) => bytes.push((high << 4) | value),
        }
    }
    bytes
}

/// Splits one dump line into its address (when present) and the bytes in its hex columns.
/// Parsing stops at two spaces, which begin the character column, or at a non-hex byte.
pub(super) fn decode_dump_line(line: &[u8]) -> (Option<u64>, Vec<u8>) {
    let (address, body) = match line.iter().position(|&b| b == b':') {
        Some(colon) => {
            let text = String::from_utf8_lossy(&line[..colon]).trim().to_string();
            (u64::from_str_radix(&text, 16).ok(), &line[colon + 1..])
        }
        None => (None, line),
    };
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if body[i] == b' ' {
            if body.get(i + 1) == Some(&b' ') {
                break;
            }
            i += 1;
            continue;
        }
        match (
            hex_value(body[i]),
            body.get(i + 1).and_then(|&c| hex_value(c)),
        ) {
            (Some(high), Some(low)) => {
                bytes.push((high << 4) | low);
                i += 2;
            }
            _ => break,
        }
    }
    (address, bytes)
}

pub(super) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
