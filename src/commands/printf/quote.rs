//! `%q`: quotes an argument so a shell can read it back, following GNU `printf`.
//!
//! GNU differs from bash `printf %q`: a leading `-` stays bare, `a b'c` becomes
//! `"a b'c"`, and bytes outside printable ASCII use `$'...'` with octal escapes.
//! This is not `common::escape::shell_quote`, which follows bash.

/// Returns the quoted form of `arg` as bytes.
pub fn quote(arg: &str) -> Vec<u8> {
    let bytes = arg.as_bytes();
    if bytes.is_empty() {
        return b"''".to_vec();
    }
    if bytes
        .iter()
        .enumerate()
        .all(|(index, &b)| is_plain(b, index))
    {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(bytes.len() * 2);
    if !is_printable(bytes[0]) {
        out.extend_from_slice(b"''");
    }
    let mut index = 0;
    while index < bytes.len() {
        let printable = is_printable(bytes[index]);
        let start = index;
        while index < bytes.len() && is_printable(bytes[index]) == printable {
            index += 1;
        }
        let run = &bytes[start..index];
        if printable {
            push_single_quoted(&mut out, run);
        } else {
            push_ansi_c(&mut out, run);
        }
    }
    out
}

/// Bytes GNU leaves unquoted: letters, digits and `% + , - . / : @ _ { }`. `#` and `~` are
/// bare only after the first byte.
fn is_plain(byte: u8, index: usize) -> bool {
    if byte.is_ascii_alphanumeric() || b"%+,-./:@_{}".contains(&byte) {
        return true;
    }
    index > 0 && (byte == b'#' || byte == b'~')
}

fn is_printable(byte: u8) -> bool {
    (0x20..0x7f).contains(&byte)
}

/// Single quotes, or double quotes when the text has a single quote but no double quote,
/// or `'...'\''...'` when it has both.
fn push_single_quoted(out: &mut Vec<u8>, run: &[u8]) {
    let has_single = run.contains(&b'\'');
    let has_double = run.contains(&b'"');
    if has_single && !has_double {
        out.push(b'"');
        for &b in run {
            if matches!(b, b'\\' | b'"' | b'$' | b'`') {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b'"');
    } else if has_single {
        out.push(b'\'');
        for &b in run {
            if b == b'\'' {
                out.extend_from_slice(b"'\\''");
            } else {
                out.push(b);
            }
        }
        out.push(b'\'');
    } else {
        out.push(b'\'');
        out.extend_from_slice(run);
        out.push(b'\'');
    }
}

/// `$'...'` with C escapes for the named control characters and octal for the rest.
fn push_ansi_c(out: &mut Vec<u8>, run: &[u8]) {
    out.extend_from_slice(b"$'");
    for &b in run {
        match b {
            0x07 => out.extend_from_slice(b"\\a"),
            0x08 => out.extend_from_slice(b"\\b"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            0x0b => out.extend_from_slice(b"\\v"),
            0x0c => out.extend_from_slice(b"\\f"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\'' => out.extend_from_slice(b"\\'"),
            _ if is_printable(b) => out.push(b),
            _ => {
                out.push(b'\\');
                out.extend_from_slice(format!("{b:03o}").as_bytes());
            }
        }
    }
    out.push(b'\'');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> String {
        String::from_utf8(quote(text)).unwrap()
    }

    #[test]
    fn plain_text_is_left_bare() {
        assert_eq!(q("abc-1.2"), "abc-1.2");
        assert_eq!(q("-x"), "-x");
        assert_eq!(q("a#"), "a#");
        assert_eq!(q("{a,b}"), "{a,b}");
    }

    #[test]
    fn empty_and_special_characters_are_quoted() {
        assert_eq!(q(""), "''");
        assert_eq!(q("a b"), "'a b'");
        assert_eq!(q("~a"), "'~a'");
        assert_eq!(q("#b"), "'#b'");
        assert_eq!(q("a=b"), "'a=b'");
    }

    #[test]
    fn single_quote_switches_quote_style() {
        assert_eq!(q("'"), "\"'\"");
        assert_eq!(q("a\"b"), "'a\"b'");
        assert_eq!(q("a'b\"c"), "'a'\\''b\"c'");
    }

    #[test]
    fn control_and_non_ascii_bytes_use_ansi_c() {
        assert_eq!(q("\u{1}"), "''$'\\001'");
        assert_eq!(q("a\tb"), "'a'$'\\t''b'");
        assert_eq!(q("\u{e9}"), "''$'\\303\\251'");
        assert_eq!(q("a\\b"), "'a\\b'");
    }
}
