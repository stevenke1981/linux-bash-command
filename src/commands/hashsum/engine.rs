use blake2::digest::{
    block_api::{Buffer, UpdateCore, VariableOutputCore},
    Digest, Output,
};
use std::io::{self, Read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Blake2b(usize),
    Crc,
    Crc32b,
    Bsd,
    Sysv,
}

impl Algorithm {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "md5" => Self::Md5,
            "sha1" => Self::Sha1,
            "sha224" => Self::Sha224,
            "sha256" => Self::Sha256,
            "sha384" => Self::Sha384,
            "sha512" => Self::Sha512,
            "blake2b" | "blake2b-512" => Self::Blake2b(64),
            "crc" => Self::Crc,
            "crc32b" => Self::Crc32b,
            "bsd" => Self::Bsd,
            "sysv" => Self::Sysv,
            _ => return None,
        })
    }
    pub fn crypto(self) -> bool {
        !matches!(self, Self::Crc | Self::Crc32b | Self::Bsd | Self::Sysv)
    }
    pub fn tag(self) -> String {
        match self {
            Self::Md5 => "MD5".into(),
            Self::Sha1 => "SHA1".into(),
            Self::Sha224 => "SHA224".into(),
            Self::Sha256 => "SHA256".into(),
            Self::Sha384 => "SHA384".into(),
            Self::Sha512 => "SHA512".into(),
            Self::Blake2b(64) => "BLAKE2b".into(),
            Self::Blake2b(n) => format!("BLAKE2b-{}", n * 8),
            Self::Crc => "CRC".into(),
            Self::Crc32b => "CRC32B".into(),
            Self::Bsd => "BSD".into(),
            Self::Sysv => "SYSV".into(),
        }
    }
    pub fn bytes(self) -> usize {
        match self {
            Self::Md5 => 16,
            Self::Sha1 => 20,
            Self::Sha224 => 28,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
            Self::Blake2b(n) => n,
            Self::Crc | Self::Crc32b => 4,
            Self::Bsd | Self::Sysv => 2,
        }
    }
}

trait StreamDigest {
    fn update(&mut self, data: &[u8]);
    fn finish(self: Box<Self>) -> Vec<u8>;
}
struct Fixed<D: Digest>(D);
impl<D: Digest> StreamDigest for Fixed<D> {
    fn update(&mut self, data: &[u8]) {
        Digest::update(&mut self.0, data);
    }
    fn finish(self: Box<Self>) -> Vec<u8> {
        self.0.finalize().to_vec()
    }
}
struct Blake {
    core: blake2::Blake2bVarCore,
    buffer: Buffer<blake2::Blake2bVarCore>,
    bytes: usize,
}
impl StreamDigest for Blake {
    fn update(&mut self, data: &[u8]) {
        let core = &mut self.core;
        self.buffer
            .digest_blocks(data, |blocks| core.update_blocks(blocks));
    }
    fn finish(mut self: Box<Self>) -> Vec<u8> {
        let mut result = Output::<blake2::Blake2bVarCore>::default();
        self.core
            .finalize_variable_core(&mut self.buffer, &mut result);
        result[..self.bytes].to_vec()
    }
}

fn crypto(algorithm: Algorithm) -> Box<dyn StreamDigest> {
    match algorithm {
        Algorithm::Md5 => Box::new(Fixed(md5::Md5::new())),
        Algorithm::Sha1 => Box::new(Fixed(sha1::Sha1::new())),
        Algorithm::Sha224 => Box::new(Fixed(sha2::Sha224::new())),
        Algorithm::Sha256 => Box::new(Fixed(sha2::Sha256::new())),
        Algorithm::Sha384 => Box::new(Fixed(sha2::Sha384::new())),
        Algorithm::Sha512 => Box::new(Fixed(sha2::Sha512::new())),
        Algorithm::Blake2b(bytes) => Box::new(Blake {
            core: blake2::Blake2bVarCore::new(bytes).expect("validated digest size"),
            buffer: Buffer::<blake2::Blake2bVarCore>::default(),
            bytes,
        }),
        _ => unreachable!(),
    }
}

pub struct Value {
    pub bytes: Vec<u8>,
    pub size: u64,
}
impl Value {
    pub fn hex(&self) -> String {
        self.bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
    pub fn number(&self) -> u32 {
        self.bytes.iter().fold(0, |n, b| (n << 8) | u32::from(*b))
    }
}

pub fn hash(algorithm: Algorithm, reader: &mut dyn Read) -> io::Result<Value> {
    let mut engine = algorithm.crypto().then(|| crypto(algorithm));
    let mut crc = if algorithm == Algorithm::Crc32b {
        u32::MAX
    } else {
        0
    };
    let mut sum = 0u32;
    let mut size = 0u64;
    let mut buffer = [0u8; crate::common::io::BUFFER_SIZE];
    loop {
        let count = match reader.read(&mut buffer) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("input too large"))?;
        if let Some(engine) = &mut engine {
            engine.update(&buffer[..count]);
        } else {
            update_numeric(algorithm, &buffer[..count], &mut crc, &mut sum);
        }
    }
    let bytes = if let Some(engine) = engine {
        engine.finish()
    } else {
        match algorithm {
            Algorithm::Crc => {
                let mut length = size;
                while length != 0 {
                    crc = crc_byte(crc, length as u8);
                    length >>= 8;
                }
                (!crc).to_be_bytes().to_vec()
            }
            Algorithm::Crc32b => (!crc).to_be_bytes().to_vec(),
            Algorithm::Bsd => (sum as u16).to_be_bytes().to_vec(),
            Algorithm::Sysv => {
                sum = (sum & 0xffff) + (sum >> 16);
                sum = (sum & 0xffff) + (sum >> 16);
                (sum as u16).to_be_bytes().to_vec()
            }
            _ => unreachable!(),
        }
    };
    Ok(Value { bytes, size })
}
fn update_numeric(algorithm: Algorithm, data: &[u8], crc: &mut u32, sum: &mut u32) {
    for &byte in data {
        match algorithm {
            Algorithm::Crc => *crc = crc_byte(*crc, byte),
            Algorithm::Crc32b => *crc = (*crc >> 8) ^ CRC32[((*crc as u8) ^ byte) as usize],
            Algorithm::Bsd => {
                *sum = u32::from((*sum as u16).rotate_right(1).wrapping_add(u16::from(byte)))
            }
            Algorithm::Sysv => *sum = sum.wrapping_add(u32::from(byte)),
            _ => unreachable!(),
        }
    }
}
fn crc_byte(crc: u32, byte: u8) -> u32 {
    (crc << 8) ^ POSIX[((crc >> 24) as u8 ^ byte) as usize]
}
const fn table(reflected: bool) -> [u32; 256] {
    let mut result = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = if reflected {
            i as u32
        } else {
            (i as u32) << 24
        };
        let mut j = 0;
        while j < 8 {
            crc = if reflected {
                if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb88320
                } else {
                    crc >> 1
                }
            } else if crc & 0x80000000 != 0 {
                (crc << 1) ^ 0x04c11db7
            } else {
                crc << 1
            };
            j += 1;
        }
        result[i] = crc;
        i += 1;
    }
    result
}
const POSIX: [u32; 256] = table(false);
const CRC32: [u32; 256] = table(true);

#[cfg(test)]
mod tests {
    use super::*;
    fn vector(algorithm: Algorithm, name: &str, hash_name: &str) {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/data/hash_vectors.json")).unwrap();
        let case = cases
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        let hex = case["hex"].as_str().unwrap();
        let pattern: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let data = pattern.repeat(case["repeat"].as_u64().unwrap() as usize);
        let result = hash(algorithm, &mut &data[..]).unwrap();
        assert_eq!(result.hex(), case["digests"][hash_name].as_str().unwrap());
        assert_eq!(result.size, data.len() as u64);
    }
    macro_rules! vectors {
        ($module:ident,$algorithm:expr,$name:literal) => {
            mod $module {
                use super::*;
                #[test]
                fn empty() {
                    vector($algorithm, "empty", $name);
                }
                #[test]
                fn abc() {
                    vector($algorithm, "abc", $name);
                }
                #[test]
                fn binary() {
                    vector($algorithm, "binary", $name);
                }
                #[test]
                fn blocks() {
                    vector($algorithm, "blocks", $name);
                }
                #[test]
                fn tail() {
                    vector($algorithm, "tail", $name);
                }
            }
        };
    }
    vectors!(md5, Algorithm::Md5, "md5");
    vectors!(sha1, Algorithm::Sha1, "sha1");
    vectors!(sha224, Algorithm::Sha224, "sha224");
    vectors!(sha256, Algorithm::Sha256, "sha256");
    vectors!(sha384, Algorithm::Sha384, "sha384");
    vectors!(sha512, Algorithm::Sha512, "sha512");
    vectors!(blake2b, Algorithm::Blake2b(64), "blake2b");
    #[test]
    fn checksum_vectors() {
        for (algorithm, expected) in [
            (Algorithm::Crc, 930766865),
            (Algorithm::Crc32b, 0xcbf43926),
            (Algorithm::Bsd, 53615),
            (Algorithm::Sysv, 477),
        ] {
            assert_eq!(
                hash(algorithm, &mut &b"123456789"[..]).unwrap().number(),
                expected
            );
        }
    }
    #[test]
    fn crc_empty() {
        assert_eq!(
            hash(Algorithm::Crc, &mut io::empty()).unwrap().number(),
            u32::MAX
        );
    }
    #[test]
    fn empty_bsd() {
        assert_eq!(hash(Algorithm::Bsd, &mut io::empty()).unwrap().number(), 0);
    }
    #[test]
    fn sysv_carry() {
        assert_eq!(
            hash(Algorithm::Sysv, &mut &vec![255; 1000][..])
                .unwrap()
                .number(),
            58395
        );
    }
    #[test]
    fn names_and_lengths() {
        for name in [
            "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "blake2b", "crc", "crc32b",
            "bsd", "sysv",
        ] {
            let a = Algorithm::named(name).unwrap();
            assert!(!a.tag().is_empty());
            assert!(a.bytes() >= 2);
        }
        assert!(Algorithm::named("bad").is_none());
    }
}
