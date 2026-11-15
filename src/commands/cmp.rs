use crate::common::{args, error};
use std::io::{BufReader, Read, Write};

const HELP: &str = "Usage: cmp [OPTION]... FILE1 [FILE2 [SKIP1 [SKIP2]]]\nCompare two files byte by byte.\n  -b, --print-bytes          print differing bytes\n  -i, --ignore-initial=SKIP1[:SKIP2]  skip bytes at start\n  -l, --verbose              output all differing bytes\n  -n, --bytes=LIMIT          compare at most LIMIT bytes\n  -s, --quiet, --silent      suppress output\n      --help                 display this help\n      --version              output version information\n";

#[derive(Default, Debug)]
struct Opt {
    bytes: Option<u64>,
    skip1: u64,
    skip2: u64,
    verbose: bool,
    print: bool,
    quiet: bool,
    files: Vec<String>,
    width: usize,
}
fn specs() -> [args::OptSpec; 8] {
    [
        args::OptSpec::flag("print-bytes", 'b'),
        args::OptSpec::value("ignore-initial", 'i'),
        args::OptSpec::flag("verbose", 'l'),
        args::OptSpec::value("bytes", 'n'),
        args::OptSpec::flag("quiet", 's'),
        args::OptSpec::flag("silent", '\0'),
        args::OptSpec::flag("help", '\0'),
        args::OptSpec::flag("version", '\0'),
    ]
}
fn parse(a: &[String]) -> Result<Opt, i32> {
    let items =
        args::parse(a, &specs(), true).map_err(|message| error::usage("cmp", message, 2))?;
    if let Some(args::Item::Opt(which, _)) = items
        .iter()
        .find(|item| matches!(item, args::Item::Opt("help" | "version", _)))
    {
        return Err(args::help_version("cmp", &[format!("--{which}")], HELP, true).unwrap());
    }
    let mut o = Opt::default();
    for item in items {
        match item {
            args::Item::Operand(x) => o.files.push(x),
            args::Item::Opt("print-bytes", _) => o.print = true,
            args::Item::Opt("verbose", _) => o.verbose = true,
            args::Item::Opt("quiet" | "silent", _) => o.quiet = true,
            args::Item::Opt("bytes", Some(x)) => {
                o.bytes =
                    Some(number(&x).ok_or_else(|| {
                        error::usage("cmp", format!("invalid byte limit: '{x}'"), 2)
                    })?)
            }
            args::Item::Opt("ignore-initial", Some(x)) => {
                let p = x.split(':').collect::<Vec<_>>();
                if p.len() > 2 {
                    return Err(error::usage("cmp", "invalid skip count", 2));
                }
                o.skip1 =
                    number(p[0]).ok_or_else(|| error::usage("cmp", "invalid skip count", 2))?;
                o.skip2 = if p.len() == 2 {
                    number(p[1]).ok_or_else(|| error::usage("cmp", "invalid skip count", 2))?
                } else {
                    o.skip1
                };
            }
            _ => unreachable!(),
        }
    }
    finish_operands(o)
}
fn finish_operands(mut o: Opt) -> Result<Opt, i32> {
    if o.files.len() > 4 {
        return Err(error::usage("cmp", "extra operand", 2));
    }
    if o.files.len() >= 3 {
        let skip1 =
            number(&o.files[2]).ok_or_else(|| error::usage("cmp", "invalid skip count", 2))?;
        o.skip1 = o.skip1.max(skip1);
        if o.files.len() >= 4 {
            let skip2 =
                number(&o.files[3]).ok_or_else(|| error::usage("cmp", "invalid skip count", 2))?;
            o.skip2 = o.skip2.max(skip2);
        }
        o.files.truncate(2);
    }
    if o.files.is_empty() {
        return Err(error::usage("cmp", "missing operand", 2));
    }
    if o.files.len() == 1 {
        o.files.push("-".into());
    }
    Ok(o)
}
fn number(s: &str) -> Option<u64> {
    let s = s.strip_prefix('+').unwrap_or(s);
    let (digits, suffix, radix) =
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            let end = hex
                .find(|c: char| !c.is_ascii_hexdigit())
                .unwrap_or(hex.len());
            (&hex[..end], &hex[end..], 16)
        } else {
            let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
            let digits = &s[..end];
            (
                digits,
                &s[end..],
                if digits.len() > 1 && digits.starts_with('0') {
                    8
                } else {
                    10
                },
            )
        };
    let value = u64::from_str_radix(digits, radix).ok()?;
    if suffix.is_empty() {
        return Some(value);
    }
    let power = match suffix.as_bytes().first()? {
        b'k' | b'K' => 1,
        b'M' => 2,
        b'G' => 3,
        b'T' => 4,
        b'P' => 5,
        b'E' => 6,
        b'Z' => 7,
        b'Y' => 8,
        _ => return None,
    };
    let base: u64 = match &suffix[1..] {
        "" | "iB" => 1024,
        "B" => 1000,
        _ => return None,
    };
    Some(value.saturating_mul(base.saturating_pow(power)))
}
fn display(b: u8) -> String {
    let (prefix, b) = if b >= 128 { ("M-", b - 128) } else { ("", b) };
    let value = match b {
        0..=31 => format!("^{}", (b + 64) as char),
        127 => "^?".into(),
        _ => (b as char).to_string(),
    };
    format!("{prefix}{value}")
}
fn report_difference(
    out: &mut dyn std::io::Write,
    o: &Opt,
    offset: u64,
    line: u64,
    x: u8,
    y: u8,
) -> std::io::Result<()> {
    let width = o.width.max(1);
    if o.verbose {
        if o.print {
            writeln!(
                out,
                "{offset:width$} {x:3o} {:<4} {y:3o} {}",
                display(x),
                display(y)
            )
        } else {
            writeln!(out, "{offset:width$} {x:3o} {y:3o}")
        }
    } else if o.print {
        writeln!(
            out,
            "{} {} differ: byte {offset}, line {line} is {x:3o} {} {y:3o} {}",
            o.files[0],
            o.files[1],
            display(x),
            display(y)
        )
    } else {
        writeln!(
            out,
            "{} {} differ: byte {offset}, line {line}",
            o.files[0], o.files[1]
        )
    }
}
fn eof_message(o: &Opt, name: &str, offset: u64, line: u64) {
    if o.quiet {
        return;
    }
    if offset == 0 {
        eprintln!("cmp: EOF on {name} which is empty");
    } else {
        eprintln!("cmp: EOF on {name} after byte {offset}, line {line}");
    }
}
fn compare_streams(
    o: &Opt,
    left: &mut dyn Read,
    right: &mut dyn Read,
    out: &mut dyn std::io::Write,
) -> std::io::Result<i32> {
    skip(left, o.skip1)?;
    skip(right, o.skip2)?;
    let mut line = 1u64;
    let mut offset = 0u64;
    let mut differs = false;
    let mut last_newline = false;
    while offset < o.bytes.unwrap_or(u64::MAX) {
        let mut x = [0u8; 1];
        let mut y = [0u8; 1];
        let nx = left.read(&mut x)?;
        let ny = right.read(&mut y)?;
        if nx == 0 || ny == 0 {
            if nx == ny {
                return Ok(i32::from(differs));
            }
            let name = if nx == 0 { &o.files[0] } else { &o.files[1] };
            eof_message(
                o,
                name,
                offset,
                line.saturating_sub(u64::from(last_newline)),
            );
            return Ok(1);
        }
        offset += 1;
        if x[0] != y[0] {
            differs = true;
            if !o.quiet {
                report_difference(out, o, offset, line, x[0], y[0])?;
            }
            if !o.verbose || o.quiet {
                return Ok(1);
            }
        }
        last_newline = x[0] == b'\n';
        if last_newline {
            line += 1;
        }
    }
    Ok(i32::from(differs))
}
pub fn run(a: &[String]) -> i32 {
    let mut o = match parse(a) {
        Ok(x) => x,
        Err(c) => return c,
    };
    o.width = output_width(&o);
    if o.files[0] == "-" && o.files[1] == "-" && o.skip1 == o.skip2 {
        return 0;
    }
    let mut left = match open(&o.files[0]) {
        Ok(v) => v,
        Err(e) => {
            if !o.quiet {
                eprintln!("cmp: {}: {}", o.files[0], error::io_text(&e));
            }
            return 2;
        }
    };
    let mut right = match open(&o.files[1]) {
        Ok(v) => v,
        Err(e) => {
            if !o.quiet {
                eprintln!("cmp: {}: {}", o.files[1], error::io_text(&e));
            }
            return 2;
        }
    };
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::with_capacity(65536, stdout.lock());
    match compare_streams(&o, &mut left, &mut right, &mut out)
        .and_then(|code| out.flush().map(|()| code))
    {
        Ok(code) => code,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            if !o.quiet {
                eprintln!("cmp: {}", error::io_text(&e));
            }
            2
        }
    }
}

fn output_width(o: &Opt) -> usize {
    let mut maximum = o.bytes;
    for (index, path) in o.files.iter().enumerate() {
        if path == "-" {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.is_file() {
                let skip = if index == 0 { o.skip1 } else { o.skip2 };
                let size = meta.len().saturating_sub(skip);
                maximum = Some(maximum.map_or(size, |limit| limit.min(size)));
            }
        }
    }
    maximum.map_or(1, |n| n.to_string().len())
}

fn open(path: &str) -> std::io::Result<Box<dyn Read>> {
    if path == "-" {
        Ok(Box::new(BufReader::new(std::io::stdin())))
    } else {
        Ok(Box::new(BufReader::new(std::fs::File::open(path)?)))
    }
}
fn skip(reader: &mut dyn Read, mut n: u64) -> std::io::Result<()> {
    let mut buffer = [0u8; 8192];
    while n > 0 {
        let amount = usize::try_from(n.min(buffer.len() as u64)).unwrap();
        let got = reader.read(&mut buffer[..amount])?;
        if got == 0 {
            break;
        }
        n -= got as u64;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn check(left: &[u8], right: &[u8], extra: &[&str]) -> (i32, Vec<u8>) {
        let mut argv: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        argv.extend(["a".into(), "b".into()]);
        let opt = parse(&argv).unwrap();
        let mut out = Vec::new();
        let status = compare_streams(&opt, &mut &*left, &mut &*right, &mut out).unwrap();
        (status, out)
    }
    #[test]
    fn equal_and_limit() {
        assert_eq!(check(b"a", b"a", &[]).0, 0);
        assert_eq!(check(b"ab", b"ac", &["-n", "1"]).0, 0);
    }
    #[test]
    fn plain_and_printed() {
        assert_eq!(check(b"a", b"b", &[]).1, b"a b differ: byte 1, line 1\n");
        assert_eq!(
            check(b"a", b"b", &["-b"]).1,
            b"a b differ: byte 1, line 1 is 141 a 142 b\n"
        );
    }
    #[test]
    fn verbose_and_quiet() {
        assert_eq!(check(b"ab", b"cd", &["-l"]).1, b"1 141 143\n2 142 144\n");
        assert!(check(b"a", b"b", &["-s"]).1.is_empty());
    }
    #[test]
    fn skip_and_operands() {
        assert_eq!(check(b"xa", b"ya", &["-i", "1"]).0, 0);
        assert_eq!(parse(&["a".into()]).unwrap().files, ["a", "-"]);
        let combined = parse(&[
            "-i".into(),
            "1:3".into(),
            "a".into(),
            "b".into(),
            "2".into(),
            "0".into(),
        ])
        .unwrap();
        assert_eq!((combined.skip1, combined.skip2), (2, 3));
        assert_eq!(
            parse(&[
                "-i".into(),
                "1".into(),
                "a".into(),
                "b".into(),
                "invalid".into()
            ])
            .unwrap_err(),
            2
        );
        assert_eq!(
            parse(&["a".into(), "b".into(), "0".into(), "0".into(), "x".into()]).unwrap_err(),
            2
        );
    }
    #[test]
    fn control_display() {
        assert_eq!(display(0), "^@");
        assert_eq!(display(127), "^?");
        assert_eq!(display(255), "M-^?");
    }
    #[test]
    fn numeric_suffixes_and_radices() {
        assert_eq!(number("010"), Some(8));
        assert_eq!(number("0x10"), Some(16));
        assert_eq!(number("2KB"), Some(2000));
        assert_eq!(number("2KiB"), Some(2048));
        assert_eq!(number("-1"), None);
        assert_eq!(number("08"), None);
    }
}
