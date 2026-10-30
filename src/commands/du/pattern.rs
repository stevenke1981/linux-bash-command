//! Shell wildcard matching for `du --exclude` and `-X`: `*`, `?`, `[...]` classes and
//! backslash escapes. Matching is byte-wise, as in C `fnmatch` without `FNM_PATHNAME`.

/// True when `name` matches `pattern` in full.
pub fn matches(pattern: &str, name: &str) -> bool {
    let pat = pattern.as_bytes();
    let text = name.as_bytes();
    match_here(pat, text)
}

fn match_here(pat: &[u8], text: &[u8]) -> bool {
    let Some((&first, rest)) = pat.split_first() else {
        return text.is_empty();
    };
    match first {
        b'*' => {
            let mut skip = 0;
            loop {
                if match_here(rest, &text[skip..]) {
                    return true;
                }
                if skip == text.len() {
                    return false;
                }
                skip += 1;
            }
        }
        b'?' => !text.is_empty() && match_here(rest, &text[1..]),
        b'[' => match_class(pat, text),
        b'\\' if !rest.is_empty() => {
            !text.is_empty() && text[0] == rest[0] && match_here(&rest[1..], &text[1..])
        }
        literal => !text.is_empty() && text[0] == literal && match_here(rest, &text[1..]),
    }
}

/// Matches a `[...]` class at the start of `pat` against the first byte of `text`. An
/// unterminated class matches a literal `[`.
fn match_class(pat: &[u8], text: &[u8]) -> bool {
    let Some(end) = class_end(pat) else {
        return !text.is_empty() && text[0] == b'[' && match_here(&pat[1..], &text[1..]);
    };
    if text.is_empty() {
        return false;
    }
    let (negated, body) = match pat.get(1) {
        Some(b'!') | Some(b'^') => (true, &pat[2..end]),
        _ => (false, &pat[1..end]),
    };
    let found = class_contains(body, text[0]);
    found != negated && match_here(&pat[end + 1..], &text[1..])
}

/// Index of the `]` that closes the class starting at `pat[0]`. A `]` right after the
/// opening bracket (or after `!`/`^`) is literal.
fn class_end(pat: &[u8]) -> Option<usize> {
    let mut index = 1;
    if matches!(pat.get(index), Some(b'!') | Some(b'^')) {
        index += 1;
    }
    if pat.get(index) == Some(&b']') {
        index += 1;
    }
    while index < pat.len() {
        if pat[index] == b']' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn class_contains(body: &[u8], byte: u8) -> bool {
    let mut index = 0;
    while index < body.len() {
        if index + 2 < body.len() && body[index + 1] == b'-' {
            if (body[index]..=body[index + 2]).contains(&byte) {
                return true;
            }
            index += 3;
        } else {
            if body[index] == byte {
                return true;
            }
            index += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn star_and_question() {
        assert!(matches("*.txt", "a.txt"));
        assert!(matches("*", ""));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "ac"));
        assert!(!matches("*.txt", "a.rs"));
    }

    #[test]
    fn classes_ranges_and_negation() {
        assert!(matches("[abc]x", "bx"));
        assert!(matches("[a-c]x", "cx"));
        assert!(!matches("[a-c]x", "dx"));
        assert!(matches("[!a-c]x", "dx"));
        assert!(matches("[^a]x", "bx"));
        assert!(matches("[]]", "]"));
    }

    #[test]
    fn unterminated_class_is_literal() {
        assert!(matches("[abc", "[abc"));
        assert!(!matches("[abc", "a"));
    }

    #[test]
    fn escapes_and_exact_match() {
        assert!(matches("\\*", "*"));
        assert!(!matches("\\*", "a"));
        assert!(matches("file", "file"));
        assert!(!matches("file", "files"));
        assert!(!matches("file", "fil"));
    }
}
