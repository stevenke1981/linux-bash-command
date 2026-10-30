//! Opening the archive: standard input and output, gzip filtering and record padding.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};

use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

/// GNU tar writes archives in records of 20 blocks of 512 bytes.
pub const RECORD_SIZE: u64 = 10240;
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const BUFFER: usize = 64 * 1024;

/// Opens the archive for reading. Gzip input is detected by its magic number, as GNU tar
/// does, so `-z` is not needed to list or extract a compressed archive.
pub fn open_source(archive: Option<&str>) -> io::Result<Box<dyn BufRead>> {
    let mut raw: Box<dyn BufRead> = match archive {
        None | Some("-") => Box::new(io::stdin().lock()),
        Some(path) => Box::new(BufReader::with_capacity(BUFFER, File::open(path)?)),
    };
    let compressed = raw.fill_buf()?.starts_with(&GZIP_MAGIC);
    if compressed {
        raw = Box::new(BufReader::with_capacity(BUFFER, MultiGzDecoder::new(raw)));
    }
    Ok(raw)
}

/// Destination of a created archive, optionally compressed with gzip.
pub struct Sink {
    inner: Inner,
}

enum Inner {
    Plain(Box<dyn Write>),
    Gzip(GzEncoder<Box<dyn Write>>),
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.inner {
            Inner::Plain(out) => out.write(buf),
            Inner::Gzip(out) => out.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            Inner::Plain(out) => out.flush(),
            Inner::Gzip(out) => out.flush(),
        }
    }
}

impl Sink {
    /// Writes the gzip trailer when compressing and flushes everything.
    pub fn finish(self) -> io::Result<()> {
        match self.inner {
            Inner::Plain(mut out) => out.flush(),
            Inner::Gzip(encoder) => {
                let mut out = encoder.finish()?;
                out.flush()
            }
        }
    }
}

/// Opens the archive for writing. `-` and no name mean standard output.
pub fn open_sink(archive: Option<&str>, gzip: bool) -> io::Result<Sink> {
    let raw: Box<dyn Write> = match archive {
        None | Some("-") => Box::new(BufWriter::with_capacity(BUFFER, io::stdout())),
        Some(path) => Box::new(BufWriter::with_capacity(BUFFER, File::create(path)?)),
    };
    let inner = if gzip {
        Inner::Gzip(GzEncoder::new(raw, Compression::default()))
    } else {
        Inner::Plain(raw)
    };
    Ok(Sink { inner })
}

/// Counts the bytes written through it, so the caller can pad to a record boundary.
pub struct Counting<W: Write> {
    inner: W,
    pub written: u64,
}

impl<W: Write> Counting<W> {
    pub fn new(inner: W) -> Self {
        Self { inner, written: 0 }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(buf)?;
        self.written += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Zero bytes that bring `written` up to the next full record.
pub fn record_padding(written: u64) -> u64 {
    (RECORD_SIZE - written % RECORD_SIZE) % RECORD_SIZE
}

/// True when `-a` should compress, judged from the archive name.
pub fn auto_gzip(archive: Option<&str>) -> bool {
    match archive {
        Some(name) => name.ends_with(".gz") || name.ends_with(".tgz"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn padding_reaches_the_next_record() {
        assert_eq!(record_padding(0), 0);
        assert_eq!(record_padding(1024), RECORD_SIZE - 1024);
        assert_eq!(record_padding(RECORD_SIZE), 0);
        assert_eq!(record_padding(RECORD_SIZE + 1), RECORD_SIZE - 1);
    }

    #[test]
    fn counting_tracks_bytes() {
        let mut counted = Counting::new(Vec::new());
        counted.write_all(b"abc").unwrap();
        counted.write_all(b"de").unwrap();
        assert_eq!(counted.written, 5);
        assert_eq!(counted.into_inner(), b"abcde");
    }

    #[test]
    fn auto_gzip_follows_the_name() {
        assert!(auto_gzip(Some("a.tar.gz")));
        assert!(auto_gzip(Some("a.tgz")));
        assert!(!auto_gzip(Some("a.tar")));
        assert!(!auto_gzip(None));
    }

    #[test]
    fn gzip_sink_output_decodes_back() {
        let dir = std::env::temp_dir().join(format!("agentcmd-tar-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.tgz");
        let name = path.to_string_lossy().into_owned();
        let mut sink = open_sink(Some(&name), true).unwrap();
        sink.write_all(b"payload").unwrap();
        sink.finish().unwrap();
        let mut reader = open_source(Some(&name)).unwrap();
        let mut text = Vec::new();
        reader.read_to_end(&mut text).unwrap();
        assert_eq!(text, b"payload");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
