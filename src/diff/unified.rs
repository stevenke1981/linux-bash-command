//! Unified format (`-u`, `-U N`): `---`/`+++` headers, `@@` hunk headers, and lines marked
//! with a space (context), `-` (removed) or `+` (added).

use super::hunks::{group_hunks, push_line, Change, Diffed, Hunk};

/// The unified diff, or None when the texts are equal under the ignore options.
pub fn render(names: &(String, String), diffed: &Diffed<'_>, context: usize) -> Option<Vec<u8>> {
    let hunks = group_hunks(&diffed.changes, context, diffed.old.len(), diffed.new.len());
    if hunks.is_empty() {
        return None;
    }
    let mut text = format!("--- {}\n+++ {}\n", names.0, names.1).into_bytes();
    for hunk in &hunks {
        text.extend(render_hunk(hunk, &diffed.changes, &diffed.old, &diffed.new));
    }
    Some(text)
}

fn render_hunk(
    hunk: &Hunk,
    changes: &[Change],
    old_lines: &[&[u8]],
    new_lines: &[&[u8]],
) -> Vec<u8> {
    let mut body = Vec::new();
    let (mut old_count, mut new_count) = (0, 0);
    let mut position = hunk.old_start;
    for change in &changes[hunk.changes.clone()] {
        let unchanged = &old_lines[position..change.old.start];
        for line in unchanged {
            push_line(&mut body, b" ", line);
        }
        old_count += unchanged.len();
        new_count += unchanged.len();
        position = change.old.end;
        if change.silent {
            continue;
        }
        for line in &old_lines[change.old.clone()] {
            push_line(&mut body, b"-", line);
        }
        for line in &new_lines[change.new.clone()] {
            push_line(&mut body, b"+", line);
        }
        old_count += change.old.len();
        new_count += change.new.len();
    }
    let trailing = &old_lines[position..hunk.old_end];
    for line in trailing {
        push_line(&mut body, b" ", line);
    }
    old_count += trailing.len();
    new_count += trailing.len();
    let mut text = format!(
        "@@ -{} +{} @@\n",
        range_text(hunk.old_start, old_count),
        range_text(hunk.new_start, new_count)
    )
    .into_bytes();
    text.extend(body);
    text
}

fn range_text(start: usize, count: usize) -> String {
    match count {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        _ => format!("{},{count}", start + 1),
    }
}
