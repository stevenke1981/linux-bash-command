use std::{io::BufRead, path::PathBuf};
#[derive(Clone, Debug)]
pub(super) struct Line {
    pub bytes: Vec<u8>,
    pub newline: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Change {
    pub kind: u8,
    pub line: Line,
}
#[derive(Clone, Debug)]
pub(super) struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub changes: Vec<Change>,
}
#[derive(Debug)]
pub(super) struct Patch {
    pub old: PathBuf,
    pub new: PathBuf,
    pub hunks: Vec<Hunk>,
    pub old_mode: Option<u32>,
    pub new_mode: Option<u32>,
}
fn range(text: &str, prefix: char) -> Result<(usize, usize), String> {
    let text = text.strip_prefix(prefix).ok_or("malformed hunk header")?;
    let (a, b) = text.split_once(',').unwrap_or((text, "1"));
    Ok((
        a.parse().map_err(|_| "invalid hunk line number")?,
        b.parse().map_err(|_| "invalid hunk line count")?,
    ))
}
#[cfg(unix)]
fn from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}
#[cfg(not(unix))]
fn from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}
fn filename(bytes: &[u8]) -> Result<PathBuf, String> {
    if bytes.first() != Some(&b'"') {
        return Ok(from_bytes(
            bytes.split(|b| *b == b'\t').next().unwrap().to_vec(),
        ));
    }
    let mut out = vec![];
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok(from_bytes(out)),
            b'\\' => {
                i += 1;
                let first = *bytes.get(i).ok_or("unterminated quoted filename")?;
                match first {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'0'..=b'7' => {
                        let mut n = u16::from(first - b'0');
                        for _ in 0..2 {
                            if let Some(b @ b'0'..=b'7') = bytes.get(i + 1) {
                                i += 1;
                                n = n * 8 + u16::from(*b - b'0');
                            } else {
                                break;
                            }
                        }
                        out.push(u8::try_from(n).map_err(|_| "invalid filename escape")?);
                    }
                    _ => out.push(first),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    Err("unterminated quoted filename".into())
}
#[derive(Debug)]
struct GitHeader {
    old: PathBuf,
    new: PathBuf,
    old_mode: Option<u32>,
    new_mode: Option<u32>,
}
fn git_token(bytes: &[u8]) -> Result<(PathBuf, &[u8]), String> {
    let bytes = bytes.strip_prefix(b" ").unwrap_or(bytes);
    if bytes.first() == Some(&b'"') {
        let mut escape = false;
        for i in 1..bytes.len() {
            if !escape && bytes[i] == b'"' {
                return Ok((filename(&bytes[..=i])?, &bytes[i + 1..]));
            }
            escape = !escape && bytes[i] == b'\\';
        }
        Err("unterminated git filename".into())
    } else {
        let end = bytes.iter().position(|b| *b == b' ').unwrap_or(bytes.len());
        Ok((filename(&bytes[..end])?, &bytes[end..]))
    }
}
fn git_header(line: &[u8]) -> Result<GitHeader, String> {
    let bytes = line
        .strip_prefix(b"diff --git ")
        .ok_or("malformed git header")?;
    let unquoted = if bytes.starts_with(b"a/") {
        bytes.windows(3).enumerate().find_map(|(i, window)| {
            if window == b" b/" && bytes.get(2..i) == bytes.get(i + 3..) {
                Some((
                    from_bytes(bytes[..i].to_vec()),
                    from_bytes(bytes[i + 1..].to_vec()),
                ))
            } else {
                None
            }
        })
    } else {
        None
    };
    let (old, new) = match unquoted {
        Some(paths) => paths,
        None => {
            let (old, rest) = git_token(bytes)?;
            let (new, _) = git_token(rest)?;
            (old, new)
        }
    };
    Ok(GitHeader {
        old,
        new,
        old_mode: None,
        new_mode: None,
    })
}

fn mode(bytes: &[u8]) -> Result<u32, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid git mode")?;
    let value = u32::from_str_radix(text.trim(), 8).map_err(|_| "invalid git mode")?;
    if value & 0o170000 != 0o100000 {
        return Err("unsupported git file mode".into());
    }
    Ok(value)
}
fn finish_git(git: Option<GitHeader>, patches: &mut Vec<Patch>) {
    if let Some(g) = git {
        if g.new_mode.is_some() || g.old_mode.is_some() {
            patches.push(Patch {
                old: if g.new_mode.is_some() {
                    PathBuf::from("/dev/null")
                } else {
                    g.old
                },
                new: if g.old_mode.is_some() {
                    PathBuf::from("/dev/null")
                } else {
                    g.new
                },
                old_mode: g.old_mode,
                new_mode: g.new_mode,
                hunks: vec![],
            });
        }
    }
}
fn header(line: &[u8]) -> Result<(usize, usize, usize, usize), String> {
    let text = std::str::from_utf8(line).map_err(|_| "invalid hunk header")?;
    let parts: Vec<_> = text.split_whitespace().collect();
    if parts.len() < 4 || parts[0] != "@@" || parts[3] != "@@" {
        return Err("malformed hunk header".into());
    }
    let (a, b) = range(parts[1], '-')?;
    let (c, d) = range(parts[2], '+')?;
    if b > 0 && a == 0 || d > 0 && c == 0 {
        return Err("invalid hunk line number".into());
    }
    Ok((a, b, c, d))
}
fn hunk(lines: &[Vec<u8>], at: &mut usize) -> Result<Hunk, String> {
    let (old_start, old_len, new_start, new_len) = header(&lines[*at])?;
    *at += 1;
    let (mut old, mut new) = (0usize, 0usize);
    let mut changes: Vec<Change> = vec![];
    while *at < lines.len() {
        let raw = &lines[*at];
        if raw.starts_with(b"\\ No newline at end of file") {
            let previous = changes
                .last_mut()
                .ok_or("misplaced missing-newline marker")?;
            previous.line.newline = false;
            *at += 1;
            continue;
        }
        if old == old_len && new == new_len {
            break;
        }
        let kind = if raw.is_empty() { b' ' } else { raw[0] };
        if !b" +-".contains(&kind) {
            return Err("malformed patch hunk".into());
        }
        if kind != b'+' {
            old = old.checked_add(1).ok_or("hunk too large")?;
        }
        if kind != b'-' {
            new = new.checked_add(1).ok_or("hunk too large")?;
        }
        if old > old_len || new > new_len {
            return Err("hunk line counts do not match".into());
        }
        changes.push(Change {
            kind,
            line: Line {
                bytes: raw.get(1..).unwrap_or_default().to_vec(),
                newline: true,
            },
        });
        *at += 1;
    }
    if old != old_len || new != new_len {
        return Err("unexpected end of patch".into());
    }
    Ok(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
        changes,
    })
}
pub(super) fn parse(reader: &mut dyn BufRead) -> Result<Vec<Patch>, String> {
    // The diff is a program, like an awk/sed script. Source file data is indexed
    // and streamed separately by the patch executor.
    let mut lines = vec![];
    let mut row = vec![];
    while reader
        .read_until(b'\n', &mut row)
        .map_err(|e| e.to_string())?
        != 0
    {
        if row.last() == Some(&b'\n') {
            row.pop();
        }
        lines.push(std::mem::take(&mut row));
    }
    for index in 0..lines.len() {
        let marker_follows = lines
            .get(index + 1)
            .is_some_and(|line| line.starts_with(b"\\ No newline at end of file"));
        if !marker_follows && lines[index].last() == Some(&b'\r') {
            lines[index].pop();
        }
    }
    let mut patches = vec![];
    let mut git: Option<GitHeader> = None;
    let mut at = 0;
    while at < lines.len() {
        let line = &lines[at];
        if line.starts_with(b"GIT binary patch")
            || line.starts_with(b"Binary files ")
            || line.starts_with(b"rename from ")
            || line.starts_with(b"rename to ")
        {
            return Err("binary and rename patches are not supported".into());
        }
        if line.starts_with(b"diff --git ") {
            finish_git(git.take(), &mut patches);
            git = Some(git_header(line)?);
            at += 1;
            continue;
        }
        if let Some(value) = line.strip_prefix(b"new file mode ") {
            git.as_mut().ok_or("git mode without header")?.new_mode = Some(mode(value)?);
            at += 1;
            continue;
        }
        if let Some(value) = line.strip_prefix(b"deleted file mode ") {
            git.as_mut().ok_or("git mode without header")?.old_mode = Some(mode(value)?);
            at += 1;
            continue;
        }
        if !line.starts_with(b"--- ") {
            if line.starts_with(b"*** ") || line.first().is_some_and(u8::is_ascii_digit) {
                return Err("only unified patches are supported".into());
            }
            at += 1;
            continue;
        }
        let old = filename(&line[4..])?;
        at += 1;
        let next = lines.get(at).ok_or("missing +++ file header")?;
        let new = filename(
            next.strip_prefix(b"+++ ")
                .ok_or("missing +++ file header")?,
        )?;
        at += 1;
        let mut hunks = vec![];
        while at < lines.len() && lines[at].starts_with(b"@@ ") {
            hunks.push(hunk(&lines, &mut at)?);
        }
        if hunks.is_empty() {
            return Err("patch contains no hunks".into());
        }
        let (old_mode, new_mode) = git
            .take()
            .map_or((None, None), |g| (g.old_mode, g.new_mode));
        patches.push(Patch {
            old,
            new,
            hunks,
            old_mode,
            new_mode,
        });
    }
    finish_git(git, &mut patches);
    if patches.is_empty() {
        return Err("no unified patch found".into());
    }
    Ok(patches)
}
pub(super) fn reverse(hunk: &Hunk) -> Hunk {
    Hunk {
        old_start: hunk.new_start,
        old_len: hunk.new_len,
        new_start: hunk.old_start,
        new_len: hunk.old_len,
        changes: hunk
            .changes
            .iter()
            .map(|c| Change {
                kind: match c.kind {
                    b'+' => b'-',
                    b'-' => b'+',
                    x => x,
                },
                line: c.line.clone(),
            })
            .collect(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unified_counts_and_no_newline() {
        let p = parse(
            &mut &b"--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n"[..],
        )
        .unwrap();
        assert_eq!(p[0].hunks[0].old_len, 1);
        assert!(!p[0].hunks[0].changes[1].line.newline);
    }
    #[test]
    fn malformed_counts_and_unsupported_formats() {
        for s in [
            "--- a\n+++ b\n@@ -1,2 +1 @@\n-a\n+b\n",
            "*** a\n",
            "GIT binary patch\n",
        ] {
            assert!(parse(&mut s.as_bytes()).is_err());
        }
    }
    #[test]
    fn quoted_git_paths_and_reverse() {
        assert_eq!(filename(br#""a/a\040b""#).unwrap(), PathBuf::from("a/a b"));
        let h = Hunk {
            old_start: 2,
            old_len: 1,
            new_start: 3,
            new_len: 0,
            changes: vec![Change {
                kind: b'-',
                line: Line {
                    bytes: b"x".to_vec(),
                    newline: true,
                },
            }],
        };
        let r = reverse(&h);
        assert_eq!(r.changes[0].kind, b'+');
        assert_eq!(r.old_start, 3);
    }
}
