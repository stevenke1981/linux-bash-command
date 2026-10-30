//! Side-by-side format (`-y`): the left file in the left column and the right file in the
//! right column, joined by a gutter character. Columns follow GNU's layout: text is padded
//! with tabs to 8-column stops where possible, and the gutter sits at `gutter` columns.

use super::hunks::{Change, Diffed};
use super::Options;

const TAB: usize = 8;

/// Column positions for one output width.
#[derive(Debug, PartialEq, Eq)]
struct Layout {
    /// Column of the gutter character on changed lines.
    gutter: usize,
    /// Column where the right-hand text starts.
    right: usize,
    /// Largest number of bytes of text shown in either column.
    text: usize,
}

impl Layout {
    fn new(width: usize) -> Self {
        let half = width.saturating_sub(1) / 2;
        let right = round_to_tab(half + 2);
        let gutter = half.min(right.saturating_sub(2));
        let text = gutter.saturating_sub(1).min(width.saturating_sub(right));
        Self {
            gutter,
            right,
            text,
        }
    }
}

/// Rounds to the nearest multiple of [`TAB`], with halves rounding up.
fn round_to_tab(value: usize) -> usize {
    (value + TAB / 2) / TAB * TAB
}

/// The side-by-side rendering of every row, including the common rows between changes.
pub fn render(diffed: &Diffed<'_>, options: &Options) -> Vec<u8> {
    let layout = Layout::new(options.width);
    let mut out = Vec::new();
    let (mut old_index, mut new_index) = (0, 0);
    for change in &diffed.changes {
        while old_index < change.old.start {
            let row = Row::common(diffed, old_index, new_index);
            emit(&mut out, &layout, options, row);
            old_index += 1;
            new_index += 1;
        }
        emit_change(&mut out, &layout, options, diffed, change);
        old_index = change.old.end;
        new_index = change.new.end;
    }
    while old_index < diffed.old.len() && new_index < diffed.new.len() {
        let row = Row::common(diffed, old_index, new_index);
        emit(&mut out, &layout, options, row);
        old_index += 1;
        new_index += 1;
    }
    out
}

/// Rows of a change: pairs of lines first, then the unmatched lines of the longer side.
/// A blank-only change (hidden by `-B`) pairs its lines as common rows and shows no extras.
fn emit_change(
    out: &mut Vec<u8>,
    layout: &Layout,
    options: &Options,
    diffed: &Diffed<'_>,
    change: &Change,
) {
    let pairs = change.old.len().min(change.new.len());
    for offset in 0..pairs {
        let (old, new) = (change.old.start + offset, change.new.start + offset);
        let row = if change.silent {
            Row::common(diffed, old, new)
        } else {
            Row::changed(diffed.old[old], diffed.new[new])
        };
        emit(out, layout, options, row);
    }
    if change.silent {
        return;
    }
    for old in change.old.start + pairs..change.old.end {
        emit(out, layout, options, Row::deleted(diffed.old[old]));
    }
    for new in change.new.start + pairs..change.new.end {
        emit(out, layout, options, Row::added(diffed.new[new]));
    }
}

/// One output row: the left line, the right line, and how they relate.
struct Row<'a> {
    left: Option<&'a [u8]>,
    right: Option<&'a [u8]>,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Common,
    Changed,
    Deleted,
    Added,
}

impl<'a> Row<'a> {
    fn common(diffed: &Diffed<'a>, old: usize, new: usize) -> Self {
        Self {
            left: Some(diffed.old[old]),
            right: Some(diffed.new[new]),
            kind: Kind::Common,
        }
    }

    fn changed(left: &'a [u8], right: &'a [u8]) -> Self {
        Self {
            left: Some(left),
            right: Some(right),
            kind: Kind::Changed,
        }
    }

    fn deleted(left: &'a [u8]) -> Self {
        Self {
            left: Some(left),
            right: None,
            kind: Kind::Deleted,
        }
    }

    fn added(right: &'a [u8]) -> Self {
        Self {
            left: None,
            right: Some(right),
            kind: Kind::Added,
        }
    }
}

fn emit(out: &mut Vec<u8>, layout: &Layout, options: &Options, row: Row<'_>) {
    if row.kind == Kind::Common && options.suppress_common {
        return;
    }
    let mut col = 0;
    match row.kind {
        Kind::Common if options.left_column => {
            push_text(out, &mut col, clip(row.left, layout.text));
            pad_to(out, &mut col, layout.gutter);
            out.push(b'(');
        }
        Kind::Common => {
            push_text(out, &mut col, clip(row.left, layout.text));
            let right = clip(row.right, layout.text);
            if !right.is_empty() {
                pad_to(out, &mut col, layout.right);
                push_text(out, &mut col, right);
            }
        }
        Kind::Changed => {
            push_text(out, &mut col, clip(row.left, layout.text));
            pad_to(out, &mut col, layout.gutter);
            out.push(changed_gutter(row.left, row.right));
            push_right(out, &mut col, clip(row.right, layout.text));
        }
        Kind::Deleted => {
            push_text(out, &mut col, clip(row.left, layout.text));
            pad_to(out, &mut col, layout.gutter);
            out.push(b'<');
        }
        Kind::Added => {
            pad_to(out, &mut col, layout.gutter);
            out.push(b'>');
            push_right(out, &mut col, clip(row.right, layout.text));
        }
    }
    if row_has_newline(&row) {
        out.push(b'\n');
    }
}

/// The gutter of a changed row. A side without its final newline is marked by `\` (left)
/// or `/` (right); when neither side ends in a newline the row keeps the plain `|`.
fn changed_gutter(left: Option<&[u8]>, right: Option<&[u8]>) -> u8 {
    let left_has = left.is_some_and(|line| line.ends_with(b"\n"));
    let right_has = right.is_some_and(|line| line.ends_with(b"\n"));
    match (left_has, right_has) {
        (false, true) => b'\\',
        (true, false) => b'/',
        _ => b'|',
    }
}

/// A row ends with a newline unless every side it shows lacks one.
fn row_has_newline(row: &Row<'_>) -> bool {
    let lacks = |line: Option<&[u8]>| !line.is_some_and(|line| line.ends_with(b"\n"));
    !(lacks(row.left) && lacks(row.right))
}

/// The line without its newline, cut before the first character that would pass `limit`
/// columns. A tab counts up to its next stop, so a tab that does not fit is dropped.
fn clip(line: Option<&[u8]>, limit: usize) -> &[u8] {
    let body = line.unwrap_or_default();
    let body = body.strip_suffix(b"\n").unwrap_or(body);
    let mut col = 0;
    for (index, &byte) in body.iter().enumerate() {
        let next = if byte == b'\t' {
            next_stop(col)
        } else {
            col + 1
        };
        if next > limit {
            return &body[..index];
        }
        col = next;
    }
    body
}

fn push_text(out: &mut Vec<u8>, col: &mut usize, text: &[u8]) {
    for &byte in text {
        if byte == b'\t' {
            *col = next_stop(*col);
        } else {
            *col += 1;
        }
        out.push(byte);
    }
}

/// The right-hand text after a gutter: a tab to the right column, then the text. An empty
/// text ends the row at the gutter, as GNU does.
fn push_right(out: &mut Vec<u8>, col: &mut usize, text: &[u8]) {
    if !text.is_empty() {
        push_tab(out, col);
        push_text(out, col, text);
    }
}

fn push_tab(out: &mut Vec<u8>, col: &mut usize) {
    out.push(b'\t');
    *col = next_stop(*col);
}

fn next_stop(col: usize) -> usize {
    (col / TAB + 1) * TAB
}

/// Pads to `target`: tabs while a tab stop does not pass the target, then spaces.
fn pad_to(out: &mut Vec<u8>, col: &mut usize, target: usize) {
    while next_stop(*col) <= target {
        push_tab(out, col);
    }
    while *col < target {
        out.push(b' ');
        *col += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::super::hunks::diff_lines;
    use super::super::Options;
    use super::{render, Layout};

    fn side(left: &[u8], right: &[u8], width: usize) -> String {
        let options = Options {
            width,
            ..Options::default()
        };
        let diffed = diff_lines(left, right, &options);
        String::from_utf8(render(&diffed, &options)).unwrap()
    }

    #[test]
    fn layout_matches_the_gnu_columns_for_common_widths() {
        assert_eq!(
            Layout::new(130),
            Layout {
                gutter: 62,
                right: 64,
                text: 61
            }
        );
        assert_eq!(
            Layout::new(40),
            Layout {
                gutter: 19,
                right: 24,
                text: 16
            }
        );
        assert_eq!(
            Layout::new(30),
            Layout {
                gutter: 14,
                right: 16,
                text: 13
            }
        );
    }

    #[test]
    fn changed_line_has_gutter_and_tab_before_right_text() {
        assert_eq!(side(b"b\n", b"x\n", 130), "b\t\t\t\t\t\t\t      |\tx\n");
    }

    #[test]
    fn common_line_pads_to_the_right_column() {
        assert_eq!(side(b"a\n", b"a\n", 40), "a\t\t\ta\n");
    }

    #[test]
    fn insertion_and_deletion_use_one_sided_gutters() {
        assert_eq!(side(b"a\n", b"a\nX\n", 40), "a\t\t\ta\n\t\t   >\tX\n");
        assert_eq!(side(b"a\nX\n", b"a\n", 40), "a\t\t\ta\nX\t\t   <\n");
    }

    #[test]
    fn missing_final_newline_changes_the_gutter() {
        assert_eq!(side(b"q", b"r\n", 40), "q\t\t   \\\tr\n");
        assert_eq!(side(b"q\n", b"r", 40), "q\t\t   /\tr\n");
    }

    #[test]
    fn tab_that_passes_the_column_limit_is_dropped() {
        assert_eq!(side(b"a\tb\n", b"a\tX\n", 10), "a   |\ta\n");
    }

    #[test]
    fn long_text_is_cut_to_the_column() {
        assert_eq!(
            side(b"abcdefghij klmnopqrstuvwxyz\n", b"x\n", 30),
            "abcdefghij kl |\tx\n"
        );
    }
}
