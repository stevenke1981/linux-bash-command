//! Regular expressions for sed.
//!
//! GNU escapes (`\n \t \xHH \oNNN \dNNN \cX`) are expanded first. BRE syntax is then
//! translated by the shared `common::regex::bre_to_regex`, ERE syntax by
//! [`ere_to_regex`] in this module. Matching is byte oriented (`regex::bytes`, no
//! Unicode mode), as in the C locale.

use std::rc::Rc;

use ::regex::bytes::{Regex, RegexBuilder};

use crate::common::regex::bre_to_regex;

/// A regex operand. `None` is the empty regex, which reuses the last regex used.
#[derive(Clone, Debug, Default)]
pub struct Pattern(pub Option<Rc<Regex>>);

/// Compiles a sed regex. `multiline` is the `M` flag: `^` and `$` also match at
/// embedded newlines and `.` stops matching newline, as `REG_NEWLINE` does.
pub fn compile(pattern: &[u8], ere: bool, icase: bool, multiline: bool) -> Result<Regex, String> {
    let text = String::from_utf8_lossy(pattern);
    let expanded = expand_escapes(&text)?;
    let source = if ere {
        ere_to_regex(&expanded)?
    } else {
        bre_to_regex(&expanded)?
    };
    RegexBuilder::new(&source)
        .unicode(false)
        .case_insensitive(icase)
        .multi_line(multiline)
        .dot_matches_new_line(!multiline)
        .build()
        .map_err(|error| error.to_string())
}

/// Returns the regex to use for `pattern`, updating the last-used regex.
pub fn resolve(pattern: &Pattern, last: &mut Option<Rc<Regex>>) -> Result<Rc<Regex>, String> {
    match &pattern.0 {
        Some(regex) => {
            *last = Some(Rc::clone(regex));
            Ok(Rc::clone(regex))
        }
        None => last
            .clone()
            .ok_or_else(|| "no previous regular expression".to_string()),
    }
}

/// Expands GNU character escapes so the BRE/ERE translators see plain syntax.
/// A backreference (`\1`..`\9`) is rejected because the `regex` crate has none.
fn expand_escapes(text: &str) -> Result<String, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(&kind) = chars.get(i) else {
            out.push('\\');
            break;
        };
        i += 1;
        match kind {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            'a' => out.push('\u{7}'),
            '1'..='9' => return Err("unsupported backreference in regex".into()),
            'd' | 'o' | 'x' => {
                let (radix, max) = match kind {
                    'd' => (10, 3),
                    'o' => (8, 3),
                    _ => (16, 2),
                };
                let (value, used) = number_escape(&chars[i..], radix, max);
                if used == 0 {
                    out.push(kind);
                } else {
                    i += used;
                    push_literal(&mut out, char::from(value));
                }
            }
            'c' => {
                let Some(&target) = chars.get(i) else {
                    return Err("stray \\".into());
                };
                i += 1;
                let value = (target.to_ascii_uppercase() as u8) ^ 0x40;
                push_literal(&mut out, char::from(value));
            }
            _ => {
                out.push('\\');
                out.push(kind);
            }
        }
    }
    Ok(out)
}

/// Reads up to `max` digits in `radix` from `chars`. Returns the value and the number
/// of characters used (0 when no digit follows).
pub fn number_escape(chars: &[char], radix: u32, max: usize) -> (u8, usize) {
    let mut value: u32 = 0;
    let mut used = 0;
    while used < max {
        let Some(digit) = chars.get(used).and_then(|c| c.to_digit(radix)) else {
            break;
        };
        value = value * radix + digit;
        used += 1;
    }
    (value as u8, used)
}

/// Appends `c` as a literal that both translators read as itself.
fn push_literal(out: &mut String, c: char) {
    match c {
        '\\' => out.push_str("\\\\"),
        '^' => out.push_str("\\^"),
        '[' => out.push_str("\\["),
        ']' => out.push(']'),
        '.' | '*' | '$' | '+' | '?' | '(' | ')' | '{' | '}' | '|' => {
            out.push('[');
            out.push(c);
            out.push(']');
        }
        _ => out.push(c),
    }
}

/// Translates an ERE (`-E`) to `regex` syntax.
pub fn ere_to_regex(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    let mut expr_start = true;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        let at_start = std::mem::replace(&mut expr_start, false);
        match c {
            '\\' => {
                let Some(&next) = chars.get(i) else {
                    return Err("trailing backslash".into());
                };
                i += 1;
                ere_escape(next, &mut out);
            }
            '(' | '|' => {
                out.push(c);
                expr_start = true;
            }
            '^' => {
                out.push('^');
                expr_start = true;
            }
            ')' | '.' | '$' => out.push(c),
            '*' | '+' | '?' if !at_start => out.push(c),
            '*' | '+' | '?' => return Err("Invalid preceding regular expression".into()),
            '{' if !at_start => i = copy_interval(&chars, i, &mut out)?,
            '[' => i = copy_bracket(&chars, i, &mut out)?,
            _ => push_literal(&mut out, c),
        }
    }
    Ok(out)
}

fn ere_escape(next: char, out: &mut String) {
    match next {
        '<' | '>' => out.push_str("\\b"),
        '`' => out.push_str("\\A"),
        '\'' => out.push_str("\\z"),
        'w' | 'W' | 's' | 'S' | 'b' | 'B' => {
            out.push('\\');
            out.push(next);
        }
        _ if next.is_ascii_alphanumeric() => out.push(next),
        _ => push_literal(out, next),
    }
}

/// Copies `{m,n}` starting after the `{`. Returns the index after the closing `}`.
fn copy_interval(chars: &[char], start: usize, out: &mut String) -> Result<usize, String> {
    let end = chars[start..]
        .iter()
        .position(|&c| c == '}')
        .ok_or_else(|| "unmatched { or \\{".to_string())?;
    let body: String = chars[start..start + end].iter().collect();
    if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit() || c == ',') {
        return Err("invalid content of \\{\\}".into());
    }
    out.push('{');
    if body.starts_with(',') {
        out.push('0');
    }
    out.push_str(&body);
    out.push('}');
    Ok(start + end + 1)
}

/// Copies a bracket expression whose `[` precedes `start`. Backslash is an ordinary
/// character inside POSIX brackets. Returns the index after the closing `]`.
fn copy_bracket(chars: &[char], start: usize, out: &mut String) -> Result<usize, String> {
    let mut i = start;
    out.push('[');
    if chars.get(i) == Some(&'^') {
        out.push('^');
        i += 1;
    }
    if chars.get(i) == Some(&']') {
        out.push_str("\\]");
        i += 1;
    }
    loop {
        let Some(&c) = chars.get(i) else {
            return Err("unmatched [".into());
        };
        i += 1;
        match c {
            ']' => {
                out.push(']');
                return Ok(i);
            }
            '[' if chars.get(i) == Some(&':') => {
                let close = (i + 1..chars.len().saturating_sub(1))
                    .find(|&j| chars[j] == ':' && chars[j + 1] == ']')
                    .ok_or_else(|| "unmatched [".to_string())?;
                out.extend(&chars[i - 1..=close + 1]);
                i = close + 2;
            }
            '[' if matches!(chars.get(i), Some('.' | '=')) => {
                return Err("collating symbols and equivalence classes are not supported".into());
            }
            '[' => out.push_str("\\["),
            '\\' => out.push_str("\\\\"),
            '&' | '~' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bre(pattern: &str) -> Result<Regex, String> {
        compile(pattern.as_bytes(), false, false, false)
    }

    fn ere(pattern: &str) -> Result<Regex, String> {
        compile(pattern.as_bytes(), true, false, false)
    }

    #[test]
    fn bre_basic_match() {
        assert!(bre("a\\+b").unwrap().is_match(b"aaab"));
        assert!(bre("x\\|y").unwrap().is_match(b"y"));
        assert!(!bre("^b").unwrap().is_match(b"ab"));
    }

    #[test]
    fn ere_basic_match() {
        assert!(ere("(cat|dog)s?").unwrap().is_match(b"dogs"));
        assert!(ere("a{,2}b").unwrap().is_match(b"aab"));
    }

    #[test]
    fn newline_and_tab_escapes_expand() {
        assert!(bre("a\\nb").unwrap().is_match(b"a\nb"));
        assert!(ere("a\\tb").unwrap().is_match(b"a\tb"));
        assert!(bre("[\\n]").unwrap().is_match(b"\n"));
    }

    #[test]
    fn numeric_escapes_expand() {
        assert!(bre("\\o101").unwrap().is_match(b"A"));
        assert!(bre("\\d65").unwrap().is_match(b"A"));
        assert!(bre("\\x41").unwrap().is_match(b"A"));
        assert!(bre("\\cA").unwrap().is_match(b"\x01"));
        assert!(bre("a\\x2ab").unwrap().is_match(b"a*b"));
    }

    #[test]
    fn backreference_is_rejected() {
        let error = bre("\\(a\\)\\1").unwrap_err();
        assert!(error.contains("backreference"), "{error}");
        assert!(ere("(a)\\1").is_err());
    }

    #[test]
    fn word_boundaries_and_buffer_anchors() {
        assert!(bre("\\<foo\\>").unwrap().is_match(b"a foo b"));
        assert!(!bre("\\<foo\\>").unwrap().is_match(b"food"));
        assert!(ere("\\bfoo\\b").unwrap().is_match(b"foo"));
        assert!(bre("\\`a").unwrap().is_match(b"a"));
        assert!(ere("a\\'").unwrap().is_match(b"ba"));
    }

    #[test]
    fn ere_leading_repetition_is_an_error() {
        assert!(ere("*a").is_err());
        assert!(ere("(+a)").is_err());
        assert!(ere("a|*b").is_err());
        assert!(bre("*a").unwrap().is_match(b"*a"));
    }

    #[test]
    fn ere_escaped_metacharacters_are_literal() {
        assert!(ere("a\\.b").unwrap().is_match(b"a.b"));
        assert!(!ere("a\\.b").unwrap().is_match(b"axb"));
        assert!(ere("\\(x\\)").unwrap().is_match(b"(x)"));
    }

    #[test]
    fn bracket_backslash_is_literal() {
        assert!(bre("[\\]").unwrap().is_match(b"\\"));
        assert!(ere("[a\\]+").unwrap().is_match(b"\\"));
        assert!(ere("[[:digit:]]").unwrap().is_match(b"7"));
    }

    #[test]
    fn unmatched_bracket_is_an_error() {
        assert!(bre("[abc").is_err());
        assert!(ere("[abc").is_err());
    }

    #[test]
    fn multiline_changes_anchors_and_dot() {
        let re = compile(b"^b$", false, false, true).unwrap();
        assert!(re.is_match(b"a\nb\nc"));
        let plain = compile(b"a.b", false, false, false).unwrap();
        assert!(plain.is_match(b"a\nb"));
        let multi = compile(b"a.b", false, false, true).unwrap();
        assert!(!multi.is_match(b"a\nb"));
    }

    #[test]
    fn case_insensitive_flag() {
        let re = compile(b"fOo", false, true, false).unwrap();
        assert!(re.is_match(b"FOO"));
    }

    #[test]
    fn empty_pattern_resolves_to_last_regex() {
        let mut last = None;
        assert!(resolve(&Pattern::default(), &mut last).is_err());
        let explicit = Pattern(Some(Rc::new(bre("x").unwrap())));
        resolve(&explicit, &mut last).unwrap();
        let reused = resolve(&Pattern::default(), &mut last).unwrap();
        assert!(reused.is_match(b"x"));
    }

    #[test]
    fn stray_backslash_at_end_is_kept_for_the_translator() {
        assert!(bre("a\\").is_err());
        assert!(ere("a\\").is_err());
    }

    #[test]
    fn dangling_c_escape_is_an_error() {
        assert!(bre("a\\c").is_err());
    }

    #[test]
    fn interval_forms() {
        assert!(bre("^a\\{2,\\}$").unwrap().is_match(b"aaa"));
        assert!(ere("^a{2}$").unwrap().is_match(b"aa"));
        assert!(ere("a{x").is_err());
    }
}
