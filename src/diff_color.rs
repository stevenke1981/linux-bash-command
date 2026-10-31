//! Terminal colors for unified diff output, using GNU diff's default palette.
//!
//! Every function takes `on`; when it is false the output is byte-for-byte the
//! plain diff, so piped output never contains escape sequences.
use crate::common::color::paint_bytes;

/// File headers (`---`/`+++`) and recursive `diff -r` command lines.
const BOLD: &str = "1";
/// `@@ -a,b +c,d @@` hunk headers.
const CYAN: &str = "36";
/// Lines removed from the left file.
const RED: &str = "31";
/// Lines added by the right file.
const GREEN: &str = "32";

/// Appends a bold file header or command line, with its newline.
pub(crate) fn header_line(out: &mut Vec<u8>, on: bool, text: &str) {
    push_text(out, on, BOLD, text);
}

/// Appends a hunk header line, with its newline.
pub(crate) fn hunk_header(out: &mut Vec<u8>, on: bool, text: &str) {
    push_text(out, on, CYAN, text);
}

/// Appends one hunk body line: `-`/`+` lines are colored, context lines stay plain.
/// `line` is raw bytes that normally end with `\n`; a final line without one gets
/// GNU's `\ No newline at end of file` marker, which is never colored.
pub(crate) fn body_line(out: &mut Vec<u8>, on: bool, prefix: u8, line: &[u8]) {
    let (content, has_newline) = match line.strip_suffix(b"\n") {
        Some(content) => (content, true),
        None => (line, false),
    };
    let mut text = Vec::with_capacity(content.len() + 1);
    text.push(prefix);
    text.extend_from_slice(content);
    let sgr = match prefix {
        b'-' => RED,
        b'+' => GREEN,
        _ => "",
    };
    paint_bytes(out, on, sgr, &text);
    if has_newline {
        out.push(b'\n');
    } else {
        out.extend_from_slice(b"\n\\ No newline at end of file\n");
    }
}

fn push_text(out: &mut Vec<u8>, on: bool, sgr: &str, text: &str) {
    paint_bytes(out, on, sgr, text.as_bytes());
    out.push(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_when_off() {
        let mut out = Vec::new();
        header_line(&mut out, false, "--- a");
        hunk_header(&mut out, false, "@@ -1 +1 @@");
        body_line(&mut out, false, b'-', b"old\n");
        body_line(&mut out, false, b'+', b"new");
        assert_eq!(
            out,
            b"--- a\n@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n"
        );
    }

    #[test]
    fn colors_wrap_lines_without_newline() {
        let mut out = Vec::new();
        body_line(&mut out, true, b'-', b"old\n");
        body_line(&mut out, true, b'+', b"new");
        body_line(&mut out, true, b' ', b"same\n");
        assert_eq!(
            out,
            b"\x1b[31m-old\x1b[0m\n\x1b[32m+new\x1b[0m\n\\ No newline at end of file\n same\n"
        );
    }
}
