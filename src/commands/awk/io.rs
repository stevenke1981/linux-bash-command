use super::regexp::Cache;
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

pub struct Records {
    reader: Box<dyn BufRead>,
    buffer: Vec<u8>,
    start: usize,
    eof: bool,
}
impl Records {
    pub fn new(reader: Box<dyn BufRead>) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            start: 0,
            eof: false,
        }
    }
    pub fn next(&mut self, separator: &[u8], cache: &mut Cache) -> Result<Option<Vec<u8>>, String> {
        if separator.len() == 1 {
            return self.character(separator[0]);
        }
        if separator.is_empty() {
            return self.paragraph();
        }
        self.compact();
        let regex = cache.get(separator)?;
        let mut scanner = regex.scanner(self.start, true);
        loop {
            let result = scanner.feed(&self.buffer, self.eof);
            if let Some((begin, end)) = result.found {
                if !result.pending {
                    return Ok(Some(self.take(begin, end)));
                }
            }
            if self.eof {
                return Ok(self.tail(false));
            }
            self.fill()?;
        }
    }

    fn character(&mut self, separator: u8) -> Result<Option<Vec<u8>>, String> {
        loop {
            if let Some(index) = self.buffer[self.start..]
                .iter()
                .position(|b| *b == separator)
            {
                let end = self.start + index;
                return Ok(Some(self.take(end, end + 1)));
            }
            if self.eof {
                return Ok(self.tail(false));
            }
            self.fill()?;
        }
    }
    fn paragraph(&mut self) -> Result<Option<Vec<u8>>, String> {
        loop {
            while self.buffer.get(self.start) == Some(&b'\n') {
                self.start += 1;
            }
            if self.start == self.buffer.len() && !self.eof {
                self.fill()?;
                continue;
            }
            if let Some(offset) = self.buffer[self.start..]
                .windows(2)
                .position(|w| w == b"\n\n")
            {
                let end = self.start + offset;
                let mut after = end + 2;
                while self.buffer.get(after) == Some(&b'\n') {
                    after += 1;
                }
                return Ok(Some(self.take(end, after)));
            }
            if self.eof {
                return Ok(self.tail(true));
            }
            self.fill()?;
        }
    }
    fn take(&mut self, end: usize, after: usize) -> Vec<u8> {
        let record = self.buffer[self.start..end].to_vec();
        self.start = after;
        record
    }
    fn tail(&mut self, paragraph: bool) -> Option<Vec<u8>> {
        if self.start == self.buffer.len() {
            return None;
        }
        let mut end = self.buffer.len();
        if paragraph && self.buffer.last() == Some(&b'\n') {
            end -= 1;
        }
        Some(self.take(end, self.buffer.len()))
    }
    fn compact(&mut self) {
        if self.start > 65536 || self.start > self.buffer.len() / 2 {
            let consumed = self.start.saturating_sub(1);
            self.buffer.drain(..consumed);
            self.start -= consumed;
        }
    }
    fn fill(&mut self) -> Result<(), String> {
        self.compact();
        let bytes = self
            .reader
            .fill_buf()
            .map_err(|e| crate::common::error::io_text(&e))?;
        if bytes.is_empty() {
            self.eof = true;
        } else {
            let length = bytes.len();
            self.buffer.extend_from_slice(bytes);
            self.reader.consume(length);
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct Files {
    pub readers: HashMap<PathBuf, Records>,
    writers: HashMap<PathBuf, Box<dyn Write>>,
    stdout_open: bool,
}
impl Files {
    pub fn getline(
        &mut self,
        name: &[u8],
        rs: &[u8],
        cache: &mut Cache,
    ) -> Result<Option<Vec<u8>>, String> {
        let path = path(name);
        if !self.readers.contains_key(&path) {
            self.readers.insert(
                path.clone(),
                Records::new(reader(&path, false).map_err(|e| crate::common::error::io_text(&e))?),
            );
        }
        self.readers.get_mut(&path).unwrap().next(rs, cache)
    }
    pub fn write(
        &mut self,
        name: &[u8],
        append: bool,
        bytes: &[u8],
        stdout: &mut dyn Write,
    ) -> io::Result<()> {
        if name == b"/dev/stdout" {
            self.stdout_open = true;
            return stdout.write_all(bytes);
        }
        let path = path(name);
        if !self.writers.contains_key(&path) {
            self.writers.insert(path.clone(), writer(&path, append)?);
        }
        self.writers.get_mut(&path).unwrap().write_all(bytes)
    }
    pub fn close(&mut self, name: &[u8], stdout: &mut dyn Write) -> i32 {
        let path = path(name);
        let read = self.readers.remove(&path).is_some();
        if name == b"/dev/stdout" && self.stdout_open {
            self.stdout_open = false;
            return if stdout.flush().is_ok() { 0 } else { -1 };
        }
        match self.writers.remove(&path) {
            Some(mut writer) => {
                if writer.flush().is_ok() {
                    0
                } else {
                    -1
                }
            }
            None => {
                if read {
                    0
                } else {
                    -1
                }
            }
        }
    }
    pub fn flush(&mut self, name: Option<&[u8]>) -> io::Result<bool> {
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            return match self.writers.get_mut(&path(name)) {
                Some(writer) => writer.flush().map(|()| true),
                None => Ok(false),
            };
        }
        for writer in self.writers.values_mut() {
            writer.flush()?;
        }
        Ok(true)
    }
}
pub fn reader(path: &Path, main: bool) -> io::Result<Box<dyn BufRead>> {
    if path == Path::new("/dev/null") {
        return Ok(Box::new(io::empty()));
    }
    if path == Path::new("/dev/stdin") || (main && path == Path::new("-")) {
        return Ok(Box::new(BufReader::with_capacity(65536, io::stdin())));
    }
    Ok(Box::new(BufReader::with_capacity(65536, File::open(path)?)))
}
fn writer(path: &Path, append: bool) -> io::Result<Box<dyn Write>> {
    Ok(match path.to_str().unwrap_or("") {
        "/dev/stdout" => Box::new(io::stdout()),
        "/dev/stderr" => Box::new(io::stderr()),
        "/dev/null" => Box::new(io::sink()),
        _ => Box::new(BufWriter::with_capacity(
            65536,
            OpenOptions::new()
                .write(true)
                .create(true)
                .append(append)
                .truncate(!append)
                .open(path)?,
        )),
    })
}
pub fn path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes.to_vec()).into()
    }
    #[cfg(not(unix))]
    {
        String::from_utf8_lossy(bytes).into_owned().into()
    }
}
pub fn assignment(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = bytes.iter().position(|b| *b == b'=')?;
    let name = &bytes[..at];
    if name.is_empty()
        || !(name[0].is_ascii_alphabetic() || name[0] == b'_')
        || !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return None;
    }
    Some((name, &bytes[at + 1..]))
}
pub fn argument(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.contains(&b'\n') {
        return Err("POSIX does not allow physical newlines in string values".into());
    }
    let (mut out, mut at) = (Vec::new(), 0);
    while at < bytes.len() {
        let byte = bytes[at];
        at += 1;
        if byte != b'\\' || at == bytes.len() {
            out.push(byte);
            continue;
        }
        let next = bytes[at];
        at += 1;
        if (b'0'..=b'7').contains(&next) {
            let mut value = u16::from(next - b'0');
            for _ in 0..2 {
                if let Some(&b) = bytes.get(at).filter(|b| (b'0'..=b'7').contains(b)) {
                    value = value * 8 + u16::from(b - b'0');
                    at += 1;
                } else {
                    break;
                }
            }
            out.push(value as u8);
        } else {
            out.push(match next {
                b'a' => 7,
                b'b' => 8,
                b'f' => 12,
                b'n' => 10,
                b'r' => 13,
                b't' => 9,
                b'v' => 11,
                _ => next,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn records(input: &[u8], rs: &[u8]) -> Vec<Vec<u8>> {
        let mut r = Records::new(Box::new(io::Cursor::new(input.to_vec())));
        let mut c = Cache::default();
        let mut out = Vec::new();
        while let Some(line) = r.next(rs, &mut c).unwrap() {
            out.push(line);
        }
        out
    }
    #[test]
    fn lines_and_final_partial() {
        assert_eq!(records(b"a\nb", b"\n"), [b"a", b"b"]);
    }
    #[test]
    fn paragraph_separators() {
        assert_eq!(
            records(b"\n\na\nb\n\n\nc\n", b""),
            [b"a\nb".to_vec(), b"c".to_vec()]
        );
    }
    #[test]
    fn regex_record_separators() {
        assert_eq!(records(b"a::b:::c", b":+"), [b"a", b"b", b"c"]);
    }
    #[test]
    fn regex_separator_longest() {
        assert_eq!(records(b"a::b", b":|::"), [b"a", b"b"]);
    }
    #[test]
    fn empty_regex_matches_do_not_split() {
        assert_eq!(records(b"abc", b"()"), [b"abc"]);
    }
    #[test]
    fn argv_assignments() {
        assert_eq!(
            assignment(b"name=a=b"),
            Some((b"name".as_slice(), b"a=b".as_slice()))
        );
        assert!(assignment(b"1name=x").is_none());
        assert_eq!(argument(br"a\tb").unwrap(), b"a\tb");
    }
    #[test]
    fn separator_may_extend_across_reader_chunks() {
        let input = BufReader::with_capacity(2, io::Cursor::new(b"a::::b"));
        let mut reader = Records::new(Box::new(input));
        let mut cache = Cache::default();
        assert_eq!(reader.next(b":+", &mut cache).unwrap(), Some(b"a".to_vec()));
        assert_eq!(reader.next(b":+", &mut cache).unwrap(), Some(b"b".to_vec()));
        assert!(reader.next(b":+", &mut cache).unwrap().is_none());
    }
    #[test]
    fn assignment_escapes_are_data_and_follow_posix_rules() {
        assert_eq!(argument(b"a\\\"b").unwrap(), b"a\"b");
        assert_eq!(argument(b"last\\").unwrap(), b"last\\");
        assert_eq!(argument(br"\141\x41").unwrap(), b"ax41");
        assert!(argument(b"a\nb").is_err());
    }
    #[test]
    fn regex_alternative_is_independent_of_input_chunk_size() {
        for capacity in 1..=8 {
            let input = BufReader::with_capacity(capacity, io::Cursor::new(b"ZaXbQ"));
            let mut reader = Records::new(Box::new(input));
            let mut cache = Cache::default();
            assert_eq!(
                reader.next(b"a.*b|a", &mut cache).unwrap(),
                Some(b"Z".to_vec())
            );
            assert_eq!(
                reader.next(b"a.*b|a", &mut cache).unwrap(),
                Some(b"Q".to_vec())
            );
            assert!(reader.next(b"a.*b|a", &mut cache).unwrap().is_none());
        }
    }
}
