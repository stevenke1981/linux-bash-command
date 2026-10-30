use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::io::BufRead;
const HELP: &str = "Usage: paste [OPTION]... [FILE]...\nMerge corresponding lines of files.\n  -d, --delimiters=LIST     reuse delimiters from LIST\n  -s, --serial              paste one file at a time\n  -z, --zero-terminated     NUL line delimiter\n      --help                display help\n      --version             display version\n";
const SPECS: &[OptSpec] = &[
    OptSpec::value("delimiters", 'd'),
    OptSpec::flag("serial", 's'),
    OptSpec::flag("zero-terminated", 'z'),
    OptSpec::flag("help", '\0'),
    OptSpec::flag("version", '\0'),
];

fn delimiters(list: &str) -> Vec<Option<u8>> {
    let mut bytes = list.bytes();
    let mut out = Vec::new();
    while let Some(c) = bytes.next() {
        out.push(if c == b'\\' {
            match bytes.next() {
                Some(b'0') => None,
                Some(b'n') => Some(b'\n'),
                Some(b't') => Some(b'\t'),
                Some(c) => Some(c),
                None => Some(b'\\'),
            }
        } else {
            Some(c)
        });
    }
    if out.is_empty() {
        out.push(None);
    }
    out
}

fn line(reader: &mut dyn BufRead, delimiter: u8, buf: &mut Vec<u8>) -> std::io::Result<bool> {
    buf.clear();
    let n = reader.read_until(delimiter, buf)?;
    if buf.last() == Some(&delimiter) {
        buf.pop();
    }
    Ok(n != 0)
}

fn serial(
    reader: &mut dyn BufRead,
    out: &mut dyn std::io::Write,
    delims: &[Option<u8>],
    record: u8,
) -> std::io::Result<()> {
    let (mut buf, mut count) = (Vec::new(), 0);
    while line(reader, record, &mut buf)? {
        if count != 0 {
            if let Some(d) = delims[(count - 1) % delims.len()] {
                out.write_all(&[d])?;
            }
        }
        out.write_all(&buf)?;
        count += 1;
    }
    out.write_all(&[record])
}

enum Source {
    Stdin,
    File(Box<dyn BufRead>),
}

pub fn run(argv: &[String]) -> i32 {
    let items = match args::parse(argv, SPECS, true) {
        Ok(v) => v,
        Err(e) => return error::usage("paste", &e, 1),
    };
    if let Some(id) = items.iter().find_map(|item| match item {
        Item::Opt(id @ ("help" | "version"), _) => Some(*id),
        _ => None,
    }) {
        return args::help_version("paste", &[format!("--{id}")], HELP, true).unwrap();
    }
    let (mut names, mut delims, mut is_serial, mut record) =
        (Vec::new(), vec![Some(b'\t')], false, b'\n');
    for item in items {
        match item {
            Item::Operand(f) => names.push(f),
            Item::Opt("delimiters", v) => delims = delimiters(&v.unwrap()),
            Item::Opt("serial", _) => is_serial = true,
            Item::Opt("zero-terminated", _) => record = 0,
            _ => {}
        }
    }
    if names.is_empty() {
        names.push("-".into());
    }
    let mut sources = Vec::new();
    for name in names {
        if name == "-" || name == "/dev/stdin" {
            sources.push(Source::Stdin);
        } else {
            match io::input(&name) {
                Ok(r) => sources.push(Source::File(r)),
                Err(e) => {
                    eprintln!("paste: {name}: {}", error::io_text(&e));
                    return 1;
                }
            }
        }
    }
    io::output("paste", |out| {
        let mut stdin = io::input("-")?;
        if is_serial {
            for source in &mut sources {
                let reader: &mut dyn BufRead = match source {
                    Source::Stdin => &mut stdin,
                    Source::File(r) => r,
                };
                serial(reader, out, &delims, record)?;
            }
        } else {
            let mut columns = vec![Vec::new(); sources.len()];
            loop {
                let mut any = false;
                for (source, column) in sources.iter_mut().zip(&mut columns) {
                    let reader: &mut dyn BufRead = match source {
                        Source::Stdin => &mut stdin,
                        Source::File(r) => r,
                    };
                    any |= line(reader, record, column)?;
                }
                if !any {
                    break;
                }
                for (i, column) in columns.iter().enumerate() {
                    if i != 0 {
                        if let Some(d) = delims[(i - 1) % delims.len()] {
                            out.write_all(&[d])?;
                        }
                    }
                    out.write_all(column)?;
                }
                out.write_all(&[record])?;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes() {
        assert_eq!(
            delimiters("\\n\\t\\\\\\0"),
            [Some(10), Some(9), Some(92), None]
        );
    }
    #[test]
    fn empty_delims() {
        assert_eq!(delimiters(""), [None]);
    }
    #[test]
    fn literal_delims() {
        assert_eq!(delimiters(",;"), [Some(44), Some(59)]);
    }
    #[test]
    fn serial_cycle() {
        let mut out = Vec::new();
        serial(
            &mut std::io::Cursor::new(b"a\nb\nc\nd"),
            &mut out,
            &delimiters(",;"),
            10,
        )
        .unwrap();
        assert_eq!(out, b"a,b;c,d\n");
    }
    #[test]
    fn serial_empty() {
        let mut out = Vec::new();
        serial(&mut std::io::Cursor::new(b""), &mut out, &[None], 10).unwrap();
        assert_eq!(out, b"\n");
    }
    #[test]
    fn nul_records() {
        let mut out = Vec::new();
        serial(
            &mut std::io::Cursor::new(b"a\0b\0"),
            &mut out,
            &[Some(9)],
            0,
        )
        .unwrap();
        assert_eq!(out, b"a\tb\0");
    }
}
