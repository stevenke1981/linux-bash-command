use super::{automatic_suffix, suffix, Mode};
use crate::common::io as common_io;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Spool {
    path: PathBuf,
    file: Option<File>,
}
impl Spool {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let name = format!(
                "agentcmd-split-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(name);
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("cannot create temporary input"))
    }
    fn independent_reader(&self) -> io::Result<File> {
        // The scanner and range copier need independent file offsets.
        let reader = File::open(&self.path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = reader.metadata()?;
            let held = self.file.as_ref().unwrap().metadata()?;
            if opened.dev() != held.dev() || opened.ino() != held.ino() {
                return Err(io::Error::other("temporary input was replaced"));
            }
        }
        Ok(reader)
    }
}
impl Drop for Spool {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

struct Outputs<'a> {
    input: &'a str,
    prefix: &'a str,
    extra: &'a str,
    width: usize,
    fixed: bool,
    base: u8,
    first: usize,
    verbose: bool,
    selected: Option<usize>,
    next: usize,
    bucket_names: std::collections::HashMap<usize, String>,
}
impl Outputs<'_> {
    fn name(&self, index: usize) -> io::Result<String> {
        let index = self
            .first
            .checked_add(index)
            .ok_or_else(|| io::Error::other("output file suffixes exhausted"))?;
        let name = if self.fixed {
            suffix(index, self.width, self.base)
        } else {
            automatic_suffix(index, self.width, self.base)
        }
        .ok_or_else(|| io::Error::other("output file suffixes exhausted"))?;
        Ok(format!("{}{name}{}", self.prefix, self.extra))
    }
    fn create(&mut self, index: usize) -> io::Result<String> {
        let path = self.name(index)?;
        if self.input != "-"
            && self.input != "/dev/stdin"
            && crate::common::meta::same_file(
                std::path::Path::new(self.input),
                std::path::Path::new(&path),
            )
            .unwrap_or(false)
        {
            return Err(io::Error::other(format!(
                "'{path}' would overwrite input; aborting"
            )));
        }
        if self.verbose {
            writeln!(io::stdout().lock(), "creating file '{path}'")?;
        }
        File::create(&path)?;
        Ok(path)
    }
    fn bucket(&mut self, index: usize, elide: bool) -> io::Result<Option<String>> {
        if self.selected.is_some() {
            return Ok(None);
        }
        if let Some(path) = self.bucket_names.get(&index) {
            return Ok(Some(path.clone()));
        }
        let file_index = if elide {
            let n = self.next;
            self.next += 1;
            n
        } else {
            index
        };
        let path = self.create(file_index)?;
        self.bucket_names.insert(index, path.clone());
        Ok(Some(path))
    }
}
fn range(file: &mut File, start: u64, end: u64, out: &mut dyn Write) -> io::Result<()> {
    file.seek(SeekFrom::Start(start))?;
    io::copy(&mut file.take(end - start), out)?;
    Ok(())
}
fn write_piece(
    file: &mut File,
    start: u64,
    end: u64,
    index: usize,
    outputs: &mut Outputs<'_>,
    elide: bool,
) -> io::Result<()> {
    if let Some(selected) = outputs.selected {
        if selected == index {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            range(file, start, end, &mut out)?;
        }
    } else {
        let path = outputs.bucket(index, elide)?.unwrap();
        let mut out = OpenOptions::new().append(true).open(path)?;
        range(file, start, end, &mut out)?;
    }
    Ok(())
}
fn scan_records(
    mut input: BufReader<File>,
    sep: u8,
    mut found: impl FnMut(u64, u64) -> io::Result<()>,
) -> io::Result<()> {
    let mut offset = 0u64;
    let mut start = 0u64;
    loop {
        let bytes = input.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let len = bytes.len();
        for (i, &byte) in bytes.iter().enumerate() {
            if byte == sep {
                let end = offset + i as u64 + 1;
                found(start, end)?;
                start = end;
            }
        }
        offset += len as u64;
        input.consume(len);
    }
    if start < offset {
        found(start, offset)?;
    }
    Ok(())
}
fn run_inner(
    input: &str,
    mode: Mode,
    sep: u8,
    outputs: &mut Outputs<'_>,
    elide: bool,
) -> io::Result<()> {
    let mut spool = Spool::new()?;
    let mut reader = common_io::input(input)?;
    let len = io::copy(&mut reader, spool.file.as_mut().unwrap())?;
    let mut data = spool.file.as_ref().unwrap().try_clone()?;
    match mode {
        Mode::Bytes(n) => {
            let mut start = 0u64;
            let mut index = 0;
            while start < len {
                let end = start.saturating_add(n as u64).min(len);
                let path = outputs.create(index)?;
                let mut out = OpenOptions::new().append(true).open(path)?;
                range(&mut data, start, end, &mut out)?;
                index += 1;
                start = end;
            }
        }
        Mode::Lines(n) => {
            let scanner = BufReader::new(spool.independent_reader()?);
            let mut count = 0usize;
            let mut start = 0u64;
            let mut index = 0;
            scan_records(scanner, sep, |_, end| {
                count += 1;
                if count == n {
                    let path = outputs.create(index)?;
                    let mut out = OpenOptions::new().append(true).open(path)?;
                    range(&mut data, start, end, &mut out)?;
                    index += 1;
                    start = end;
                    count = 0;
                }
                Ok(())
            })?;
            if start < len {
                let path = outputs.create(index)?;
                let mut out = OpenOptions::new().append(true).open(path)?;
                range(&mut data, start, len, &mut out)?;
            }
        }
        Mode::LineBytes(n) => {
            let mut start = 0u64;
            let mut index = 0;
            let mut scan = spool.independent_reader()?;
            let mut buf = [0u8; 65536];
            while start < len {
                let limit = start.saturating_add(n as u64).min(len);
                let mut pos = start;
                let mut last = None;
                scan.seek(SeekFrom::Start(start))?;
                while pos < limit {
                    let cap = ((limit - pos) as usize).min(buf.len());
                    let got = scan.read(&mut buf[..cap])?;
                    if got == 0 {
                        break;
                    }
                    for (i, &b) in buf[..got].iter().enumerate() {
                        if b == sep {
                            last = Some(pos + i as u64 + 1);
                        }
                    }
                    pos += got as u64;
                }
                let end = if limit - start < n as u64 {
                    len
                } else {
                    last.unwrap_or(limit)
                };
                let path = outputs.create(index)?;
                let mut out = OpenOptions::new().append(true).open(path)?;
                range(&mut data, start, end, &mut out)?;
                index += 1;
                start = end;
            }
        }
        Mode::Number(kind, which, total) => {
            outputs.selected = which.map(|x| x - 1);
            if which.is_none() && !elide {
                for i in 0..total {
                    outputs.bucket(i, false)?;
                }
            }
            let part = (len / total as u64).max(1);
            if kind == 'b' {
                let mut start = 0u64;
                for i in 0..total {
                    let end = if i + 1 == total {
                        len
                    } else {
                        start.saturating_add(part).min(len)
                    };
                    if start < end {
                        write_piece(&mut data, start, end, i, outputs, elide)?;
                    }
                    start = end;
                }
            } else {
                let scanner = BufReader::new(spool.independent_reader()?);
                let mut line = 0usize;
                scan_records(scanner, sep, |start, end| {
                    let bucket = if kind == 'r' {
                        line % total
                    } else {
                        ((start / part) as usize).min(total - 1)
                    };
                    line += 1;
                    write_piece(&mut data, start, end, bucket, outputs, elide)
                })?;
            }
        }
    }
    Ok(())
}

fn round_robin_live(
    input: &str,
    total: usize,
    selected: Option<usize>,
    sep: u8,
    outputs: &mut Outputs<'_>,
    elide: bool,
) -> io::Result<()> {
    let mut reader = common_io::input(input)?;
    outputs.selected = selected.map(|n| n - 1);
    if selected.is_none() && !elide {
        for i in 0..total {
            outputs.bucket(i, false)?;
        }
    }
    let mut bucket = 0usize;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let len = bytes.len();
        let mut start = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            if byte == sep {
                write_live(&bytes[start..=i], bucket, outputs, elide)?;
                bucket = (bucket + 1) % total;
                start = i + 1;
            }
        }
        if start < len {
            write_live(&bytes[start..], bucket, outputs, elide)?;
        }
        reader.consume(len);
    }
    Ok(())
}

fn write_live(
    bytes: &[u8],
    bucket: usize,
    outputs: &mut Outputs<'_>,
    elide: bool,
) -> io::Result<()> {
    if let Some(selected) = outputs.selected {
        if selected == bucket {
            let mut out = io::stdout().lock();
            out.write_all(bytes)?;
            out.flush()?;
        }
    } else {
        let path = outputs.bucket(bucket, elide)?.unwrap();
        OpenOptions::new()
            .append(true)
            .open(path)?
            .write_all(bytes)?;
    }
    Ok(())
}

fn new_output(outputs: &mut Outputs<'_>, index: usize) -> io::Result<File> {
    let path = outputs.create(index)?;
    OpenOptions::new().append(true).open(path)
}

fn live_regular(input: &str, mode: Mode, sep: u8, outputs: &mut Outputs<'_>) -> io::Result<()> {
    let mut reader = common_io::input(input)?;
    if let Mode::LineBytes(limit) = mode {
        return live_line_bytes(&mut *reader, limit, sep, outputs);
    }
    let mut current: Option<File> = None;
    let mut index = 0usize;
    let mut count = 0usize;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let len = bytes.len();
        let mut pos = 0;
        while pos < len {
            if current.is_none() {
                current = Some(new_output(outputs, index)?);
            }
            let (end, complete) = match mode {
                Mode::Bytes(limit) => {
                    let take = (limit - count).min(len - pos);
                    count += take;
                    (pos + take, count == limit)
                }
                Mode::Lines(limit) => {
                    let next = bytes[pos..]
                        .iter()
                        .position(|&byte| byte == sep)
                        .map_or(len, |n| pos + n + 1);
                    if next > pos && bytes[next - 1] == sep {
                        count += 1;
                    }
                    (next, count == limit)
                }
                _ => unreachable!(),
            };
            current.as_mut().unwrap().write_all(&bytes[pos..end])?;
            pos = end;
            if complete {
                current = None;
                count = 0;
                index += 1;
            }
        }
        reader.consume(len);
    }
    Ok(())
}

fn live_line_bytes(
    reader: &mut dyn BufRead,
    limit: usize,
    sep: u8,
    outputs: &mut Outputs<'_>,
) -> io::Result<()> {
    let mut spool = Spool::new()?;
    let file = spool.file.as_mut().unwrap();
    let mut pending = 0usize;
    let mut last = None;
    let mut index = 0usize;
    let mut move_buffer = [0u8; 65536];
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let len = bytes.len();
        let mut pos = 0usize;
        while pos < len {
            let take = (limit - pending).min(len - pos);
            for (i, &byte) in bytes[pos..pos + take].iter().enumerate() {
                if byte == sep {
                    last = Some(pending + i + 1);
                }
            }
            file.write_all(&bytes[pos..pos + take])?;
            pending += take;
            pos += take;
            if pending == limit {
                let end = last.unwrap_or(limit);
                let mut out = new_output(outputs, index)?;
                range(file, 0, end as u64, &mut out)?;
                index += 1;
                let mut from = end;
                let mut to = 0usize;
                while from < pending {
                    let take = (pending - from).min(move_buffer.len());
                    file.seek(SeekFrom::Start(from as u64))?;
                    file.read_exact(&mut move_buffer[..take])?;
                    file.seek(SeekFrom::Start(to as u64))?;
                    file.write_all(&move_buffer[..take])?;
                    from += take;
                    to += take;
                }
                pending -= end;
                file.set_len(pending as u64)?;
                file.seek(SeekFrom::Start(pending as u64))?;
                last = None;
            }
        }
        reader.consume(len);
    }
    if pending != 0 {
        let mut out = new_output(outputs, index)?;
        range(file, 0, pending as u64, &mut out)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    input: &str,
    prefix: &str,
    mode: Mode,
    sep: u8,
    width: usize,
    fixed: bool,
    base: u8,
    first: usize,
    extra: &str,
    elide: bool,
    verbose: bool,
    unbuffered: bool,
) -> i32 {
    let mut outputs = Outputs {
        input,
        prefix,
        extra,
        width,
        fixed,
        base,
        first,
        verbose,
        selected: None,
        next: 0,
        bucket_names: std::collections::HashMap::new(),
    };
    let result = match (unbuffered, mode) {
        (true, Mode::Number('r', selected, total)) => {
            round_robin_live(input, total, selected, sep, &mut outputs, elide)
        }
        (_, Mode::Bytes(_) | Mode::Lines(_) | Mode::LineBytes(_)) => {
            live_regular(input, mode, sep, &mut outputs)
        }
        _ => run_inner(input, mode, sep, &mut outputs, elide),
    };
    match result {
        Ok(()) => 0,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("split: {}", crate::common::error::io_text(&e));
            1
        }
    }
}
