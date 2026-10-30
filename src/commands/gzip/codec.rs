//! gzip container handling on top of flate2's raw deflate decoder.
//!
//! The header and trailer are parsed here so that member boundaries, the header length and
//! the CRC/ISIZE checks follow GNU gzip exactly. Only the deflate payload is delegated.

use std::io::{self, BufRead, Read, Write};

use flate2::bufread::DeflateDecoder;
use flate2::{Compression, Crc, GzBuilder};

const MAGIC: [u8; 2] = [0x1f, 0x8b];
const METHOD_DEFLATE: u8 = 8;
const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;
const RESERVED: u8 = 0xe0;
/// Bytes in the trailer: CRC-32 and input size, both little endian.
pub const TRAILER_LEN: u64 = 8;
const CHUNK: usize = 64 * 1024;
/// gzip's "Unix" operating-system code, written into compressed headers.
const OS_UNIX: u8 = 3;

/// Why a gzip stream could not be processed. `message` gives the GNU wording.
#[derive(Debug)]
pub enum DecodeError {
    NotGzip,
    Truncated,
    Crc,
    Length,
    Format,
    Io(io::Error),
}

impl DecodeError {
    pub fn message(&self) -> String {
        match self {
            Self::NotGzip => "not in gzip format".to_string(),
            Self::Truncated => "unexpected end of file".to_string(),
            Self::Crc => "invalid compressed data--crc error".to_string(),
            Self::Length => "invalid compressed data--length error".to_string(),
            Self::Format => "invalid compressed data--format violated".to_string(),
            Self::Io(error) => crate::common::error::io_error_text(error),
        }
    }
}

/// The parts of a member header that the commands use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// Total header length in bytes, including every optional field.
    pub len: u64,
    pub mtime: u32,
    pub name: Option<Vec<u8>>,
}

/// Figures for one decoded member, taken from the decoded data and the trailer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Member {
    pub out_len: u64,
    pub crc: u32,
}

/// Result of decoding a whole file: every member, plus trailing data that was ignored.
#[derive(Debug, Default)]
pub struct Summary {
    pub members: u64,
    pub first: Header,
    pub last: Member,
    pub out_total: u64,
    pub trailing_garbage: bool,
}

/// Reads one member header. Fails with `Truncated` at end of input and `NotGzip` on a bad
/// magic number or method.
pub fn read_header<R: BufRead + ?Sized>(reader: &mut R) -> Result<Header, DecodeError> {
    let mut magic = [0u8; 2];
    read_exact(reader, &mut magic)?;
    if magic != MAGIC {
        return Err(DecodeError::NotGzip);
    }
    let mut fixed = [0u8; 10];
    fixed[..2].copy_from_slice(&magic);
    read_exact(reader, &mut fixed[2..])?;
    if fixed[2] != METHOD_DEFLATE {
        return Err(DecodeError::NotGzip);
    }
    let flags = fixed[3];
    if flags & RESERVED != 0 {
        return Err(DecodeError::Format);
    }
    let mut header = Header {
        len: 10,
        mtime: u32::from_le_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]),
        name: None,
    };
    if flags & FEXTRA != 0 {
        let mut len = [0u8; 2];
        read_exact(reader, &mut len)?;
        let extra = u64::from(u16::from_le_bytes(len));
        skip(reader, extra)?;
        header.len += 2 + extra;
    }
    if flags & FNAME != 0 {
        let name = read_cstring(reader)?;
        header.len += name.len() as u64 + 1;
        header.name = Some(name);
    }
    if flags & FCOMMENT != 0 {
        header.len += read_cstring(reader)?.len() as u64 + 1;
    }
    if flags & FHCRC != 0 {
        skip(reader, 2)?;
        header.len += 2;
    }
    Ok(header)
}

fn read_exact<R: BufRead + ?Sized>(reader: &mut R, buf: &mut [u8]) -> Result<(), DecodeError> {
    reader.read_exact(buf).map_err(|error| match error.kind() {
        io::ErrorKind::UnexpectedEof => DecodeError::Truncated,
        _ => DecodeError::Io(error),
    })
}

fn skip<R: BufRead + ?Sized>(reader: &mut R, mut count: u64) -> Result<(), DecodeError> {
    while count > 0 {
        let available = reader.fill_buf().map_err(DecodeError::Io)?;
        if available.is_empty() {
            return Err(DecodeError::Truncated);
        }
        let step = available
            .len()
            .min(usize::try_from(count).unwrap_or(usize::MAX));
        reader.consume(step);
        count -= step as u64;
    }
    Ok(())
}

fn read_cstring<R: BufRead + ?Sized>(reader: &mut R) -> Result<Vec<u8>, DecodeError> {
    let mut text = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        read_exact(reader, &mut byte)?;
        if byte[0] == 0 {
            return Ok(text);
        }
        text.push(byte[0]);
    }
}

/// Decodes one member's deflate payload and trailer, writing the data to `sink`.
pub fn decode_member<R: BufRead + ?Sized>(
    reader: &mut R,
    sink: &mut dyn Write,
) -> Result<Member, DecodeError> {
    let mut crc = Crc::new();
    let mut out_len: u64 = 0;
    {
        let mut inflater = DeflateDecoder::new(&mut *reader);
        let mut buf = vec![0u8; CHUNK];
        loop {
            let read = match inflater.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) => return Err(map_inflate_error(error)),
            };
            crc.update(&buf[..read]);
            out_len += read as u64;
            sink.write_all(&buf[..read]).map_err(DecodeError::Io)?;
        }
    }
    let mut trailer = [0u8; 8];
    read_exact(reader, &mut trailer)?;
    let stored_crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let stored_len = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    if stored_crc != crc.sum() {
        return Err(DecodeError::Crc);
    }
    if stored_len != out_len as u32 {
        return Err(DecodeError::Length);
    }
    Ok(Member {
        out_len,
        crc: crc.sum(),
    })
}

fn map_inflate_error(error: io::Error) -> DecodeError {
    match error.kind() {
        io::ErrorKind::UnexpectedEof => DecodeError::Truncated,
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => DecodeError::Format,
        _ => DecodeError::Io(error),
    }
}

/// Decodes every member in the stream. Data after the last member that does not begin
/// with the gzip magic is reported as `trailing_garbage` and otherwise ignored.
pub fn decode_stream<R: BufRead + ?Sized>(
    reader: &mut R,
    sink: &mut dyn Write,
) -> Result<Summary, DecodeError> {
    let mut summary = Summary {
        first: read_header(reader)?,
        ..Summary::default()
    };
    summary.members = 1;
    summary.last = decode_member(reader, sink)?;
    summary.out_total = summary.last.out_len;
    loop {
        let pending = reader.fill_buf().map_err(DecodeError::Io)?;
        if pending.is_empty() {
            break;
        }
        if pending.len() < 2 || pending[..2] != MAGIC {
            summary.trailing_garbage = true;
            break;
        }
        read_header(reader)?;
        summary.members += 1;
        summary.last = decode_member(reader, sink)?;
        summary.out_total += summary.last.out_len;
    }
    Ok(summary)
}

/// Compresses `input` into `out` as one gzip member. `name` is stored in the header when
/// present; `mtime` is the modification time in seconds (0 when not stored).
pub fn encode<R: Read + ?Sized, W: Write>(
    input: &mut R,
    out: W,
    name: Option<&[u8]>,
    mtime: u32,
    level: u32,
) -> io::Result<W> {
    let mut builder = GzBuilder::new().mtime(mtime).operating_system(OS_UNIX);
    if let Some(name) = name {
        builder = builder.filename(name.to_vec());
    }
    let mut encoder = builder.write(out, Compression::new(level));
    io::copy(input, &mut encoder)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn gz(data: &[u8], name: Option<&[u8]>, mtime: u32) -> Vec<u8> {
        let mut input = Cursor::new(data.to_vec());
        encode(&mut input, Vec::new(), name, mtime, 6).unwrap()
    }

    #[test]
    fn round_trips_one_member() {
        let packed = gz(b"hello world\n", None, 0);
        let mut sink = Vec::new();
        let summary = decode_stream(&mut Cursor::new(packed), &mut sink).unwrap();
        assert_eq!(sink, b"hello world\n");
        assert_eq!(summary.members, 1);
        assert_eq!(summary.out_total, 12);
        assert!(!summary.trailing_garbage);
    }

    #[test]
    fn concatenated_members_decode_in_order() {
        let mut packed = gz(b"first\n", None, 0);
        packed.extend(gz(b"second\n", None, 0));
        let mut sink = Vec::new();
        let summary = decode_stream(&mut Cursor::new(packed), &mut sink).unwrap();
        assert_eq!(sink, b"first\nsecond\n");
        assert_eq!(summary.members, 2);
        assert_eq!(summary.last.out_len, 7);
    }

    #[test]
    fn header_length_counts_the_stored_name() {
        let packed = gz(b"x", Some(b"name.txt"), 1_700_000_000);
        let header = read_header(&mut Cursor::new(packed)).unwrap();
        assert_eq!(header.len, 10 + 9);
        assert_eq!(header.name.as_deref(), Some(&b"name.txt"[..]));
        assert_eq!(header.mtime, 1_700_000_000);
    }

    #[test]
    fn trailing_garbage_is_reported_not_fatal() {
        let mut packed = gz(b"data", None, 0);
        packed.extend(b"junk");
        let mut sink = Vec::new();
        let summary = decode_stream(&mut Cursor::new(packed), &mut sink).unwrap();
        assert!(summary.trailing_garbage);
        assert_eq!(sink, b"data");
    }

    #[test]
    fn empty_and_non_gzip_inputs_fail_with_gnu_wording() {
        let mut sink = Vec::new();
        let empty = decode_stream(&mut Cursor::new(Vec::new()), &mut sink).unwrap_err();
        assert_eq!(empty.message(), "unexpected end of file");
        let text = decode_stream(&mut Cursor::new(b"plain text".to_vec()), &mut sink).unwrap_err();
        assert_eq!(text.message(), "not in gzip format");
    }

    #[test]
    fn corrupt_trailer_and_truncation_are_detected() {
        let mut packed = gz(b"checksum me", None, 0);
        let len = packed.len();
        packed[len - 8] ^= 0xff;
        let mut sink = Vec::new();
        let crc = decode_stream(&mut Cursor::new(packed.clone()), &mut sink).unwrap_err();
        assert_eq!(crc.message(), "invalid compressed data--crc error");
        packed.truncate(len - 4);
        let cut = decode_stream(&mut Cursor::new(packed), &mut sink).unwrap_err();
        assert_eq!(cut.message(), "unexpected end of file");
    }

    #[test]
    fn length_mismatch_is_reported() {
        let mut packed = gz(b"abc", None, 0);
        let len = packed.len();
        packed[len - 1] = 0x7f;
        let mut sink = Vec::new();
        let error = decode_stream(&mut Cursor::new(packed), &mut sink).unwrap_err();
        assert!(matches!(error, DecodeError::Length));
    }

    #[test]
    fn reserved_flag_bits_are_format_errors() {
        let mut bytes = gz(b"z", None, 0);
        bytes[3] = 0x20;
        let error = read_header(&mut Cursor::new(bytes)).unwrap_err();
        assert!(matches!(error, DecodeError::Format));
    }
}
