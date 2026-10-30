//! Hunk placement and application.
//!
//! A hunk is placed by searching outward from its expected line for the lines it
//! expects, first exactly, then with 1, 2, ... fuzz (ignoring leading and trailing context
//! lines). The expected line is the header line plus the offset of the previous hunk.
//! Hunks are matched against the original file and may not overlap, so a file is built
//! in one pass after all hunks are placed.

use std::borrow::Cow;

use super::parse::{Hunk, HunkLine, Kind};

/// One line of the target file. `newline` is false only for a final line without `\n`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FLine {
    pub text: Vec<u8>,
    pub newline: bool,
}

/// Matching options.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub max_fuzz: usize,
    pub ignore_ws: bool,
}

/// A hunk placed in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    /// 0-based index of the first matched line.
    pub pos: usize,
    /// Number of file lines the hunk replaces.
    pub old_len: usize,
    pub fuzz: usize,
    /// Distance from the header position, in lines.
    pub offset: isize,
    pub new_lines: Vec<FLine>,
}

/// Splits file bytes into lines. A final line without `\n` is kept with `newline` false.
pub fn split_file(data: &[u8]) -> Vec<FLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, &byte) in data.iter().enumerate() {
        if byte == b'\n' {
            lines.push(FLine {
                text: data[start..index].to_vec(),
                newline: true,
            });
            start = index + 1;
        }
    }
    if start < data.len() {
        lines.push(FLine {
            text: data[start..].to_vec(),
            newline: false,
        });
    }
    lines
}

/// Joins lines back into bytes. Every line but the last gets `\n`; the last gets one only
/// when its `newline` flag is set.
pub fn join_file(lines: &[FLine]) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        out.extend_from_slice(&line.text);
        if index + 1 < lines.len() || line.newline {
            out.push(b'\n');
        }
    }
    out
}

/// Comparison form of a line: a trailing CR is ignored (CRLF files match LF patches) and,
/// with `ignore_ws`, each run of white space becomes one space. Leading white space is
/// kept, as GNU patch does: `-l` ignores amounts of white space, not its presence.
pub fn normalize(text: &[u8], ignore_ws: bool) -> Cow<'_, [u8]> {
    let text = text.strip_suffix(b"\r").unwrap_or(text);
    if !ignore_ws {
        return Cow::Borrowed(text);
    }
    let mut out = Vec::with_capacity(text.len());
    let mut in_space = false;
    for &byte in text {
        if byte.is_ascii_whitespace() {
            if !in_space {
                out.push(b' ');
            }
            in_space = true;
        } else {
            out.push(byte);
            in_space = false;
        }
    }
    Cow::Owned(out)
}

/// The reversed hunk: additions and deletions swapped.
pub fn reverse_hunk(hunk: &Hunk) -> Hunk {
    let lines = hunk
        .lines
        .iter()
        .map(|line| HunkLine {
            kind: match line.kind {
                Kind::Context => Kind::Context,
                Kind::Delete => Kind::Add,
                Kind::Add => Kind::Delete,
            },
            text: line.text.clone(),
            newline: line.newline,
        })
        .collect();
    Hunk {
        old_start: hunk.new_start,
        old_count: hunk.new_count,
        new_start: hunk.old_start,
        new_count: hunk.old_count,
        header: hunk.header.clone(),
        lines,
    }
}

/// Places `hunk` in `file` at or after `floor`, searching outward from the expected
/// position `header_index + last_offset`. Returns the first match at the lowest fuzz.
pub fn place_hunk(
    file: &[FLine],
    hunk: &Hunk,
    floor: usize,
    last_offset: isize,
    opts: Options,
) -> Option<Placed> {
    let pattern: Vec<&HunkLine> = hunk.lines.iter().filter(|l| l.kind != Kind::Add).collect();
    let header_index = if hunk.old_count == 0 {
        hunk.old_start
    } else {
        hunk.old_start.saturating_sub(1)
    };
    let expected = (header_index as isize + last_offset).max(0) as usize;
    let lead_ctx = hunk
        .lines
        .iter()
        .take_while(|l| l.kind == Kind::Context)
        .count();
    let trail_ctx = hunk
        .lines
        .iter()
        .rev()
        .take_while(|l| l.kind == Kind::Context)
        .count();
    let shape = Shape {
        lead_ctx: lead_ctx.min(pattern.len()),
        trail_ctx: trail_ctx.min(pattern.len()),
    };
    for fuzz in 0..=opts.max_fuzz {
        let found = candidates(expected, file.len())
            .filter(|&pos| pos >= floor)
            .find(|&pos| matches_at(file, &pattern, shape, pos, fuzz, opts.ignore_ws));
        if let Some(pos) = found {
            let new_lines = build_lines(file, hunk, pos);
            return Some(Placed {
                pos,
                old_len: pattern.len(),
                fuzz,
                offset: pos as isize - header_index as isize,
                new_lines,
            });
        }
    }
    None
}

/// Prefix and suffix context counts of a hunk.
#[derive(Clone, Copy)]
struct Shape {
    lead_ctx: usize,
    trail_ctx: usize,
}

/// Start positions in search order: expected, expected+1, expected-1, ... within the file.
fn candidates(expected: usize, len: usize) -> impl Iterator<Item = usize> {
    let max_distance = expected.max(len.saturating_sub(expected));
    (0..=max_distance).flat_map(move |d| {
        let forward = (expected + d <= len).then(|| expected + d);
        let backward = (d > 0 && d <= expected).then(|| expected - d);
        forward.into_iter().chain(backward)
    })
}

/// Whether the pattern matches at `pos` with `fuzz`. A hunk whose prefix context exceeds
/// its suffix context needs `prefix - suffix` fuzz unless it ends at the end of the file.
fn matches_at(
    file: &[FLine],
    pattern: &[&HunkLine],
    shape: Shape,
    pos: usize,
    fuzz: usize,
    ignore_ws: bool,
) -> bool {
    let n = pattern.len();
    if pos + n > file.len() {
        return false;
    }
    let at_eof = pos + n == file.len();
    let base = if at_eof {
        0
    } else {
        shape.lead_ctx.saturating_sub(shape.trail_ctx)
    };
    if fuzz < base {
        return false;
    }
    let lead = fuzz.min(shape.lead_ctx).min(n);
    let trail = (fuzz - base).min(shape.trail_ctx).min(n - lead);
    (lead..n - trail).all(|i| {
        normalize(&file[pos + i].text, ignore_ws) == normalize(&pattern[i].text, ignore_ws)
    })
}

/// Builds the replacement lines for a hunk placed at `pos`. Context lines keep the
/// file's own text; added lines get a CR when the file uses CRLF.
fn build_lines(file: &[FLine], hunk: &Hunk, pos: usize) -> Vec<FLine> {
    let crlf = file.first().is_some_and(|l| l.text.ends_with(b"\r"));
    let mut out = Vec::new();
    let mut index = pos;
    for line in &hunk.lines {
        match line.kind {
            Kind::Context => {
                out.push(file[index].clone());
                index += 1;
            }
            Kind::Delete => index += 1,
            Kind::Add => {
                let mut text = line.text.clone();
                if crlf && !text.ends_with(b"\r") {
                    text.push(b'\r');
                }
                out.push(FLine {
                    text,
                    newline: line.newline,
                });
            }
        }
    }
    out
}

/// Builds the new file from the original lines and the placed hunks, which must be in
/// file order and not overlap.
pub fn assemble(file: &[FLine], placed: &[Placed]) -> Vec<FLine> {
    let mut out = Vec::with_capacity(file.len());
    let mut cursor = 0;
    for hunk in placed {
        out.extend_from_slice(&file[cursor..hunk.pos]);
        out.extend(hunk.new_lines.iter().cloned());
        cursor = hunk.pos + hunk.old_len;
    }
    out.extend_from_slice(&file[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::patch::parse::parse;

    const OPTS: Options = Options {
        max_fuzz: 2,
        ignore_ws: false,
    };

    fn hunk_of(diff: &str) -> Hunk {
        let mut text = String::from("--- f\n+++ f\n");
        text.push_str(diff);
        parse(text.as_bytes()).unwrap().remove(0).hunks.remove(0)
    }

    fn file_of(text: &str) -> Vec<FLine> {
        split_file(text.as_bytes())
    }

    fn apply_one(text: &str, diff: &str, last_offset: isize) -> Option<(Placed, String)> {
        let file = file_of(text);
        let hunk = hunk_of(diff);
        let placed = place_hunk(&file, &hunk, 0, last_offset, OPTS)?;
        let out = assemble(&file, std::slice::from_ref(&placed));
        Some((placed, String::from_utf8(join_file(&out)).unwrap()))
    }

    #[test]
    fn split_and_join_round_trip_with_missing_newline() {
        for data in [&b"a\nb\n"[..], b"a\nb", b"", b"\n", b"x\r\ny\r\n"] {
            assert_eq!(join_file(&split_file(data)), data);
        }
        assert!(!split_file(b"a\nb").last().unwrap().newline);
    }

    #[test]
    fn exact_match_places_at_header_line() {
        let (placed, out) = apply_one("one\ntwo\nthree\n", "@@ -2 +2 @@\n-two\n+TWO\n", 0).unwrap();
        assert_eq!((placed.pos, placed.fuzz, placed.offset), (1, 0, 0));
        assert_eq!(out, "one\nTWO\nthree\n");
    }

    #[test]
    fn offset_search_finds_moved_hunk() {
        let text = "p1\np2\np3\na\nb\nc\nd\ne\n";
        let (placed, out) = apply_one(text, "@@ -2,3 +2,3 @@\n b\n-c\n+C\n d\n", 0).unwrap();
        assert_eq!(placed.offset, 3);
        assert!(out.contains("b\nC\nd\n"));
    }

    #[test]
    fn fuzz_ignores_changed_context_lines() {
        let diff = "@@ -1,4 +1,4 @@\n one\n two\n-three\n+THREE\n four\n";
        let (placed, out) = apply_one("one\nTWO\nthree\nfour\n", diff, 0).unwrap();
        assert_eq!(placed.fuzz, 2);
        assert_eq!(out, "one\nTWO\nTHREE\nfour\n");
        let (placed, _) = apply_one("one\ntwo\nthree\nfour\n", diff, 0).unwrap();
        assert_eq!(placed.fuzz, 0);
    }

    #[test]
    fn no_match_returns_none() {
        assert!(apply_one("a\nb\n", "@@ -1 +1 @@\n-zzz\n+y\n", 0).is_none());
    }

    #[test]
    fn last_offset_moves_expected_position() {
        let text = "x\nx\nx\nmatch\n";
        let diff = "@@ -1 +1 @@\n-match\n+MATCH\n";
        let (placed, _) = apply_one(text, diff, 3).unwrap();
        assert_eq!(placed.pos, 3);
        assert_eq!(placed.offset, 3);
    }

    #[test]
    fn insertion_hunk_with_zero_count_inserts_after_header_line() {
        let (placed, out) = apply_one("a\nb\n", "@@ -1,0 +2 @@\n+ins\n", 0).unwrap();
        assert_eq!(placed.pos, 1);
        assert_eq!(out, "a\nins\nb\n");
        let (_, out) = apply_one("a\nb\n", "@@ -0,0 +1 @@\n+top\n", 0).unwrap();
        assert_eq!(out, "top\na\nb\n");
    }

    #[test]
    fn no_newline_at_end_is_preserved_and_crlf_is_kept() {
        let (_, out) = apply_one("a\nb", "@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n", 0).unwrap();
        assert_eq!(out, "a\nc");
        let (_, out) =
            apply_one("a\r\nb\r\nc\r\n", "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n", 0).unwrap();
        assert_eq!(out, "a\r\nB\r\nc\r\n");
    }

    #[test]
    fn reverse_hunk_swaps_sides() {
        let hunk = hunk_of("@@ -1,2 +1,2 @@\n a\n-b\n+c\n");
        let back = reverse_hunk(&hunk);
        assert_eq!((back.old_start, back.old_count), (1, 2));
        assert_eq!(back.lines[1].kind, Kind::Add);
        assert_eq!(back.lines[2].kind, Kind::Delete);
        assert_eq!(reverse_hunk(&back), hunk);
    }

    #[test]
    fn normalize_handles_cr_and_whitespace() {
        assert_eq!(&*normalize(b"x\r", false), b"x");
        assert_eq!(&*normalize(b"  a \t b  ", true), b" a b ");
        assert_eq!(&*normalize(b"a  b", true), &*normalize(b"a \tb", true));
        assert_ne!(&*normalize(b"a b", false), &*normalize(b"a  b", false));
    }

    #[test]
    fn asymmetric_context_needs_fuzz_unless_at_end() {
        let text = "one\ntwo\nthree\nfour\nfive\n";
        let diff = "@@ -2,2 +2,2 @@\n two\n-three\n+THREE\n";
        let (placed, _) = apply_one(text, diff, 0).unwrap();
        assert_eq!(placed.fuzz, 1);
    }
}
