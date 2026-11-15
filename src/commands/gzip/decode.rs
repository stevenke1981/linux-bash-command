use flate2::bufread::GzDecoder;
use std::io::{self, BufRead, BufReader, Read};

struct Prefix<R> {
    inner: R,
    bytes: [u8; 2],
    offset: usize,
    length: usize,
}
impl<R: BufRead> BufRead for Prefix<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.offset < self.length {
            Ok(&self.bytes[self.offset..self.length])
        } else {
            self.inner.fill_buf()
        }
    }
    fn consume(&mut self, count: usize) {
        if self.offset < self.length {
            self.offset += count.min(self.length - self.offset);
        } else {
            self.inner.consume(count);
        }
    }
}
impl<R: BufRead> Read for Prefix<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}
type Member<R> = GzDecoder<Prefix<BufReader<R>>>;
pub struct Decoder<R: Read> {
    member: Option<Member<R>>,
    trailing: bool,
}
impl<R: Read> Decoder<R> {
    pub fn new(input: R) -> (Self, super::codec::Header) {
        let input = Prefix {
            inner: BufReader::new(input),
            bytes: [0; 2],
            offset: 0,
            length: 0,
        };
        let member = GzDecoder::new(input);
        let header = member
            .header()
            .map_or_else(super::codec::Header::default, |h| super::codec::Header {
                name: h.filename().map(<[u8]>::to_vec),
                mtime: h.mtime(),
            });
        (
            Self {
                member: Some(member),
                trailing: false,
            },
            header,
        )
    }
    pub fn trailing_garbage(&self) -> bool {
        self.trailing
    }
    fn next_member(&mut self) -> io::Result<bool> {
        let mut source = self.member.take().unwrap().into_inner();
        let mut padding = false;
        loop {
            let mut first = [0u8; 1];
            if source.read(&mut first)? == 0 {
                return Ok(false);
            }
            if first[0] == 0 {
                padding = true;
                continue;
            }
            if padding || first[0] != 0x1f {
                self.trailing = true;
                return Ok(false);
            }
            let mut second = [0u8; 1];
            source.read_exact(&mut second)?;
            if second[0] != 0x8b {
                self.trailing = true;
                return Ok(false);
            }
            source.bytes = [first[0], second[0]];
            source.offset = 0;
            source.length = 2;
            self.member = Some(GzDecoder::new(source));
            return Ok(true);
        }
    }
}
impl<R: Read> Read for Decoder<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            let Some(member) = self.member.as_mut() else {
                return Ok(0);
            };
            let n = member.read(out)?;
            if n != 0 {
                return Ok(n);
            }
            if !self.next_member()? {
                return Ok(0);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn member(bytes: &[u8]) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(bytes).unwrap();
        gz.finish().unwrap()
    }
    #[test]
    fn padding_and_garbage() {
        for suffix in [&b"\0"[..], &b"\0\0\0\0\0\0\0\0"[..]] {
            let mut input = member(b"abc");
            input.extend_from_slice(suffix);
            let (mut dec, _) = Decoder::new(&input[..]);
            let mut output = Vec::new();
            dec.read_to_end(&mut output).unwrap();
            assert_eq!(output, b"abc");
            assert!(!dec.trailing_garbage());
        }
        let mut input = member(b"abc");
        input.extend_from_slice(b"\0junk");
        let (mut dec, _) = Decoder::new(&input[..]);
        let mut output = Vec::new();
        dec.read_to_end(&mut output).unwrap();
        assert_eq!(output, b"abc");
        assert!(dec.trailing_garbage());
    }
    #[test]
    fn members_and_truncation() {
        let mut input = member(b"one");
        input.extend_from_slice(&member(b"two"));
        let (mut dec, _) = Decoder::new(&input[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"onetwo");
        let mut input = member(b"one");
        input.push(0x1f);
        assert!(Decoder::new(&input[..])
            .0
            .read_to_end(&mut Vec::new())
            .is_err());
    }
}
