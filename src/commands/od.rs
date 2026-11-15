use crate::common::{
    args::{self, Item, OptSpec},
    io,
};
use std::io::Read;
mod strings;

const HELP: &str = "Usage: od [OPTION]... [FILE]...\nWrite an octal or other formatted dump of FILE.\n  -A, --address-radix=RADIX  d, o, x, or n\n  -j, --skip-bytes=BYTES     skip input bytes\n  -N, --read-bytes=BYTES     limit input bytes\n  -S, --strings[=N]          print strings of at least N characters\n  -t, --format=TYPE          select output format\n  -v, --output-duplicates    do not suppress duplicate lines\n  -w, --width[=N]            bytes per output line\n      --endian=big|little    input byte order\n  -a -b -c -d -f -i -l -o -s -x  traditional formats\n      --help                 display this help\n      --version              output version information\n";

#[derive(Clone, Copy, Debug)]
struct Format {
    kind: u8,
    size: usize,
    ascii: bool,
}
fn formats(s: &str) -> Result<Vec<Format>, String> {
    let mut iter = s.bytes().peekable();
    let mut out = Vec::new();
    while let Some(kind) = iter.next() {
        if !b"acdfoux".contains(&kind) {
            return Err(format!("invalid type string '{s}'"));
        }
        let mut digits = String::new();
        while iter.peek().is_some_and(u8::is_ascii_digit) {
            digits.push(iter.next().unwrap() as char);
        }
        let size = if !digits.is_empty() {
            digits
                .parse::<usize>()
                .map_err(|_| format!("invalid type string '{s}'"))?
        } else if let Some(&letter) = iter.peek() {
            match (kind, letter) {
                (b'f', b'F') => {
                    iter.next();
                    4
                }
                (b'f', b'D' | b'L') => {
                    iter.next();
                    8
                }
                (_, b'C') => {
                    iter.next();
                    1
                }
                (_, b'S') => {
                    iter.next();
                    2
                }
                (_, b'I') => {
                    iter.next();
                    4
                }
                (_, b'L') => {
                    iter.next();
                    8
                }
                _ => {
                    if kind == b'a' || kind == b'c' {
                        1
                    } else if kind == b'f' {
                        8
                    } else {
                        4
                    }
                }
            }
        } else if kind == b'a' || kind == b'c' {
            1
        } else if kind == b'f' {
            8
        } else {
            4
        };
        if ![1, 2, 4, 8].contains(&size)
            || (kind == b'f' && size != 4 && size != 8)
            || ((kind == b'a' || kind == b'c') && size != 1)
        {
            return Err(format!("invalid size in type '{s}'"));
        }
        let ascii = if iter.peek() == Some(&b'z') {
            iter.next();
            true
        } else {
            false
        };
        out.push(Format { kind, size, ascii });
    }
    if out.is_empty() {
        return Err("empty type string".into());
    }
    Ok(out)
}
fn size(s: &str) -> Result<usize, String> {
    let (radix, begin) = if s.starts_with("0x") || s.starts_with("0X") {
        (16, 2)
    } else if s.starts_with('0') {
        (8, 0)
    } else {
        (10, 0)
    };
    let i = begin
        + s[begin..]
            .bytes()
            .take_while(|b| (*b as char).is_digit(radix))
            .count();
    let n =
        usize::from_str_radix(&s[begin..i], radix).map_err(|_| format!("invalid number '{s}'"))?;
    let (base, exponent) = match &s[i..] {
        "" => (1usize, 0),
        "b" => (512, 1),
        "k" | "K" | "KiB" => (1024, 1),
        "kB" | "KB" => (1000, 1),
        "m" | "M" | "MiB" => (1024, 2),
        "MB" => (1000, 2),
        "G" | "GiB" => (1024, 3),
        "GB" => (1000, 3),
        "T" | "TiB" => (1024, 4),
        "TB" => (1000, 4),
        "P" | "PiB" => (1024, 5),
        "PB" => (1000, 5),
        "E" | "EiB" => (1024, 6),
        "EB" => (1000, 6),
        "Z" | "ZiB" => (1024, 7),
        "ZB" => (1000, 7),
        "Y" | "YiB" => (1024, 8),
        "YB" => (1000, 8),
        _ => return Err(format!("invalid number '{s}'")),
    };
    if n == 0 {
        return Ok(0);
    }
    base.checked_pow(exponent)
        .and_then(|mul| n.checked_mul(mul))
        .ok_or_else(|| format!("invalid number '{s}'"))
}
fn width(f: Format) -> usize {
    match f.kind {
        b'a' | b'c' => 3,
        b'x' => f.size * 2,
        b'o' => (f.size * 8).div_ceil(3),
        b'u' => match f.size {
            1 => 3,
            2 => 5,
            4 => 10,
            _ => 20,
        },
        b'd' => match f.size {
            1 => 4,
            2 => 6,
            4 => 11,
            _ => 20,
        },
        b'f' => {
            if f.size == 4 {
                15
            } else {
                24
            }
        }
        _ => 0,
    }
}
fn float_text(scientific: String, fixed: String, minimum_precision: usize) -> String {
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific.to_ascii_lowercase();
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return scientific;
    };
    let precision = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .count()
        .max(minimum_precision);
    if exponent < -4 || exponent >= precision as i32 {
        format!(
            "{mantissa}e{}{:02}",
            if exponent < 0 { '-' } else { '+' },
            exponent.unsigned_abs()
        )
    } else {
        fixed
    }
}
fn value(bytes: &[u8], f: Format, big: bool) -> String {
    const NAMES: [&str; 32] = [
        "nul", "soh", "stx", "etx", "eot", "enq", "ack", "bel", "bs", "ht", "nl", "vt", "ff", "cr",
        "so", "si", "dle", "dc1", "dc2", "dc3", "dc4", "nak", "syn", "etb", "can", "em", "sub",
        "esc", "fs", "gs", "rs", "us",
    ];
    if f.kind == b'a' {
        let c = bytes[0] & 127;
        return if c < 32 {
            NAMES[c as usize].to_string()
        } else if c == 32 {
            "sp".into()
        } else if c == 127 {
            "del".into()
        } else {
            (c as char).to_string()
        };
    }
    if f.kind == b'c' {
        let c = bytes[0];
        return match c {
            0 => "\\0".into(),
            7 => "\\a".into(),
            8 => "\\b".into(),
            9 => "\\t".into(),
            10 => "\\n".into(),
            11 => "\\v".into(),
            12 => "\\f".into(),
            13 => "\\r".into(),
            32..=126 => (c as char).to_string(),
            _ => format!("{c:03o}"),
        };
    }
    let mut padded = [0u8; 8];
    padded[..bytes.len()].copy_from_slice(bytes);
    let mut num = 0u64;
    if big {
        for &b in &padded[..f.size] {
            num = (num << 8) | u64::from(b);
        }
    } else {
        for (i, &b) in padded[..f.size].iter().enumerate() {
            num |= u64::from(b) << (8 * i);
        }
    }
    match f.kind {
        b'x' => format!("{num:0width$x}", width = f.size * 2),
        b'o' => format!("{num:0width$o}", width = width(f)),
        b'u' => num.to_string(),
        b'd' => {
            let shift = 64 - f.size * 8;
            (((num << shift) as i64) >> shift).to_string()
        }
        b'f' => {
            if f.size == 4 {
                let n = f32::from_bits(num as u32);
                float_text(format!("{n:e}"), n.to_string(), 6)
            } else {
                let n = f64::from_bits(num);
                float_text(format!("{n:e}"), n.to_string(), 15)
            }
        }
        _ => String::new(),
    }
}
fn address(n: usize, radix: char) -> String {
    match radix {
        'n' => String::new(),
        'x' => format!("{n:06x}"),
        'd' => format!("{n:07}"),
        _ => format!("{n:07o}"),
    }
}

struct Inputs {
    names: std::vec::IntoIter<String>,
    current: Option<(String, Box<dyn std::io::BufRead>)>,
    failed: bool,
}
impl Inputs {
    fn open_next(&mut self) -> bool {
        for name in self.names.by_ref() {
            match io::input(&name) {
                Ok(input) => {
                    self.current = Some((name, input));
                    return true;
                }
                Err(e) => {
                    io::report_open("od", &name, &e);
                    self.failed = true;
                }
            }
        }
        false
    }
}
impl Read for Inputs {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.current.is_none() && !self.open_next() {
                return Ok(0);
            }
            let (name, input) = self.current.as_mut().unwrap();
            match input.read(buf) {
                Ok(0) => self.current = None,
                Ok(n) => return Ok(n),
                Err(e) => {
                    io::report_failure("od", name, &e);
                    self.failed = true;
                    self.current = None;
                }
            }
        }
    }
}

pub fn run(arguments: &[String]) -> i32 {
    let specs = [
        OptSpec::value("address-radix", 'A'),
        OptSpec::value("skip-bytes", 'j'),
        OptSpec::value("read-bytes", 'N'),
        OptSpec::optional("strings", 'S'),
        OptSpec::value("format", 't'),
        OptSpec::flag("output-duplicates", 'v'),
        OptSpec::optional("width", 'w'),
        OptSpec::value("endian", '\0'),
        OptSpec::flag("", 'a'),
        OptSpec::flag("", 'b'),
        OptSpec::flag("", 'c'),
        OptSpec::flag("", 'd'),
        OptSpec::flag("", 'f'),
        OptSpec::flag("", 'i'),
        OptSpec::flag("", 'l'),
        OptSpec::flag("", 'o'),
        OptSpec::flag("", 's'),
        OptSpec::flag("", 'x'),
    ];
    // Empty long names have the same parser ID, so give each legacy switch a unique ID.
    let mut specs = specs;
    for (spec, id) in specs[8..].iter_mut().zip([
        "legacy-a", "legacy-b", "legacy-c", "legacy-d", "legacy-f", "legacy-i", "legacy-l",
        "legacy-o", "legacy-s", "legacy-x",
    ]) {
        spec.id = id;
    }
    let items = match args::parse_command("od", arguments, HELP, &specs, true) {
        Ok(x) => x,
        Err(x) => return x,
    };
    let mut fs = Vec::new();
    let mut radix = 'o';
    let mut skip = 0;
    let mut count = None;
    let mut strings = None;
    let mut columns = 16;
    let mut duplicates = false;
    let mut big = cfg!(target_endian = "big");
    let mut files = Vec::new();
    for item in items {
        match item {
            Item::Opt("address-radix", Some(v)) => {
                if v.len() != 1 || !"doxn".contains(&v) {
                    eprintln!("od: invalid address radix '{v}'");
                    return 1;
                }
                radix = v.chars().next().unwrap();
            }
            Item::Opt(id @ ("skip-bytes" | "read-bytes"), Some(v)) => match size(&v) {
                Ok(n) => {
                    if id == "skip-bytes" {
                        skip = n
                    } else {
                        count = Some(n)
                    }
                }
                Err(e) => {
                    eprintln!("od: {e}");
                    return 1;
                }
            },
            Item::Opt("strings", v) => match v.as_deref().map(size).unwrap_or(Ok(3)) {
                Ok(n) => strings = Some(n),
                Err(e) => {
                    eprintln!("od: {e}");
                    return 1;
                }
            },
            Item::Opt("width", v) => {
                match v.as_deref().map(str::parse::<usize>).unwrap_or(Ok(32)) {
                    Ok(n) => columns = n,
                    _ => {
                        eprintln!("od: invalid width");
                        return 1;
                    }
                }
            }
            Item::Opt("format", Some(v)) => match formats(&v) {
                Ok(v) => fs.extend(v),
                Err(e) => {
                    eprintln!("od: {e}");
                    return 1;
                }
            },
            Item::Opt("endian", Some(v)) => match v.as_str() {
                "big" => big = true,
                "little" => big = false,
                _ => {
                    eprintln!("od: invalid endianness '{v}'");
                    return 1;
                }
            },
            Item::Opt("output-duplicates", _) => duplicates = true,
            Item::Opt(id, _) if id.starts_with("legacy-") => {
                let f = match id {
                    "legacy-a" => "a",
                    "legacy-b" => "o1",
                    "legacy-c" => "c",
                    "legacy-d" => "u2",
                    "legacy-f" => "f4",
                    "legacy-i" => "d4",
                    "legacy-l" => "d8",
                    "legacy-o" => "o2",
                    "legacy-s" => "d2",
                    _ => "x2",
                };
                fs.extend(formats(f).unwrap());
            }
            Item::Operand(v) => files.push(v),
            _ => {}
        }
    }
    if fs.is_empty() {
        fs = formats("o2").unwrap();
    }
    let unit = fs.iter().map(|f| f.size).max().unwrap_or(1);
    if columns < unit || !columns.is_multiple_of(unit) {
        let adjusted = (columns / unit).max(1) * unit;
        eprintln!("od: warning: invalid width {columns}; using {adjusted} instead");
        columns = adjusted;
    }
    if files.is_empty() {
        files.push("-".into());
    }
    let mut input = Inputs {
        names: files.into_iter(),
        current: None,
        failed: false,
    };
    if !input.open_next() {
        return i32::from(input.failed);
    }
    let mut skip_failed = false;
    let status = io::output("od", |out| {
        let mut left_to_skip = skip;
        let mut scratch = [0u8; 8192];
        while left_to_skip != 0 {
            let limit = left_to_skip.min(scratch.len());
            let n = input.read(&mut scratch[..limit])?;
            if n == 0 {
                eprintln!("od: cannot skip past end of combined input");
                skip_failed = true;
                return Ok(());
            }
            left_to_skip -= n;
        }
        let mut position = skip;
        let mut remaining = count.unwrap_or(usize::MAX);
        if let Some(min) = strings {
            return strings::run(&mut input, out, position, remaining, min, radix);
        }
        let mut previous: Option<Vec<u8>> = None;
        let mut starred = false;
        let group_size = fs.iter().map(|f| f.size).max().unwrap_or(1);
        let group_width = fs
            .iter()
            .map(|f| group_size / f.size * (width(*f) + 1))
            .max()
            .unwrap_or(0);
        while remaining != 0 {
            let requested = columns.min(remaining);
            let mut chunk = vec![0; requested];
            let mut used = 0;
            while used < requested {
                let n = input.read(&mut chunk[used..])?;
                if n == 0 {
                    break;
                }
                used += n;
            }
            chunk.truncate(used);
            if chunk.is_empty() {
                break;
            }
            remaining -= used;
            let line_address = position;
            position += used;
            if !duplicates && previous.as_deref() == Some(chunk.as_slice()) {
                if !starred {
                    writeln!(out, "*")?;
                    starred = true;
                }
                continue;
            }
            starred = false;
            previous = Some(chunk.clone());
            for (fi, &f) in fs.iter().enumerate() {
                if fi == 0 {
                    write!(out, "{}", address(line_address, radix))?;
                } else if radix != 'n' {
                    write!(out, "{}", " ".repeat(address(0, radix).len()))?;
                }
                let cell = width(f);
                let mut emitted = 0;
                for group in chunk.chunks(group_size) {
                    let padding = group_width - group_size / f.size * (cell + 1);
                    write!(out, "{}", " ".repeat(padding))?;
                    emitted += padding;
                    for value_bytes in group.chunks(f.size) {
                        write!(out, " {:>cell$}", value(value_bytes, f, big))?;
                        emitted += cell + 1;
                    }
                }
                if f.ascii {
                    let expected = columns.div_ceil(group_size) * group_width;
                    write!(
                        out,
                        "{}  >{}<",
                        " ".repeat(expected.saturating_sub(emitted)),
                        chunk
                            .iter()
                            .map(|b| if (32..=126).contains(b) {
                                *b as char
                            } else {
                                '.'
                            })
                            .collect::<String>()
                    )?;
                }
                writeln!(out)?;
            }
        }
        if radix != 'n' {
            writeln!(out, "{}", address(position, radix))?;
        }
        Ok(())
    });
    if input.failed || skip_failed {
        1
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn types() {
        assert_eq!(formats("x1c").unwrap().len(), 2);
        assert!(formats("f3").is_err());
        assert!(formats("q").is_err());
    }
    #[test]
    fn numbers() {
        let f = formats("x2").unwrap()[0];
        assert_eq!(value(&[1, 2], f, false), "0201");
        assert_eq!(value(&[1, 2], f, true), "0102");
    }
    #[test]
    fn characters() {
        assert_eq!(value(b"\n", formats("c").unwrap()[0], false), "\\n");
        assert_eq!(address(16, 'o'), "0000020");
    }
    #[test]
    fn type_sizes_and_ascii_suffix() {
        for (name, expected) in [
            ("xC", 1),
            ("xS", 2),
            ("xI", 4),
            ("xL", 8),
            ("fF", 4),
            ("fD", 8),
            ("fL", 8),
            ("x", 4),
            ("f", 8),
        ] {
            assert_eq!(formats(name).unwrap()[0].size, expected, "{name}");
        }
        assert!(formats("x1z").unwrap()[0].ascii);
        for bad in [
            "",
            "a2",
            "c4",
            "f1",
            "x3",
            "q",
            "x999999999999999999999999999999999",
        ] {
            assert!(formats(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn byte_counts_and_radices() {
        for (text, expected) in [
            ("0", 0),
            ("010", 8),
            ("0x10", 16),
            ("1b", 512),
            ("1k", 1024),
            ("1KB", 1000),
            ("1MiB", 1048576),
            ("1MB", 1000000),
            ("0Y", 0),
            ("0ZB", 0),
        ] {
            assert_eq!(size(text), Ok(expected), "{text}");
        }
        for bad in ["", "08", "0x", "1bad", "1Y"] {
            assert!(size(bad).is_err(), "{bad}");
        }
        assert_eq!(address(16, 'd'), "0000016");
        assert_eq!(address(16, 'x'), "000010");
        assert_eq!(address(16, 'n'), "");
    }
    #[test]
    fn byte_formats_have_expected_values() {
        let named = formats("a").unwrap()[0];
        for (byte, expected) in [
            (0, "nul"),
            (10, "nl"),
            (32, "sp"),
            (65, "A"),
            (127, "del"),
            (255, "del"),
        ] {
            assert_eq!(value(&[byte], named, false), expected);
        }
        let character = formats("c").unwrap()[0];
        for (byte, expected) in [
            (0, "\\0"),
            (7, "\\a"),
            (8, "\\b"),
            (9, "\\t"),
            (11, "\\v"),
            (12, "\\f"),
            (13, "\\r"),
            (255, "377"),
        ] {
            assert_eq!(value(&[byte], character, false), expected);
        }
        assert_eq!(value(&[255], formats("d1").unwrap()[0], false), "-1");
        assert_eq!(value(&[255], formats("u1").unwrap()[0], false), "255");
        assert_eq!(value(&[255], formats("o1").unwrap()[0], false), "377");
        assert_eq!(
            value(&1.5f64.to_le_bytes(), formats("f8").unwrap()[0], false),
            "1.5"
        );
    }
    #[test]
    fn format_widths_and_float_notation() {
        for (format, expected) in [
            ("u1", 3),
            ("u2", 5),
            ("u4", 10),
            ("u8", 20),
            ("d1", 4),
            ("d2", 6),
            ("d4", 11),
            ("d8", 20),
            ("f4", 15),
            ("f8", 24),
        ] {
            assert_eq!(width(formats(format).unwrap()[0]), expected);
        }
        assert_eq!(float_text("1e-5".into(), "0.00001".into(), 6), "1e-05");
        assert_eq!(float_text("1e6".into(), "1000000".into(), 6), "1e+06");
        assert_eq!(float_text("1.5e0".into(), "1.5".into(), 6), "1.5");
        assert_eq!(float_text("inf".into(), "inf".into(), 6), "inf");
    }
}
