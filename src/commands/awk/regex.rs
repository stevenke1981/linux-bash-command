//! awk regular expressions: translation of POSIX ERE (with the usual awk escapes) to the
//! `regex` crate syntax, and a cache of compiled patterns.
//!
//! Matching is byte-oriented (`unicode(false)`), `.` matches any byte including newline,
//! and `^`/`$` anchor the whole subject string. Alternation is leftmost-first rather than
//! POSIX leftmost-longest; this is a known difference.

use std::collections::HashMap;
use std::rc::Rc;

use regex::bytes::{Regex, RegexBuilder};

const MAX_CACHED: usize = 256;

/// Compiled regexes keyed by their awk source text.
#[derive(Default)]
pub struct RegexCache {
    map: HashMap<Vec<u8>, Rc<Regex>>,
}

impl RegexCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&mut self, source: &[u8]) -> Result<Rc<Regex>, String> {
        if let Some(found) = self.map.get(source) {
            return Ok(Rc::clone(found));
        }
        let pattern = translate(source)?;
        let regex = RegexBuilder::new(&pattern)
            .unicode(false)
            .build()
            .map_err(|_| {
                format!(
                    "invalid regular expression: {}",
                    String::from_utf8_lossy(source)
                )
            })?;
        let regex = Rc::new(regex);
        if self.map.len() >= MAX_CACHED {
            self.map.clear();
        }
        self.map.insert(source.to_vec(), Rc::clone(&regex));
        Ok(regex)
    }
}

/// Characters that must be escaped to be literal in the output syntax.
fn is_meta(c: char) -> bool {
    matches!(
        c,
        '\\' | '.'
            | '+'
            | '*'
            | '?'
            | '('
            | ')'
            | '|'
            | '['
            | ']'
            | '{'
            | '}'
            | '^'
            | '$'
            | '#'
            | '&'
            | '-'
            | '~'
    )
}

fn push_literal(out: &mut String, c: char) {
    if is_meta(c) {
        out.push('\\');
    }
    out.push(c);
}

/// The character named by a simple escape letter (`\n`, `\t`, ...), if any.
fn simple_escape(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'f' => '\x0c',
        'v' => '\x0b',
        'a' => '\x07',
        'b' => '\x08',
        _ => return None,
    })
}

/// Translates an awk ERE into `regex` crate syntax.
pub fn translate(source: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(source);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len() * 2 + 4);
    out.push_str("(?s)");
    let mut i = 0;
    // True when the previous item can take a repetition operator.
    let mut atom = false;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' => {
                atom = true;
                i = escape_outside(&chars, i, &mut out);
            }
            '[' => {
                atom = true;
                i = bracket(&chars, i, &mut out)?;
            }
            '(' => {
                atom = false;
                out.push('(');
            }
            ')' => {
                atom = true;
                out.push(')');
            }
            '|' => {
                atom = false;
                out.push('|');
            }
            '^' => {
                atom = false;
                out.push('^');
            }
            '$' => {
                atom = true;
                out.push('$');
            }
            '.' => {
                atom = true;
                out.push('.');
            }
            '*' | '+' | '?' if atom => out.push(c),
            '*' | '+' | '?' => {
                atom = true;
                push_literal(&mut out, c);
            }
            '{' => {
                if let Some(end) = interval_end(&chars, i) {
                    out.extend(&chars[i - 1..end]);
                    i = end;
                    atom = true;
                } else {
                    atom = true;
                    out.push_str("\\{");
                }
            }
            _ => {
                atom = true;
                push_literal(&mut out, c);
            }
        }
    }
    Ok(out)
}

/// Handles the escape whose backslash precedes `chars[i]`; returns the index after it.
fn escape_outside(chars: &[char], i: usize, out: &mut String) -> usize {
    let Some(&next) = chars.get(i) else {
        out.push_str("\\\\");
        return i;
    };
    let i = i + 1;
    if let Some(literal) = simple_escape(next) {
        push_literal(out, literal);
        return i;
    }
    match next {
        'w' | 'W' | 's' | 'S' | 'B' => {
            out.push('\\');
            out.push(next);
        }
        'y' | '<' | '>' => out.push_str("\\b"),
        '`' => out.push_str("\\A"),
        '\'' => out.push_str("\\z"),
        '0'..='7' => {
            let mut value = next.to_digit(8).unwrap_or(0);
            let mut index = i;
            for _ in 0..2 {
                match chars.get(index).and_then(|c| c.to_digit(8)) {
                    Some(digit) => {
                        value = value * 8 + digit;
                        index += 1;
                    }
                    None => break,
                }
            }
            push_literal(out, char::from_u32(value).unwrap_or('\u{fffd}'));
            return index;
        }
        other => push_literal(out, other),
    }
    i
}

/// Translates a bracket expression whose `[` precedes `chars[i]`; returns the index after `]`.
fn bracket(chars: &[char], mut i: usize, out: &mut String) -> Result<usize, String> {
    out.push('[');
    if chars.get(i) == Some(&'^') {
        out.push('^');
        i += 1;
    }
    let mut first = true;
    loop {
        let Some(&c) = chars.get(i) else {
            return Err("unterminated bracket expression".into());
        };
        if c == ']' && !first {
            out.push(']');
            return Ok(i + 1);
        }
        first = false;
        i += 1;
        if c == '[' && chars.get(i) == Some(&':') {
            if let Some(len) = posix_class_len(&chars[i..]) {
                out.extend(&chars[i - 1..i + len]);
                i += len;
                continue;
            }
        }
        if c == '\\' {
            let Some(&next) = chars.get(i) else {
                return Err("unterminated bracket expression".into());
            };
            i += 1;
            let literal = simple_escape(next).unwrap_or(next);
            push_class_char(out, literal);
            continue;
        }
        push_class_char(out, c);
    }
}

/// Length in `chars` of a `:name:]` tail (starting at the colon), when well formed.
fn posix_class_len(chars: &[char]) -> Option<usize> {
    let close = chars.windows(2).position(|w| w == [':', ']'])?;
    let name = &chars[1..close];
    if close > 0 && name.iter().all(|c| c.is_ascii_alphabetic()) {
        Some(close + 2)
    } else {
        None
    }
}

fn push_class_char(out: &mut String, c: char) {
    if c.is_ascii_alphanumeric() || c == '-' || !c.is_ascii() || c == ' ' {
        out.push(c);
    } else {
        out.push('\\');
        out.push(c);
    }
}

/// If `chars[i..]` is `m}`, `m,}` or `m,n}` (the body of an interval), the index after `}`.
fn interval_end(chars: &[char], mut i: usize) -> Option<usize> {
    let start = i;
    while chars.get(i).is_some_and(char::is_ascii_digit) {
        i += 1;
    }
    if i == start {
        return None;
    }
    if chars.get(i) == Some(&',') {
        i += 1;
        while chars.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
    }
    (chars.get(i) == Some(&'}')).then_some(i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(source: &str, subject: &str) -> bool {
        let mut cache = RegexCache::new();
        cache
            .get(source.as_bytes())
            .expect("compile")
            .is_match(subject.as_bytes())
    }

    #[test]
    fn basic_ere_operators() {
        assert!(matches("^a+b*$", "aaa"));
        assert!(matches("colou?r", "color"));
        assert!(matches("(cat|dog)s", "dogs"));
        assert!(!matches("^x$", "xy"));
        assert!(matches("a.c", "a\nc"));
    }

    #[test]
    fn intervals_and_literal_braces() {
        assert!(matches("^a{2,3}$", "aaa"));
        assert!(!matches("^a{2}$", "aaa"));
        assert!(matches("^{x}$", "{x}"));
        assert!(matches("a{", "a{"));
    }

    #[test]
    fn bracket_expressions() {
        assert!(matches("^[]a]+$", "]a]"));
        assert!(matches("^[^0-9]$", "x"));
        assert!(!matches("^[^0-9]$", "5"));
        assert!(matches("^[[:digit:]]+$", "123"));
        assert!(matches("^[a[]$", "["));
        assert!(matches("^[a\\]]$", "]"));
    }

    #[test]
    fn awk_escapes_inside_and_outside_brackets() {
        assert!(matches("^a\\.b$", "a.b"));
        assert!(!matches("^a\\.b$", "axb"));
        assert!(matches("\\t", "\t"));
        assert!(matches("^\\101$", "A"));
        assert!(matches("\\y", "x"));
        assert!(matches("^[\\t]$", "\t"));
    }

    #[test]
    fn leading_repetition_is_literal() {
        assert!(matches("*x", "*x"));
        assert!(matches("+", "+"));
    }

    #[test]
    fn invalid_patterns_are_reported() {
        let mut cache = RegexCache::new();
        assert!(cache.get(b"[abc").is_err());
        assert!(translate(b"[a").is_err());
    }

    #[test]
    fn cache_returns_the_same_compiled_regex() {
        let mut cache = RegexCache::new();
        let first = cache.get(b"x+").unwrap();
        let second = cache.get(b"x+").unwrap();
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn translation_output_for_plain_text() {
        assert_eq!(translate(b"a.b").unwrap(), "(?s)a.b");
        assert_eq!(translate(b"a\\/b").unwrap_or_default(), "(?s)a/b");
    }
}
