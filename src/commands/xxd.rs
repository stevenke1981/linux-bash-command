use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::{
    fs::{self, File},
    io as stdio,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

fn write_output_file(
    path: &str,
    reverse: bool,
    write: impl FnOnce(&mut File) -> stdio::Result<()>,
) -> stdio::Result<()> {
    use crate::common::external::TemporaryFile;
    let target = Path::new(path);
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let metadata = fs::metadata(target).ok();
    let mut temp = TemporaryFile::new(parent)?;
    if reverse && metadata.is_some() {
        stdio::copy(&mut File::open(target)?, temp.file())?;
        temp.file().seek(SeekFrom::Start(0))?;
    }
    write(temp.file())?;
    if let Some(metadata) = metadata {
        temp.file().set_permissions(metadata.permissions())?;
    } else {
        // An empty mode probe captures the process umask without exposing staged data.
        let probe = TemporaryFile::with_mode(parent, 0o666)?;
        temp.file()
            .set_permissions(fs::metadata(probe.path())?.permissions())?;
    }
    temp.persist(target)
}

const HELP: &str = "Usage: xxd [OPTION]... [INFILE [OUTFILE]]\nMake a hexdump or reverse it.\n  -a               toggle autoskip of zero lines\n  -b               binary digit dump\n  -c COLS          bytes per line\n  -C               capitalize include variable names\n  -d               decimal offsets\n  -e               little-endian dump\n  -g BYTES         bytes per group\n  -i               include output as C source\n  -l LEN           stop after LEN bytes\n  -n NAME          set include variable name\n  -o OFFSET        add OFFSET to displayed offsets\n  -p, -ps, -plain  plain hexdump\n  -r               reverse hexdump\n  -s [+][-]SEEK    start at SEEK bytes into input\n  -u               uppercase hexadecimal digits\n      --help       display this help\n      --version    output version information\n";

#[derive(Debug, Default)]
struct Opt {
    autoskip: bool,
    binary: bool,
    cols: Option<usize>,
    capitalize: bool,
    decimal: bool,
    little: bool,
    group: Option<usize>,
    include: bool,
    name: Option<String>,
    length: Option<u64>,
    offset: u64,
    plain: bool,
    reverse: bool,
    seek: Option<SeekSpec>,
    uppercase: bool,
    files: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
struct SeekSpec {
    amount: i64,
    from_current: bool,
}

fn parse_number(text: &str) -> Option<u64> {
    let s = text.trim();
    if let Some(v) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(v, 16).ok()
    } else if s.len() > 1 && s.starts_with('0') {
        u64::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse().ok()
    }
}

fn normalize(args_in: &[String]) -> Vec<String> {
    args_in
        .iter()
        .map(|arg| match arg.as_str() {
            "-plain" | "-postscript" | "-ps" => "-p".to_owned(),
            "-cols" => "-c".to_owned(),
            "-seek" => "-s".to_owned(),
            "-groupsize" => "-g".to_owned(),
            "-offset" => "-o".to_owned(),
            "-name" => "-n".to_owned(),
            "-autoskip" => "-a".to_owned(),
            "-bits" => "-b".to_owned(),
            "-include" => "-i".to_owned(),
            "-revert" => "-r".to_owned(),
            "-uppercase" => "-u".to_owned(),
            "-len" => "-l".to_owned(),
            _ => arg.clone(),
        })
        .collect()
}

fn parse(arguments: &[String]) -> Result<Opt, i32> {
    let normalized = normalize(arguments);
    let specs = [
        OptSpec::flag("autoskip", 'a'),
        OptSpec::flag("binary", 'b'),
        OptSpec::value("cols", 'c'),
        OptSpec::flag("capitalize", 'C'),
        OptSpec::flag("decimal", 'd'),
        OptSpec::flag("little-endian", 'e'),
        OptSpec::value("group", 'g'),
        OptSpec::flag("include", 'i'),
        OptSpec::value("len", 'l'),
        OptSpec::value("name", 'n'),
        OptSpec::value("offset", 'o'),
        OptSpec::flag("plain", 'p'),
        OptSpec::flag("reverse", 'r'),
        OptSpec::value("seek", 's'),
        OptSpec::flag("uppercase", 'u'),
    ];
    let items = args::parse_command("xxd", &normalized, HELP, &specs, true)?;
    let mut o = Opt::default();
    for item in items {
        match item {
            Item::Operand(v) => o.files.push(v),
            Item::Opt("autoskip", _) => o.autoskip = !o.autoskip,
            Item::Opt("binary", _) => o.binary = true,
            Item::Opt("capitalize", _) => o.capitalize = true,
            Item::Opt("decimal", _) => o.decimal = true,
            Item::Opt("little-endian", _) => o.little = true,
            Item::Opt("include", _) => o.include = true,
            Item::Opt("plain", _) => o.plain = true,
            Item::Opt("reverse", _) => o.reverse = true,
            Item::Opt("uppercase", _) => o.uppercase = true,
            Item::Opt("cols", Some(v)) => {
                o.cols = Some(
                    parse_number(&v)
                        .and_then(|n| usize::try_from(n).ok())
                        .filter(|n| *n > 0)
                        .ok_or_else(|| usage(format!("invalid number of columns: '{v}'")))?,
                )
            }
            Item::Opt("group", Some(v)) => {
                o.group = Some(
                    parse_number(&v)
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(|| {
                            usage(format!("invalid number of bytes per group: '{v}'"))
                        })?,
                )
            }
            Item::Opt("len", Some(v)) => {
                o.length =
                    Some(parse_number(&v).ok_or_else(|| usage(format!("invalid length: '{v}'")))?)
            }
            Item::Opt("offset", Some(v)) => {
                o.offset =
                    parse_number(&v).ok_or_else(|| usage(format!("invalid offset: '{v}'")))?
            }
            Item::Opt("name", Some(v)) => o.name = Some(v),
            Item::Opt("seek", Some(v)) => {
                o.seek = Some(parse_seek(&v).ok_or_else(|| usage(format!("invalid seek: '{v}'")))?)
            }
            Item::Opt(_, _) => unreachable!(),
        }
    }
    if o.little
        && o.group
            .is_some_and(|g| g != 0 && g <= o.cols.unwrap_or(16) && !g.is_power_of_two())
    {
        return Err(usage(
            "number of octets per group must be a power of 2 with -e".into(),
        ));
    }
    if o.files.len() > 2 {
        return Err(usage("too many file operands".into()));
    }
    if o.reverse && o.include {
        return Err(usage("cannot combine -r and -i".into()));
    }
    if o.include && o.plain {
        return Err(usage("cannot combine -i and -p".into()));
    }
    if o.binary && o.plain {
        return Err(usage("cannot combine -b and -p".into()));
    }
    Ok(o)
}

fn usage(message: String) -> i32 {
    error::usage("xxd", message, 1)
}
fn parse_seek(s: &str) -> Option<SeekSpec> {
    let from_current = s.starts_with('+');
    let value = s.strip_prefix('+').unwrap_or(s);
    let negative = value.starts_with('-');
    let value = value.strip_prefix('-').unwrap_or(value);
    let amount = i64::try_from(parse_number(value)?).ok()?;
    Some(SeekSpec {
        amount: if negative { -amount } else { amount },
        from_current,
    })
}

enum Input {
    Stdin(stdio::Stdin),
    File(File),
}
impl Read for Input {
    fn read(&mut self, b: &mut [u8]) -> stdio::Result<usize> {
        match self {
            Self::Stdin(s) => s.read(b),
            Self::File(f) => f.read(b),
        }
    }
}

fn open_input(name: Option<&str>) -> Result<Input, (String, stdio::Error)> {
    match name {
        None | Some("-") => Ok(Input::Stdin(stdio::stdin())),
        Some(path) => File::open(path)
            .map(Input::File)
            .map_err(|e| (path.into(), e)),
    }
}

fn seek_input(reader: &mut Input, spec: Option<SeekSpec>) -> Result<u64, String> {
    let Some(spec) = spec else { return Ok(0) };
    if let Input::File(file) = reader {
        let position = if spec.from_current {
            SeekFrom::Current(spec.amount)
        } else if spec.amount >= 0 {
            SeekFrom::Start(spec.amount as u64)
        } else {
            SeekFrom::End(spec.amount)
        };
        return file.seek(position).map_err(|e| error::io_text(&e));
    }
    if spec.amount < 0 {
        return Err("cannot seek on standard input".into());
    }
    let mut remaining = spec.amount as u64;
    let mut buf = [0u8; 8192];
    let mut skipped = 0;
    while remaining > 0 {
        let amount = remaining.min(buf.len() as u64) as usize;
        let n = reader
            .read(&mut buf[..amount])
            .map_err(|e| error::io_text(&e))?;
        if n == 0 {
            break;
        }
        remaining -= n as u64;
        skipped += n as u64;
    }
    Ok(skipped)
}

fn dump<R: Read, W: Write + ?Sized>(
    reader: &mut R,
    out: &mut W,
    o: &Opt,
    start: u64,
) -> stdio::Result<()> {
    if o.plain {
        return dump_plain(reader, out, o);
    }
    if o.include {
        return dump_include(reader, out, o);
    }
    let cols = o.cols.unwrap_or(if o.binary { 6 } else { 16 });
    let group = o
        .group
        .filter(|g| *g != 0)
        .unwrap_or(if o.group == Some(0) {
            cols
        } else if o.little {
            4
        } else if o.binary {
            1
        } else {
            2
        });
    let mut address = start.saturating_add(o.offset);
    let mut remaining = o.length;
    let mut zero_rows: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut zero_count = 0usize;
    let mut buf = vec![0; cols];
    loop {
        if remaining == Some(0) {
            break;
        }
        let want = remaining.map_or(cols, |n| cols.min(n.min(usize::MAX as u64) as usize));
        let mut got = 0;
        while got < want {
            let n = reader.read(&mut buf[got..want])?;
            if n == 0 {
                break;
            }
            got += n;
        }
        if got == 0 {
            break;
        }
        if let Some(n) = &mut remaining {
            *n -= got as u64;
        }
        let row = &buf[..got];
        let zero = got == cols && row.iter().all(|b| *b == 0);
        if o.autoskip && zero {
            zero_count += 1;
            zero_rows.push((address, row.to_vec()));
            if zero_rows.len() == 1 {
                print_row(out, o, address, row, cols, group)?;
            } else if zero_rows.len() > 3 {
                zero_rows.remove(1);
            }
        } else {
            flush_zeros(out, o, &zero_rows, zero_count, cols, group, false)?;
            zero_rows.clear();
            zero_count = 0;
            print_row(out, o, address, row, cols, group)?;
        }
        address = address.saturating_add(got as u64);
    }
    flush_zeros(out, o, &zero_rows, zero_count, cols, group, true)?;
    Ok(())
}

fn flush_zeros<W: Write + ?Sized>(
    out: &mut W,
    o: &Opt,
    rows: &[(u64, Vec<u8>)],
    count: usize,
    cols: usize,
    group: usize,
    at_end: bool,
) -> stdio::Result<()> {
    if count >= 3 && (!at_end || count >= 4) {
        writeln!(out, "*")?;
    }
    if at_end && count == 3 {
        let (address, row) = &rows[1];
        print_row(out, o, *address, row, cols, group)?;
    }
    if (count == 2 || at_end && count >= 2) && !rows.is_empty() {
        let (address, row) = rows.last().unwrap();
        print_row(out, o, *address, row, cols, group)?;
    }
    Ok(())
}
fn print_row<W: Write + ?Sized>(
    out: &mut W,
    o: &Opt,
    address: u64,
    row: &[u8],
    cols: usize,
    group: usize,
) -> stdio::Result<()> {
    let radix = if o.decimal {
        format!("{address:08}")
    } else {
        format!("{address:08x}")
    };
    if o.little && o.group.is_some_and(|g| g == 0 || g > cols) {
        return print_little_ungrouped(out, &radix, row, cols, o.uppercase);
    }
    let mut field = String::new();
    for i in (0..row.len()).step_by(group) {
        if i > 0 {
            field.push(' ');
        }
        let end = (i + group).min(row.len());
        if o.little {
            field.push_str(&" ".repeat((group - (end - i)) * 2));
        }
        let bytes: Box<dyn Iterator<Item = &u8>> = if o.little {
            Box::new(row[i..end].iter().rev())
        } else {
            Box::new(row[i..end].iter())
        };
        for byte in bytes {
            if o.binary {
                use std::fmt::Write as _;
                let _ = write!(field, "{byte:08b}");
            } else {
                push_hex(&mut field, *byte, o.uppercase);
            }
        }
    }
    let width_cols = if o.little {
        cols.div_ceil(group) * group
    } else {
        cols
    };
    let width = width_cols * if o.binary { 8 } else { 2 } + (cols - 1) / group;
    write!(
        out,
        "{radix}: {field:<width$}  {}",
        if o.little { " " } else { "" }
    )?;
    for b in row {
        out.write_all(&[ascii(*b)])?;
    }
    writeln!(out)
}

// Vim's ungrouped little-endian layout writes each hex and ASCII byte into
// the same row buffer, including overlapping positions for non-power-of-two columns.
fn print_little_ungrouped<W: Write + ?Sized>(
    out: &mut W,
    radix: &str,
    row: &[u8],
    cols: usize,
    upper: bool,
) -> stdio::Result<()> {
    let display_len = cols * 2 + 3 + row.len();
    let max_index = row
        .iter()
        .enumerate()
        .map(|(i, _)| i ^ (cols - 1))
        .max()
        .unwrap_or(0);
    let mut field = vec![b' '; display_len.max(max_index * 2 + max_index / cols + 2)];
    let digits = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    for (i, &byte) in row.iter().enumerate() {
        let index = i ^ (cols - 1);
        let at = index * 2 + index / cols;
        field[at] = digits[(byte >> 4) as usize];
        field[at + 1] = digits[(byte & 15) as usize];
        field[cols * 2 + 3 + i] = ascii(byte);
    }
    write!(out, "{radix}: ")?;
    out.write_all(&field[..display_len])?;
    writeln!(out)
}

fn ascii(b: u8) -> u8 {
    if (0x20..=0x7e).contains(&b) {
        b
    } else {
        b'.'
    }
}
fn push_hex(s: &mut String, b: u8, upper: bool) {
    if upper {
        use std::fmt::Write;
        let _ = write!(s, "{b:02X}");
    } else {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
}

fn dump_plain<R: Read, W: Write + ?Sized>(r: &mut R, out: &mut W, o: &Opt) -> stdio::Result<()> {
    let cols = o.cols.unwrap_or(30);
    let mut buf = vec![0; cols];
    let mut remaining = o.length;
    loop {
        if remaining == Some(0) {
            break;
        }
        let want = remaining.map_or(cols, |n| cols.min(n.min(usize::MAX as u64) as usize));
        let mut n = 0;
        while n < want {
            let got = r.read(&mut buf[n..want])?;
            if got == 0 {
                break;
            }
            n += got
        }
        if n == 0 {
            break;
        }
        if let Some(left) = &mut remaining {
            *left -= n as u64
        }
        for b in &buf[..n] {
            if o.uppercase {
                write!(out, "{b:02X}")?
            } else {
                write!(out, "{b:02x}")?
            }
        }
        writeln!(out)?;
    }
    Ok(())
}

fn dump_include<R: Read, W: Write + ?Sized>(r: &mut R, out: &mut W, o: &Opt) -> stdio::Result<()> {
    let name = o.name.clone().unwrap_or_else(|| "xxd".into());
    let name = if o.capitalize {
        name.to_ascii_uppercase()
    } else {
        name
    };
    if o.name.is_some() {
        writeln!(out, "unsigned char {name}[] = {{")?;
    }
    let mut limited = r.take(o.length.unwrap_or(u64::MAX));
    let mut buf = [0; 4096];
    let (mut count, mut col) = (0usize, 0usize);
    let cols = o.cols.unwrap_or(12);
    loop {
        let n = limited.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            if count == 0 {
                out.write_all(b"  ")?;
            } else if col == 0 {
                out.write_all(b",\n  ")?;
            } else {
                out.write_all(b", ")?;
            }
            if o.uppercase {
                write!(out, "0X{b:02X}")?;
            } else {
                write!(out, "0x{b:02x}")?;
            }
            count += 1;
            col = (col + 1) % cols;
        }
    }
    if count > 0 {
        writeln!(out)?;
    }
    if o.name.is_some() {
        writeln!(out, "}};")?;
        let suffix = if o.capitalize { "LEN" } else { "len" };
        writeln!(out, "unsigned int {name}_{suffix} = {count};")?;
    }
    Ok(())
}

fn read_reverse<R: BufRead, W: Write + Seek>(
    input: &mut R,
    out: &mut W,
    o: &Opt,
) -> stdio::Result<()> {
    let mut line = Vec::new();
    let mut high = None;
    let mut remaining = o.length;
    if o.plain {
        let seek = o.seek.map_or(0, |s| s.amount);
        if seek < 0 {
            return Err(stdio::Error::new(
                stdio::ErrorKind::InvalidInput,
                "cannot seek backwards",
            ));
        }
        out.seek(SeekFrom::Start(seek as u64))?;
    }
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let hex = if o.plain {
            &line[..]
        } else {
            let Some(colon) = line.iter().position(|b| *b == b':') else {
                continue;
            };
            let address = String::from_utf8_lossy(&line[..colon]);
            let Ok(address) = u64::from_str_radix(address.trim(), 16) else {
                continue;
            };
            let position = i128::from(address) + i128::from(o.seek.map_or(0, |s| s.amount));
            if position < 0 || position > i128::from(u64::MAX) {
                return Err(stdio::Error::new(
                    stdio::ErrorKind::InvalidInput,
                    "cannot seek backwards",
                ));
            }
            out.seek(SeekFrom::Start(position as u64))?;
            high = None;
            let rest = &line[colon + 1..];
            let end = rest
                .windows(2)
                .position(|w| w == b"  ")
                .unwrap_or(rest.len());
            &rest[..end]
        };
        for &b in hex {
            if b.is_ascii_whitespace() {
                continue;
            }
            if !b.is_ascii_hexdigit() {
                break;
            }
            if let Some(h) = high.take() {
                if remaining == Some(0) {
                    return Ok(());
                }
                out.write_all(&[(h << 4) | hexval(b)])?;
                if let Some(left) = &mut remaining {
                    *left -= 1;
                }
            } else {
                high = Some(hexval(b));
            }
        }
    }
    Ok(())
}
fn hexval(b: u8) -> u8 {
    (b as char).to_digit(16).unwrap_or(0) as u8
}

pub fn run(arguments: &[String]) -> i32 {
    let mut o = match parse(arguments) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let name = o.files.first().map(String::as_str);
    if o.include && o.name.is_none() {
        if let Some(path) = name.filter(|n| *n != "-") {
            let basename = std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let mut derived = basename
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            if derived.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                derived.insert(0, '_');
            }
            if !derived.is_empty() {
                o.name = Some(derived);
            }
        }
    }
    let mut input = match open_input(name) {
        Ok(v) => v,
        Err((p, e)) => {
            eprintln!("xxd: {p}: {}", error::io_text(&e));
            return 2;
        }
    };
    let start = match if o.reverse {
        Ok(0)
    } else {
        seek_input(&mut input, o.seek)
    } {
        Ok(v) => v,
        Err(e) => {
            eprintln!("xxd: {e}");
            return 1;
        }
    };
    let status = if let Some(path) = o.files.get(1).filter(|p| p.as_str() != "-") {
        match write_output_file(path, o.reverse, |output| {
            if o.reverse {
                read_reverse(&mut BufReader::new(input), output, &o)
            } else {
                dump(&mut input, output, &o, start)
            }
        }) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("xxd: {path}: {}", error::io_text(&e));
                2
            }
        }
    } else {
        io::output("xxd", |stdout| {
            if o.reverse {
                {
                    let mut temp =
                        crate::common::external::TemporaryFile::new(&std::env::temp_dir())?;
                    read_reverse(&mut BufReader::new(input), temp.file(), &o)?;
                    temp.file().seek(SeekFrom::Start(0))?;
                    stdio::copy(temp.file(), stdout)?;
                    Ok(())
                }
            } else {
                dump(&mut input, stdout, &o, start)
            }
        })
    };
    if status == 0 {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plain_dump() {
        let mut input = &b"abc"[..];
        let mut out = Vec::new();
        dump(
            &mut input,
            &mut out,
            &Opt {
                cols: Some(2),
                ..Opt::default()
            },
            0,
        )
        .unwrap();
        assert!(String::from_utf8(out)
            .unwrap()
            .starts_with("00000000: 6162"));
    }
    #[test]
    fn little_endian_groups() {
        let mut input = &b"abcd"[..];
        let mut out = Vec::new();
        dump(
            &mut input,
            &mut out,
            &Opt {
                little: true,
                group: Some(2),
                ..Opt::default()
            },
            0,
        )
        .unwrap();
        assert!(String::from_utf8(out).unwrap().contains("6261 6463"));
    }
    #[test]
    fn reverse_plain_bytes() {
        let mut input = BufReader::new(&b"4142ff\n"[..]);
        let mut out = stdio::Cursor::new(Vec::new());
        read_reverse(
            &mut input,
            &mut out,
            &Opt {
                plain: true,
                ..Opt::default()
            },
        )
        .unwrap();
        assert_eq!(out.into_inner(), [0x41, 0x42, 0xff]);
    }
    #[test]
    fn failed_staging_preserves_existing_file() {
        let dir = std::env::temp_dir().join(format!("xxd-stage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("output");
        std::fs::write(&output, b"original").unwrap();
        let result = write_output_file(output.to_str().unwrap(), false, |file| {
            file.write_all(b"partial")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(file.metadata()?.permissions().mode() & 0o077, 0);
            }
            Err(stdio::Error::other("late failure"))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
