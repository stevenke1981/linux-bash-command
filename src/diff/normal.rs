//! Normal format, the default: `NcM`, `NaM` and `NdM` headers followed by `<` lines (from the
//! left file), a `---` separator when both sides have lines, and `>` lines (from the right).

use std::ops::Range;

use super::hunks::{push_line, Diffed};

/// Renders every visible change. The caller only calls this when a change is visible.
pub fn render(diffed: &Diffed<'_>) -> Vec<u8> {
    let mut text = Vec::new();
    for change in diffed.changes.iter().filter(|change| !change.silent) {
        let (old, new) = (&change.old, &change.new);
        let header = match (old.is_empty(), new.is_empty()) {
            // The number before `a` and `d` is the line the change follows on the other side.
            (true, _) => format!("{}a{}", old.start, span(new)),
            (_, true) => format!("{}d{}", span(old), new.start),
            _ => format!("{}c{}", span(old), span(new)),
        };
        text.extend_from_slice(header.as_bytes());
        text.push(b'\n');
        for line in &diffed.old[old.clone()] {
            push_line(&mut text, b"< ", line);
        }
        if !old.is_empty() && !new.is_empty() {
            text.extend_from_slice(b"---\n");
        }
        for line in &diffed.new[new.clone()] {
            push_line(&mut text, b"> ", line);
        }
    }
    text
}

/// A 1-based line range: `N` for one line, `N,M` otherwise.
fn span(range: &Range<usize>) -> String {
    let first = range.start + 1;
    if range.len() == 1 {
        first.to_string()
    } else {
        format!("{first},{}", range.end)
    }
}

#[cfg(test)]
mod tests {
    use super::super::hunks::diff_lines;
    use super::super::Options;
    use super::render;

    fn normal(left: &[u8], right: &[u8]) -> String {
        let diffed = diff_lines(left, right, &Options::default());
        String::from_utf8(render(&diffed)).unwrap()
    }

    #[test]
    fn change_prints_both_sides_with_separator() {
        assert_eq!(normal(b"a\nb\nc\n", b"a\nx\nc\n"), "2c2\n< b\n---\n> x\n");
    }

    #[test]
    fn insertion_and_deletion_use_the_neighbouring_line_numbers() {
        assert_eq!(normal(b"a\nb\n", b"a\nX\nb\n"), "1a2\n> X\n");
        assert_eq!(normal(b"a\nX\nb\n", b"a\nb\n"), "2d1\n< X\n");
    }

    #[test]
    fn multi_line_ranges_are_written_as_pairs() {
        assert_eq!(
            normal(b"a\nb\nc\n", b"x\ny\nz\n"),
            "1,3c1,3\n< a\n< b\n< c\n---\n> x\n> y\n> z\n"
        );
        assert_eq!(normal(b"a\nb\n", b"a\n"), "2d1\n< b\n");
    }

    #[test]
    fn missing_final_newline_is_marked_on_its_line() {
        assert_eq!(
            normal(b"p\nq", b"p\nr\n"),
            "2c2\n< q\n\\ No newline at end of file\n---\n> r\n"
        );
    }

    #[test]
    fn empty_left_side_is_an_insertion_at_the_top() {
        assert_eq!(normal(b"", b"a\n"), "0a1\n> a\n");
    }
}
