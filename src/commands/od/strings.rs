use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Candidate {
    path: PathBuf,
    file: Option<File>,
}
impl Candidate {
    fn create() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let path = std::env::temp_dir().join(format!(
                "agentcmd-od-{}-{}",
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
                    return Ok(Self {
                        path,
                        file: Some(file),
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("cannot create temporary string buffer"))
    }
    fn reset(&mut self) -> io::Result<()> {
        let f = self.file.as_mut().unwrap();
        f.set_len(0)?;
        f.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}
impl Drop for Candidate {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) fn run(
    input: &mut dyn Read,
    out: &mut dyn Write,
    mut position: usize,
    mut remaining: usize,
    min: usize,
    radix: char,
) -> io::Result<()> {
    let mut temp = Candidate::create()?;
    let mut buffer = Vec::with_capacity(8192);
    let mut scratch = [0u8; 8192];
    let mut length = 0usize;
    let mut start = position;
    while remaining != 0 {
        let cap = remaining.min(scratch.len());
        let n = input.read(&mut scratch[..cap])?;
        if n == 0 {
            break;
        }
        for &byte in &scratch[..n] {
            if byte.is_ascii_graphic() || byte == b' ' {
                if length == 0 {
                    start = position;
                }
                buffer.push(byte);
                length += 1;
                if buffer.len() == 8192 {
                    temp.file.as_mut().unwrap().write_all(&buffer)?;
                    buffer.clear();
                }
            } else {
                if byte == 0 && length >= min {
                    if length == 0 {
                        start = position;
                    }
                    let file = temp.file.as_mut().unwrap();
                    file.write_all(&buffer)?;
                    file.seek(SeekFrom::Start(0))?;
                    if radix != 'n' {
                        write!(out, "{} ", super::address(start, radix))?;
                    }
                    io::copy(file, out)?;
                    writeln!(out)?;
                }
                if length != 0 {
                    buffer.clear();
                    length = 0;
                    temp.reset()?;
                }
            }
            position += 1;
        }
        remaining -= n;
    }
    Ok(())
}
