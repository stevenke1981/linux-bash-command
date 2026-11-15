use flate2::{Compression, GzBuilder};
use std::io::{self, Read, Write};

#[derive(Clone, Debug, Default)]
pub struct Header {
    pub name: Option<Vec<u8>>,
    pub mtime: u32,
}
pub fn decoder<R: Read>(input: R) -> (super::decode::Decoder<R>, Header) {
    super::decode::Decoder::new(input)
}
pub fn compress(
    input: &mut dyn Read,
    output: &mut dyn Write,
    level: u32,
    header: &Header,
) -> io::Result<u64> {
    let mut builder = GzBuilder::new().mtime(header.mtime);
    if let Some(name) = &header.name {
        builder = builder.filename(name.clone());
    }
    let mut encoder = builder.write(output, Compression::new(level));
    let bytes = io::copy(input, &mut encoder)?;
    encoder.finish()?;
    Ok(bytes)
}
#[cfg(test)]
pub fn decompress(input: impl Read, output: &mut dyn Write) -> io::Result<(u64, Header)> {
    let (mut decoder, header) = decoder(input);
    let bytes = io::copy(&mut decoder, output)?;
    Ok((bytes, header))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_header() {
        let mut encoded = Vec::new();
        compress(
            &mut &b"hello\0world"[..],
            &mut encoded,
            6,
            &Header {
                name: Some(b"test".to_vec()),
                mtime: 123,
            },
        )
        .unwrap();
        let mut out = Vec::new();
        let (n, head) = decompress(&encoded[..], &mut out).unwrap();
        assert_eq!(n, 11);
        assert_eq!(out, b"hello\0world");
        assert_eq!(head.name.unwrap(), b"test");
        assert_eq!(head.mtime, 123);
    }
    #[test]
    fn concatenated_members() {
        let mut encoded = Vec::new();
        for data in [b"one".as_slice(), b"two".as_slice()] {
            compress(&mut &*data, &mut encoded, 1, &Header::default()).unwrap();
        }
        let mut out = Vec::new();
        decompress(&encoded[..], &mut out).unwrap();
        assert_eq!(out, b"onetwo");
    }
    #[test]
    fn truncated_and_bad_crc_fail() {
        let mut encoded = Vec::new();
        compress(&mut &b"hello"[..], &mut encoded, 6, &Header::default()).unwrap();
        assert!(decompress(&encoded[..encoded.len() - 2], &mut Vec::new()).is_err());
        let end = encoded.len();
        encoded[end - 8] ^= 1;
        assert!(decompress(&encoded[..], &mut Vec::new()).is_err());
    }
}
