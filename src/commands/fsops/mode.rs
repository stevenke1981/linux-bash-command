//! Parsing of octal and symbolic permission modes.
pub fn parse(text: &str, initial: u32, is_dir: bool) -> Result<u32, String> {
    if !text.is_empty() && text.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return u32::from_str_radix(text, 8)
            .ok()
            .filter(|n| *n <= 0o7777)
            .ok_or_else(|| format!("invalid mode: '{text}'"));
    }
    let mut bits = initial;
    for clause in text.split(',') {
        let bytes = clause.as_bytes();
        let mut i = 0;
        let mut who = 0u32;
        while i < bytes.len() {
            who |= match bytes[i] {
                b'u' => 0o4700,
                b'g' => 0o2070,
                b'o' => 0o1007,
                b'a' => 0o7777,
                _ => break,
            };
            i += 1;
        }
        if who == 0 {
            who = 0o7777;
        }
        let mut had_op = false;
        while i < bytes.len() {
            let op = bytes[i];
            if !matches!(op, b'+' | b'-' | b'=') {
                return Err(format!("invalid mode: '{text}'"));
            }
            had_op = true;
            i += 1;
            let start = i;
            let mut permissions = 0u32;
            while i < bytes.len() && !matches!(bytes[i], b'+' | b'-' | b'=') {
                permissions |= match bytes[i] {
                    b'r' => 0o444,
                    b'w' => 0o222,
                    b'x' => 0o111,
                    b'X' if is_dir || bits & 0o111 != 0 => 0o111,
                    b'X' => 0,
                    b's' => 0o6000,
                    b't' => 0o1000,
                    b'u' => {
                        let v = (bits >> 6) & 7;
                        v * 0o111
                    }
                    b'g' => {
                        let v = (bits >> 3) & 7;
                        v * 0o111
                    }
                    b'o' => {
                        let v = bits & 7;
                        v * 0o111
                    }
                    _ => return Err(format!("invalid mode: '{text}'")),
                };
                i += 1;
            }
            if i == start && op != b'=' {
                return Err(format!("invalid mode: '{text}'"));
            }
            permissions &= who;
            match op {
                b'+' => bits |= permissions,
                b'-' => bits &= !permissions,
                b'=' => bits = (bits & !who) | permissions,
                _ => unreachable!(),
            }
        }
        if !had_op {
            return Err(format!("invalid mode: '{text}'"));
        }
    }
    Ok(bits)
}

#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn octal() {
        assert_eq!(parse("0750", 0, true).unwrap(), 0o750);
    }
    #[test]
    fn symbolic() {
        assert_eq!(parse("u=rwx,g=rx,o=", 0, true).unwrap(), 0o750);
    }
    #[test]
    fn copy_class() {
        assert_eq!(parse("g=u", 0o700, true).unwrap(), 0o770);
    }
    #[test]
    fn conditional_execute() {
        assert_eq!(parse("a+X", 0o600, false).unwrap(), 0o600);
        assert_eq!(parse("a+X", 0o600, true).unwrap(), 0o711);
    }
    #[test]
    fn malformed() {
        for s in ["", "999", "u", "u+z", "a+"] {
            assert!(parse(s, 0, true).is_err(), "{s}");
        }
    }
}
