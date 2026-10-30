//! `sort -V`: the GNU `filevercmp` ordering. Runs of digits compare by value, `~` sorts
//! before anything (even the end of the text), and a trailing file-name suffix such as
//! `.tar.gz` is compared only after the rest of the name.

use std::cmp::Ordering;

/// Rank of one byte in the lexical comparison; digits and the end of text rank 0.
fn order(byte: Option<u8>) -> i32 {
    match byte {
        None => 0,
        Some(b) if b.is_ascii_digit() => 0,
        Some(b) if b.is_ascii_alphabetic() => i32::from(b),
        Some(b'~') => -1,
        Some(b) => 256 + i32::from(b),
    }
}

/// Compares two byte strings with digit runs taken by value (leading zeros ignored).
fn verrevcmp(s: &[u8], t: &[u8]) -> i32 {
    let (mut i, mut j) = (0, 0);
    while i < s.len() || j < t.len() {
        while (i < s.len() && !s[i].is_ascii_digit()) || (j < t.len() && !t[j].is_ascii_digit()) {
            let (sc, tc) = (order(s.get(i).copied()), order(t.get(j).copied()));
            if sc != tc {
                return sc - tc;
            }
            i += 1;
            j += 1;
        }
        while i < s.len() && s[i] == b'0' {
            i += 1;
        }
        while j < t.len() && t[j] == b'0' {
            j += 1;
        }
        let mut diff = 0;
        while i < s.len() && j < t.len() && s[i].is_ascii_digit() && t[j].is_ascii_digit() {
            if diff == 0 {
                diff = i32::from(s[i]) - i32::from(t[j]);
            }
            i += 1;
            j += 1;
        }
        if i < s.len() && s[i].is_ascii_digit() {
            return 1;
        }
        if j < t.len() && t[j].is_ascii_digit() {
            return -1;
        }
        if diff != 0 {
            return diff;
        }
    }
    0
}

/// True when `text` is a run of `.` groups, each a letter or `~` followed by letters,
/// digits or `~`: the shape of a file-name suffix.
fn is_suffix(text: &[u8]) -> bool {
    let mut i = 0;
    while i < text.len() {
        if text[i] != b'.' {
            return false;
        }
        i += 1;
        if i >= text.len() || !(text[i].is_ascii_alphabetic() || text[i] == b'~') {
            return false;
        }
        i += 1;
        while i < text.len() && (text[i].is_ascii_alphanumeric() || text[i] == b'~') {
            i += 1;
        }
    }
    true
}

/// Length of the part of `name` before its longest file-name suffix.
fn prefix_len(name: &[u8]) -> usize {
    (0..=name.len())
        .find(|&start| is_suffix(&name[start..]))
        .unwrap_or(name.len())
}

/// `sort -V` comparison of two keys.
pub fn compare_version(a: &[u8], b: &[u8]) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    let (pa, pb) = (prefix_len(a), prefix_len(b));
    let by_prefix = verrevcmp(&a[..pa], &b[..pb]);
    if by_prefix != 0 {
        return by_prefix.cmp(&0);
    }
    verrevcmp(&a[pa..], &b[pb..]).cmp(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(items: &[&str]) -> Vec<String> {
        let mut v: Vec<&str> = items.to_vec();
        v.sort_by(|a, b| compare_version(a.as_bytes(), b.as_bytes()));
        v.into_iter().map(str::to_owned).collect()
    }

    #[test]
    fn numbers_compare_by_value() {
        assert_eq!(
            sorted(&["a10", "a2", "a1.10", "a1.9"]),
            ["a1.9", "a1.10", "a2", "a10"]
        );
        assert_eq!(compare_version(b"1.01", b"1.1"), Ordering::Equal);
    }

    #[test]
    fn tilde_sorts_first() {
        assert_eq!(sorted(&["x", "x~"]), ["x~", "x"]);
    }

    #[test]
    fn suffixes_compare_last() {
        assert_eq!(
            sorted(&["x-1.0.zip", "x-1.0", "x-1.0.1", "x-1.0.tar.gz"]),
            ["x-1.0", "x-1.0.tar.gz", "x-1.0.zip", "x-1.0.1"]
        );
    }

    #[test]
    fn prefix_len_finds_suffix_start() {
        assert_eq!(prefix_len(b"a.b.c"), 1);
        assert_eq!(prefix_len(b"a.1"), 3);
        assert_eq!(prefix_len(b"plain"), 5);
    }
}
