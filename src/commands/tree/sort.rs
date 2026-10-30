//! Ordering of the entries in each directory: by name (byte order, as in the C locale), by
//! version (natural order of digit runs), by size (largest first) or time (oldest first, as
//! tree 2.x does), with ties by name. Optionally reversed, and optionally with directories or
//! files grouped first.

use std::cmp::Ordering;

use super::model::Node;

/// The key that orders entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Name,
    Version,
    Size,
    Mtime,
    Ctime,
    /// `-U`: keep the order the directory was read in.
    Unsorted,
}

/// Full ordering settings for one run.
#[derive(Clone, Copy, Debug)]
pub struct Order {
    pub key: Key,
    pub reverse: bool,
    pub dirs_first: bool,
    pub files_first: bool,
}

impl Order {
    /// Sorts `nodes` in place. Reversal happens before grouping, so `-r` reverses within
    /// each group.
    pub fn apply(&self, nodes: &mut [Node]) {
        match self.key {
            Key::Unsorted => {}
            Key::Name => nodes.sort_by(name_order),
            Key::Version => nodes.sort_by(|a, b| natural_cmp(a.name.as_bytes(), b.name.as_bytes())),
            Key::Size => {
                nodes.sort_by(|a, b| b.stat.size.cmp(&a.stat.size).then_with(|| name_order(a, b)))
            }
            Key::Mtime => nodes.sort_by(|a, b| {
                a.stat
                    .mtime
                    .cmp(&b.stat.mtime)
                    .then_with(|| name_order(a, b))
            }),
            Key::Ctime => nodes.sort_by(|a, b| {
                a.stat
                    .ctime
                    .cmp(&b.stat.ctime)
                    .then_with(|| name_order(a, b))
            }),
        }
        if self.reverse {
            nodes.reverse();
        }
        if self.dirs_first {
            nodes.sort_by_key(|node| !node.is_dir());
        } else if self.files_first {
            nodes.sort_by_key(Node::is_dir);
        }
    }
}

fn name_order(a: &Node, b: &Node) -> Ordering {
    a.name.as_bytes().cmp(b.name.as_bytes())
}

/// Compares two names so that digit runs compare by value: `file2` sorts before `file10`.
pub fn natural_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (end_a, end_b) = (digit_end(a, i), digit_end(b, j));
            let (run_a, run_b) = (strip_zeros(&a[i..end_a]), strip_zeros(&b[j..end_b]));
            let order = run_a.len().cmp(&run_b.len()).then_with(|| run_a.cmp(run_b));
            if order != Ordering::Equal {
                return order;
            }
            i = end_a;
            j = end_b;
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

fn digit_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|b| !b.is_ascii_digit())
        .map_or(bytes.len(), |offset| start + offset)
}

fn strip_zeros(digits: &[u8]) -> &[u8] {
    let first = digits
        .iter()
        .position(|&b| b != b'0')
        .unwrap_or(digits.len());
    &digits[first..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_digit_runs_by_value() {
        assert_eq!(natural_cmp(b"file2", b"file10"), Ordering::Less);
        assert_eq!(natural_cmp(b"file10", b"file2"), Ordering::Greater);
        assert_eq!(natural_cmp(b"a01", b"a1"), Ordering::Equal);
        assert_eq!(natural_cmp(b"abc", b"abd"), Ordering::Less);
        assert_eq!(natural_cmp(b"ab", b"abc"), Ordering::Less);
    }

    #[test]
    fn byte_order_puts_uppercase_first() {
        assert_eq!("B_upper".as_bytes().cmp("a.d".as_bytes()), Ordering::Less);
    }
}
