//! Pattern translation for grep: BRE/ERE/fixed/PCRE handling, globs and match spans.

use super::{Options, Syntax};
use crate::common::regex::bre_to_regex;
use regex::bytes::{Regex as ByteRegex, RegexBuilder as ByteRegexBuilder};
use regex::Regex as NameRegex;

// ---------------------------------------------------------------- pattern translation

/// Builds the single matcher. Group 1 always holds the pattern span, which
/// `-o` relies on; `-w` and `-x` wrap that group with boundary checks.
pub(super) fn build_regex(options: &Options) -> Result<ByteRegex, String> {
    let mut alternatives = Vec::with_capacity(options.patterns.len());
    for pattern in &options.patterns {
        let translated = match options.syntax {
            Syntax::Fixed => regex::escape(pattern),
            Syntax::Basic => bre_to_regex(pattern)
                .map_err(|error| format!("invalid regular expression: {error}"))?,
            Syntax::Extended | Syntax::Perl => pattern.clone(),
        };
        alternatives.push(format!("(?:{translated})"));
    }
    // An empty pattern list matches nothing (for example an empty -f file).
    let body = if alternatives.is_empty() {
        String::from(r"\b\B")
    } else {
        alternatives.join("|")
    };
    let wrapped = if options.line {
        format!("^({body})$")
    } else if options.word {
        format!(r"(?:^|\W)({body})(?:\W|$)")
    } else {
        format!("({body})")
    };
    ByteRegexBuilder::new(&wrapped)
        .case_insensitive(options.ignore_case)
        // With -z a record may hold newlines, which `.` must match as GNU does.
        .dot_matches_new_line(options.zero)
        .build()
        .map_err(|error| match options.syntax {
            Syntax::Perl => format!(
                "invalid or unsupported -P pattern (lookaround and backreferences are not supported): {error}"
            ),
            _ => format!("invalid regular expression: {error}"),
        })
}

/// Translates a glob (`*`, `?`, `[...]`, `{a,b}`) into an anchored regex.
pub(super) fn glob_to_regex(glob: &str) -> Result<NameRegex, String> {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::from("^");
    let mut in_braces = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => match glob_class_end(&chars, i) {
                Some(end) => {
                    out.push_str(&glob_class(&chars[i..end]));
                    i = end + 1;
                }
                None => out.push_str("\\["),
            },
            '{' if !in_braces => {
                in_braces = true;
                out.push_str("(?:");
            }
            '}' if in_braces => {
                in_braces = false;
                out.push(')');
            }
            ',' if in_braces => out.push('|'),
            '\\' if i < chars.len() => {
                out.push_str(&regex::escape(&chars[i].to_string()));
                i += 1;
            }
            _ => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    if in_braces {
        return Err(format!("invalid glob (unmatched '{{'): {glob}"));
    }
    out.push('$');
    NameRegex::new(&out).map_err(|error| format!("invalid glob {glob}: {error}"))
}

fn glob_class_end(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start;
    if chars.get(j) == Some(&'!') {
        j += 1;
    }
    if chars.get(j) == Some(&']') {
        j += 1;
    }
    chars[j..]
        .iter()
        .position(|&c| c == ']')
        .map(|offset| j + offset)
}

fn glob_class(body: &[char]) -> String {
    let mut class = String::from("[");
    let mut rest = body;
    if rest.first() == Some(&'!') {
        class.push('^');
        rest = &rest[1..];
    }
    for &c in rest {
        match c {
            '[' | ']' | '\\' | '^' | '&' | '~' => {
                class.push('\\');
                class.push(c);
            }
            _ => class.push(c),
        }
    }
    class.push(']');
    class
}

/// Returns each non-empty span matched by the pattern (group 1), scanning left to right.
pub(super) fn spans(regex: &ByteRegex, line: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut position = 0;
    while position <= line.len() {
        let Some(captures) = regex.captures_at(line, position) else {
            break;
        };
        let Some(group) = captures.get(1) else {
            break;
        };
        if group.start() == group.end() {
            position = group.end() + 1;
        } else {
            found.push((group.start(), group.end()));
            position = group.end();
        }
    }
    found
}
