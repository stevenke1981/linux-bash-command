use super::{Change, Hunk, Options};
use crate::common::color::paint_bytes;
use std::ops::Range;

fn span(range: &Range<usize>) -> String {
    match range.len() {
        0 => range.start.to_string(),
        1 => (range.start + 1).to_string(),
        _ => format!("{},{}", range.start + 1, range.end),
    }
}
fn line(out: &mut Vec<u8>, prefix: &[u8], data: &[u8], colored: bool) {
    let mut row = prefix.to_vec();
    row.extend_from_slice(data.strip_suffix(b"\n").unwrap_or(data));
    let color = match prefix.first() {
        Some(b'<') | Some(b'-') => "31",
        Some(b'>') | Some(b'+') => "32",
        Some(b'!') => "36",
        _ => "",
    };
    paint_bytes(out, colored, color, &row);
    out.push(b'\n');
    if !data.ends_with(b"\n") {
        out.extend_from_slice(b"\\ No newline at end of file\n");
    }
}
pub(super) fn normal(changes: &[Change], old: &[&[u8]], new: &[&[u8]], colored: bool) -> Vec<u8> {
    let mut out = vec![];
    for c in changes.iter().filter(|c| !c.silent) {
        let operation = if c.old.is_empty() {
            'a'
        } else if c.new.is_empty() {
            'd'
        } else {
            'c'
        };
        out.extend_from_slice(format!("{}{operation}{}\n", span(&c.old), span(&c.new)).as_bytes());
        for data in &old[c.old.clone()] {
            line(&mut out, b"< ", data, colored);
        }
        if operation == 'c' {
            out.extend_from_slice(b"---\n");
        }
        for data in &new[c.new.clone()] {
            line(&mut out, b"> ", data, colored);
        }
    }
    out
}
fn context_section(
    out: &mut Vec<u8>,
    lines: &[&[u8]],
    range: Range<usize>,
    changes: &[Change],
    old: bool,
    colored: bool,
) {
    for (index, data) in lines.iter().enumerate().take(range.end).skip(range.start) {
        let change = changes.iter().find(|c| {
            !c.silent
                && if old {
                    c.old.contains(&index)
                } else {
                    c.new.contains(&index)
                }
        });
        let prefix = match change {
            None => b"  ",
            Some(c) if c.old.is_empty() => b"+ ",
            Some(c) if c.new.is_empty() => b"- ",
            Some(_) => b"! ",
        };
        line(out, prefix, data, colored);
    }
}
pub(super) fn context(
    hunks: &[Hunk],
    changes: &[Change],
    old: &[&[u8]],
    new: &[&[u8]],
    names: &(String, String),
    o: &Options,
) -> Vec<u8> {
    let mut out = format!("*** {}\n--- {}\n", names.0, names.1).into_bytes();
    for h in hunks {
        let changes = &changes[h.changes.clone()];
        let old_range = h.old_start..h.old_end;
        let new_len = old_range.len() + changes.iter().map(|c| c.new.len()).sum::<usize>()
            - changes.iter().map(|c| c.old.len()).sum::<usize>();
        let new_range = h.new_start..h.new_start + new_len;
        out.extend_from_slice(
            format!("***************\n*** {} ****\n", span(&old_range)).as_bytes(),
        );
        if changes.iter().any(|c| !c.silent && !c.old.is_empty()) {
            context_section(&mut out, old, old_range, changes, true, o.colored);
        }
        out.extend_from_slice(format!("--- {} ----\n", span(&new_range)).as_bytes());
        if changes.iter().any(|c| !c.silent && !c.new.is_empty()) {
            context_section(&mut out, new, new_range, changes, false, o.colored);
        }
    }
    out
}
fn pad(out: &mut Vec<u8>, mut at: usize, target: usize) {
    while at < target {
        let next = (at / 8 + 1) * 8;
        if next <= target {
            out.push(b'\t');
            at = next;
        } else {
            out.push(b' ');
            at += 1;
        }
    }
}
fn clip(data: &[u8], limit: usize) -> (Vec<u8>, usize) {
    let mut out = vec![];
    let mut width = 0;
    for b in data.strip_suffix(b"\n").unwrap_or(data) {
        let next = if *b == b'\t' {
            (width / 8 + 1) * 8
        } else {
            width + 1
        };
        if next > limit {
            break;
        }
        out.push(*b);
        width = next;
    }
    (out, width)
}
fn side_line(
    out: &mut Vec<u8>,
    left: Option<&[u8]>,
    right: Option<&[u8]>,
    marker: u8,
    width: usize,
    colored: bool,
) {
    let right_start = ((width.saturating_add(11) / 16) * 8).min(width);
    let cell = right_start
        .saturating_sub(3)
        .min(width.saturating_sub(right_start));
    let gutter = (cell + right_start).saturating_sub(1) / 2;
    let (a, used) = clip(left.unwrap_or_default(), cell);
    paint_bytes(out, colored, if marker == b' ' { "" } else { "31" }, &a);
    if marker != b' ' {
        pad(out, used, gutter);
        out.push(marker);
        if let Some(right) = right {
            pad(out, gutter + 1, right_start);
            let (b, _) = clip(right, cell);
            paint_bytes(out, colored, "32", &b);
        }
    } else if let Some(right) = right {
        pad(out, used, right_start);
        let (b, _) = clip(right, cell);
        out.extend_from_slice(&b);
    }
    out.push(b'\n');
}
pub(super) fn side(
    changes: &[Change],
    old: &[&[u8]],
    new: &[&[u8]],
    width: usize,
    colored: bool,
) -> Vec<u8> {
    let mut out = vec![];
    let (mut a, mut b) = (0, 0);
    for c in changes {
        while a < c.old.start && b < c.new.start {
            side_line(&mut out, Some(old[a]), Some(new[b]), b' ', width, colored);
            a += 1;
            b += 1;
        }
        if !c.silent {
            for i in 0..c.old.len().max(c.new.len()) {
                let left = old
                    .get(c.old.start + i)
                    .copied()
                    .filter(|_| i < c.old.len());
                let right = new
                    .get(c.new.start + i)
                    .copied()
                    .filter(|_| i < c.new.len());
                let marker = match (left, right) {
                    (Some(l), Some(r))
                        if l.strip_suffix(b"\n").unwrap_or(l)
                            == r.strip_suffix(b"\n").unwrap_or(r) =>
                    {
                        if l.ends_with(b"\n") {
                            b'/'
                        } else {
                            b'\\'
                        }
                    }
                    (Some(_), Some(_)) => b'|',
                    (Some(_), None) => b'<',
                    _ => b'>',
                };
                side_line(&mut out, left, right, marker, width, colored);
            }
        }
        a = c.old.end;
        b = c.new.end;
    }
    while a < old.len() && b < new.len() {
        side_line(&mut out, Some(old[a]), Some(new[b]), b' ', width, colored);
        a += 1;
        b += 1;
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_include_empty_insertion_points() {
        assert_eq!(span(&(0..0)), "0");
        assert_eq!(span(&(1..3)), "2,3");
    }
    #[test]
    fn normal_replacement_keeps_missing_newline_marker() {
        let c = Change {
            old: 0..1,
            new: 0..1,
            silent: false,
        };
        assert_eq!(
            normal(&[c], &[b"a"], &[b"b\n"], false),
            b"1c1\n< a\n\\ No newline at end of file\n---\n> b\n"
        );
    }
    #[test]
    fn side_gutter_matches_tab_stops() {
        let mut out = vec![];
        side_line(&mut out, Some(b"old\n"), Some(b"new\n"), b'|', 20, false);
        assert_eq!(out, b"old   |\tnew\n");
    }
}
