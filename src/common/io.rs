use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};

pub const BUFFER_SIZE: usize = 65536;

pub fn input(path: &str) -> io::Result<Box<dyn BufRead>> {
    match path {
        "-" | "/dev/stdin" => Ok(Box::new(BufReader::with_capacity(BUFFER_SIZE, io::stdin()))),
        "/dev/null" => Ok(Box::new(io::empty())),
        _ => Ok(Box::new(BufReader::with_capacity(
            BUFFER_SIZE,
            File::open(path)?,
        ))),
    }
}

pub fn output(command: &str, action: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> i32 {
    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(BUFFER_SIZE, stdout.lock());
    match action(&mut out).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("{command}: write error: {}", super::error::io_text(&error));
            1
        }
    }
}

pub fn for_inputs(
    command: &str,
    files: &[String],
    mut action: impl FnMut(&str, &mut dyn BufRead) -> io::Result<()>,
) -> io::Result<bool> {
    let default = ["-".to_owned()];
    let mut failed = false;
    for name in if files.is_empty() { &default } else { files } {
        match input(name) {
            Ok(mut reader) => action(name, &mut reader)?,
            Err(error) => {
                eprintln!("{command}: {name}: {}", super::error::io_text(&error));
                failed = true;
            }
        }
    }
    Ok(failed)
}

pub fn records(
    reader: &mut dyn BufRead,
    delimiter: u8,
    mut action: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut line = Vec::new();
    while reader.read_until(delimiter, &mut line)? != 0 {
        if line.last() == Some(&delimiter) {
            line.pop();
        }
        action(&line)?;
        line.clear();
    }
    Ok(())
}

/// A closed reader is not a failure of the command; it exits quietly like a SIGPIPE'd tool.
pub fn is_closed_pipe(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
}

pub fn report_open(command: &str, path: &str, error: &io::Error) {
    eprintln!(
        "{command}: cannot open '{path}' for reading: {}",
        super::error::io_text(error)
    );
}

pub fn report_failure(command: &str, path: &str, error: &io::Error) {
    eprintln!("{command}: '{path}': {}", super::error::io_text(error));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collect(data: &[u8], delim: u8) -> Vec<Vec<u8>> {
        let mut got = Vec::new();
        records(&mut io::Cursor::new(data), delim, |r| {
            got.push(r.to_vec());
            Ok(())
        })
        .unwrap();
        got
    }
    #[test]
    fn empty() {
        assert!(collect(b"", b'\n').is_empty());
    }
    #[test]
    fn lines() {
        assert_eq!(collect(b"a\nb\n", b'\n'), [b"a", b"b"]);
    }
    #[test]
    fn nul() {
        assert_eq!(collect(b"a\0b\0", 0), [b"a", b"b"]);
    }
    #[test]
    fn partial() {
        assert_eq!(collect(b"x", 0), [b"x"]);
    }
    #[test]
    fn raw() {
        assert_eq!(collect(b"\xff\n", b'\n'), [b"\xff"]);
    }
}
