//! Shell-style wildcard matching used by find's `-name`, `-iname`, `-path` and `-ipath`.
//!
//! Supported syntax: `*` (any sequence, including `/`), `?` (one character),
//! `[abc]`, `[a-z]`, `[!...]` / `[^...]` (negated), and `\` to escape the next character.
//! An unterminated `[` is matched literally, as fnmatch does.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(char),
    AnyChar,
    AnySequence,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

/// A compiled wildcard pattern.
#[derive(Debug, Clone)]
pub struct Glob {
    tokens: Vec<Token>,
    ignore_case: bool,
}

impl Glob {
    pub fn new(pattern: &str, ignore_case: bool) -> Self {
        Self {
            tokens: tokenize(pattern),
            ignore_case,
        }
    }

    /// Returns true when the whole `text` matches the pattern.
    pub fn is_match(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        let count = self.tokens.len();
        // previous[j] means: the first j tokens match the characters consumed so far.
        let mut previous = vec![false; count + 1];
        previous[0] = true;
        for (index, token) in self.tokens.iter().enumerate() {
            if *token == Token::AnySequence {
                previous[index + 1] = previous[index];
            }
        }
        for &character in &text {
            let mut current = vec![false; count + 1];
            for (index, token) in self.tokens.iter().enumerate() {
                current[index + 1] = match token {
                    Token::AnySequence => current[index] || previous[index + 1],
                    Token::AnyChar => previous[index],
                    Token::Literal(expected) => {
                        previous[index] && self.same_char(*expected, character)
                    }
                    Token::Class { .. } => previous[index] && self.class_matches(token, character),
                };
            }
            previous = current;
        }
        previous[count]
    }

    fn same_char(&self, expected: char, actual: char) -> bool {
        expected == actual || (self.ignore_case && lower(expected) == lower(actual))
    }

    fn class_matches(&self, token: &Token, character: char) -> bool {
        let Token::Class { negated, ranges } = token else {
            return false;
        };
        let candidates = if self.ignore_case {
            vec![character, lower(character), upper(character)]
        } else {
            vec![character]
        };
        let inside = candidates.into_iter().any(|candidate| {
            ranges
                .iter()
                .any(|(low, high)| *low <= candidate && candidate <= *high)
        });
        inside != *negated
    }
}

fn lower(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

fn upper(character: char) -> char {
    character.to_uppercase().next().unwrap_or(character)
}

fn tokenize(pattern: &str) -> Vec<Token> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens: Vec<Token> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let (token, next) = match chars[index] {
            '*' => (Token::AnySequence, index + 1),
            '?' => (Token::AnyChar, index + 1),
            '\\' if index + 1 < chars.len() => (Token::Literal(chars[index + 1]), index + 2),
            '[' => parse_class(&chars, index).unwrap_or((Token::Literal('['), index + 1)),
            other => (Token::Literal(other), index + 1),
        };
        let repeated_star =
            token == Token::AnySequence && tokens.last() == Some(&Token::AnySequence);
        if !repeated_star {
            tokens.push(token);
        }
        index = next;
    }
    tokens
}

/// Parses a bracket expression starting at `start` (the `[`). Returns the token and
/// the index just after the closing `]`, or `None` when the bracket is not closed.
fn parse_class(chars: &[char], start: usize) -> Option<(Token, usize)> {
    let mut index = start + 1;
    let negated = matches!(chars.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let mut ranges = Vec::new();
    let mut first = true;
    while index < chars.len() {
        if chars[index] == ']' && !first {
            return Some((Token::Class { negated, ranges }, index + 1));
        }
        first = false;
        let (low, mut next) = if chars[index] == '\\' && index + 1 < chars.len() {
            (chars[index + 1], index + 2)
        } else {
            (chars[index], index + 1)
        };
        let high = if next + 1 < chars.len() && chars[next] == '-' && chars[next + 1] != ']' {
            let high = chars[next + 1];
            next += 2;
            high
        } else {
            low
        };
        ranges.push((low, high));
        index = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Glob;

    #[test]
    fn star_and_question_match_names() {
        assert!(Glob::new("*.rs", false).is_match("main.rs"));
        assert!(Glob::new("?.txt", false).is_match("a.txt"));
        assert!(!Glob::new("?.txt", false).is_match("long.txt"));
        assert!(Glob::new("a*c", false).is_match("abc"));
        assert!(Glob::new("*", false).is_match(""));
    }

    #[test]
    fn bracket_classes_and_escapes() {
        assert!(Glob::new("[abc].md", false).is_match("b.md"));
        assert!(!Glob::new("[abc].md", false).is_match("d.md"));
        assert!(Glob::new("[a-c]x", false).is_match("bx"));
        assert!(Glob::new("[!a-c]x", false).is_match("dx"));
        assert!(!Glob::new("[!a-c]x", false).is_match("bx"));
        assert!(Glob::new(r"a\*b", false).is_match("a*b"));
        assert!(!Glob::new(r"a\*b", false).is_match("axb"));
    }

    #[test]
    fn unterminated_bracket_is_literal() {
        assert!(Glob::new("a[b", false).is_match("a[b"));
    }

    #[test]
    fn ignore_case_folds_literals_and_classes() {
        assert!(Glob::new("readme*", true).is_match("README.md"));
        assert!(Glob::new("[a-c]X", true).is_match("bx"));
        assert!(!Glob::new("readme*", false).is_match("README.md"));
    }
}
