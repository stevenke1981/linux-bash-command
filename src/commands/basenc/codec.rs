use std::io::{self, Read, Write};

#[derive(Debug)]
struct ReadFailure(io::Error);
impl std::fmt::Display for ReadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for ReadFailure {}
pub fn read_error(error: &io::Error) -> Option<&io::Error> {
    error.get_ref()?.downcast_ref::<ReadFailure>().map(|e| &e.0)
}
fn read(input: &mut dyn Read, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match input.read(buffer) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io::Error::new(e.kind(), ReadFailure(e))),
            result => return result,
        }
    }
}

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
impl Encoding {
    pub const NAMES: &'static [&'static str] = &[
        "base64",
        "base64url",
        "base32",
        "base32hex",
        "base16",
        "base2msbf",
        "base2lsbf",
        "z85",
    ];
    pub fn named(s: &str) -> Option<Self> {
        Some(match s {
            "base64" => Self::Base64,
            "base64url" => Self::Base64Url,
            "base32" => Self::Base32,
            "base32hex" => Self::Base32Hex,
            "base16" => Self::Base16,
            "base2msbf" => Self::Base2Msbf,
            "base2lsbf" => Self::Base2Lsbf,
            "z85" => Self::Z85,
            _ => return None,
        })
    }
    fn alphabet(self) -> &'static [u8] {
        match self{
        Self::Base64=>b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",Self::Base64Url=>b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
        Self::Base32=>b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",Self::Base32Hex=>b"0123456789ABCDEFGHIJKLMNOPQRSTUV",Self::Base16=>b"0123456789ABCDEF",
        Self::Base2Msbf|Self::Base2Lsbf=>b"01",Self::Z85=>b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#"}
    }
    fn bits(self) -> u32 {
        match self {
            Self::Base64 | Self::Base64Url => 6,
            Self::Base32 | Self::Base32Hex => 5,
            Self::Base16 => 4,
            Self::Base2Msbf | Self::Base2Lsbf => 1,
            Self::Z85 => 0,
        }
    }
    fn group(self) -> usize {
        match self {
            Self::Base64 | Self::Base64Url => 4,
            Self::Base32 | Self::Base32Hex => 8,
            _ => 0,
        }
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid input")
}
struct Wrapped<'a> {
    out: &'a mut dyn Write,
    bytes: Vec<u8>,
    width: usize,
    column: usize,
    total: usize,
}
impl Wrapped<'_> {
    fn byte(&mut self, b: u8) -> io::Result<()> {
        self.bytes.push(b);
        self.column += 1;
        self.total += 1;
        if self.width != 0 && self.column == self.width {
            self.bytes.push(b'\n');
            self.column = 0;
        }
        if self.bytes.len() >= 8192 {
            self.out.write_all(&self.bytes)?;
            self.bytes.clear();
        }
        Ok(())
    }
    fn finish(mut self) -> io::Result<()> {
        if self.width != 0 && self.column != 0 {
            self.bytes.push(b'\n');
        }
        self.out.write_all(&self.bytes)
    }
}
pub fn encode(
    input: &mut dyn Read,
    out: &mut dyn Write,
    e: Encoding,
    wrap: usize,
) -> io::Result<()> {
    let mut writer = Wrapped {
        out,
        bytes: Vec::with_capacity(8192),
        width: wrap,
        column: 0,
        total: 0,
    };
    if e == Encoding::Z85 {
        return encode85(input, writer);
    }
    let (mut acc, mut bits) = (0u32, 0u32);
    let width = e.bits();
    let mask = (1 << width) - 1;
    let mut buf = [0; 65536];
    loop {
        let n = read(input, &mut buf)?;
        if n == 0 {
            break;
        }
        for mut byte in buf[..n].iter().copied() {
            if e == Encoding::Base2Lsbf {
                byte = byte.reverse_bits();
            }
            acc = (acc << 8) | u32::from(byte);
            bits += 8;
            while bits >= width {
                bits -= width;
                writer.byte(e.alphabet()[((acc >> bits) & mask) as usize])?;
            }
        }
    }
    if bits != 0 {
        writer.byte(e.alphabet()[((acc << (width - bits)) & mask) as usize])?;
    }
    let group = e.group();
    if group != 0 {
        while !writer.total.is_multiple_of(group) {
            writer.byte(b'=')?;
        }
    }
    writer.finish()
}
fn encode85(input: &mut dyn Read, mut out: Wrapped<'_>) -> io::Result<()> {
    let mut chunk = [0; 4];
    let mut n = 0;
    let mut buf = [0; 65536];
    loop {
        let count = read(input, &mut buf)?;
        if count == 0 {
            break;
        }
        for byte in &buf[..count] {
            chunk[n] = *byte;
            n += 1;
            if n == 4 {
                let mut value = u32::from_be_bytes(chunk);
                let mut chars = [0; 5];
                for c in chars.iter_mut().rev() {
                    *c = Encoding::Z85.alphabet()[(value % 85) as usize];
                    value /= 85;
                }
                for c in chars {
                    out.byte(c)?;
                }
                n = 0;
            }
        }
    }
    if n != 0 {
        out.out.write_all(&out.bytes)?;
        return Err(invalid());
    }
    out.finish()
}

pub fn decode(
    input: &mut dyn Read,
    out: &mut dyn Write,
    e: Encoding,
    garbage: bool,
) -> io::Result<()> {
    if e == Encoding::Z85 {
        return decode85(input, out, garbage);
    }
    let (mut acc, mut bits, mut symbols, mut padding) = (0u32, 0u32, 0usize, 0usize);
    let mut stopped = false;
    let whole_groups = matches!(e, Encoding::Base32 | Encoding::Base32Hex);
    let mut pending = Vec::with_capacity(5);
    let mut decoded = Vec::with_capacity(65536);
    let mut buf = [0; 65536];
    loop {
        let count = read(input, &mut buf)?;
        if count == 0 {
            break;
        }
        for &byte in &buf[..count] {
            if byte == b'\n' {
                continue;
            }
            if byte == b'=' && e.group() == 0 {
                out.write_all(&decoded)?;
                return Err(invalid());
            }
            if byte == b'=' && e.group() != 0 {
                padding += 1;
                stopped = true;
                if whole_groups && valid_tail(e, symbols, padding, bits) {
                    decoded.append(&mut pending);
                }
                continue;
            }
            let symbol = e.alphabet().iter().position(|b| *b == byte);
            let Some(symbol) = symbol else {
                if garbage {
                    continue;
                }
                out.write_all(&decoded)?;
                return Err(invalid());
            };
            if stopped {
                // Padded groups may be concatenated; validate and restart.
                if !valid_tail(e, symbols, padding, bits) {
                    out.write_all(&decoded)?;
                    return Err(invalid());
                }
                stopped = false;
                acc = 0;
                bits = 0;
                symbols = 0;
                padding = 0;
            }
            acc = (acc << e.bits()) | symbol as u32;
            bits += e.bits();
            symbols += 1;
            if bits >= 8 {
                bits -= 8;
                let mut b = (acc >> bits) as u8;
                if e == Encoding::Base2Lsbf {
                    b = b.reverse_bits();
                }
                if whole_groups {
                    pending.push(b);
                } else {
                    decoded.push(b);
                }
            }
            if whole_groups && symbols % 8 == 0 {
                decoded.append(&mut pending);
            }
        }
        out.write_all(&decoded)?;
        decoded.clear();
    }
    out.write_all(&decoded)?;
    if valid_tail(e, symbols, padding, bits) {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn valid_tail(e: Encoding, symbols: usize, padding: usize, bits: u32) -> bool {
    let group = e.group();
    if group == 0 {
        return bits == 0 && padding == 0;
    }
    if symbols == 0 {
        return padding == 0;
    }
    let remainder = symbols % group;
    if padding == 0 {
        return remainder == 0;
    }
    let valid = match e {
        Encoding::Base64 | Encoding::Base64Url => matches!(remainder, 2 | 3),
        _ => matches!(remainder, 2 | 4 | 5 | 7),
    };
    valid && remainder + padding == group
}
fn decode85(input: &mut dyn Read, out: &mut dyn Write, garbage: bool) -> io::Result<()> {
    let (mut value, mut n) = (0u64, 0usize);
    let mut buf = [0; 65536];
    let mut decoded = Vec::with_capacity(65536);
    loop {
        let count = read(input, &mut buf)?;
        if count == 0 {
            break;
        }
        for &byte in &buf[..count] {
            if byte == b'\n' {
                continue;
            }
            let Some(d) = Encoding::Z85.alphabet().iter().position(|b| *b == byte) else {
                if garbage {
                    continue;
                }
                out.write_all(&decoded)?;
                return Err(invalid());
            };
            value = value * 85 + d as u64;
            n += 1;
            if n == 5 {
                if value > u32::MAX as u64 {
                    out.write_all(&decoded)?;
                    return Err(invalid());
                }
                decoded.extend_from_slice(&(value as u32).to_be_bytes());
                n = 0;
                value = 0;
            }
        }
        out.write_all(&decoded)?;
        decoded.clear();
    }
    if n != 0 {
        Err(invalid())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn enc(e: Encoding, s: &[u8], w: usize) -> Vec<u8> {
        let mut out = Vec::new();
        encode(&mut &s[..], &mut out, e, w).unwrap();
        out
    }
    fn dec(e: Encoding, s: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        decode(&mut &s[..], &mut out, e, false).unwrap();
        out
    }
    #[test]
    fn b64_empty() {
        assert!(enc(Encoding::Base64, b"", 76).is_empty());
    }
    #[test]
    fn b64_vectors() {
        assert_eq!(enc(Encoding::Base64, b"foobar", 0), b"Zm9vYmFy");
        assert_eq!(enc(Encoding::Base64, b"f", 0), b"Zg==");
    }
    #[test]
    fn b64_decode() {
        assert_eq!(dec(Encoding::Base64, b"Zm9v\nYmFy"), b"foobar");
    }
    #[test]
    fn b64_partial() {
        let mut out = Vec::new();
        assert!(decode(&mut &b"Zg="[..], &mut out, Encoding::Base64, false).is_err());
        assert_eq!(out, b"f");
    }
    #[test]
    fn wraps() {
        assert_eq!(enc(Encoding::Base64, b"foobar", 4), b"Zm9v\nYmFy\n");
    }
    #[test]
    fn b32_empty() {
        assert!(enc(Encoding::Base32, b"", 0).is_empty());
    }
    #[test]
    fn b32_vectors() {
        assert_eq!(enc(Encoding::Base32, b"foobar", 0), b"MZXW6YTBOI======");
    }
    #[test]
    fn b32_decode() {
        assert_eq!(dec(Encoding::Base32, b"MZXW6YTBOI======"), b"foobar");
    }
    #[test]
    fn b32_invalid_tail_emits_only_complete_groups() {
        for (input, expected) in [
            (b"MY".as_slice(), b"".as_slice()),
            (b"MZXQ=", b""),
            (b"MZXW6YTBMY", b"fooba"),
            (b"MY======MY", b"f"),
            (b"MY======?", b"f"),
        ] {
            let mut out = Vec::new();
            assert!(decode(&mut &input[..], &mut out, Encoding::Base32, false).is_err());
            assert_eq!(out, expected);
        }
    }
    #[test]
    fn b32_binary() {
        let b: Vec<_> = (0..=255).collect();
        assert_eq!(dec(Encoding::Base32, &enc(Encoding::Base32, &b, 0)), b);
    }
    #[test]
    fn b32_wrap() {
        assert_eq!(enc(Encoding::Base32, b"f", 4), b"MY==\n====\n");
    }
    #[test]
    fn all_roundtrips() {
        for name in Encoding::NAMES {
            let e = Encoding::named(name).unwrap();
            let b: Vec<_> = (0..=255).collect();
            assert_eq!(dec(e, &enc(e, &b, 0)), b, "{name}");
        }
    }
    #[test]
    fn hex() {
        assert_eq!(enc(Encoding::Base16, b"\xff\0", 0), b"FF00");
    }
    #[test]
    fn bits() {
        assert_eq!(enc(Encoding::Base2Msbf, b"\x81", 0), b"10000001");
        assert_eq!(enc(Encoding::Base2Lsbf, b"\x01", 0), b"10000000");
    }
    #[test]
    fn z85_vector() {
        assert_eq!(
            enc(
                Encoding::Z85,
                &[0x86, 0x4f, 0xd2, 0x6f, 0xb5, 0x59, 0xf7, 0x5b],
                0
            ),
            b"HelloWorld"
        );
    }
    #[test]
    fn garbage() {
        let mut out = Vec::new();
        decode(&mut &b"?Zg==!"[..], &mut out, Encoding::Base64, true).unwrap();
        assert_eq!(out, b"f");
    }
    #[test]
    fn z85_lengths() {
        let mut out = Vec::new();
        assert!(encode(&mut &b"x"[..], &mut out, Encoding::Z85, 0).is_err());
        assert!(decode(&mut &b"0"[..], &mut out, Encoding::Z85, false).is_err());
    }
    #[test]
    fn reports_reader_errors_separately_from_invalid_data() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            }
        }
        for e in [Encoding::Base64, Encoding::Z85] {
            let error = encode(&mut Broken, &mut Vec::new(), e, 76).unwrap_err();
            assert_eq!(
                read_error(&error).unwrap().kind(),
                io::ErrorKind::PermissionDenied
            );
            let error = decode(&mut Broken, &mut Vec::new(), e, false).unwrap_err();
            assert_eq!(
                read_error(&error).unwrap().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }
}
