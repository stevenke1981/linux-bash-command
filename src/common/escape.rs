#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Echo,
    Printf,
    String,
}

pub fn decode(input: &[u8], mode: Mode) -> Result<(Vec<u8>, bool), String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let byte = input[i];
        i += 1;
        if byte != b'\\' || i == input.len() {
            out.push(byte);
            continue;
        }
        let escape = input[i];
        i += 1;
        match escape {
            b'c' => return Ok((out, true)),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'e' => out.push(27),
            b'f' => out.push(12),
            b'n' => out.push(10),
            b'r' => out.push(13),
            b't' => out.push(9),
            b'v' => out.push(11),
            b'\\' => out.push(b'\\'),
            b'x' | b'u' | b'U' if escape == b'x' || mode == Mode::Printf => {
                let limit = match escape {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let start = i;
                let mut value = 0u32;
                while i < input.len() && i - start < limit {
                    let Some(digit) = char::from(input[i]).to_digit(16) else {
                        break;
                    };
                    value = value * 16 + digit;
                    i += 1;
                }
                if start == i {
                    if mode == Mode::Echo {
                        out.extend_from_slice(&[b'\\', escape]);
                    } else {
                        return Err("missing hexadecimal number in escape".into());
                    }
                } else if escape == b'x' {
                    out.push(value as u8);
                } else {
                    let ch = char::from_u32(value).ok_or("invalid universal character name")?;
                    if ch.is_ascii() {
                        out.push(value as u8);
                    } else if value <= 0xffff {
                        out.extend_from_slice(format!("\\u{value:04X}").as_bytes());
                    } else {
                        out.extend_from_slice(format!("\\U{value:08X}").as_bytes());
                    }
                }
            }
            b'0'..=b'7' if escape == b'0' || mode == Mode::Printf => {
                let prefixed = escape == b'0' && mode != Mode::Printf;
                let mut value = if prefixed {
                    0
                } else {
                    u16::from(escape - b'0')
                };
                let mut digits = usize::from(!prefixed);
                while i < input.len() && digits < 3 && (b'0'..=b'7').contains(&input[i]) {
                    value = value * 8 + u16::from(input[i] - b'0');
                    i += 1;
                    digits += 1;
                }
                out.push(value as u8);
            }
            _ => out.extend_from_slice(&[b'\\', escape]),
        }
    }
    Ok((out, false))
}

pub fn quote_name(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return b"''".to_vec();
    }
    if bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(b))
    {
        return bytes.to_vec();
    }
    if bytes.iter().any(|b| !b.is_ascii_graphic() && *b != b' ') {
        let mut out = b"'".to_vec();
        let mut escaped = false;
        for &b in bytes {
            let control = !(32..=126).contains(&b);
            if control != escaped {
                out.extend_from_slice(if control { b"'$'" } else { b"''" });
                escaped = control;
            }
            if control {
                match b {
                    7 => out.extend_from_slice(b"\\a"),
                    8 => out.extend_from_slice(b"\\b"),
                    9 => out.extend_from_slice(b"\\t"),
                    10 => out.extend_from_slice(b"\\n"),
                    11 => out.extend_from_slice(b"\\v"),
                    12 => out.extend_from_slice(b"\\f"),
                    13 => out.extend_from_slice(b"\\r"),
                    _ => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
                }
            } else if b == b'\'' {
                out.extend_from_slice(b"'\\''");
            } else {
                out.push(b);
            }
        }
        out.push(b'\'');
        return out;
    }
    if !bytes.contains(&b'\'') || bytes.iter().any(|b| b"$`\\\"".contains(b)) {
        let mut out = vec![b'\''];
        for &b in bytes {
            if b == b'\'' {
                out.extend_from_slice(b"'\\''");
            } else {
                out.push(b);
            }
        }
        out.push(b'\'');
        return out;
    }
    let mut out = vec![b'"'];
    for b in bytes {
        if b"$`\\\"".contains(b) {
            out.push(b'\\');
        }
        out.push(*b);
    }
    out.push(b'"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls() {
        assert_eq!(
            decode(b"\\a\\b\\e\\f\\n\\r\\t\\v", Mode::Echo).unwrap().0,
            [7, 8, 27, 12, 10, 13, 9, 11]
        );
    }
    #[test]
    fn octal() {
        assert_eq!(decode(b"\\0101", Mode::Echo).unwrap().0, b"A");
    }
    #[test]
    fn printf_octal() {
        assert_eq!(decode(b"\\101", Mode::Printf).unwrap().0, b"A");
    }
    #[test]
    fn stop() {
        assert_eq!(
            decode(b"x\\cy", Mode::String).unwrap(),
            (b"x".to_vec(), true)
        );
    }
    #[test]
    fn unicode() {
        assert_eq!(decode(b"\\u03bb", Mode::Printf).unwrap().0, b"\\u03BB");
        assert_eq!(decode(b"\\u0043", Mode::Printf).unwrap().0, b"C");
    }
    #[test]
    fn quoting() {
        assert_eq!(quote_name(b"a b"), b"'a b'");
        assert_eq!(quote_name(b""), b"''");
        assert_eq!(quote_name(b"a'b"), b"\"a'b\"");
        assert_eq!(quote_name(b"a'b$c"), b"'a'\\''b$c'");
        assert_eq!(quote_name(b"a'b\"c"), b"'a'\\''b\"c'");
        assert_eq!(quote_name(b"a\nb"), b"'a'$'\\n''b'");
    }
}
