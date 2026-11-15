use std::io::{self, BufReader, BufWriter, Read, Write};

#[derive(Clone, Copy, Default)]
pub(super) struct Settings {
    pub to_dos: bool,
    pub force: bool,
    pub keep_bom: bool,
    pub remove_bom: bool,
    pub add_bom: bool,
    pub add_eol: bool,
}

pub(super) fn convert(input: impl Read, output: impl Write, o: Settings) -> io::Result<bool> {
    let mut input = BufReader::with_capacity(65536, input).bytes();
    let mut output = BufWriter::with_capacity(65536, output);
    let mut prefix = Vec::with_capacity(3);
    for _ in 0..3 {
        match input.next() {
            Some(v) => prefix.push(v?),
            None => break,
        }
    }
    let bom = prefix == [0xef, 0xbb, 0xbf];
    if o.add_bom || (bom && !o.remove_bom && (o.to_dos || o.keep_bom)) {
        output.write_all(&[0xef, 0xbb, 0xbf])?;
    }
    let prefix = if bom { &[][..] } else { &prefix[..] };
    let ok = write_body(prefix.iter().copied().map(Ok).chain(input), &mut output, o)?;
    output.flush()?;
    Ok(ok)
}

fn write_body(
    input: impl Iterator<Item = io::Result<u8>>,
    output: &mut dyn Write,
    o: Settings,
) -> io::Result<bool> {
    let mut pending_cr = false;
    let mut last_newline = true;
    let mut has_data = false;
    for next in input {
        let byte = next?;
        if !o.force && byte < 32 && !matches!(byte, b'\t' | b'\n' | 12 | b'\r') {
            return Ok(false);
        }
        has_data = true;
        if pending_cr {
            if o.to_dos && byte == b'\r' {
                output.write_all(b"\r\r")?;
                pending_cr = false;
                last_newline = false;
                continue;
            }
            if byte == b'\n' {
                output.write_all(if o.to_dos { b"\r\n" } else { b"\n" })?;
                last_newline = true;
                pending_cr = false;
                continue;
            }
            output.write_all(b"\r")?;
            pending_cr = false;
            last_newline = false;
        }
        match byte {
            b'\r' => pending_cr = true,
            b'\n' => {
                output.write_all(if o.to_dos { b"\r\n" } else { b"\n" })?;
                last_newline = true;
            }
            _ => {
                output.write_all(&[byte])?;
                last_newline = false;
            }
        }
    }
    finish(output, o, pending_cr, has_data, last_newline)?;
    Ok(true)
}
fn finish(
    output: &mut dyn Write,
    o: Settings,
    pending_cr: bool,
    has_data: bool,
    mut last_newline: bool,
) -> io::Result<()> {
    if pending_cr {
        output.write_all(b"\r")?;
        last_newline = false;
    }
    if o.add_eol && has_data && !last_newline {
        output.write_all(if o.to_dos { b"\r\n" } else { b"\n" })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn converted(data: &[u8], settings: Settings) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        convert(data, &mut out, settings).unwrap().then_some(out)
    }
    #[test]
    fn line_endings_across_chunks() {
        let mut input = vec![b'x'; 65535];
        input.extend_from_slice(b"\r\ny\r\nz");
        let out = converted(&input, Settings::default()).unwrap();
        assert_eq!(&out[65535..], b"\ny\nz");
        let o = Settings {
            to_dos: true,
            ..Settings::default()
        };
        assert_eq!(converted(b"\r\r\n", o).unwrap(), b"\r\r\r\n");
    }
    #[test]
    fn bom_variants() {
        let dos = Settings {
            to_dos: true,
            ..Settings::default()
        };
        assert_eq!(
            converted(
                b"x\n",
                Settings {
                    keep_bom: true,
                    ..dos
                }
            )
            .unwrap(),
            b"x\r\n"
        );
        assert_eq!(
            converted(
                b"\xef\xbb\xbfx",
                Settings {
                    add_bom: true,
                    ..Settings::default()
                }
            )
            .unwrap(),
            b"\xef\xbb\xbfx"
        );
    }
    #[test]
    fn binary_and_force() {
        assert!(converted(b"a\0b", Settings::default()).is_none());
        assert!(converted(b"a\x1bb", Settings::default()).is_none());
        assert_eq!(
            converted(b"a\x0cb", Settings::default()).unwrap(),
            b"a\x0cb"
        );
        assert_eq!(
            converted(
                b"a\0b",
                Settings {
                    force: true,
                    ..Settings::default()
                }
            )
            .unwrap(),
            b"a\0b"
        );
    }
    #[test]
    fn final_newline_and_empty() {
        let o = Settings {
            add_eol: true,
            ..Settings::default()
        };
        assert_eq!(converted(b"a\r", o).unwrap(), b"a\r\n");
        assert!(converted(b"", o).unwrap().is_empty());
    }
}
