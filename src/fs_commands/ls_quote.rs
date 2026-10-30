//! `--quoting-style` for `ls` names, following GNU `ls` in the C locale: bytes outside
//! printable ASCII are non-printable and are written as C escapes or octal.

use super::ls_options::Quoting;
use crate::common::escape::quote_name;

/// Quotes `name` in `style`. [`Quoting::Unset`] is treated as literal.
pub fn quote(name: &str, style: Quoting) -> String {
    match style {
        Quoting::Literal | Quoting::Unset => name.to_string(),
        Quoting::Shell => quote_name(name),
        Quoting::ShellAlways => shell_always(name),
        Quoting::ShellEscape => shell_escape(name, false),
        Quoting::ShellEscapeAlways => shell_escape(name, true),
        Quoting::C => c_quoted(name),
        Quoting::Escape => escaped(name),
    }
}

fn is_printable(byte: u8) -> bool {
    (0x20..0x7f).contains(&byte)
}

/// The C escape for one byte, without quotes: `\n`, `\t`, `\\`, or octal for the rest of the
/// non-printable bytes. `"` is returned as it is; callers decide whether to escape it.
fn c_escape(byte: u8) -> String {
    match byte {
        0x07 => "\\a".to_string(),
        0x08 => "\\b".to_string(),
        0x09 => "\\t".to_string(),
        0x0a => "\\n".to_string(),
        0x0b => "\\v".to_string(),
        0x0c => "\\f".to_string(),
        0x0d => "\\r".to_string(),
        b'\\' => "\\\\".to_string(),
        _ if is_printable(byte) => (byte as char).to_string(),
        _ => format!("\\{byte:03o}"),
    }
}

/// `--quoting-style=escape`: C escapes without surrounding quotes.
fn escaped(name: &str) -> String {
    name.bytes().map(c_escape).collect()
}

/// `--quoting-style=c` (`-Q`): C escapes inside double quotes.
fn c_quoted(name: &str) -> String {
    let body: String = name
        .bytes()
        .map(|byte| {
            if byte == b'"' {
                "\\\"".to_string()
            } else {
                c_escape(byte)
            }
        })
        .collect();
    format!("\"{body}\"")
}

/// `--quoting-style=shell-always`: always quoted, with `"..."` when the name holds `'`.
fn shell_always(name: &str) -> String {
    if name.contains('\'') {
        quote_name(name)
    } else {
        format!("'{name}'")
    }
}

/// `--quoting-style=shell-escape`: shell quoting, with control and non-ASCII bytes written as
/// `$'...'` segments. Names without such bytes are quoted as `shell` quotes them unless
/// `always` is set.
fn shell_escape(name: &str, always: bool) -> String {
    if name.bytes().all(is_printable) {
        return if always {
            shell_always(name)
        } else {
            quote_name(name)
        };
    }
    let mut out = String::new();
    let mut in_single = false;
    for byte in name.bytes() {
        if is_printable(byte) {
            if !in_single {
                out.push('\'');
                in_single = true;
            }
            match byte {
                b'\'' => out.push_str("'\\''"),
                _ => out.push(byte as char),
            }
        } else {
            if in_single {
                out.push('\'');
                in_single = false;
            }
            out.push_str("$'");
            out.push_str(&c_escape(byte));
            out.push('\'');
        }
    }
    if in_single {
        out.push('\'');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_style_escapes_and_quotes() {
        assert_eq!(quote("tab\there", Quoting::C), "\"tab\\there\"");
        assert_eq!(quote("q\"d", Quoting::C), "\"q\\\"d\"");
        assert_eq!(quote("\u{7f}", Quoting::C), "\"\\177\"");
        assert_eq!(quote("é", Quoting::C), "\"\\303\\251\"");
    }

    #[test]
    fn escape_style_has_no_quotes() {
        assert_eq!(quote("nl\nx", Quoting::Escape), "nl\\nx");
        assert_eq!(quote("q\"d", Quoting::Escape), "q\"d");
        assert_eq!(quote("back\\slash", Quoting::Escape), "back\\\\slash");
    }

    #[test]
    fn shell_styles_match_gnu_quoting() {
        assert_eq!(quote("a.txt", Quoting::Shell), "a.txt");
        assert_eq!(quote("sp ace", Quoting::Shell), "'sp ace'");
        assert_eq!(quote("it's", Quoting::Shell), "\"it's\"");
        assert_eq!(quote("a.txt", Quoting::ShellAlways), "'a.txt'");
        assert_eq!(quote("it's", Quoting::ShellAlways), "\"it's\"");
    }

    #[test]
    fn shell_escape_uses_dollar_quotes_for_control_bytes() {
        assert_eq!(quote("nl\nx", Quoting::ShellEscape), "'nl'$'\\n''x'");
        assert_eq!(
            quote("tab\there", Quoting::ShellEscape),
            "'tab'$'\\t''here'"
        );
        assert_eq!(quote("plain", Quoting::ShellEscape), "plain");
        assert_eq!(quote("plain", Quoting::ShellEscapeAlways), "'plain'");
    }

    #[test]
    fn literal_and_unset_keep_names() {
        assert_eq!(quote("a b\n", Quoting::Literal), "a b\n");
        assert_eq!(quote("a b", Quoting::Unset), "a b");
    }
}
