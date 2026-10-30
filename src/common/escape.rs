//! Backslash escapes (`echo -e`, `printf` formats) and shell-quoting of names
//! (`ls --quoting-style=shell`, `printf %q`).

/// Which escape dialect [`unescape`] applies.
///
/// - `Echo`: `echo -e` (`\0NNN`, `\xHH`, `\c` stops).
/// - `Printf`: `printf` format strings (`\NNN`, `\xHH`, `\uHHHH`, `\UHHHHHHHH`, `\"`, `\c`).
///
/// Both accept `\\ \a \b \e \f \n \r \t \v`. Unknown escapes keep their backslash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeMode {
    Echo,
    Printf,
}

/// Decodes backslash escapes. Returns the bytes and whether a `\c` was seen, which stops
/// output (the caller also drops any trailing newline in `echo`).
///
/// A `\u` or `\U` value that is not a Unicode scalar value is kept as written.
pub fn unescape(s: &str, mode: EscapeMode) -> (Vec<u8>, bool) {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' || index + 1 == bytes.len() {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        let code = bytes[index + 1];
        index += 2;
        if let Some(byte) = simple_escape(code, mode) {
            out.push(byte);
            continue;
        }
        match (code, mode) {
            (b'c', _) => return (out, true),
            (b'0', EscapeMode::Echo) => {
                let (value, used) = digits(&bytes[index..], 8, 3);
                index += used;
                out.push(value as u8);
            }
            (b'0'..=b'7', EscapeMode::Printf) => {
                let (value, used) = digits(&bytes[index - 1..], 8, 3);
                index += used - 1;
                out.push(value as u8);
            }
            (b'x', _) => {
                let (value, used) = digits(&bytes[index..], 16, 2);
                if used == 0 {
                    push_literal(&mut out, code);
                } else {
                    index += used;
                    out.push(value as u8);
                }
            }
            (b'u' | b'U', EscapeMode::Printf) => {
                let max = if code == b'u' { 4 } else { 8 };
                let (value, used) = digits(&bytes[index..], 16, max);
                match (used, char::from_u32(value)) {
                    (0, _) | (_, None) => push_literal(&mut out, code),
                    (_, Some(c)) => {
                        index += used;
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
            }
            _ => push_literal(&mut out, code),
        }
    }
    (out, false)
}

fn simple_escape(code: u8, mode: EscapeMode) -> Option<u8> {
    Some(match code {
        b'\\' => b'\\',
        b'a' => 0x07,
        b'b' => 0x08,
        b'e' => 0x1b,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0b,
        b'"' if mode == EscapeMode::Printf => b'"',
        _ => return None,
    })
}

/// Pushes a backslash and the escape letter unchanged.
fn push_literal(out: &mut Vec<u8>, code: u8) {
    out.push(b'\\');
    out.push(code);
}

/// Reads up to `max` digits in `radix`. Returns the value and how many bytes were used.
fn digits(bytes: &[u8], radix: u32, max: usize) -> (u32, usize) {
    let mut value = 0u32;
    let mut used = 0;
    for &byte in bytes.iter().take(max) {
        let Some(digit) = (byte as char).to_digit(radix) else {
            break;
        };
        value = value * radix + digit;
        used += 1;
    }
    (value, used)
}

/// Quotes a file name the way GNU `ls --quoting-style=shell` does.
///
/// Names made only of ASCII letters, digits and `% + , - . / : @ _ { }` (with `~` and `#`
/// allowed after the first character, and a leading `-` quoted) and any non-ASCII text are
/// returned unchanged. Otherwise the name is
/// wrapped in single quotes; a name containing a single quote is wrapped in double quotes
/// instead, or, if it also contains a double quote, in single quotes with each `'`
/// written as `'\''`. An empty name becomes `''`.
pub fn quote_name(name: &str) -> String {
    if name.is_empty() {
        return "''".to_owned();
    }
    if is_plain_name(name) {
        return name.to_owned();
    }
    if !name.contains('\'') {
        return format!("'{name}'");
    }
    if !name.contains('"') {
        let mut out = String::with_capacity(name.len() + 2);
        out.push('"');
        for c in name.chars() {
            if matches!(c, '\\' | '"' | '$' | '`') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        return out;
    }
    format!("'{}'", name.replace('\'', r"'\''"))
}

fn is_plain_name(name: &str) -> bool {
    name.char_indices().all(|(index, c)| {
        if !c.is_ascii() {
            return true;
        }
        let first = index == 0;
        if c.is_ascii_alphanumeric() || "%+,-.:/@_{}".contains(c) {
            return !(first && c == '-');
        }
        !first && matches!(c, '~' | '#')
    })
}

/// Quotes a string for reuse as shell input, following bash `printf %q`.
///
/// Letters, digits and `% + , - . / : = @ _` pass through, `~` and `#` only after the first
/// character, and every other ASCII character is backslash-escaped. Strings containing
/// control or non-ASCII bytes use `$'...'` with octal escapes for the bytes. An empty
/// string becomes `''`.
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_owned();
    }
    if s.bytes().any(|b| !(0x20..0x7f).contains(&b)) {
        return ansi_c_quote(s.as_bytes());
    }
    let mut out = String::with_capacity(s.len() * 2);
    for (index, c) in s.char_indices() {
        let safe = c.is_ascii_alphanumeric()
            || "%+,-./:=@_".contains(c)
            || (index > 0 && matches!(c, '~' | '#'));
        if !safe {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn ansi_c_quote(bytes: &[u8]) -> String {
    let mut out = String::from("$'");
    for &byte in bytes {
        match byte {
            0x07 => out.push_str("\\a"),
            0x08 => out.push_str("\\b"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            0x0b => out.push_str("\\v"),
            0x0c => out.push_str("\\f"),
            b'\r' => out.push_str("\\r"),
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\{byte:03o}")),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo(text: &str) -> Vec<u8> {
        unescape(text, EscapeMode::Echo).0
    }

    fn printf(text: &str) -> Vec<u8> {
        unescape(text, EscapeMode::Printf).0
    }

    #[test]
    fn common_escapes_work_in_both_modes() {
        let text = r"\\\a\b\e\f\n\r\t\v";
        let expected = b"\\\x07\x08\x1b\x0c\n\r\t\x0b";
        assert_eq!(echo(text), expected);
        assert_eq!(printf(text), expected);
    }

    #[test]
    fn echo_octal_needs_leading_zero() {
        assert_eq!(echo(r"\0101"), b"A");
        assert_eq!(echo(r"\0"), b"\0");
        assert_eq!(echo(r"\01x"), b"\x01x");
        assert_eq!(echo(r"\101"), b"\\101");
    }

    #[test]
    fn printf_octal_takes_up_to_three_digits() {
        assert_eq!(printf(r"\101"), b"A");
        assert_eq!(printf(r"\0101"), b"\x081");
        assert_eq!(printf(r"\0"), b"\0");
        assert_eq!(printf(r"\7x"), b"\x07x");
    }

    #[test]
    fn hex_takes_up_to_two_digits() {
        assert_eq!(echo(r"\x41\x4g"), b"A\x04g");
        assert_eq!(printf(r"\x41"), b"A");
        assert_eq!(printf(r"\x4"), b"\x04");
        assert_eq!(printf(r"\xg"), b"\\xg");
    }

    #[test]
    fn unicode_escapes_are_printf_only() {
        assert_eq!(printf(r"\u00e9"), "é".as_bytes());
        assert_eq!(printf(r"\U0001F600"), "😀".as_bytes());
        assert_eq!(printf(r"\u4"), b"\x04");
        assert_eq!(echo(r"\u00e9"), b"\\u00e9");
    }

    #[test]
    fn invalid_unicode_scalar_is_kept_as_written() {
        assert_eq!(printf(r"\uD800"), b"\\uD800");
        assert_eq!(printf(r"\U00110000"), b"\\U00110000");
    }

    #[test]
    fn printf_accepts_escaped_double_quote_only() {
        assert_eq!(printf(r#"\""#), b"\"");
        assert_eq!(echo(r#"\""#), b"\\\"");
    }

    #[test]
    fn backslash_c_stops_output() {
        assert_eq!(
            unescape(r"abc\cdef", EscapeMode::Echo),
            (b"abc".to_vec(), true)
        );
        assert_eq!(
            unescape(r"x\n\cy", EscapeMode::Printf),
            (b"x\n".to_vec(), true)
        );
        assert_eq!(
            unescape("plain", EscapeMode::Echo),
            (b"plain".to_vec(), false)
        );
    }

    #[test]
    fn unknown_escapes_and_trailing_backslash_are_kept() {
        assert_eq!(echo(r"\q"), b"\\q");
        assert_eq!(printf(r"\q"), b"\\q");
        assert_eq!(echo("end\\"), b"end\\");
    }

    #[test]
    fn plain_names_are_not_quoted() {
        assert_eq!(quote_name("file.txt"), "file.txt");
        assert_eq!(quote_name("a+b,c%d@e:f_g"), "a+b,c%d@e:f_g");
        assert_eq!(quote_name("a#b~c{d}e"), "a#b~c{d}e");
        assert_eq!(quote_name("café"), "café");
    }

    #[test]
    fn names_with_special_characters_use_single_quotes() {
        assert_eq!(quote_name("a b"), "'a b'");
        assert_eq!(quote_name("a$b"), "'a$b'");
        assert_eq!(quote_name("x\"y"), "'x\"y'");
        assert_eq!(quote_name("a\\b"), "'a\\b'");
        assert_eq!(quote_name("a\tb"), "'a\tb'");
    }

    #[test]
    fn leading_special_characters_are_quoted() {
        assert_eq!(quote_name("#hash"), "'#hash'");
        assert_eq!(quote_name("~tilde"), "'~tilde'");
        assert_eq!(quote_name("=eq"), "'=eq'");
        assert_eq!(quote_name("a=b"), "'a=b'");
        assert_eq!(quote_name("-dash"), "'-dash'");
    }

    #[test]
    fn names_with_single_quote_use_double_quotes() {
        assert_eq!(quote_name("it's"), "\"it's\"");
        assert_eq!(quote_name("it's $HOME"), "\"it's \\$HOME\"");
    }

    #[test]
    fn names_with_both_quote_kinds_use_escaped_single_quotes() {
        assert_eq!(quote_name("it's \"x\""), "'it'\\''s \"x\"'");
    }

    #[test]
    fn empty_name_is_two_quotes() {
        assert_eq!(quote_name(""), "''");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn shell_quote_escapes_with_backslashes() {
        assert_eq!(shell_quote("a b"), "a\\ b");
        assert_eq!(shell_quote("it's"), "it\\'s");
        assert_eq!(shell_quote("a$b"), "a\\$b");
        assert_eq!(shell_quote("a=b"), "a=b");
        assert_eq!(shell_quote("a~b"), "a~b");
        assert_eq!(shell_quote("~a"), "\\~a");
        assert_eq!(shell_quote("#a"), "\\#a");
        assert_eq!(shell_quote("a{b"), "a\\{b");
    }

    #[test]
    fn shell_quote_uses_ansi_c_for_control_and_non_ascii() {
        assert_eq!(shell_quote("a\tb"), "$'a\\tb'");
        assert_eq!(shell_quote("a\nb"), "$'a\\nb'");
        assert_eq!(shell_quote("é"), "$'\\303\\251'");
        assert_eq!(shell_quote("\x7f"), "$'\\177'");
        assert_eq!(shell_quote("a\tb'\\"), "$'a\\tb\\'\\\\'");
    }

    #[test]
    fn shell_quote_backslashes_plain_ascii_quotes() {
        assert_eq!(shell_quote("it's\\"), "it\\'s\\\\");
    }

    #[test]
    fn shell_quote_escapes_every_metacharacter() {
        let text = "a b$c'd\"e\\f";
        assert_eq!(shell_quote(text), "a\\ b\\$c\\'d\\\"e\\\\f");
    }
}
