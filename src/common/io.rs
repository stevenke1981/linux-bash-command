//! Input and output helpers shared by the commands: `-` as stdin, buffered stdout,
//! record reading and BrokenPipe/flush policy.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Stdout, Write};

use super::error::{io_error_text, EXIT_FAIL, EXIT_OK};

const STDOUT_BUFFER: usize = 64 * 1024;

/// Opens an input operand. `-` and `/dev/stdin` read standard input. On Windows
/// `/dev/null` opens `NUL`.
pub fn open_input(path: &str) -> io::Result<Box<dyn BufRead>> {
    if path == "-" || path == "/dev/stdin" {
        return Ok(Box::new(io::stdin().lock()));
    }
    let target = if cfg!(windows) && path == "/dev/null" {
        "NUL"
    } else {
        path
    };
    Ok(Box::new(BufReader::new(File::open(target)?)))
}

/// Standard output wrapped in a 64 KiB buffer. Callers must finish with [`finish_output`].
pub fn stdout_writer() -> BufWriter<Stdout> {
    BufWriter::with_capacity(STDOUT_BUFFER, io::stdout())
}

/// True when a write failed because the reader went away.
pub fn is_broken_pipe(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
}

/// Reads one record ending in `delim` (kept in `buf`), replacing the previous contents.
/// Returns `Ok(false)` at end of input.
pub fn read_record(reader: &mut dyn BufRead, delim: u8, buf: &mut Vec<u8>) -> io::Result<bool> {
    buf.clear();
    Ok(reader.read_until(delim, buf)? > 0)
}

/// Flushes `w` and converts the result to an exit status. A closed pipe is silent success;
/// other failures print `cmd: write error: <reason>`.
pub fn finish_output(cmd: &str, w: &mut impl Write) -> i32 {
    match w.flush() {
        Ok(()) => EXIT_OK,
        Err(error) if is_broken_pipe(&error) => EXIT_OK,
        Err(error) => {
            eprintln!("{cmd}: write error: {}", io_error_text(&error));
            EXIT_FAIL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_pipe_is_detected_by_kind() {
        assert!(is_broken_pipe(&io::Error::from(io::ErrorKind::BrokenPipe)));
        assert!(!is_broken_pipe(&io::Error::from(io::ErrorKind::NotFound)));
    }

    #[test]
    fn records_keep_delimiters_and_end_at_eof() {
        let data: &[u8] = b"one\ntwo\nlast";
        let mut reader: Box<dyn BufRead> = Box::new(data);
        let mut buf = Vec::new();
        assert!(read_record(reader.as_mut(), b'\n', &mut buf).unwrap());
        assert_eq!(buf, b"one\n");
        assert!(read_record(reader.as_mut(), b'\n', &mut buf).unwrap());
        assert_eq!(buf, b"two\n");
        assert!(read_record(reader.as_mut(), b'\n', &mut buf).unwrap());
        assert_eq!(buf, b"last");
        assert!(!read_record(reader.as_mut(), b'\n', &mut buf).unwrap());
        assert!(buf.is_empty());
    }

    #[test]
    fn records_split_on_nul_when_asked() {
        let data: &[u8] = b"a\0b\0";
        let mut reader: Box<dyn BufRead> = Box::new(data);
        let mut buf = Vec::new();
        assert!(read_record(reader.as_mut(), 0, &mut buf).unwrap());
        assert_eq!(buf, b"a\0");
        assert!(read_record(reader.as_mut(), 0, &mut buf).unwrap());
        assert_eq!(buf, b"b\0");
        assert!(!read_record(reader.as_mut(), 0, &mut buf).unwrap());
    }

    struct Failing(io::ErrorKind);

    impl Write for Failing {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from(self.0))
        }
    }

    #[test]
    fn finish_output_maps_flush_results() {
        assert_eq!(finish_output("demo", &mut Vec::new()), 0);
        assert_eq!(
            finish_output("demo", &mut Failing(io::ErrorKind::BrokenPipe)),
            0
        );
        assert_eq!(
            finish_output("demo", &mut Failing(io::ErrorKind::StorageFull)),
            1
        );
    }

    #[test]
    fn open_input_reads_files_and_reports_missing_ones() {
        let dir = std::env::temp_dir().join(format!("agentcmd-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("input.txt");
        std::fs::write(&file, b"hello\n").unwrap();
        let mut reader = open_input(&file.to_string_lossy()).unwrap();
        let mut text = String::new();
        reader.read_line(&mut text).unwrap();
        assert_eq!(text, "hello\n");
        let missing = dir.join("missing.txt");
        let error = open_input(&missing.to_string_lossy()).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stdout_writer_is_constructible() {
        let writer = stdout_writer();
        assert_eq!(writer.capacity(), STDOUT_BUFFER);
    }
}
