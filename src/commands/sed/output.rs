use super::parse::{Action, Program};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

pub struct Stream<W> {
    inner: W,
    missing: bool,
    delimiter: u8,
}
impl<W: Write> Stream<W> {
    pub fn new(inner: W, delimiter: u8) -> Self {
        Self {
            inner,
            missing: false,
            delimiter,
        }
    }
    pub fn pattern(&mut self, text: &[u8], terminated: bool, first: bool) -> io::Result<()> {
        if self.missing {
            self.inner.write_all(&[self.delimiter])?;
            self.missing = false;
        }
        if first {
            if let Some(end) = text.iter().position(|&b| b == self.delimiter) {
                return self.write_all(&text[..=end]);
            }
        }
        self.write_all(text)?;
        if terminated {
            self.write_all(&[self.delimiter])?;
        } else {
            self.missing = true;
        }
        Ok(())
    }
}
impl<W: Write> Write for Stream<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !bytes.is_empty() && self.missing {
            self.inner.write_all(&[self.delimiter])?;
            self.missing = false;
        }
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
pub struct Outputs {
    files: HashMap<PathBuf, Stream<Box<dyn Write>>>,
    readers: HashMap<PathBuf, Box<dyn BufRead>>,
}
impl Outputs {
    pub fn new(program: &Program, delimiter: u8) -> io::Result<Self> {
        let mut result = Self {
            files: HashMap::new(),
            readers: HashMap::new(),
        };
        for instruction in &program.instructions {
            let name = match &instruction.action {
                Action::Write(name, _) => Some(name),
                Action::Substitute(s) => s.write.as_ref(),
                _ => None,
            };
            if let Some(name) = name {
                if !result.files.contains_key(name) {
                    let writer = open_writer(name)?;
                    result
                        .files
                        .insert(name.clone(), Stream::new(writer, delimiter));
                }
            }
        }
        Ok(result)
    }
    pub fn write(
        &mut self,
        name: &Path,
        text: &[u8],
        terminated: bool,
        delimiter: u8,
        first: bool,
    ) -> io::Result<()> {
        let writer = self.files.get_mut(name).unwrap();
        let _ = delimiter;
        writer.pattern(text, terminated, first)?;
        writer.flush()
    }
    pub fn read(
        &mut self,
        name: &Path,
        single: bool,
        delimiter: u8,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        if !single {
            reject_directory(name)?;
            if let Ok(mut reader) = open_reader(name) {
                io::copy(&mut reader, out)?;
            }
            return Ok(());
        }
        out.write_all(&self.read_record(name, delimiter)?)
    }
    pub fn read_record(&mut self, name: &Path, delimiter: u8) -> io::Result<Vec<u8>> {
        if !self.readers.contains_key(name) {
            reject_directory(name)?;
            let reader = open_reader(name).unwrap_or_else(|_| Box::new(io::empty()));
            self.readers.insert(name.to_owned(), reader);
        }
        let mut record = Vec::new();
        self.readers
            .get_mut(name)
            .unwrap()
            .read_until(delimiter, &mut record)?;
        Ok(record)
    }
    pub fn reset_readers(&mut self) {
        self.readers.clear();
    }
    pub fn flush(&mut self) -> io::Result<()> {
        for file in self.files.values_mut() {
            file.flush()?;
        }
        Ok(())
    }
}

fn reject_directory(path: &Path) -> io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(io::Error::other(format!(
            "read error on {}: Is a directory",
            crate::common::path::display_path(path)
        )));
    }
    Ok(())
}
fn open_writer(path: &Path) -> io::Result<Box<dyn Write>> {
    Ok(match path.to_str().unwrap_or("") {
        "/dev/stdout" => Box::new(io::stdout()),
        "/dev/stderr" => Box::new(io::stderr()),
        "/dev/null" => Box::new(io::sink()),
        _ => Box::new(BufWriter::with_capacity(
            65536,
            File::create(path).map_err(|e| {
                io::Error::other(format!(
                    "couldn't open file {}: {}",
                    crate::common::path::display_path(path),
                    crate::common::error::io_text(&e)
                ))
            })?,
        )),
    })
}
fn open_reader(path: &Path) -> io::Result<Box<dyn BufRead>> {
    if matches!(path.to_str(), Some("/dev/stdin" | "/dev/null")) {
        return crate::common::io::input(path.to_str().unwrap());
    }
    Ok(Box::new(BufReader::with_capacity(65536, File::open(path)?)))
}

pub fn list(text: &[u8], width: usize, delimiter: u8, out: &mut dyn Write) -> io::Result<()> {
    let mut column = 0;
    for &byte in text {
        let token = match byte {
            b'\\' => b"\\\\".to_vec(),
            7 => b"\\a".to_vec(),
            8 => b"\\b".to_vec(),
            9 => b"\\t".to_vec(),
            10 => b"\\n".to_vec(),
            11 => b"\\v".to_vec(),
            12 => b"\\f".to_vec(),
            13 => b"\\r".to_vec(),
            32..=126 => vec![byte],
            _ => format!("\\{byte:03o}").into_bytes(),
        };
        if width > 0 && column + token.len() >= width && column > 0 {
            out.write_all(&[b'\\', delimiter])?;
            column = 0;
        }
        out.write_all(&token)?;
        column += token.len();
    }
    if width > 0 && column >= width {
        out.write_all(&[b'\\', delimiter])?;
    }
    out.write_all(b"$")?;
    out.write_all(&[delimiter])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_partial() {
        let mut o = Vec::new();
        Stream::new(&mut o, b'\n')
            .pattern(b"a", false, false)
            .unwrap();
        assert_eq!(o, b"a");
    }
    #[test]
    fn first_line() {
        let mut o = Vec::new();
        Stream::new(&mut o, b'\n')
            .pattern(b"a\nb", false, true)
            .unwrap();
        assert_eq!(o, b"a\n");
    }
    #[test]
    fn list_controls() {
        let mut o = Vec::new();
        list(b"a\t\xff", 0, b'\n', &mut o).unwrap();
        assert_eq!(o, b"a\\t\\377$\n");
    }
    #[test]
    fn list_wrap() {
        let mut o = Vec::new();
        list(b"abcde", 4, b'\n', &mut o).unwrap();
        assert_eq!(o, b"abc\\\nde$\n");
    }
}
