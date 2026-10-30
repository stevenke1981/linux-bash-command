//! Digest algorithms shared by `md5sum` … `b2sum` and `cksum`, with a streaming
//! hasher over a `BufRead` source.

use std::io::{self, BufRead};

use blake2::digest::block_api::{UpdateCore, VariableOutputCore};
use blake2::digest::Digest;
use blake2::Blake2bVarCore;
use md5::Md5;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};

/// A cryptographic digest. `Blake2b` carries its output length in bytes (1..=64).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Blake2b(usize),
}

impl Algo {
    /// Digest length in bytes.
    pub fn digest_len(self) -> usize {
        match self {
            Algo::Md5 => 16,
            Algo::Sha1 => 20,
            Algo::Sha224 => 28,
            Algo::Sha256 => 32,
            Algo::Sha384 => 48,
            Algo::Sha512 => 64,
            Algo::Blake2b(bytes) => bytes,
        }
    }

    /// The upper-case label used in BSD-style lines (`MD5 (file) = …`).
    pub fn label(self) -> &'static str {
        match self {
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA1",
            Algo::Sha224 => "SHA224",
            Algo::Sha256 => "SHA256",
            Algo::Sha384 => "SHA384",
            Algo::Sha512 => "SHA512",
            Algo::Blake2b(_) => "BLAKE2b",
        }
    }

    /// The BSD-style tag: BLAKE2b digests shorter than the maximum carry `-BITS`.
    pub fn tag(self) -> String {
        match self {
            Algo::Blake2b(bytes) if bytes != 64 => format!("BLAKE2b-{}", bytes * 8),
            _ => self.label().to_owned(),
        }
    }

    /// Matches a BSD-style tag such as `SHA256` or `BLAKE2b-256`. Returns `None` for
    /// unknown names and for BLAKE2b lengths GNU would reject.
    pub fn from_tag(tag: &str) -> Option<Algo> {
        Some(match tag {
            "MD5" => Algo::Md5,
            "SHA1" => Algo::Sha1,
            "SHA224" => Algo::Sha224,
            "SHA256" => Algo::Sha256,
            "SHA384" => Algo::Sha384,
            "SHA512" => Algo::Sha512,
            "BLAKE2b" => Algo::Blake2b(64),
            _ => {
                let bits = tag.strip_prefix("BLAKE2b-")?.parse::<usize>().ok()?;
                let bytes = bits / 8;
                if bits % 8 != 0 || !(1..=64).contains(&bytes) {
                    return None;
                }
                Algo::Blake2b(bytes)
            }
        })
    }

    /// Creates a fresh hasher for this algorithm.
    pub fn hasher(self) -> Hasher {
        match self {
            Algo::Md5 => Hasher::Md5(Md5::new()),
            Algo::Sha1 => Hasher::Sha1(Sha1::new()),
            Algo::Sha224 => Hasher::Sha224(Sha224::new()),
            Algo::Sha256 => Hasher::Sha256(Sha256::new()),
            Algo::Sha384 => Hasher::Sha384(Sha384::new()),
            Algo::Sha512 => Hasher::Sha512(Sha512::new()),
            Algo::Blake2b(bytes) => Hasher::Blake2b(Blake2bDyn::new(bytes)),
        }
    }
}

/// BLAKE2b with a runtime output length. The digest crate's ready-made wrappers fix the
/// length at compile time, so this drives the variable-length core and its block buffer
/// directly. BLAKE2 derives the shorter digest from its own parameters, and the caller
/// keeps the first `bytes` bytes of the 64-byte output.
pub struct Blake2bDyn {
    core: Blake2bVarCore,
    buffer: blake2::digest::block_api::Buffer<Blake2bVarCore>,
    bytes: usize,
}

impl Blake2bDyn {
    fn new(bytes: usize) -> Self {
        let core = Blake2bVarCore::new(bytes).expect("BLAKE2b length is validated by callers");
        Self {
            core,
            buffer: Default::default(),
            bytes,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let core = &mut self.core;
        self.buffer
            .digest_blocks(data, |blocks| core.update_blocks(blocks));
    }

    fn finish(mut self) -> Vec<u8> {
        let mut out = blake2::digest::Output::<Blake2bVarCore>::default();
        self.core.finalize_variable_core(&mut self.buffer, &mut out);
        out[..self.bytes].to_vec()
    }
}

/// A running digest of any supported algorithm.
pub enum Hasher {
    Md5(Md5),
    Sha1(Sha1),
    Sha224(Sha224),
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
    Blake2b(Blake2bDyn),
}

impl Hasher {
    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Md5(h) => h.update(data),
            Hasher::Sha1(h) => h.update(data),
            Hasher::Sha224(h) => h.update(data),
            Hasher::Sha256(h) => h.update(data),
            Hasher::Sha384(h) => h.update(data),
            Hasher::Sha512(h) => h.update(data),
            Hasher::Blake2b(h) => h.update(data),
        }
    }

    pub fn finish(self) -> Vec<u8> {
        match self {
            Hasher::Md5(h) => h.finalize().to_vec(),
            Hasher::Sha1(h) => h.finalize().to_vec(),
            Hasher::Sha224(h) => h.finalize().to_vec(),
            Hasher::Sha256(h) => h.finalize().to_vec(),
            Hasher::Sha384(h) => h.finalize().to_vec(),
            Hasher::Sha512(h) => h.finalize().to_vec(),
            Hasher::Blake2b(h) => h.finish(),
        }
    }
}

/// Reads `reader` to the end and returns the digest and the number of bytes read.
pub fn digest_reader(algo: Algo, reader: &mut dyn BufRead) -> io::Result<(Vec<u8>, u64)> {
    let mut hasher = algo.hasher();
    let mut total = 0u64;
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            break;
        }
        hasher.update(chunk);
        let used = chunk.len();
        total += used as u64;
        reader.consume(used);
    }
    Ok((hasher.finish(), total))
}

/// Lower-case hexadecimal text of `bytes`.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

/// Decodes hexadecimal text (either case). Returns `None` for odd lengths or other
/// characters.
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(
        text.as_bytes()
            .chunks(2)
            .map(|pair| {
                let hi = (pair[0] as char).to_digit(16).unwrap_or(0);
                let lo = (pair[1] as char).to_digit(16).unwrap_or(0);
                (hi * 16 + lo) as u8
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(algo: Algo, data: &[u8]) -> String {
        let mut reader: &[u8] = data;
        to_hex(&digest_reader(algo, &mut reader).expect("digest").0)
    }

    #[test]
    fn known_vectors_for_fixed_algorithms() {
        assert_eq!(
            digest_of(Algo::Md5, b"abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            digest_of(Algo::Sha1, b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            digest_of(Algo::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest_of(Algo::Sha224, b""),
            "d14a028c2a3a2bc9476102bb288234c415a2b01f828ea62ac5b3e42f"
        );
    }

    #[test]
    fn blake2b_variable_length_is_a_prefix_of_its_own_parameters() {
        // BLAKE2b-512("") is a published vector; shorter digests are different hashes.
        let full = digest_of(Algo::Blake2b(64), b"");
        assert!(full.starts_with("786a02f742015903c6c6fd852552d272"));
        let short = digest_of(Algo::Blake2b(32), b"");
        assert_eq!(short.len(), 64);
        assert_ne!(short, full[..64]);
    }

    #[test]
    fn tags_round_trip() {
        assert_eq!(Algo::Blake2b(64).tag(), "BLAKE2b");
        assert_eq!(Algo::Blake2b(32).tag(), "BLAKE2b-256");
        assert_eq!(Algo::from_tag("BLAKE2b-256"), Some(Algo::Blake2b(32)));
        assert_eq!(Algo::from_tag("BLAKE2b-7"), None);
        assert_eq!(Algo::from_tag("BLAKE2b-1024"), None);
        assert_eq!(Algo::from_tag("SHA3"), None);
    }

    #[test]
    fn hex_encoding_round_trips() {
        assert_eq!(to_hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(from_hex("00ABff"), Some(vec![0x00, 0xab, 0xff]));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn reader_reports_byte_count() {
        let data = vec![7u8; 100_000];
        let mut reader: &[u8] = &data;
        let (digest, total) = digest_reader(Algo::Sha512, &mut reader).expect("digest");
        assert_eq!(total, 100_000);
        assert_eq!(digest.len(), 64);
    }
}
