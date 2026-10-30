//! The non-cryptographic checksums: POSIX `cksum` CRC, the reflected CRC-32 used by
//! `crc32b`, and the BSD and System V `sum` algorithms.

use std::io::{self, BufRead};

/// A legacy checksum algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legacy {
    /// POSIX `cksum` CRC (polynomial 0x04C11DB7, length appended).
    Crc,
    /// CRC-32 as used by zlib and gzip.
    Crc32b,
    /// BSD `sum -r`: rotating 16-bit sum over 1024-byte blocks.
    Bsd,
    /// System V `sum -s`: 16-bit sum over 512-byte blocks.
    Sysv,
}

/// Result of one checksum: the value and the number of bytes read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sum {
    pub value: u32,
    pub size: u64,
}

const POSIX_POLY: u32 = 0x04C1_1DB7;
const IEEE_POLY_REFLECTED: u32 = 0xEDB8_8320;

/// Incremental state for one algorithm.
pub struct State {
    kind: Legacy,
    crc: u32,
    bsd: u16,
    sysv: u32,
    size: u64,
    posix_table: [u32; 256],
    ieee_table: [u32; 256],
}

impl State {
    pub fn new(kind: Legacy) -> Self {
        let crc = match kind {
            Legacy::Crc32b => 0xFFFF_FFFF,
            _ => 0,
        };
        Self {
            kind,
            crc,
            bsd: 0,
            sysv: 0,
            size: 0,
            posix_table: posix_table(),
            ieee_table: ieee_table(),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.size += data.len() as u64;
        match self.kind {
            Legacy::Crc => {
                for &byte in data {
                    let index = (((self.crc >> 24) ^ u32::from(byte)) & 0xff) as usize;
                    self.crc = (self.crc << 8) ^ self.posix_table[index];
                }
            }
            Legacy::Crc32b => {
                for &byte in data {
                    let index = ((self.crc ^ u32::from(byte)) & 0xff) as usize;
                    self.crc = (self.crc >> 8) ^ self.ieee_table[index];
                }
            }
            Legacy::Bsd => {
                for &byte in data {
                    self.bsd = self.bsd.rotate_right(1).wrapping_add(u16::from(byte));
                }
            }
            Legacy::Sysv => {
                for &byte in data {
                    self.sysv = self.sysv.wrapping_add(u32::from(byte));
                }
            }
        }
    }

    /// Finishes the computation and returns the value and byte count.
    pub fn finish(mut self) -> Sum {
        let value = match self.kind {
            Legacy::Crc => {
                // The byte count is appended least significant byte first, then the
                // CRC is complemented.
                let mut length = self.size;
                while length != 0 {
                    let index = (((self.crc >> 24) ^ (length as u32)) & 0xff) as usize;
                    self.crc = (self.crc << 8) ^ self.posix_table[index];
                    length >>= 8;
                }
                !self.crc
            }
            Legacy::Crc32b => !self.crc,
            Legacy::Bsd => u32::from(self.bsd),
            Legacy::Sysv => {
                let folded = (self.sysv & 0xffff) + (self.sysv >> 16);
                (folded & 0xffff) + (folded >> 16)
            }
        };
        Sum {
            value,
            size: self.size,
        }
    }
}

/// Reads `reader` to the end and returns the checksum of its bytes.
pub fn checksum(kind: Legacy, reader: &mut dyn BufRead) -> io::Result<Sum> {
    let mut state = State::new(kind);
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            break;
        }
        state.update(chunk);
        let used = chunk.len();
        reader.consume(used);
    }
    Ok(state.finish())
}

/// Number of `unit`-byte blocks needed for `size` bytes, as `sum` reports them.
pub fn blocks(size: u64, unit: u64) -> u64 {
    size.div_ceil(unit)
}

fn posix_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut value = (index as u32) << 24;
        for _ in 0..8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ POSIX_POLY
            } else {
                value << 1
            };
        }
        *slot = value;
    }
    table
}

fn ieee_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut value = index as u32;
        for _ in 0..8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ IEEE_POLY_REFLECTED
            } else {
                value >> 1
            };
        }
        *slot = value;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum_of(kind: Legacy, data: &[u8]) -> Sum {
        let mut reader: &[u8] = data;
        checksum(kind, &mut reader).expect("checksum")
    }

    #[test]
    fn posix_crc_matches_known_values() {
        // `printf 'hello\n' | cksum` reports 3015617425 6.
        assert_eq!(
            sum_of(Legacy::Crc, b"hello\n"),
            Sum {
                value: 3_015_617_425,
                size: 6
            }
        );
        assert_eq!(
            sum_of(Legacy::Crc, b""),
            Sum {
                value: 4_294_967_295,
                size: 0
            }
        );
    }

    #[test]
    fn crc32b_matches_zlib() {
        // zlib.crc32(b"hello\n") == 0x363a3020
        assert_eq!(sum_of(Legacy::Crc32b, b"hello\n").value, 0x363a_3020);
        assert_eq!(sum_of(Legacy::Crc32b, b"").value, 0);
    }

    #[test]
    fn bsd_and_sysv_sums_match_sum_tool() {
        // `sum` on a file holding "hello\n": BSD 36979 (1 block), SysV 542 (1 block).
        assert_eq!(sum_of(Legacy::Bsd, b"hello\n").value, 36_979);
        assert_eq!(sum_of(Legacy::Sysv, b"hello\n").value, 542);
    }

    #[test]
    fn blocks_round_up() {
        assert_eq!(blocks(0, 1024), 0);
        assert_eq!(blocks(1, 1024), 1);
        assert_eq!(blocks(1025, 1024), 2);
        assert_eq!(blocks(513, 512), 2);
    }

    #[test]
    fn streaming_matches_one_shot() {
        let data: Vec<u8> = (0..10_000u32).map(|n| (n * 31 % 251) as u8).collect();
        let mut state = State::new(Legacy::Crc);
        for piece in data.chunks(77) {
            state.update(piece);
        }
        assert_eq!(state.finish(), sum_of(Legacy::Crc, &data));
    }
}
