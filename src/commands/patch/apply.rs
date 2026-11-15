use super::parse::{Change, Hunk, Line};
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};
pub(super) struct Image {
    file: Option<File>,
    offsets: Vec<u64>,
    pub crlf: bool,
    pub permissions: Option<fs::Permissions>,
}
impl Image {
    pub fn open(path: Option<&Path>) -> io::Result<Self> {
        let Some(path) = path else {
            return Ok(Self {
                file: None,
                offsets: vec![0],
                crlf: false,
                permissions: None,
            });
        };
        let file = File::open(path)?;
        let permissions = Some(file.metadata()?.permissions());
        let mut reader = BufReader::with_capacity(65536, file);
        let mut offsets = vec![0u64];
        let mut row = vec![];
        let mut crlf = None;
        while reader.read_until(b'\n', &mut row)? != 0 {
            if crlf.is_none() && row.ends_with(b"\n") {
                crlf = Some(row.ends_with(b"\r\n"));
            }
            let next = offsets
                .last()
                .unwrap()
                .checked_add(row.len() as u64)
                .ok_or_else(|| io::Error::other("file too large"))?;
            offsets.push(next);
            row.clear();
        }
        Ok(Self {
            file: Some(reader.into_inner()),
            offsets,
            crlf: crlf.unwrap_or(false),
            permissions,
        })
    }
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }
    pub fn raw(&mut self, index: usize) -> io::Result<Vec<u8>> {
        let length = usize::try_from(self.offsets[index + 1] - self.offsets[index])
            .map_err(|_| io::Error::other("line too large"))?;
        let mut row = vec![0; length];
        let file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(self.offsets[index]))?;
        file.read_exact(&mut row)?;
        Ok(row)
    }
    pub fn copy(&mut self, out: &mut dyn Write, start: usize, end: usize) -> io::Result<()> {
        if start == end {
            return Ok(());
        }
        let file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(self.offsets[start]))?;
        io::copy(&mut file.take(self.offsets[end] - self.offsets[start]), out)?;
        Ok(())
    }
}
fn source(raw: &[u8]) -> Line {
    let newline = raw.ends_with(b"\n");
    let bytes = raw.strip_suffix(b"\n").unwrap_or(raw);
    let bytes = if newline {
        bytes.strip_suffix(b"\r").unwrap_or(bytes)
    } else {
        bytes
    };
    Line {
        bytes: bytes.to_vec(),
        newline,
    }
}
fn blanks(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    let mut space = false;
    for b in bytes {
        if matches!(b, b' ' | b'\t') {
            space = true;
        } else {
            if space {
                out.push(b' ');
                space = false;
            }
            out.push(*b);
        }
    }
    out
}
fn same(left: &Line, right: &Line, loose: bool) -> bool {
    left.newline == right.newline
        && if loose {
            blanks(&left.bytes) == blanks(&right.bytes)
        } else {
            left.bytes == right.bytes
        }
}
fn contexts(changes: &[Change]) -> (usize, usize) {
    (
        changes.iter().take_while(|c| c.kind == b' ').count(),
        changes.iter().rev().take_while(|c| c.kind == b' ').count(),
    )
}
fn matches(
    image: &mut Image,
    hunk: &Hunk,
    at: usize,
    fuzz: usize,
    loose: bool,
) -> io::Result<bool> {
    let old: Vec<_> = hunk.changes.iter().filter(|c| c.kind != b'+').collect();
    let (leading, trailing) = contexts(&hunk.changes);
    let start = fuzz.min(leading);
    let end = old.len().saturating_sub(fuzz.min(trailing));
    for (i, expected) in old.iter().enumerate().take(end).skip(start) {
        if !same(&source(&image.raw(at + i)?), &expected.line, loose) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) struct Located {
    pub at: usize,
    pub offset: i128,
    pub fuzz: usize,
}
pub(super) fn locate(
    image: &mut Image,
    hunk: &Hunk,
    cursor: usize,
    offset: i128,
    max_fuzz: usize,
    loose: bool,
) -> io::Result<Option<Located>> {
    if hunk.old_len > image.len() {
        return Ok(None);
    }
    let max = image.len() - hunk.old_len;
    if cursor > max {
        return Ok(None);
    }
    let base = hunk
        .old_start
        .saturating_sub(usize::from(hunk.old_len != 0));
    let expected = (base as i128)
        .saturating_add(offset)
        .clamp(cursor as i128, max as i128) as usize;
    let (a, b) = contexts(&hunk.changes);
    let max_fuzz = max_fuzz.min(a.max(b));
    for fuzz in 0..=max_fuzz {
        for distance in 0..=max.saturating_sub(cursor) {
            let candidates = if distance == 0 {
                [Some(expected), None]
            } else {
                [
                    expected.checked_add(distance).filter(|p| *p <= max),
                    expected.checked_sub(distance).filter(|p| *p >= cursor),
                ]
            };
            for at in candidates.into_iter().flatten() {
                if matches(image, hunk, at, fuzz, loose)? {
                    return Ok(Some(Located {
                        at,
                        offset: at as i128 - base as i128,
                        fuzz,
                    }));
                }
            }
        }
    }
    Ok(None)
}
pub(super) fn write_hunk(
    image: &mut Image,
    hunk: &Hunk,
    location: &Located,
    out: &mut dyn Write,
) -> io::Result<usize> {
    let mut at = location.at;
    for change in &hunk.changes {
        match change.kind {
            b' ' => {
                out.write_all(&image.raw(at)?)?;
                at += 1;
            }
            b'-' => at += 1,
            b'+' => {
                out.write_all(&change.line.bytes)?;
                if change.line.newline {
                    out.write_all(if image.crlf { b"\r\n" } else { b"\n" })?;
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(at)
}
#[cfg(unix)]
fn filename_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}
#[cfg(not(unix))]
fn filename_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(path.to_string_lossy().as_bytes().to_vec())
}
fn range(start: usize, len: usize) -> String {
    if len == 1 {
        start.to_string()
    } else {
        format!("{start},{len}")
    }
}
pub(super) fn reject(
    out: &mut dyn Write,
    old: &Path,
    new: &Path,
    hunks: &[Hunk],
) -> io::Result<()> {
    out.write_all(b"--- ")?;
    out.write_all(&filename_bytes(old))?;
    out.write_all(b"\n+++ ")?;
    out.write_all(&filename_bytes(new))?;
    out.write_all(b"\n")?;
    for h in hunks {
        writeln!(
            out,
            "@@ -{} +{} @@",
            range(h.old_start, h.old_len),
            range(h.new_start, h.new_len)
        )?;
        for c in &h.changes {
            out.write_all(&[c.kind])?;
            out.write_all(&c.line.bytes)?;
            out.write_all(b"\n")?;
            if !c.line.newline {
                out.write_all(b"\\ No newline at end of file\n")?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whitespace_matching_preserves_nonblank_bytes() {
        assert_eq!(blanks(b" a\t b  "), b" a b");
        assert_ne!(blanks(b"a b"), blanks(b"ab"));
    }
    #[test]
    fn source_newlines_have_explicit_identity() {
        let a = source(b"a\r\n");
        assert!(a.newline);
        assert_eq!(a.bytes, b"a");
        assert!(!same(&a, &source(b"a"), false));
    }
    #[test]
    fn context_counts_ignore_changes() {
        let mk = |kind| Change {
            kind,
            line: Line {
                bytes: vec![],
                newline: true,
            },
        };
        assert_eq!(contexts(&[mk(b' '), mk(b'-'), mk(b'+'), mk(b' ')]), (1, 1));
    }
}
