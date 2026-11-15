//! Permuted indexing with a bounded external keyword sort and disk-backed context.
mod layout;
mod options;
use crate::common::{
    error::io_text,
    external::{Config, Sorter, TemporaryFile},
    io::{input, output},
};
use options::Options;
use std::cmp::Ordering;
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
type Compare = fn(&[u8], &[u8]) -> Ordering;
struct Spool {
    file: TemporaryFile,
    length: u64,
}
impl Spool {
    fn new() -> io::Result<Self> {
        Ok(Self {
            file: TemporaryFile::new(&std::env::temp_dir())?,
            length: 0,
        })
    }
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.file().write_all(bytes)?;
        self.length += bytes.len() as u64;
        Ok(())
    }
    fn read(&mut self, start: u64, end: u64) -> io::Result<Vec<u8>> {
        self.file.file().seek(SeekFrom::Start(start))?;
        let mut bytes = vec![0; (end - start) as usize];
        self.file.file().read_exact(&mut bytes)?;
        Ok(bytes)
    }
}
fn key(record: &[u8]) -> &[u8] {
    let length = u32::from_be_bytes(record[..4].try_into().unwrap()) as usize;
    &record[4..4 + length]
}
fn compare_records(left: &[u8], right: &[u8]) -> Ordering {
    key(left).cmp(key(right))
}
fn encode(key: &[u8], reference: &[u8], context: u64, start: u64, end: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(key.len() + reference.len() + 32);
    bytes.extend(
        u32::try_from(key.len())
            .map_err(io::Error::other)?
            .to_be_bytes(),
    );
    bytes.extend(key);
    for value in [context, start, end] {
        bytes.extend(value.to_be_bytes());
    }
    bytes.extend(reference);
    Ok(bytes)
}
struct Index<'a, 's> {
    options: &'a Options,
    sorter: Sorter<'s, Compare>,
    text: Spool,
    contexts: Spool,
    context_start: u64,
    max_reference: usize,
}
impl<'a, 's> Index<'a, 's> {
    fn new(options: &'a Options, config: Config, compare: &'s Compare) -> io::Result<Self> {
        Ok(Self {
            options,
            sorter: Sorter::new(config, compare),
            text: Spool::new()?,
            contexts: Spool::new()?,
            context_start: 0,
            max_reference: 0,
        })
    }
    fn finish_context(&mut self) -> io::Result<()> {
        self.contexts.append(&self.context_start.to_be_bytes())?;
        self.contexts.append(&self.text.length.to_be_bytes())?;
        self.context_start = self.text.length;
        Ok(())
    }
    fn segment(&mut self, raw: &[u8], line: usize, file: &str, reference: &[u8]) -> io::Result<()> {
        let normalized: Vec<_> = raw
            .iter()
            .map(|b| {
                if matches!(b, b'\t' | b'\n' | b'\r' | 11 | 12) {
                    b' '
                } else {
                    *b
                }
            })
            .collect();
        let offset = self.text.length;
        self.text.append(&normalized)?;
        let reference = if self.options.automatic {
            format!(
                "{}:{line}",
                if file == "-" {
                    String::new()
                } else {
                    crate::display_path(std::path::Path::new(file))
                }
            )
            .into_bytes()
        } else {
            reference.to_vec()
        };
        self.max_reference = self.max_reference.max(reference.len());
        let words = raw.strip_suffix(b"\n").unwrap_or(raw);
        for found in self.options.word.find_iter(words) {
            if found.is_empty() {
                continue;
            }
            let key = if self.options.folded {
                found.as_bytes().to_ascii_lowercase()
            } else {
                found.as_bytes().to_vec()
            };
            if self.options.ignore.contains(&key)
                || self
                    .options
                    .only
                    .as_ref()
                    .is_some_and(|set| !set.contains(&key))
            {
                continue;
            }
            self.sorter.push(encode(
                &key,
                &reference,
                self.contexts.length / 16,
                offset + found.start() as u64,
                offset + found.end() as u64,
            )?)?;
        }
        Ok(())
    }
    fn read(&mut self, reader: &mut dyn BufRead, file: &str) -> io::Result<()> {
        let default = regex::bytes::RegexBuilder::new(r#"[.!?][\"')\]]*(?:[ \t\r\n]|$)"#)
            .unicode(false)
            .build()
            .unwrap();
        let regex = self.options.sentence.as_ref().unwrap_or(&default).clone();
        let mut buffer = Vec::new();
        let mut line = 0;
        let result = (|| {
            loop {
                buffer.clear();
                if reader.read_until(b'\n', &mut buffer)? == 0 {
                    break;
                }
                line += 1;
                let (mut begin, mut reference) = (0, Vec::new());
                if self.options.references {
                    while begin < buffer.len() && buffer[begin].is_ascii_whitespace() {
                        begin += 1
                    }
                    let start = begin;
                    while begin < buffer.len() && !buffer[begin].is_ascii_whitespace() {
                        begin += 1
                    }
                    reference.extend(&buffer[start..begin]);
                    while begin < buffer.len() && buffer[begin].is_ascii_whitespace() {
                        begin += 1
                    }
                }
                let raw = &buffer[begin..];
                let raw = if (self.options.traditional || self.options.references)
                    && raw.last() == Some(&b'\n')
                {
                    &raw[..raw.len() - 1]
                } else {
                    raw
                };
                if self.options.references || self.options.traditional {
                    self.segment(raw, line, file, &reference)?;
                    self.finish_context()?;
                    continue;
                }
                let mut start = 0;
                for boundary in regex.find_iter(raw) {
                    let end = if self.options.sentence.is_some() {
                        boundary.start()
                    } else {
                        boundary.end()
                    };
                    self.segment(&raw[start..end], line, file, &reference)?;
                    self.finish_context()?;
                    start = boundary.end();
                }
                self.segment(&raw[start..], line, file, &reference)?;
                if raw.iter().all(u8::is_ascii_whitespace) {
                    self.finish_context()?;
                }
            }
            Ok(())
        })();
        self.finish_context()?;
        result
    }
    fn emit(self, out: &mut dyn Write) -> io::Result<()> {
        let Self {
            options,
            sorter,
            mut text,
            mut contexts,
            max_reference,
            ..
        } = self;
        let reserve = if (options.automatic || options.references) && !options.right {
            max_reference
        } else {
            0
        };
        let width = options.width.saturating_sub(reserve);
        sorter.finish(&mut |record| {
            let data = 4 + key(record).len();
            let context = u64::from_be_bytes(record[data..data + 8].try_into().unwrap());
            let start = u64::from_be_bytes(record[data + 8..data + 16].try_into().unwrap());
            let end = u64::from_be_bytes(record[data + 16..data + 24].try_into().unwrap());
            let reference = &record[data + 24..];
            let bounds = contexts.read(context * 16, context * 16 + 16)?;
            let first = u64::from_be_bytes(bounds[..8].try_into().unwrap());
            let last = u64::from_be_bytes(bounds[8..].try_into().unwrap());
            let window = options.width.saturating_mul(3) as u64;
            let first = first.max(start.saturating_sub(window));
            let last = last.min(end.saturating_add(window));
            let bytes = text.read(first, last)?;
            let begin = (start - first) as usize;
            let end = (end - first) as usize;
            out.write_all(&layout::render(
                &bytes[..begin],
                &bytes[begin..end],
                &bytes[end..],
                reference,
                options,
                width,
            ))
        })
    }
}
pub fn run(arguments: &[String]) -> i32 {
    let (options, files, destination) = match options::parse(arguments) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut failed = false;
    let compare: Compare = compare_records;
    let mut index = match Index::new(&options, Config::default(), &compare) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("ptx: {}", io_text(&error));
            return 1;
        }
    };
    for file in &files {
        match input(file).and_then(|mut reader| index.read(&mut reader, file)) {
            Ok(()) => {}
            Err(error) => {
                eprintln!("ptx: {file}: {}", io_text(&error));
                failed = true;
            }
        }
    }
    let status = match destination.as_deref() {
        None | Some("-" | "/dev/stdout") => output("ptx", |out| index.emit(out)),
        Some(path) => {
            let target: io::Result<Box<dyn Write>> = match path {
                "/dev/null" => Ok(Box::new(io::sink())),
                "/dev/stderr" => Ok(Box::new(io::stderr())),
                _ => File::create(path).map(|file| {
                    Box::new(io::BufWriter::with_capacity(65536, file)) as Box<dyn Write>
                }),
            };
            match target.and_then(|mut out| index.emit(&mut out).and_then(|()| out.flush())) {
                Ok(()) => 0,
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
                Err(error) => {
                    eprintln!("ptx: {path}: {}", io_text(&error));
                    1
                }
            }
        }
    };
    status.max(i32::from(failed))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spool_roundtrip() {
        let mut spool = Spool::new().unwrap();
        spool.append(b"\xffbytes").unwrap();
        assert_eq!(spool.read(0, 6).unwrap(), b"\xffbytes");
    }
    #[test]
    fn opaque_keys_do_not_include_reference_payload() {
        let a = encode(b"word", b"z", 0, 0, 1).unwrap();
        let b = encode(b"word", b"a", 1, 2, 3).unwrap();
        assert_eq!(compare_records(&a, &b), Ordering::Equal);
    }
    #[test]
    fn forced_spills_preserve_index_order_and_context() {
        let options = options::parse(&["-O".into()]).unwrap().0;
        let compare: Compare = compare_records;
        let mut index = Index::new(
            &options,
            Config {
                memory_limit: 1,
                batch_size: 2,
                ..Default::default()
            },
            &compare,
        )
        .unwrap();
        index
            .read(&mut io::Cursor::new(b"beta alpha.\nalpha beta.\n"), "-")
            .unwrap();
        let mut out = Vec::new();
        index.emit(&mut out).unwrap();
        assert_eq!(out,b".xx \"\" \"beta\" \"alpha.\" \"\"\n.xx \"\" \"\" \"alpha beta.\" \"\"\n.xx \"\" \"\" \"beta alpha.\" \"\"\n.xx \"\" \"alpha\" \"beta.\" \"\"\n");
    }
}
