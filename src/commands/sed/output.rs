//! Output streams for sed. A stream remembers when its last line was written without
//! a delimiter, so that any later output on it starts on a new line, as GNU sed does.

use std::io::{self, Write};

pub struct Output {
    writer: Box<dyn Write>,
    missing_newline: bool,
}

impl Output {
    pub fn new(writer: Box<dyn Write>) -> Self {
        Output {
            writer,
            missing_newline: false,
        }
    }

    /// Writes raw text (`a`, `i`, `=`, `l`, `r` output).
    pub fn text(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.complete_line()?;
        self.writer.write_all(bytes)
    }

    /// Writes a pattern-space style line. The delimiter is written only when the line
    /// had one on input.
    pub fn line(&mut self, text: &[u8], chomped: bool, delim: u8) -> io::Result<()> {
        self.complete_line()?;
        self.writer.write_all(text)?;
        if chomped {
            self.writer.write_all(&[delim])
        } else {
            self.missing_newline = true;
            Ok(())
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    fn complete_line(&mut self) -> io::Result<()> {
        if self.missing_newline {
            self.missing_newline = false;
            self.writer.write_all(b"\n")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A writer whose bytes can be inspected after the output is dropped.
    #[derive(Clone, Default)]
    struct Shared(Rc<RefCell<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn capture() -> (Output, Shared) {
        let shared = Shared::default();
        (Output::new(Box::new(shared.clone())), shared)
    }

    #[test]
    fn terminated_lines_get_the_delimiter() {
        let (mut out, shared) = capture();
        out.line(b"a", true, b'\n').unwrap();
        out.line(b"b", true, 0).unwrap();
        assert_eq!(*shared.0.borrow(), b"a\nb\0");
    }

    #[test]
    fn unterminated_line_gets_newline_only_before_more_output() {
        let (mut out, shared) = capture();
        out.line(b"a", false, b'\n').unwrap();
        assert_eq!(*shared.0.borrow(), b"a");
        out.text(b"X\n").unwrap();
        assert_eq!(*shared.0.borrow(), b"a\nX\n");
    }

    #[test]
    fn duplicate_unterminated_line_is_separated() {
        let (mut out, shared) = capture();
        out.line(b"a", false, b'\n').unwrap();
        out.line(b"a", false, b'\n').unwrap();
        assert_eq!(*shared.0.borrow(), b"a\na");
    }
}
