//! Patch input parsing: unified diffs with the git extensions (`diff --git`, `new file
//! mode`, `deleted file mode`, `/dev/null` names). Context and normal diffs, renames and
//! binary patches are reported as unsupported.

/// Kind of one hunk line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    Delete,
    Add,
}

/// One body line of a hunk. `newline` is false when a `\ No newline at end of file`
/// marker follows the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkLine {
    pub kind: Kind,
    pub text: Vec<u8>,
    pub newline: bool,
}

/// One `@@ -a,b +c,d @@` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub header: String,
    pub lines: Vec<HunkLine>,
}

/// All hunks for one file, with the names from its `---` and `+++` lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePatch {
    pub old_line: String,
    pub new_line: String,
    pub old_name: String,
    pub new_name: String,
    pub creates: bool,
    pub deletes: bool,
    /// 1-based line number of the `---` line, for messages.
    pub line_no: usize,
    pub hunks: Vec<Hunk>,
}

/// Why a patch could not be parsed.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// No file header anywhere in the input.
    Garbage,
    /// A header or hunk body does not follow the diff grammar.
    Malformed { line: usize },
    /// A valid diff form this project does not implement.
    Unsupported(&'static str),
}

#[derive(Default)]
struct GitFlags {
    creates: bool,
    deletes: bool,
}

/// Parses the whole patch text.
pub fn parse(input: &[u8]) -> Result<Vec<FilePatch>, ParseError> {
    let lines = split_lines(input);
    let mut files: Vec<FilePatch> = Vec::new();
    let mut git = GitFlags::default();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with(b"diff --git ") {
            git = GitFlags::default();
            i += 1;
        } else if line.starts_with(b"new file mode") {
            git.creates = true;
            i += 1;
        } else if line.starts_with(b"deleted file mode") {
            git.deletes = true;
            i += 1;
        } else if line.starts_with(b"rename from ")
            || line.starts_with(b"rename to ")
            || line.starts_with(b"copy from ")
            || line.starts_with(b"copy to ")
        {
            return Err(ParseError::Unsupported("rename and copy patches"));
        } else if line.starts_with(b"GIT binary patch") || line.starts_with(b"Binary files ") {
            return Err(ParseError::Unsupported("binary patches"));
        } else if line.starts_with(b"*** ")
            && lines.get(i + 1).is_some_and(|l| l.starts_with(b"--- "))
        {
            return Err(ParseError::Unsupported("context diffs"));
        } else if is_normal_diff_header(line) {
            return Err(ParseError::Unsupported("normal diffs"));
        } else if line.starts_with(b"--- ")
            && lines.get(i + 1).is_some_and(|l| l.starts_with(b"+++ "))
        {
            let (file, next) = parse_file(&lines, i, &git)?;
            files.push(file);
            git = GitFlags::default();
            i = next;
        } else if line.starts_with(b"@@ ") {
            return Err(ParseError::Malformed { line: i + 1 });
        } else {
            i += 1;
        }
    }
    if files.is_empty() {
        return Err(ParseError::Garbage);
    }
    Ok(files)
}

/// Parses one file section starting at the `---` line `start`; returns the file and the
/// index of the first line after its hunks.
fn parse_file(
    lines: &[&[u8]],
    start: usize,
    git: &GitFlags,
) -> Result<(FilePatch, usize), ParseError> {
    let old_line = text(lines[start]);
    let new_line = text(lines[start + 1]);
    let old_name = header_name(&old_line[4..]);
    let new_name = header_name(&new_line[4..]);
    let mut file = FilePatch {
        creates: git.creates || old_name == "/dev/null",
        deletes: git.deletes || new_name == "/dev/null",
        old_line,
        new_line,
        old_name,
        new_name,
        line_no: start + 1,
        hunks: Vec::new(),
    };
    let mut i = start + 2;
    while i < lines.len() && lines[i].starts_with(b"@@ ") {
        let (hunk, next) = parse_hunk(lines, i)?;
        file.hunks.push(hunk);
        i = next;
    }
    Ok((file, i))
}

/// Parses the hunk whose `@@` header is at `start`; returns it and the next line index.
fn parse_hunk(lines: &[&[u8]], start: usize) -> Result<(Hunk, usize), ParseError> {
    let header = text(lines[start]);
    let malformed = ParseError::Malformed { line: start + 1 };
    let (old_start, old_count, new_start, new_count) =
        parse_hunk_header(&header).ok_or(ParseError::Malformed { line: start + 1 })?;
    let mut hunk = Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        header,
        lines: Vec::new(),
    };
    let (mut old_left, mut new_left) = (old_count, new_count);
    let mut i = start + 1;
    while old_left > 0 || new_left > 0 {
        let Some(line) = lines.get(i) else {
            return Err(malformed);
        };
        let (kind, body) = match line.first() {
            None | Some(b' ') => (Kind::Context, line.get(1..).unwrap_or(&[])),
            Some(b'-') => (Kind::Delete, &line[1..]),
            Some(b'+') => (Kind::Add, &line[1..]),
            Some(b'\\') => {
                mark_no_newline(&mut hunk.lines, start + 1)?;
                i += 1;
                continue;
            }
            _ => return Err(malformed),
        };
        match kind {
            Kind::Context => {
                if old_left == 0 || new_left == 0 {
                    return Err(malformed);
                }
                old_left -= 1;
                new_left -= 1;
            }
            Kind::Delete => {
                if old_left == 0 {
                    return Err(malformed);
                }
                old_left -= 1;
            }
            Kind::Add => {
                if new_left == 0 {
                    return Err(malformed);
                }
                new_left -= 1;
            }
        }
        hunk.lines.push(HunkLine {
            kind,
            text: body.to_vec(),
            newline: true,
        });
        i += 1;
    }
    while lines.get(i).is_some_and(|l| l.starts_with(b"\\")) {
        mark_no_newline(&mut hunk.lines, start + 1)?;
        i += 1;
    }
    Ok((hunk, i))
}

fn mark_no_newline(lines: &mut [HunkLine], line_no: usize) -> Result<(), ParseError> {
    match lines.last_mut() {
        Some(last) => {
            last.newline = false;
            Ok(())
        }
        None => Err(ParseError::Malformed { line: line_no }),
    }
}

/// Reads `@@ -a[,b] +c[,d] @@`; a missing count means 1.
fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize, usize)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let (old_start, old_count) = parse_range(old)?;
    let (new_start, new_count) = parse_range(new)?;
    Some((old_start, old_count, new_start, new_count))
}

fn parse_range(range: &str) -> Option<(usize, usize)> {
    match range.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

/// True for the first line of a classic `ed`-style normal diff, such as `2c2` or `1,3d0`.
fn is_normal_diff_header(line: &[u8]) -> bool {
    let digits = |s: &[u8]| !s.is_empty() && s.iter().all(u8::is_ascii_digit);
    let range = |s: &[u8]| match s.iter().position(|&b| b == b',') {
        Some(p) => digits(&s[..p]) && digits(&s[p + 1..]),
        None => digits(s),
    };
    let Some(pos) = line.iter().position(|&b| matches!(b, b'a' | b'c' | b'd')) else {
        return false;
    };
    pos > 0 && range(&line[..pos]) && range(&line[pos + 1..])
}

/// Extracts the file name from the text after `--- ` or `+++ `. The name ends at a tab
/// (where diff puts the timestamp); without a tab the whole text is used.
fn header_name(rest: &str) -> String {
    let name = match rest.split_once('\t') {
        Some((name, _)) => name,
        None => rest,
    };
    name.trim_end_matches('\r').to_string()
}

fn text(line: &[u8]) -> String {
    String::from_utf8_lossy(line)
        .trim_end_matches('\r')
        .to_string()
}

/// Splits on `\n`. A final empty piece after a trailing newline is dropped.
fn split_lines(input: &[u8]) -> Vec<&[u8]> {
    if input.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = input.split(|&b| b == b'\n').collect();
    if input.ends_with(b"\n") {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(hunk: &Hunk) -> Vec<Kind> {
        hunk.lines.iter().map(|l| l.kind).collect()
    }

    #[test]
    fn parses_unified_hunk_with_default_counts() {
        let patch = b"junk line\n--- a/t.txt\t2026-01-01\n+++ b/t.txt\n@@ -1 +1 @@\n-one\n+ONE\n";
        let files = parse(patch).unwrap();
        assert_eq!(files.len(), 1);
        let file = &files[0];
        assert_eq!(file.old_name, "a/t.txt");
        assert_eq!(file.new_name, "b/t.txt");
        assert_eq!(file.line_no, 2);
        let hunk = &file.hunks[0];
        assert_eq!((hunk.old_start, hunk.old_count), (1, 1));
        assert_eq!((hunk.new_start, hunk.new_count), (1, 1));
        assert_eq!(kinds(hunk), vec![Kind::Delete, Kind::Add]);
        assert_eq!(hunk.lines[1].text, b"ONE");
    }

    #[test]
    fn git_headers_mark_creation_and_deletion() {
        let create = b"diff --git a/n b/n\nnew file mode 100644\nindex 0000000..e69de29\n--- /dev/null\n+++ b/n\n@@ -0,0 +1 @@\n+x\n";
        let file = &parse(create).unwrap()[0];
        assert!(file.creates && !file.deletes);
        let delete = b"diff --git a/n b/n\ndeleted file mode 100644\n--- a/n\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n";
        let file = &parse(delete).unwrap()[0];
        assert!(file.deletes && !file.creates);
    }

    #[test]
    fn no_newline_marker_clears_flag_of_previous_line() {
        let patch = b"--- a\n+++ a\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n";
        let hunk = &parse(patch).unwrap()[0].hunks[0];
        assert!(hunk.lines[0].newline);
        assert!(!hunk.lines[1].newline);
        assert!(!hunk.lines[2].newline);
    }

    #[test]
    fn empty_line_inside_hunk_is_context() {
        let patch = b"--- a\n+++ a\n@@ -1,3 +1,3 @@\n x\n\n-y\n+Y\n";
        let hunk = &parse(patch).unwrap()[0].hunks[0];
        assert_eq!(
            kinds(hunk),
            vec![Kind::Context, Kind::Context, Kind::Delete, Kind::Add]
        );
        assert!(hunk.lines[1].text.is_empty());
    }

    #[test]
    fn rejects_garbage_context_normal_and_truncated_input() {
        assert_eq!(parse(b"hello\n"), Err(ParseError::Garbage));
        assert_eq!(parse(b""), Err(ParseError::Garbage));
        let context = b"*** a\n--- a\n";
        assert_eq!(
            parse(context),
            Err(ParseError::Unsupported("context diffs"))
        );
        assert_eq!(
            parse(b"2c2\n< x\n---\n> y\n"),
            Err(ParseError::Unsupported("normal diffs"))
        );
        let truncated = b"--- a\n+++ a\n@@ -1,3 +1,3 @@\n x\n";
        assert_eq!(parse(truncated), Err(ParseError::Malformed { line: 3 }));
        let bad_body = b"--- a\n+++ a\n@@ -1 +1 @@\n?oops\n";
        assert_eq!(parse(bad_body), Err(ParseError::Malformed { line: 3 }));
    }

    #[test]
    fn rename_binary_and_stray_hunks_are_errors() {
        let rename = b"diff --git a/x b/y\nrename from x\nrename to y\n";
        assert_eq!(
            parse(rename),
            Err(ParseError::Unsupported("rename and copy patches"))
        );
        let binary = b"diff --git a/x b/x\nGIT binary patch\n";
        assert_eq!(
            parse(binary),
            Err(ParseError::Unsupported("binary patches"))
        );
        assert_eq!(
            parse(b"@@ -1 +1 @@\n"),
            Err(ParseError::Malformed { line: 1 })
        );
    }

    #[test]
    fn header_range_forms() {
        assert_eq!(
            parse_hunk_header("@@ -3,4 +5,6 @@ fn x"),
            Some((3, 4, 5, 6))
        );
        assert_eq!(parse_hunk_header("@@ -0,0 +1 @@"), Some((0, 0, 1, 1)));
        assert_eq!(parse_hunk_header("@@ bogus @@"), None);
        assert_eq!(header_name("t.txt\t2026\r"), "t.txt");
        assert_eq!(header_name("t.txt\r"), "t.txt");
    }

    #[test]
    fn multiple_files_and_hunks_are_kept_in_order() {
        let patch = b"--- a\n+++ a\n@@ -1 +1 @@\n-1\n+one\n@@ -5 +5 @@\n-5\n+five\n--- b\n+++ b\n@@ -2 +2 @@\n-2\n+two\n";
        let files = parse(patch).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[1].hunks.len(), 1);
        assert_eq!(files[1].line_no, 9);
    }
}
