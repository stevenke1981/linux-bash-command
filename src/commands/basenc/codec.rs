//! Encoders and decoders for the RFC 4648 alphabets, base2 and z85.
//!
//! Bit-oriented encodings (base64, base32, base16, base2) share one accumulator. Decoding
//! emits the bytes of a quantum only once the whole quantum has been read, so a bad
//! character or padding drops the partial quantum, as GNU does. Only a truncated tail at
//! end of input (missing padding) emits its complete bytes before failing.

/// One supported encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Base64,
    Base64Url,
    Base32,
    Base32Hex,
    Base16,
    Base2Msbf,
    Base2Lsbf,
    Z85,
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BASE32HEX: &[u8; 32] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
const BASE16: &[u8; 16] = b"0123456789ABCDEF";
const BITS01: &[u8; 2] = b"01";
const Z85: &[u8; 85] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

/// The error text printed after the command name (`invalid input`, possibly with a
/// reason).
pub type CodecError = &'static str;

const INVALID: CodecError = "invalid input";

impl Encoding {
    /// Bits per symbol for the bit-oriented encodings.
    fn bits(self) -> u32 {
        match self {
            Encoding::Base64 | Encoding::Base64Url => 6,
            Encoding::Base32 | Encoding::Base32Hex => 5,
            Encoding::Base16 => 4,
            Encoding::Base2Msbf | Encoding::Base2Lsbf => 1,
            Encoding::Z85 => 0,
        }
    }

    /// Symbols per full quantum (3, 5, 1 or 1 bytes respectively for the bit encodings).
    fn quantum(self) -> usize {
        match self {
            Encoding::Base64 | Encoding::Base64Url => 4,
            Encoding::Base32 | Encoding::Base32Hex => 8,
            Encoding::Base16 => 2,
            Encoding::Base2Msbf | Encoding::Base2Lsbf => 8,
            Encoding::Z85 => 5,
        }
    }

    fn padded(self) -> bool {
        matches!(
            self,
            Encoding::Base64 | Encoding::Base64Url | Encoding::Base32 | Encoding::Base32Hex
        )
    }

    fn alphabet(self) -> &'static [u8] {
        match self {
            Encoding::Base64 => BASE64,
            Encoding::Base64Url => BASE64URL,
            Encoding::Base32 => BASE32,
            Encoding::Base32Hex => BASE32HEX,
            Encoding::Base16 => BASE16,
            Encoding::Base2Msbf | Encoding::Base2Lsbf => BITS01,
            Encoding::Z85 => Z85,
        }
    }

    /// Maps each byte to its symbol value, or 0xff when it is not in the alphabet.
    fn decode_table(self) -> [u8; 256] {
        let mut table = [0xffu8; 256];
        for (value, &symbol) in self.alphabet().iter().enumerate() {
            table[symbol as usize] = value as u8;
        }
        table
    }

    /// Whether `count` data symbols can end a padded quantum (the rest is `=`).
    fn valid_partial(self, count: usize) -> bool {
        match self {
            Encoding::Base64 | Encoding::Base64Url => matches!(count, 2 | 3),
            Encoding::Base32 | Encoding::Base32Hex => matches!(count, 2 | 4 | 5 | 7),
            _ => false,
        }
    }
}

/// Streaming encoder. Output symbols are wrapped after `wrap` columns (0 disables).
pub struct Encoder {
    enc: Encoding,
    wrap: usize,
    acc: u64,
    nbits: u32,
    column: usize,
    symbols: u64,
    bytes_pending: Vec<u8>,
}

impl Encoder {
    pub fn new(enc: Encoding, wrap: usize) -> Self {
        Self {
            enc,
            wrap,
            acc: 0,
            nbits: 0,
            column: 0,
            symbols: 0,
            bytes_pending: Vec::new(),
        }
    }

    /// Encodes `data`, appending to `out`.
    pub fn feed(&mut self, data: &[u8], out: &mut Vec<u8>) {
        if self.enc == Encoding::Z85 {
            self.bytes_pending.extend_from_slice(data);
            let whole = self.bytes_pending.len() / 4 * 4;
            let groups: Vec<u8> = self.bytes_pending.drain(..whole).collect();
            for group in groups.chunks(4) {
                let value = u32::from_be_bytes([group[0], group[1], group[2], group[3]]);
                for index in (0..5).rev() {
                    let digit = (value / 85u32.pow(index)) % 85;
                    self.emit(Z85[digit as usize], out);
                }
            }
            return;
        }
        let bits = self.enc.bits();
        let mask = (1u64 << bits) - 1;
        for &byte in data {
            let byte = if self.enc == Encoding::Base2Lsbf {
                byte.reverse_bits()
            } else {
                byte
            };
            self.acc = (self.acc << 8) | u64::from(byte);
            self.nbits += 8;
            while self.nbits >= bits {
                self.nbits -= bits;
                let symbol = (self.acc >> self.nbits) & mask;
                self.acc &= (1u64 << self.nbits) - 1;
                self.emit(self.enc.alphabet()[symbol as usize], out);
            }
        }
    }

    /// Flushes the final partial symbol and padding, and the closing newline.
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        if self.enc == Encoding::Z85 {
            if !self.bytes_pending.is_empty() {
                return Err("invalid input (length must be multiple of 4 characters)");
            }
        } else if self.nbits > 0 {
            let bits = self.enc.bits();
            let symbol = (self.acc << (bits - self.nbits)) & ((1u64 << bits) - 1);
            self.emit(self.enc.alphabet()[symbol as usize], out);
            self.nbits = 0;
        }
        if self.enc.padded() {
            let quantum = self.enc.quantum() as u64;
            while !self.symbols.is_multiple_of(quantum) {
                self.emit(b'=', out);
            }
        }
        if self.wrap > 0 && self.column > 0 {
            out.push(b'\n');
        }
        Ok(())
    }

    fn emit(&mut self, symbol: u8, out: &mut Vec<u8>) {
        if self.wrap > 0 && self.column == self.wrap {
            out.push(b'\n');
            self.column = 0;
        }
        out.push(symbol);
        self.column += 1;
        self.symbols += 1;
    }
}

/// Streaming decoder. `ignore_garbage` skips characters outside the alphabet.
pub struct Decoder {
    enc: Encoding,
    table: [u8; 256],
    ignore_garbage: bool,
    acc: u64,
    nbits: u32,
    count: usize,
    held: Vec<u8>,
    /// Pad characters seen after a partial quantum, and whether padding is complete.
    pads: usize,
    padding_done: bool,
    in_padding: bool,
    z85_group: Vec<u32>,
}

impl Decoder {
    pub fn new(enc: Encoding, ignore_garbage: bool) -> Self {
        Self {
            enc,
            table: enc.decode_table(),
            ignore_garbage,
            acc: 0,
            nbits: 0,
            count: 0,
            held: Vec::new(),
            pads: 0,
            padding_done: false,
            in_padding: false,
            z85_group: Vec::new(),
        }
    }

    /// Decodes `data`, appending complete bytes to `out`. On error, `out` holds what was
    /// decoded before the failure.
    pub fn feed(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
        for &byte in data {
            if byte == b'\n' {
                continue;
            }
            if self.enc == Encoding::Z85 {
                self.feed_z85(byte, out)?;
            } else {
                self.feed_bits(byte, out)?;
            }
        }
        Ok(())
    }

    /// Ends the input. A truncated quantum emits its complete bytes, then fails.
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        if self.enc == Encoding::Z85 {
            return if self.z85_group.is_empty() {
                Ok(())
            } else {
                Err("invalid input (length must be multiple of 5 characters)")
            };
        }
        if self.in_padding {
            return Err(INVALID);
        }
        if self.count == 0 {
            return Ok(());
        }
        out.append(&mut self.held);
        Err(INVALID)
    }

    fn feed_bits(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<(), CodecError> {
        if self.enc.padded() && byte == b'=' {
            return self.feed_pad(out);
        }
        if self.padding_done || self.in_padding {
            return self.garbage(out);
        }
        let value = self.table[byte as usize];
        if value == 0xff {
            return self.garbage(out);
        }
        let bits = self.enc.bits();
        self.acc = (self.acc << bits) | u64::from(value);
        self.nbits += bits;
        self.count += 1;
        while self.nbits >= 8 {
            self.nbits -= 8;
            let byte = (self.acc >> self.nbits) as u8;
            self.held.push(if self.enc == Encoding::Base2Lsbf {
                byte.reverse_bits()
            } else {
                byte
            });
            self.acc &= (1u64 << self.nbits) - 1;
        }
        if self.count == self.enc.quantum() {
            out.append(&mut self.held);
            self.count = 0;
            self.acc = 0;
            self.nbits = 0;
        }
        Ok(())
    }

    fn feed_pad(&mut self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        if self.padding_done {
            return self.garbage(out);
        }
        if !self.in_padding {
            if !self.enc.valid_partial(self.count) {
                self.held.clear();
                return Err(INVALID);
            }
            self.in_padding = true;
        }
        self.pads += 1;
        if self.count + self.pads == self.enc.quantum() {
            out.append(&mut self.held);
            self.padding_done = true;
            self.in_padding = false;
            self.count = 0;
            self.acc = 0;
            self.nbits = 0;
        }
        Ok(())
    }

    /// A character outside the alphabet, or padding in the wrong place. Like GNU, the
    /// complete bytes of the current quantum are kept; the rest is dropped.
    fn garbage(&mut self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        if self.ignore_garbage {
            return Ok(());
        }
        out.append(&mut self.held);
        Err(INVALID)
    }

    fn feed_z85(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<(), CodecError> {
        let value = self.table[byte as usize];
        if value == 0xff {
            if self.ignore_garbage {
                return Ok(());
            }
            return Err(INVALID);
        }
        self.z85_group.push(u32::from(value));
        if self.z85_group.len() == 5 {
            let value = self
                .z85_group
                .iter()
                .fold(0u32, |acc, &digit| acc.wrapping_mul(85).wrapping_add(digit));
            out.extend_from_slice(&value.to_be_bytes());
            self.z85_group.clear();
        }
        Ok(())
    }
}

/// Standard base64 of `bytes` without line wrapping (used for `cksum --base64`).
pub fn base64_text(bytes: &[u8]) -> String {
    let mut encoder = Encoder::new(Encoding::Base64, 0);
    let mut out = Vec::new();
    encoder.feed(bytes, &mut out);
    // Base64 never fails at finish: z85 is the only encoding that can.
    let _ = encoder.finish(&mut out);
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(enc: Encoding, wrap: usize, data: &[u8]) -> String {
        let mut encoder = Encoder::new(enc, wrap);
        let mut out = Vec::new();
        encoder.feed(data, &mut out);
        encoder.finish(&mut out).expect("encode");
        String::from_utf8(out).expect("ascii")
    }

    fn decode(enc: Encoding, text: &[u8]) -> (Vec<u8>, Result<(), CodecError>) {
        let mut decoder = Decoder::new(enc, false);
        let mut out = Vec::new();
        let result = decoder
            .feed(text, &mut out)
            .and_then(|()| decoder.finish(&mut out));
        (out, result)
    }

    #[test]
    fn base64_matches_rfc_vectors() {
        assert_eq!(encode(Encoding::Base64, 0, b"foobar"), "Zm9vYmFy");
        assert_eq!(encode(Encoding::Base64, 0, b"fo"), "Zm8=");
        assert_eq!(encode(Encoding::Base64, 0, b"f"), "Zg==");
        assert_eq!(encode(Encoding::Base64, 0, b""), "");
    }

    #[test]
    fn base32_and_base16_match_rfc_vectors() {
        assert_eq!(encode(Encoding::Base32, 0, b"foobar"), "MZXW6YTBOI======");
        assert_eq!(encode(Encoding::Base32Hex, 0, b"f"), "CO======");
        assert_eq!(encode(Encoding::Base16, 0, b"hi"), "6869");
    }

    #[test]
    fn base2_and_z85_encode() {
        assert_eq!(encode(Encoding::Base2Msbf, 0, b"h"), "01101000");
        assert_eq!(encode(Encoding::Base2Lsbf, 0, b"h"), "00010110");
        assert_eq!(
            encode(Encoding::Z85, 0, b"\x86\x4f\xd2\x6f\xb5\x59\xf7\x5b"),
            "HelloWorld"
        );
    }

    #[test]
    fn wrapping_inserts_newlines_and_a_final_one() {
        assert_eq!(
            encode(Encoding::Base64, 4, b"hello world"),
            "aGVs\nbG8g\nd29y\nbGQ=\n"
        );
        assert_eq!(
            encode(Encoding::Base64, 0, b"hello world"),
            "aGVsbG8gd29ybGQ="
        );
    }

    #[test]
    fn decoding_round_trips_each_encoding() {
        let data: Vec<u8> = (0..=255u8).collect();
        for enc in [
            Encoding::Base64,
            Encoding::Base64Url,
            Encoding::Base32,
            Encoding::Base32Hex,
            Encoding::Base16,
        ] {
            let text = encode(enc, 0, &data);
            let (out, result) = decode(enc, text.as_bytes());
            assert_eq!(result, Ok(()), "{enc:?}");
            assert_eq!(out, data, "{enc:?}");
        }
    }

    #[test]
    fn truncated_base64_emits_complete_bytes_then_fails() {
        let (out, result) = decode(Encoding::Base64, b"aGVsbG8");
        assert_eq!(out, b"hello");
        assert_eq!(result, Err(INVALID));
    }

    #[test]
    fn garbage_stops_after_the_complete_quantum() {
        let (out, result) = decode(Encoding::Base64, b"aGVs bG8=");
        assert_eq!(out, b"hel");
        assert_eq!(result, Err(INVALID));
    }

    #[test]
    fn bad_padding_drops_the_partial_quantum() {
        let (out, result) = decode(Encoding::Base32, b"NBUQ=");
        assert!(out.is_empty());
        assert_eq!(result, Err(INVALID));
    }

    #[test]
    fn ignore_garbage_skips_bad_characters() {
        let mut decoder = Decoder::new(Encoding::Base64, true);
        let mut out = Vec::new();
        decoder.feed(b"aGVs bG8=", &mut out).expect("feed");
        decoder.finish(&mut out).expect("finish");
        assert_eq!(out, b"hello");
    }

    #[test]
    fn z85_round_trip_and_length_errors() {
        let (out, result) = decode(Encoding::Z85, b"HelloWorld");
        assert_eq!(out, b"\x86\x4f\xd2\x6f\xb5\x59\xf7\x5b");
        assert_eq!(result, Ok(()));
        let (_, result) = decode(Encoding::Z85, b"Hello0");
        assert!(result.is_err());
    }

    #[test]
    fn z85_encode_requires_multiple_of_four() {
        let mut encoder = Encoder::new(Encoding::Z85, 0);
        let mut out = Vec::new();
        encoder.feed(b"hi", &mut out);
        assert!(encoder.finish(&mut out).is_err());
    }

    #[test]
    fn base64_text_has_no_wrapping() {
        assert_eq!(base64_text(b"hi"), "aGk=");
    }
}
