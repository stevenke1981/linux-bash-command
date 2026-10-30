//! The `s` command: replacement parsing and substitution.

use ::regex::bytes::{Captures, Regex};

use super::parse::Sink;
use super::pattern::{number_escape, Pattern};

/// A `\U`, `\L`, `\u`, `\l` or `\E` case conversion in a replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    OneUpper,
    OneLower,
    End,
}

/// One part of a replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Text(Vec<u8>),
    /// `&` and `\0`..`\9`.
    Group(usize),
    Case(Case),
}

/// A parsed `s` command.
#[derive(Clone, Debug)]
pub struct Subst {
    pub pattern: Pattern,
    pub replacement: Vec<Piece>,
    /// `g`: replace every match from the chosen occurrence on.
    pub global: bool,
    /// `N`: first occurrence to replace (1 by default).
    pub occurrence: u64,
    /// `p`: print the pattern space when a substitution was made.
    pub print: bool,
    /// `w FILE`: write the pattern space when a substitution was made.
    pub write: Option<Sink>,
}

/// Parses the replacement text of an `s` command.
pub fn parse_replacement(source: &[u8]) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut text = Vec::new();
    let mut i = 0;
    while i < source.len() {
        let c = source[i];
        i += 1;
        if c == b'&' {
            flush_text(&mut pieces, &mut text);
            pieces.push(Piece::Group(0));
            continue;
        }
        if c != b'\\' {
            text.push(c);
            continue;
        }
        let Some(&kind) = source.get(i) else {
            text.push(b'\\');
            break;
        };
        i += 1;
        let piece = match kind {
            b'0'..=b'9' => Some(Piece::Group(usize::from(kind - b'0'))),
            b'L' => Some(Piece::Case(Case::Lower)),
            b'U' => Some(Piece::Case(Case::Upper)),
            b'l' => Some(Piece::Case(Case::OneLower)),
            b'u' => Some(Piece::Case(Case::OneUpper)),
            b'E' => Some(Piece::Case(Case::End)),
            _ => None,
        };
        if let Some(piece) = piece {
            flush_text(&mut pieces, &mut text);
            pieces.push(piece);
            continue;
        }
        match kind {
            b'n' => text.push(b'\n'),
            b't' => text.push(b'\t'),
            b'r' => text.push(b'\r'),
            b'f' => text.push(0x0c),
            b'v' => text.push(0x0b),
            b'a' => text.push(0x07),
            b'c' => {
                let Some(&target) = source.get(i) else {
                    return Err("stray \\".into());
                };
                i += 1;
                text.push((target.to_ascii_uppercase()) ^ 0x40);
            }
            b'd' | b'o' | b'x' => {
                let (radix, max) = match kind {
                    b'd' => (10, 3),
                    b'o' => (8, 3),
                    _ => (16, 2),
                };
                let digits: Vec<char> = source[i..].iter().map(|&b| char::from(b)).collect();
                let (value, used) = number_escape(&digits, radix, max);
                if used == 0 {
                    text.push(kind);
                } else {
                    i += used;
                    text.push(value);
                }
            }
            other => text.push(other),
        }
    }
    flush_text(&mut pieces, &mut text);
    Ok(pieces)
}

fn flush_text(pieces: &mut Vec<Piece>, text: &mut Vec<u8>) {
    if !text.is_empty() {
        pieces.push(Piece::Text(std::mem::take(text)));
    }
}

/// The highest group referenced by a replacement (0 when only `&` or text).
pub fn max_group(pieces: &[Piece]) -> usize {
    pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Group(n) => Some(*n),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Substitutes `subst` in `text`. Returns `None` when nothing was replaced.
pub fn substitute(regex: &Regex, text: &[u8], subst: &Subst) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut copied = 0;
    let mut pos = 0;
    let mut count: u64 = 0;
    let mut prev_end: Option<usize> = None;
    let mut replaced = false;
    while pos <= text.len() {
        let Some(caps) = regex.captures_at(text, pos) else {
            break;
        };
        let Some(whole) = caps.get(0) else {
            break;
        };
        let (start, end) = (whole.start(), whole.end());
        if start == end && prev_end == Some(start) {
            // An empty match right after the previous match does not count.
            pos = start + 1;
            continue;
        }
        count += 1;
        if count >= subst.occurrence {
            out.extend_from_slice(&text[copied..start]);
            expand(&subst.replacement, &caps, text, &mut out);
            copied = end;
            replaced = true;
            if !subst.global {
                break;
            }
        }
        prev_end = Some(end);
        pos = if start == end { end + 1 } else { end };
    }
    if !replaced {
        return None;
    }
    out.extend_from_slice(&text[copied..]);
    Some(out)
}

/// Appends the replacement for one match, applying case conversions.
fn expand(pieces: &[Piece], caps: &Captures, text: &[u8], out: &mut Vec<u8>) {
    let mut mode: Option<Case> = None;
    let mut one: Option<Case> = None;
    for piece in pieces {
        match piece {
            Piece::Case(Case::End) => {
                mode = None;
                one = None;
            }
            Piece::Case(case @ (Case::Upper | Case::Lower)) => mode = Some(*case),
            Piece::Case(case) => one = Some(*case),
            Piece::Text(bytes) => push_cased(out, bytes, mode, &mut one),
            Piece::Group(n) => {
                if let Some(matched) = caps.get(*n) {
                    push_cased(out, &text[matched.start()..matched.end()], mode, &mut one);
                }
            }
        }
    }
}

fn push_cased(out: &mut Vec<u8>, bytes: &[u8], mode: Option<Case>, one: &mut Option<Case>) {
    for &byte in bytes {
        let converted = match one.take() {
            Some(Case::OneUpper) => byte.to_ascii_uppercase(),
            Some(Case::OneLower) => byte.to_ascii_lowercase(),
            _ => match mode {
                Some(Case::Upper) => byte.to_ascii_uppercase(),
                Some(Case::Lower) => byte.to_ascii_lowercase(),
                _ => byte,
            },
        };
        out.push(converted);
    }
}

/// Builds a `Subst` with the default flags. Used by the parser and tests.
pub fn new_subst(pattern: Pattern, replacement: Vec<Piece>) -> Subst {
    Subst {
        pattern,
        replacement,
        global: false,
        occurrence: 1,
        print: false,
        write: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::sed::pattern::compile;
    use std::rc::Rc;

    fn regex(source: &str) -> Regex {
        compile(source.as_bytes(), false, false, false).unwrap()
    }

    fn run(source: &str, replacement: &str, text: &str, flags: &str) -> Option<String> {
        let mut subst = new_subst(
            Pattern(Some(Rc::new(regex(source)))),
            parse_replacement(replacement.as_bytes()).unwrap(),
        );
        for flag in flags.chars() {
            match flag {
                'g' => subst.global = true,
                'p' => subst.print = true,
                digit => {
                    subst.occurrence = u64::from(digit.to_digit(10).unwrap());
                }
            }
        }
        let re = regex(source);
        substitute(&re, text.as_bytes(), &subst).map(|bytes| String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn first_match_only_by_default() {
        assert_eq!(run("a", "X", "aaa", "").unwrap(), "Xaa");
    }

    #[test]
    fn global_replaces_all() {
        assert_eq!(run("a", "X", "aaa", "g").unwrap(), "XXX");
    }

    #[test]
    fn numbered_occurrence_with_and_without_global() {
        assert_eq!(run("a", "X", "aaaa", "2").unwrap(), "aXaa");
        assert_eq!(run("a", "X", "aaaa", "2g").unwrap(), "aXXX");
    }

    #[test]
    fn no_match_returns_none() {
        assert!(run("z", "X", "abc", "g").is_none());
    }

    #[test]
    fn empty_matches_with_global() {
        assert_eq!(run("x*", "-", "abc", "g").unwrap(), "-a-b-c-");
    }

    #[test]
    fn empty_match_after_a_match_is_skipped() {
        assert_eq!(run("a*", "x", "baaac", "g").unwrap(), "xbxcx");
    }

    #[test]
    fn groups_and_ampersand() {
        assert_eq!(
            run("\\(a\\)\\(b\\)", "[\\2\\1&]", "ab", "").unwrap(),
            "[baab]"
        );
    }

    #[test]
    fn escaped_ampersand_is_literal() {
        assert_eq!(run("a", "\\&", "a", "").unwrap(), "&");
    }

    #[test]
    fn replacement_escapes() {
        assert_eq!(run("a", "\\n", "a", "").unwrap(), "\n");
        assert_eq!(run("a", "\\t|", "a", "").unwrap(), "\t|");
        assert_eq!(run("a", "\\x41\\o102\\d067", "a", "").unwrap(), "ABC");
    }

    #[test]
    fn case_conversion_pieces() {
        assert_eq!(
            run("\\(.*\\) \\(.*\\)", "\\U\\1\\E \\u\\2", "ab cd", "").unwrap(),
            "AB Cd"
        );
        assert_eq!(run("hello", "\\L\\uHELLO", "hello", "").unwrap(), "Hello");
    }

    #[test]
    fn one_shot_case_applies_to_next_char_only() {
        assert_eq!(run("x", "\\lABC", "x", "").unwrap(), "aBC");
    }

    #[test]
    fn replacement_parser_builds_pieces() {
        let pieces = parse_replacement(b"a&\\1").unwrap();
        assert_eq!(
            pieces,
            vec![Piece::Text(b"a".to_vec()), Piece::Group(0), Piece::Group(1)]
        );
        assert_eq!(max_group(&pieces), 1);
    }

    #[test]
    fn trailing_backslash_in_replacement_is_kept() {
        assert_eq!(
            parse_replacement(b"a\\").unwrap(),
            vec![Piece::Text(b"a\\".to_vec())]
        );
    }

    #[test]
    fn print_flag_is_recorded_on_the_struct() {
        let subst = new_subst(Pattern::default(), Vec::new());
        assert!(!subst.print);
        assert_eq!(subst.occurrence, 1);
    }
}
