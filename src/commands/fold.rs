use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::io::Write;

fn column(column: usize, byte: u8, bytes: bool) -> usize {
    if bytes {
        return column + 1;
    }
    match byte {
        b'\t' => column / 8 * 8 + 8,
        8 => column.saturating_sub(1),
        b'\r' => 0,
        _ => column + 1,
    }
}

fn fold(
    line: &[u8],
    width: usize,
    bytes: bool,
    spaces: bool,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let (mut start, mut pos, mut col, mut blank) = (0, 0, 0, None);
    while pos < line.len() {
        let next = column(col, line[pos], bytes);
        if next > width && pos > start {
            let end = if spaces {
                blank.filter(|&n| n > start).unwrap_or(pos)
            } else {
                pos
            };
            out.write_all(&line[start..end])?;
            out.write_all(b"\n")?;
            start = end;
            pos = end;
            col = 0;
            blank = None;
        } else {
            col = next;
            pos += 1;
            if matches!(line[pos - 1], b' ' | b'\t') {
                blank = Some(pos);
            }
        }
    }
    out.write_all(&line[start..])
}

pub fn run(args: &[String]) -> i32 {
    let help = "Usage: fold [OPTION]... [FILE]...\n  -b, --bytes\n  -s, --spaces\n  -w, --width=WIDTH\n  --help --version\n";
    let mut options = true;
    let args: Vec<_> = args
        .iter()
        .map(|s| {
            if s == "--" {
                options = false;
                return s.clone();
            }
            if !options {
                return s.clone();
            }
            if std::env::var_os("POSIXLY_CORRECT").is_some() && !s.starts_with('-') {
                options = false;
            }

            if s.starts_with('-') && s[1..].bytes().all(|b| b.is_ascii_digit()) && s.len() > 1 {
                format!("--width={}", &s[1..])
            } else {
                s.clone()
            }
        })
        .collect();
    let parsed = match args::parse_command(
        "fold",
        &args,
        help,
        &[
            OptSpec::flag("bytes", 'b'),
            OptSpec::flag("spaces", 's'),
            OptSpec::value("width", 'w'),
        ],
        true,
    ) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let (mut bytes, mut spaces, mut width, mut files) = (false, false, 80, Vec::new());
    for item in parsed {
        match item {
            Item::Opt("bytes", _) => bytes = true,
            Item::Opt("spaces", _) => spaces = true,
            Item::Opt("width", Some(v)) => match v.parse::<usize>() {
                Ok(n) if n > 0 => width = n,
                _ => return error::usage("fold", format!("invalid number of columns: '{v}'"), 1),
            },
            Item::Operand(s) => files.push(s),
            _ => unreachable!(),
        }
    }
    let mut failed = false;
    let code = io::output("fold", |out| {
        failed = io::for_inputs("fold", &files, |_, reader| {
            let mut line = Vec::new();
            while reader.read_until(b'\n', &mut line)? != 0 {
                let newline = line.last() == Some(&b'\n');
                if newline {
                    line.pop();
                }
                fold(&line, width, bytes, spaces, out)?;
                if newline {
                    out.write_all(b"\n")?;
                }
                line.clear();
            }
            Ok(())
        })?;
        Ok(())
    });
    code.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(s: &[u8], w: usize, b: bool, sp: bool) -> Vec<u8> {
        let mut out = Vec::new();
        fold(s, w, b, sp, &mut out).unwrap();
        out
    }
    #[test]
    fn hard() {
        assert_eq!(f(b"abcde", 2, false, false), b"ab\ncd\ne");
    }
    #[test]
    fn spaces() {
        assert_eq!(f(b"a bc de", 4, false, true), b"a \nbc \nde");
    }
    #[test]
    fn tab() {
        assert_eq!(f(b"a\tb", 8, false, false), b"a\t\nb");
    }
    #[test]
    fn bytes() {
        assert_eq!(f(b"a\tb", 2, true, false), b"a\t\nb");
    }
    #[test]
    fn controls() {
        assert_eq!(f(b"ab\rabc\x08d", 3, false, false), b"ab\rabc\x08d");
    }
    #[test]
    fn empty() {
        assert!(f(b"", 1, false, false).is_empty());
    }
}
