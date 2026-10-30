//! Context format (`-c`, `-C N`): a `***` header for the left file and a `---` header for
//! the right one, then one `***************` block per hunk. Each side of a hunk lists its lines
//! with `!` (changed), `-` or `+` marks; a side with no changes lists no lines.

use super::hunks::{group_hunks, push_line, Change, Diffed, Hunk};

/// The context diff, or None when the texts are equal under the ignore options.
pub fn render(
    names: &(String, String),
    times: &(String, String),
    diffed: &Diffed<'_>,
    context: usize,
) -> Option<Vec<u8>> {
    let hunks = group_hunks(&diffed.changes, context, diffed.old.len(), diffed.new.len());
    if hunks.is_empty() {
        return None;
    }
    let mut text = format!(
        "*** {}{}\n--- {}{}\n",
        names.0,
        tab_time(&times.0),
        names.1,
        tab_time(&times.1)
    )
    .into_bytes();
    for hunk in &hunks {
        text.extend_from_slice(b"***************\n");
        render_hunk(&mut text, hunk, diffed);
    }
    Some(text)
}

/// The tab and time after a file name in a header; a labelled file has none.
fn tab_time(time: &str) -> String {
    if time.is_empty() {
        String::new()
    } else {
        format!("\t{time}")
    }
}

/// Which side of a hunk is being written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Old,
    New,
}

fn render_hunk(text: &mut Vec<u8>, hunk: &Hunk, diffed: &Diffed<'_>) {
    let changes = &diffed.changes[hunk.changes.clone()];
    let visible = || changes.iter().filter(|change| !change.silent);
    let shows_old = visible().any(|change| !change.old.is_empty());
    let shows_new = visible().any(|change| !change.new.is_empty());

    let old_lines = if shows_old {
        side_lines(hunk, diffed, Side::Old)
    } else {
        Vec::new()
    };
    let new_lines = if shows_new {
        side_lines(hunk, diffed, Side::New)
    } else {
        Vec::new()
    };
    text.extend_from_slice(
        format!(
            "*** {} ****\n",
            range(hunk.old_start, hunk.old_end - hunk.old_start)
        )
        .as_bytes(),
    );
    for (prefix, line) in &old_lines {
        push_line(text, prefix, line);
    }
    text.extend_from_slice(
        format!(
            "--- {} ----\n",
            range(hunk.new_start, hunk.new_end - hunk.new_start)
        )
        .as_bytes(),
    );
    for (prefix, line) in &new_lines {
        push_line(text, prefix, line);
    }
}

/// The lines of one side of a hunk with their marks, in file order.
fn side_lines<'a>(hunk: &Hunk, diffed: &Diffed<'a>, side: Side) -> Vec<(&'static [u8], &'a [u8])> {
    let (lines, end) = match side {
        Side::Old => (&diffed.old, hunk.old_end),
        Side::New => (&diffed.new, hunk.new_end),
    };
    let mut out = Vec::new();
    let mut position = match side {
        Side::Old => hunk.old_start,
        Side::New => hunk.new_start,
    };
    for change in &diffed.changes[hunk.changes.clone()] {
        let block = block_of(change, side);
        for line in &lines[position..block.start] {
            out.push((b"  ".as_slice(), *line));
        }
        position = block.end;
        if change.silent {
            continue;
        }
        let mark: &'static [u8] = match (block.is_empty(), other_block(change, side).is_empty()) {
            (true, _) => continue,
            (false, false) => b"! ",
            (false, true) => match side {
                Side::Old => b"- ",
                Side::New => b"+ ",
            },
        };
        for line in &lines[block.clone()] {
            out.push((mark, *line));
        }
    }
    for line in &lines[position..end] {
        out.push((b"  ".as_slice(), *line));
    }
    out
}

fn block_of(change: &Change, side: Side) -> std::ops::Range<usize> {
    match side {
        Side::Old => change.old.clone(),
        Side::New => change.new.clone(),
    }
}

fn other_block(change: &Change, side: Side) -> std::ops::Range<usize> {
    match side {
        Side::Old => change.new.clone(),
        Side::New => change.old.clone(),
    }
}

/// A hunk range as GNU prints it: `N` for one line, `N,M` for several, and the line before
/// the change (`N`, as a count of zero) when the side has no lines.
fn range(start: usize, count: usize) -> String {
    match count {
        0 => format!("{start}"),
        1 => format!("{}", start + 1),
        _ => format!("{},{}", start + 1, start + count),
    }
}

#[cfg(test)]
mod tests {
    use super::super::hunks::diff_lines;
    use super::super::Options;
    use super::{range, render};

    fn context(left: &[u8], right: &[u8], lines: usize) -> String {
        let diffed = diff_lines(left, right, &Options::default());
        let names = ("a".to_string(), "b".to_string());
        let times = ("T1".to_string(), "T2".to_string());
        String::from_utf8(render(&names, &times, &diffed, lines).unwrap()).unwrap()
    }

    #[test]
    fn change_lists_both_sides_with_bang_marks() {
        assert_eq!(
            context(b"a\nb\nc\n", b"a\nx\nc\n", 1),
            "*** a\tT1\n--- b\tT2\n***************\n*** 1,3 ****\n  a\n! b\n  c\n--- 1,3 ----\n  a\n! x\n  c\n"
        );
    }

    #[test]
    fn pure_insertion_lists_no_lines_on_the_left() {
        assert_eq!(
            context(b"a\nb\n", b"a\nX\nb\n", 1),
            "*** a\tT1\n--- b\tT2\n***************\n*** 1,2 ****\n--- 1,3 ----\n  a\n+ X\n  b\n"
        );
    }

    #[test]
    fn range_text_matches_gnu_for_zero_one_and_many() {
        assert_eq!(range(0, 0), "0");
        assert_eq!(range(3, 0), "3");
        assert_eq!(range(0, 1), "1");
        assert_eq!(range(0, 3), "1,3");
    }
}
