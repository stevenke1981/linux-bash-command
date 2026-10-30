//! Line comparison shared by every output format: splits the inputs into lines, computes
//! the changed runs with the Myers algorithm, and groups them into hunks with context.

use std::ops::Range;

use similar::{capture_diff_slices, Algorithm, DiffOp};

use super::Options;

/// Both inputs split into lines (each keeping its `\n`) and the runs that differ.
pub struct Diffed<'a> {
    pub old: Vec<&'a [u8]>,
    pub new: Vec<&'a [u8]>,
    pub changes: Vec<Change>,
}

/// A run of differing lines. `silent` changes are blank-only changes hidden by `-B`.
#[derive(Clone)]
pub struct Change {
    pub old: Range<usize>,
    pub new: Range<usize>,
    pub silent: bool,
}

/// A hunk covers the changes in `changes` (index range) plus context lines on both sides.
pub struct Hunk {
    pub changes: Range<usize>,
    pub old_start: usize,
    pub old_end: usize,
    pub new_start: usize,
    pub new_end: usize,
}

/// Splits and compares the two inputs under the ignore options in `options`.
pub fn diff_lines<'a>(left: &'a [u8], right: &'a [u8], options: &Options) -> Diffed<'a> {
    let old: Vec<&[u8]> = split_lines(left);
    let new: Vec<&[u8]> = split_lines(right);
    let old_keys: Vec<Vec<u8>> = old.iter().map(|line| line_key(line, options)).collect();
    let new_keys: Vec<Vec<u8>> = new.iter().map(|line| line_key(line, options)).collect();
    let ops = capture_diff_slices(Algorithm::Myers, &old_keys, &new_keys);
    let changes = collect_changes(&ops, &old, &new, options.ignore_blank_lines);
    Diffed { old, new, changes }
}

/// True when some change is not hidden by `-B`.
pub fn has_visible_change(changes: &[Change]) -> bool {
    changes.iter().any(|change| !change.silent)
}

/// Splits raw bytes into lines that keep their `\n`, without assuming any text encoding.
pub fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|byte| *byte == b'\n').collect()
}

/// ASCII whitespace as recognized by the C locale (`isspace`).
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r' | b'\n')
}

/// Normalizes one line for comparison. The trailing newline is kept as a marker so that a
/// missing final newline still counts as a difference.
fn line_key(line: &[u8], options: &Options) -> Vec<u8> {
    let mut text = line.strip_suffix(b"\n").unwrap_or(line);
    if options.strip_trailing_cr {
        text = text.strip_suffix(b"\r").unwrap_or(text);
    }
    let mut key = if options.ignore_all_space {
        text.iter()
            .copied()
            .filter(|byte| !is_space(*byte))
            .collect()
    } else if options.ignore_space_change {
        text.split(|byte| is_space(*byte))
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(&b' ')
    } else {
        text.to_vec()
    };
    if options.ignore_case {
        // ASCII-only folding, as GNU diff does under the C locale: other bytes compare exactly.
        key.make_ascii_lowercase();
    }
    key.push(if line.ends_with(b"\n") { b'\n' } else { 0 });
    key
}

fn collect_changes(
    ops: &[DiffOp],
    old_lines: &[&[u8]],
    new_lines: &[&[u8]],
    ignore_blank_lines: bool,
) -> Vec<Change> {
    let mut changes: Vec<Change> = Vec::new();
    for op in ops {
        let Some((old, new)) = op_ranges(op) else {
            continue;
        };
        // GNU -B hides only pure insertions and deletions of blank lines; a blank line
        // replaced by another blank line still shows.
        let silent = ignore_blank_lines
            && (old.is_empty() || new.is_empty())
            && is_blank_change(&old_lines[old.clone()], &new_lines[new.clone()]);
        match changes.last_mut() {
            Some(last)
                if last.silent == silent
                    && last.old.end == old.start
                    && last.new.end == new.start =>
            {
                last.old.end = old.end;
                last.new.end = new.end;
            }
            _ => changes.push(Change { old, new, silent }),
        }
    }
    changes
}

fn op_ranges(op: &DiffOp) -> Option<(Range<usize>, Range<usize>)> {
    match *op {
        DiffOp::Equal { .. } => None,
        DiffOp::Delete {
            old_index,
            old_len,
            new_index,
        } => Some((old_index..old_index + old_len, new_index..new_index)),
        DiffOp::Insert {
            old_index,
            new_index,
            new_len,
        } => Some((old_index..old_index, new_index..new_index + new_len)),
        DiffOp::Replace {
            old_index,
            old_len,
            new_index,
            new_len,
        } => Some((
            old_index..old_index + old_len,
            new_index..new_index + new_len,
        )),
    }
}

fn is_blank_change(old: &[&[u8]], new: &[&[u8]]) -> bool {
    old.iter()
        .chain(new.iter())
        .all(|line| line.iter().all(|byte| is_space(*byte)))
}

/// Groups visible changes into hunks. Context never reaches into a neighbouring change,
/// so hidden (`silent`) changes are never printed as context.
pub fn group_hunks(
    changes: &[Change],
    context: usize,
    old_len: usize,
    new_len: usize,
) -> Vec<Hunk> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut span_start: Option<usize> = None;
    let mut last_visible = 0;
    for (index, change) in changes.iter().enumerate() {
        if change.silent {
            continue;
        }
        match span_start {
            Some(start)
                if change
                    .old
                    .start
                    .saturating_sub(changes[last_visible].old.end)
                    > context.saturating_mul(2) =>
            {
                spans.push(start..last_visible + 1);
                span_start = Some(index);
            }
            Some(_) => {}
            None => span_start = Some(index),
        }
        last_visible = index;
    }
    if let Some(start) = span_start {
        spans.push(start..last_visible + 1);
    }
    spans
        .into_iter()
        .map(|span| hunk_for(changes, span, context, old_len, new_len))
        .collect()
}

fn hunk_for(
    changes: &[Change],
    span: Range<usize>,
    context: usize,
    old_len: usize,
    new_len: usize,
) -> Hunk {
    let first = &changes[span.start];
    let last = &changes[span.end - 1];
    let before = if span.start == 0 {
        (0, 0)
    } else {
        let previous = &changes[span.start - 1];
        (previous.old.end, previous.new.end)
    };
    let lead = context
        .min(first.old.start - before.0)
        .min(first.new.start - before.1);
    let old_start = first.old.start - lead;
    let new_start = first.new.start - lead;
    let after = changes
        .get(span.end)
        .map_or((old_len, new_len), |next| (next.old.start, next.new.start));
    let trail = context
        .min(after.0 - last.old.end)
        .min(after.1 - last.new.end);
    Hunk {
        changes: span,
        old_start,
        old_end: last.old.end + trail,
        new_start,
        new_end: last.new.end + trail,
    }
}

/// Appends one line after its prefix, and the GNU marker when the line has no newline.
pub fn push_line(text: &mut Vec<u8>, prefix: &[u8], line: &[u8]) {
    text.extend_from_slice(prefix);
    text.extend_from_slice(line);
    if !line.ends_with(b"\n") {
        text.extend_from_slice(b"\n\\ No newline at end of file\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Options {
        Options::default()
    }

    #[test]
    fn identical_inputs_have_no_changes() {
        let diffed = diff_lines(b"a\nb\n", b"a\nb\n", &options());
        assert!(diffed.changes.is_empty());
    }

    #[test]
    fn adjacent_changes_merge_into_one_run() {
        let diffed = diff_lines(b"a\nb\nc\n", b"x\ny\nc\n", &options());
        assert_eq!(diffed.changes.len(), 1);
        assert_eq!(diffed.changes[0].old, 0..2);
        assert_eq!(diffed.changes[0].new, 0..2);
    }

    #[test]
    fn hunks_clip_context_to_the_file_edges() {
        let diffed = diff_lines(b"1\n2\n3\n", b"1\nX\n3\n", &options());
        let hunks = group_hunks(&diffed.changes, 5, 3, 3);
        assert_eq!(hunks.len(), 1);
        assert_eq!((hunks[0].old_start, hunks[0].old_end), (0, 3));
        assert_eq!((hunks[0].new_start, hunks[0].new_end), (0, 3));
    }

    #[test]
    fn distant_changes_form_separate_hunks() {
        let left = b"1\n2\n3\n4\n5\n6\n7\n8\n";
        let right = b"X\n2\n3\n4\n5\n6\n7\nY\n";
        let diffed = diff_lines(left, right, &options());
        let hunks = group_hunks(&diffed.changes, 1, 8, 8);
        assert_eq!(hunks.len(), 2);
    }

    #[test]
    fn line_without_newline_gets_the_marker() {
        let mut out = Vec::new();
        push_line(&mut out, b"<", b"x");
        assert_eq!(out, b"<x\n\\ No newline at end of file\n");
    }
}
