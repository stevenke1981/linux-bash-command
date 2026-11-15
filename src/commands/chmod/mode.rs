//! Parsing of octal and symbolic permission modes.
pub fn parse(text: &str, initial: u32, is_dir: bool, umask: u32) -> Result<u32, String> {
    let operator = text
        .as_bytes()
        .first()
        .copied()
        .filter(|b| b"+-=".contains(b));
    let numeric = if operator.is_some() { &text[1..] } else { text };
    if !numeric.is_empty() && numeric.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        let value = u32::from_str_radix(numeric, 8)
            .ok()
            .filter(|n| *n <= 0o7777)
            .ok_or_else(|| format!("invalid mode: '{text}'"))?;
        let preserve = if is_dir && !text.starts_with('=') && numeric.len() < 5 {
            initial & 0o6000
        } else {
            0
        };
        return Ok(match operator {
            Some(b'+') => initial | value,
            Some(b'-') => initial & !value,
            _ => value | preserve,
        });
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
        let implicit = who == 0;
        if implicit {
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
            let rights = &bytes[start..i];
            if rights.iter().any(|b| b"ugo".contains(b))
                && (rights.len() != 1 || !b"ugo".contains(&rights[0]))
            {
                return Err(format!("invalid mode: '{text}'"));
            }
            permissions &= who;
            if implicit {
                permissions &= !(umask & 0o777);
            }
            match op {
                b'+' => bits |= permissions,
                b'-' => bits &= !permissions,
                b'=' => {
                    let clear = if implicit {
                        who & !(umask & 0o777)
                    } else {
                        who
                    };
                    let clear = if is_dir && !bytes[start..i].contains(&b's') {
                        clear & !0o6000
                    } else {
                        clear
                    };
                    bits = (bits & !clear) | permissions;
                }
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
        assert_eq!(parse("0750", 0, true, 0).unwrap(), 0o750);
    }
    #[test]
    fn symbolic() {
        assert_eq!(parse("u=rwx,g=rx,o=", 0, true, 0).unwrap(), 0o750);
    }
    #[test]
    fn copy_class() {
        assert_eq!(parse("g=u", 0o700, true, 0).unwrap(), 0o770);
    }
    #[test]
    fn conditional_execute() {
        assert_eq!(parse("a+X", 0o600, false, 0).unwrap(), 0o600);
        assert_eq!(parse("a+X", 0o600, true, 0).unwrap(), 0o711);
    }
    #[test]
    fn directories_preserve_special_bits_unless_explicitly_cleared() {
        for text in ["755", "0755", "u=rwx,g=rx,o=rx"] {
            assert_eq!(parse(text, 0o6755, true, 0).unwrap(), 0o6755);
        }
        for text in ["00755", "=755", "u-s,g-s"] {
            assert_eq!(parse(text, 0o6755, true, 0).unwrap(), 0o755);
        }
        assert_eq!(parse("755", 0o6755, false, 0).unwrap(), 0o755);
    }
    #[test]
    fn empty_actions_and_numeric_add_remove_are_supported() {
        for text in ["a+", "u-", "u++w", "-"] {
            assert_eq!(parse(text, 0o644, false, 0).unwrap(), 0o644);
        }
        assert_eq!(parse("+0111", 0o640, false, 0).unwrap(), 0o751);
        assert_eq!(parse("-0111", 0o751, false, 0).unwrap(), 0o640);
    }
    #[test]
    fn malformed() {
        for s in ["", "999", "u", "u+z", "u+ug", "u+rwu"] {
            assert!(parse(s, 0, true, 0).is_err(), "{s}");
        }
    }
}
