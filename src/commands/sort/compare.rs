//! Line comparison for `sort`: per-key extraction and ordering, the random-key hash, and
//! the last-resort whole-line comparison that `-s` and `-u` switch off.

use std::borrow::Cow;
use std::cmp::Ordering;

use sha2::{Digest, Sha256};

use super::keys::{key_span, Key, Modes};
use super::numeric::{compare_general, compare_human, compare_month, compare_numeric};
use super::version::compare_version;

/// Everything needed to order two lines.
pub struct Comparer {
    pub keys: Vec<Key>,
    /// The `-t` field separator, if any.
    pub tab: Option<u8>,
    /// `-r` given globally: reverses the last-resort comparison too.
    pub global_reverse: bool,
    /// Neither `-s` nor `-u`: ties fall back to comparing whole lines.
    pub last_resort: bool,
    /// `-u`: equal keys mean equal lines (used by the unique filter and `-c`).
    pub unique: bool,
    /// Random seed for `-R`, mixed into every key hash.
    pub seed: Vec<u8>,
}

impl Comparer {
    /// Orders two lines: keys first, then the last-resort comparison when enabled.
    pub fn compare(&self, a: &[u8], b: &[u8]) -> Ordering {
        let diff = self.compare_keys(a, b);
        if diff != Ordering::Equal || !self.last_resort {
            return diff;
        }
        let whole = a.cmp(b);
        if self.global_reverse {
            whole.reverse()
        } else {
            whole
        }
    }

    /// Orders two lines by their keys only.
    pub fn compare_keys(&self, a: &[u8], b: &[u8]) -> Ordering {
        for key in &self.keys {
            let (sa, ea) = key_span(a, key, self.tab);
            let (sb, eb) = key_span(b, key, self.tab);
            let diff = compare_span(&a[sa..ea], &b[sb..eb], &key.modes, &self.seed);
            if diff != Ordering::Equal {
                return if key.modes.reverse {
                    diff.reverse()
                } else {
                    diff
                };
            }
        }
        Ordering::Equal
    }
}

/// Compares two key texts under one key's modes.
fn compare_span(a: &[u8], b: &[u8], modes: &Modes, seed: &[u8]) -> Ordering {
    if modes.numeric {
        compare_numeric(a, b)
    } else if modes.general {
        compare_general(a, b)
    } else if modes.human {
        compare_human(a, b)
    } else if modes.month {
        compare_month(a, b)
    } else if modes.random {
        random_hash(&normalize(a, modes), seed).cmp(&random_hash(&normalize(b, modes), seed))
    } else if modes.version {
        compare_version(&normalize(a, modes), &normalize(b, modes))
    } else {
        normalize(a, modes).cmp(&normalize(b, modes))
    }
}

/// Applies the text transformations of `-d`, `-i` and `-f`: dictionary order keeps only
/// letters, digits and blanks; `-i` keeps only printable bytes; `-f` folds to upper case.
pub fn normalize<'a>(text: &'a [u8], modes: &Modes) -> Cow<'a, [u8]> {
    if !(modes.dictionary || modes.nonprinting || modes.fold) {
        return Cow::Borrowed(text);
    }
    let kept: Vec<u8> = text
        .iter()
        .copied()
        .filter(|&b| {
            let dictionary_ok =
                !modes.dictionary || b.is_ascii_alphanumeric() || b == b' ' || b == b'\t';
            let printable_ok = !modes.nonprinting || (0x20..=0x7e).contains(&b);
            dictionary_ok && printable_ok
        })
        .map(|b| {
            if modes.fold {
                b.to_ascii_uppercase()
            } else {
                b
            }
        })
        .collect();
    Cow::Owned(kept)
}

/// SHA-256 of the seed followed by the key: equal keys hash equally, and the order of the
/// hashes is a fixed pseudo-random permutation of the keys.
fn random_hash(key: &[u8], seed: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(key);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

#[cfg(test)]
mod tests {
    use super::super::keys::{resolve_keys, KeyDef};
    use super::*;

    fn comparer(defs: &[KeyDef], global: Modes) -> Comparer {
        let keys = if defs.is_empty() {
            vec![Key {
                def: KeyDef {
                    start_field: 0,
                    start_char: 0,
                    end: None,
                    modes: global,
                },
                modes: global,
            }]
        } else {
            resolve_keys(defs, global)
        };
        Comparer {
            keys,
            tab: None,
            global_reverse: global.reverse,
            last_resort: true,
            unique: false,
            seed: b"seed".to_vec(),
        }
    }

    fn key_def(spec: &str) -> KeyDef {
        super::super::keys::parse_key(spec).unwrap()
    }

    #[test]
    fn plain_sort_uses_bytes_and_last_resort() {
        let c = comparer(&[], Modes::default());
        assert_eq!(c.compare(b"a", b"b"), Ordering::Less);
        assert_eq!(c.compare(b"B", b"a"), Ordering::Less);
    }

    #[test]
    fn fold_and_dictionary_filter_before_comparing() {
        let fold = Modes {
            fold: true,
            ..Modes::default()
        };
        assert_eq!(normalize(b"abc", &fold).as_ref(), b"ABC");
        let dict = Modes {
            dictionary: true,
            ..Modes::default()
        };
        assert_eq!(normalize(b"a-b c!", &dict).as_ref(), b"ab c");
        let nonprint = Modes {
            nonprinting: true,
            ..Modes::default()
        };
        assert_eq!(normalize(b"a\x01b", &nonprint).as_ref(), b"ab");
    }

    #[test]
    fn keys_order_before_last_resort() {
        let c = comparer(&[key_def("2,2")], Modes::default());
        assert_eq!(c.compare(b"z 1", b"a 2"), Ordering::Less);
        assert_eq!(c.compare(b"b 1", b"a 1"), Ordering::Greater);
    }

    #[test]
    fn global_reverse_flips_last_resort_only_when_global() {
        let global = Modes {
            reverse: true,
            ..Modes::default()
        };
        let c = comparer(&[], global);
        assert_eq!(c.compare(b"a", b"b"), Ordering::Greater);
        let mut keyed = comparer(&[key_def("1,1")], Modes::default());
        keyed.global_reverse = false;
        assert_eq!(keyed.compare(b"x 1", b"x 2"), Ordering::Less);
    }

    #[test]
    fn random_hash_groups_equal_keys() {
        let modes = Modes {
            random: true,
            ..Modes::default()
        };
        assert_eq!(
            compare_span(b"same", b"same", &modes, b"s"),
            Ordering::Equal
        );
        assert_ne!(random_hash(b"a", b"s"), random_hash(b"b", b"s"));
    }
}
