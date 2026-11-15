use crate::common::io as common_io;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) struct Data {
    path: PathBuf,
    file: Option<File>,
    pub(super) lines: usize,
}
impl Data {
    pub(super) fn open(input: &str) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let path = std::env::temp_dir().join(format!(
                "agentcmd-csplit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    let mut file = Some(file);
                    let result = (|| {
                        let mut source = common_io::input(input)?;
                        io::copy(&mut source, file.as_mut().unwrap())?;
                        let mut scan_file = file.as_ref().unwrap().try_clone()?;
                        scan_file.seek(SeekFrom::Start(0))?;
                        let scan = BufReader::new(scan_file);
                        let lines = count_lines(scan)?;
                        Ok(Self {
                            path: path.clone(),
                            file: file.take(),
                            lines,
                        })
                    })();
                    if result.is_err() {
                        drop(file.take());
                        let _ = fs::remove_file(&path);
                    }
                    return result;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("cannot create temporary input"))
    }
    pub(super) fn offset(&self, line: usize) -> io::Result<u64> {
        if line == 0 {
            return Ok(0);
        }
        let mut file = self.file.as_ref().unwrap().try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::new(file);
        let mut position = 0u64;
        let mut count = 0usize;
        loop {
            let bytes = reader.fill_buf()?;
            if bytes.is_empty() {
                return Ok(position);
            }
            let len = bytes.len();
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'\n' {
                    count += 1;
                    if count == line {
                        return Ok(position + i as u64 + 1);
                    }
                }
            }
            position += len as u64;
            reader.consume(len);
        }
    }
    pub(super) fn find(
        &self,
        matches: impl Fn(&[u8]) -> bool,
        start: usize,
    ) -> io::Result<Option<usize>> {
        let mut file = self.file.as_ref().unwrap().try_clone()?;
        file.seek(SeekFrom::Start(self.offset(start)?))?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        let mut index = start;
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                return Ok(None);
            }
            if matches(line.strip_suffix(b"\n").unwrap_or(&line)) {
                return Ok(Some(index));
            }
            index += 1;
        }
    }
    pub(super) fn copy_range(&self, start: u64, end: u64, out: &mut dyn Write) -> io::Result<()> {
        let mut file = self.file.as_ref().unwrap().try_clone()?;
        file.seek(SeekFrom::Start(start))?;
        io::copy(&mut file.take(end - start), out)?;
        Ok(())
    }
}
impl Drop for Data {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}
fn count_lines(mut reader: BufReader<File>) -> io::Result<usize> {
    let mut count = 0usize;
    let mut last_newline = true;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        count = count
            .checked_add(bytes.iter().filter(|&&b| b == b'\n').count())
            .ok_or_else(|| io::Error::other("too many lines"))?;
        last_newline = bytes.last() == Some(&b'\n');
        let len = bytes.len();
        reader.consume(len);
    }
    Ok(count + usize::from(!last_newline))
}
