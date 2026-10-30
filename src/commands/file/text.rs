//! Text classification: character encoding, byte order marks and line terminators.

use super::Verdict;

/// The encoding of a sample that is text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoding {
    /// Description such as `ASCII text` or `Unicode text, UTF-8 text`.
    pub base: &'static str,
    /// MIME charset parameter.
    pub charset: &'static str,
    /// True for UTF-16, whose line terminators are not inspected byte by byte.
    pub wide: bool,
}

/// Returns the encoding of `sample` when it is text, or `None` for binary data.
pub fn encoding(sample: &[u8]) -> Option<Encoding> {
    if let Some(body) = sample.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return utf8(body).then_some(Encoding {
            base: "Unicode text, UTF-8 (with BOM) text",
            charset: "utf-8",
            wide: false,
        });
    }
    if sample.starts_with(&[0xff, 0xfe]) || sample.starts_with(&[0xfe, 0xff]) {
        let little = sample[0] == 0xff;
        return utf16_text(&sample[2..], little).then_some(if little {
            Encoding {
                base: "Unicode text, UTF-16, little-endian text",
                charset: "utf-16le",
                wide: true,
            }
        } else {
            Encoding {
                base: "Unicode text, UTF-16, big-endian text",
                charset: "utf-16be",
                wide: true,
            }
        });
    }
    if sample.contains(&0) || sample.iter().any(|&b| is_control(b)) {
        return None;
    }
    if sample.is_ascii() {
        return Some(Encoding {
            base: "ASCII text",
            charset: "us-ascii",
            wide: false,
        });
    }
    utf8(sample).then_some(Encoding {
        base: "Unicode text, UTF-8 text",
        charset: "utf-8",
        wide: false,
    })
}

/// Control bytes that may appear in text: tab, line feed, vertical tab, form feed, carriage
/// return, backspace and escape.
fn is_control(byte: u8) -> bool {
    byte < 0x20 && !matches!(byte, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | 0x08 | 0x1b)
}

/// True when `bytes` is UTF-8, allowing an incomplete character at the very end (the
/// sample is cut at a fixed size).
fn utf8(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(text) => !text.chars().any(|c| {
            c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\x0c' | '\x08' | '\x1b' | '\x0b')
        }),
        Err(error) => error.error_len().is_none() && error.valid_up_to() + 4 >= bytes.len(),
    }
}

fn utf16_text(bytes: &[u8], little: bool) -> bool {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    char::decode_utf16(units).all(|unit| match unit {
        Ok(c) => {
            !c.is_control() || matches!(c, '\t' | '\n' | '\r' | '\x0c' | '\x08' | '\x1b' | '\x0b')
        }
        Err(_) => false,
    })
}

/// Line terminator and length remarks appended to a text description. Each starts with a
/// comma, or is empty when the text has CRLF or LF endings only and no long lines.
pub fn suffix(sample: &[u8]) -> String {
    let lines: Vec<&[u8]> = sample.split(|&b| b == b'\n').collect();
    let has_lf = sample.contains(&b'\n');
    let crlf = lines
        .iter()
        .take(lines.len().saturating_sub(1))
        .filter(|line| line.ends_with(b"\r"))
        .count();
    let bare = lines.len().saturating_sub(1).saturating_sub(crlf);
    let longest = lines.iter().map(|line| line.len()).max().unwrap_or(0);
    let mut out = String::new();
    if !has_lf {
        out.push_str(if longest > LONG_LINE {
            ", with very long lines"
        } else {
            ", with no line terminators"
        });
        return out;
    }
    if crlf > 0 && bare == 0 {
        out.push_str(", with CRLF line terminators");
    } else if crlf > 0 {
        out.push_str(", with CRLF, LF line terminators");
    }
    if longest > LONG_LINE {
        out.push_str(", with very long lines");
    }
    out
}

/// Lines longer than this are reported as "very long".
const LONG_LINE: usize = 4096;

/// Describes `sample` as text, or `None` when it is binary.
pub fn describe(sample: &[u8]) -> Option<Verdict> {
    let encoding = encoding(sample)?;
    let remarks = if encoding.wide {
        String::new()
    } else {
        suffix(sample)
    };
    Some(Verdict {
        description: format!("{}{remarks}", encoding.base),
        mime: "text/plain".to_string(),
        charset: encoding.charset.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_are_recognised() {
        assert_eq!(encoding(b"abc").unwrap().charset, "us-ascii");
        assert_eq!(encoding("héllo".as_bytes()).unwrap().charset, "utf-8");
        assert_eq!(
            encoding(b"\xef\xbb\xbfhi").unwrap().base,
            "Unicode text, UTF-8 (with BOM) text"
        );
        assert_eq!(encoding(b"\xff\xfeh\0i\0").unwrap().charset, "utf-16le");
        assert_eq!(encoding(b"\xfe\xff\0h\0i").unwrap().charset, "utf-16be");
        assert!(encoding(b"a\x00b").is_none());
        assert!(encoding(b"\x01\x02").is_none());
        assert!(encoding(b"\xff\xfe\x01").is_some(), "odd tail is ignored");
    }

    #[test]
    fn line_terminator_remarks() {
        assert_eq!(suffix(b"no newline"), ", with no line terminators");
        assert_eq!(suffix(b"a\r\nb\r\n"), ", with CRLF line terminators");
        assert_eq!(suffix(b"a\r\nb\n"), ", with CRLF, LF line terminators");
        assert_eq!(suffix(b"a\nb\n"), "");
        let long = vec![b'x'; LONG_LINE + 1];
        assert_eq!(suffix(&long), ", with very long lines");
    }

    #[test]
    fn describe_builds_full_text_descriptions() {
        let verdict = describe(b"a\r\nb\r\n").unwrap();
        assert_eq!(
            verdict.description,
            "ASCII text, with CRLF line terminators"
        );
        assert_eq!(verdict.mime, "text/plain");
        assert!(describe(b"\x00").is_none());
    }
}
