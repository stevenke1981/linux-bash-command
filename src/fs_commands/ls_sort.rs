//! Ordering of `ls` entries: `--sort`, `-t`, `-S`, `-X`, `-v`, `-U`, `-r` and
//! `--group-directories-first`.

use super::ls::Entry;
use super::ls_options::{LsOptions, SortKey};
use super::ls_time::stamp;
use std::cmp::Ordering;

/// Sorts `entries` as `options` ask. Ties fall back to the name, except in directory order.
pub fn sort_entries(entries: &mut [Entry], options: &LsOptions) {
    entries.sort_by(|left, right| compare(left, right, options));
    if options.reverse {
        entries.reverse();
    }
    if options.group_dirs_first {
        entries.sort_by_key(|entry| !entry.is_dir());
    }
}

fn compare(left: &Entry, right: &Entry, options: &LsOptions) -> Ordering {
    let by_name = || left.name.cmp(&right.name);
    match options.sort {
        SortKey::Name => by_name(),
        SortKey::Natural => natural_cmp(&left.name, &right.name),
        SortKey::Time => stamp(&right.meta, options.time)
            .cmp(&stamp(&left.meta, options.time))
            .then_with(by_name),
        SortKey::Size => right.meta.len().cmp(&left.meta.len()).then_with(by_name),
        SortKey::Extension => extension(&left.name)
            .cmp(extension(&right.name))
            .then_with(by_name),
        SortKey::Width => left.name.len().cmp(&right.name.len()).then_with(by_name),
        SortKey::Unsorted => Ordering::Equal,
    }
}

/// The extension as GNU compares it: from the last `.` (included), or empty.
fn extension(name: &str) -> &str {
    name.rfind('.').map_or("", |index| &name[index..])
}

pub fn natural_cmp(left: &str, right: &str) -> Ordering {
    let (mut left, mut right) = (left.as_bytes(), right.as_bytes());
    loop {
        match (left.first(), right.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let (left_digits, left_rest) = split_digits(left);
                let (right_digits, right_rest) = split_digits(right);
                match compare_numbers(left_digits, right_digits) {
                    Ordering::Equal => {}
                    other => return other,
                }
                left = left_rest;
                right = right_rest;
            }
            (Some(a), Some(b)) => {
                match a.cmp(b) {
                    Ordering::Equal => {}
                    other => return other,
                }
                left = &left[1..];
                right = &right[1..];
            }
        }
    }
}

fn split_digits(text: &[u8]) -> (&[u8], &[u8]) {
    let end = text
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

fn compare_numbers(left: &[u8], right: &[u8]) -> Ordering {
    let (left, right) = (trim_leading_zeros(left), trim_leading_zeros(right));
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn trim_leading_zeros(digits: &[u8]) -> &[u8] {
    let start = digits
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(digits.len());
    &digits[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_numbers_by_value() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("file010", "file10"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "b"), Ordering::Less);
    }

    #[test]
    fn extensions_include_the_dot() {
        assert_eq!(extension("a.txt"), ".txt");
        assert_eq!(extension("README"), "");
        assert_eq!(extension(".hidden"), ".hidden");
        assert_eq!(extension("x.tar.gz"), ".gz");
    }
}
