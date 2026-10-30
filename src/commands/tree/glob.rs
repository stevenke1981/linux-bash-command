//! Shell-style name patterns for `tree -I` and `tree -P`: `*`, `?`, `[...]` (with `!` or `^`
//! to negate) and `\` escapes. `-I` and `-P` take several patterns separated by `|`.

/// A list of `|`-separated patterns, optionally matched without case.
pub struct Patterns {
    alternatives: Vec<Vec<u8>>,
    ignore_case: bool,
}

impl Patterns {
    pub fn new(text: &str, ignore_case: bool) -> Self {
        let alternatives = text
            .split('|')
            .map(|part| fold(part.as_bytes(), ignore_case))
            .collect();
        Self {
            alternatives,
            ignore_case,
        }
    }

    /// True when any alternative matches the whole of `name`.
    pub fn matches(&self, name: &str) -> bool {
        let text = fold(name.as_bytes(), self.ignore_case);
        self.alternatives
            .iter()
            .any(|pattern| glob_match(pattern, &text))
    }
}

fn fold(bytes: &[u8], ignore_case: bool) -> Vec<u8> {
    if ignore_case {
        bytes.to_ascii_lowercase()
    } else {
        bytes.to_vec()
    }
}

/// Matches `text` against `pattern` in full.
pub fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    // Position after the last `*` seen, and the text position it currently consumes to.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match step(pattern, p, text[t]) {
            Some(next) => {
                p = next;
                t += 1;
            }
            None if p < pattern.len() && pattern[p] == b'*' => {
                star = Some((p + 1, t));
                p += 1;
            }
            None => match star {
                Some((star_p, star_t)) => {
                    p = star_p;
                    t = star_t + 1;
                    star = Some((star_p, star_t + 1));
                }
                None => return false,
            },
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// Matches one character of the text at `pattern[p]`. Returns the pattern position after
/// the matched item, or `None` when it does not match (or is a `*`).
fn step(pattern: &[u8], p: usize, c: u8) -> Option<usize> {
    match pattern.get(p)? {
        b'*' => None,
        b'?' => Some(p + 1),
        b'[' => bracket(pattern, p, c),
        b'\\' if p + 1 < pattern.len() => (pattern[p + 1] == c).then_some(p + 2),
        &literal => (literal == c).then_some(p + 1),
    }
}

/// Matches a bracket expression starting at `pattern[start] == '['`. An unclosed bracket
/// matches a literal `[`.
fn bracket(pattern: &[u8], start: usize, c: u8) -> Option<usize> {
    let mut i = start + 1;
    let negate = matches!(pattern.get(i), Some(b'!') | Some(b'^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < pattern.len() && (pattern[i] != b']' || first) {
        first = false;
        let low = pattern[i];
        if i + 2 < pattern.len() && pattern[i + 1] == b'-' && pattern[i + 2] != b']' {
            if (low..=pattern[i + 2]).contains(&c) {
                matched = true;
            }
            i += 3;
        } else {
            if low == c {
                matched = true;
            }
            i += 1;
        }
    }
    if i >= pattern.len() {
        return (c == b'[').then_some(start + 1);
    }
    (matched != negate).then_some(i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, text: &str) -> bool {
        glob_match(pattern.as_bytes(), text.as_bytes())
    }

    #[test]
    fn star_and_question_mark() {
        assert!(m("*.txt", "file.txt"));
        assert!(m("f?le*", "file.txt"));
        assert!(!m("*.rs", "file.txt"));
        assert!(m("*", ""));
        assert!(m("a*b*c", "aXbYc"));
    }

    #[test]
    fn brackets_and_escapes() {
        assert!(m("[ab]c", "bc"));
        assert!(!m("[ab]c", "cc"));
        assert!(m("[!a-c]x", "dx"));
        assert!(!m("[!a-c]x", "bx"));
        assert!(m("\\*", "*"));
        assert!(!m("\\*", "a"));
        assert!(m("[abc", "[abc"));
    }

    #[test]
    fn alternatives_and_case_folding() {
        let p = Patterns::new("a.d|sub", false);
        assert!(p.matches("sub"));
        assert!(p.matches("a.d"));
        assert!(!p.matches("b"));
        let loose = Patterns::new("FILE*", true);
        assert!(loose.matches("file.txt"));
        assert!(!Patterns::new("FILE*", false).matches("file.txt"));
    }
}
