use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::io::Read;

const HELP: &str = "Usage: wc [OPTION]... [FILE]...\nPrint newline, word, and byte counts.\n  -c, --bytes              print byte counts\n  -m, --chars              print character counts\n  -l, --lines              print newline counts\n  -w, --words              print word counts\n  -L, --max-line-length    print maximum display width\n      --files0-from=F      read NUL-terminated file names\n      --total=WHEN         auto, always, only, never\n      --help              display this help\n      --version           display version\n";
const SPECS: &[OptSpec] = &[
    OptSpec::flag("bytes", 'c'),
    OptSpec::flag("chars", 'm'),
    OptSpec::flag("lines", 'l'),
    OptSpec::flag("words", 'w'),
    OptSpec::flag("max-line-length", 'L'),
    OptSpec::value("files0-from", '\0'),
    OptSpec::value("total", '\0'),
    OptSpec::flag("help", '\0'),
    OptSpec::flag("version", '\0'),
];

#[derive(Default, Debug, PartialEq)]
struct Counts {
    lines: u64,
    words: u64,
    chars: u64,
    bytes: u64,
    longest: u64,
}

fn count(reader: &mut dyn Read) -> std::io::Result<Counts> {
    let mut result = Counts::default();
    let mut block = [0; io::BUFFER_SIZE];
    let (mut in_word, mut column, mut pending) = (false, 0, Vec::new());
    loop {
        let n = reader.read(&mut block)?;
        if n == 0 {
            break;
        }
        result.bytes += n as u64;
        for &byte in &block[..n] {
            result.lines += u64::from(byte == b'\n');
            let space = matches!(byte, 9..=13 | 32);
            if !space && !in_word {
                result.words += 1;
            }
            in_word = !space;
        }
        pending.extend_from_slice(&block[..n]);
        let mut start = 0;
        while start < pending.len() {
            let (valid, invalid, incomplete) = match std::str::from_utf8(&pending[start..]) {
                Ok(s) => (s.len(), 0, false),
                Err(e) => (
                    e.valid_up_to(),
                    e.error_len().unwrap_or(0),
                    e.error_len().is_none(),
                ),
            };
            for ch in std::str::from_utf8(&pending[start..start + valid])
                .unwrap()
                .chars()
            {
                result.chars += 1;
                match ch {
                    '\n' | '\r' | '\x0c' => {
                        result.longest = result.longest.max(column);
                        column = 0;
                    }
                    '\t' => column += 8 - column % 8,
                    _ if !ch.is_control() => column += 1,
                    _ => {}
                }
            }
            start += valid;
            if incomplete {
                break;
            }
            result.chars += invalid as u64;
            column += invalid as u64;
            start += invalid;
        }
        pending.drain(..start);
    }
    result.chars += pending.len() as u64;
    result.longest = result.longest.max(column + pending.len() as u64);
    Ok(result)
}

pub fn run(argv: &[String]) -> i32 {
    let items = match args::parse(argv, SPECS, true) {
        Ok(v) => v,
        Err(e) => return error::usage("wc", &e, 1),
    };
    if let Some(id) = items.iter().find_map(|item| match item {
        Item::Opt(id @ ("help" | "version"), _) => Some(*id),
        _ => None,
    }) {
        return args::help_version("wc", &[format!("--{id}")], HELP, true).unwrap();
    }
    let (mut flags, mut files, mut from, mut total) = (
        [false; 5],
        Vec::<std::ffi::OsString>::new(),
        None,
        "auto".to_string(),
    );
    for item in items {
        match item {
            Item::Operand(f) => files.push(f.into()),
            Item::Opt("files0-from", v) => from = v,
            Item::Opt("total", v) => total = v.unwrap(),
            Item::Opt(id, _) => {
                flags[match id {
                    "lines" => 0,
                    "words" => 1,
                    "chars" => 2,
                    "bytes" => 3,
                    _ => 4,
                }] = true
            }
        }
    }
    if !["auto", "always", "only", "never"].contains(&total.as_str()) {
        return error::usage("wc", format!("invalid argument '{total}' for 'total'"), 1);
    }
    let implicit_stdin = files.is_empty() && from.is_none();
    let mut failed = false;
    let mut input_count = files.len();
    if let Some(name) = from {
        if !files.is_empty() {
            return error::usage("wc", "extra operand with --files0-from", 1);
        }
        let mut data = Vec::new();
        match io::input(&name).and_then(|mut r| r.read_to_end(&mut data)) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("wc: {name}: {}", error::io_text(&e));
                return 1;
            }
        }
        let had_names = !data.is_empty();
        if data.last() == Some(&0) {
            data.pop();
        }
        if had_names {
            input_count = data.split(|&b| b == 0).count();
            for f in data.split(|&b| b == 0) {
                if f.is_empty() {
                    eprintln!("wc: invalid zero-length file name");
                    failed = true;
                    continue;
                }
                if name == "-" && f == b"-" {
                    eprintln!(
                        "wc: when reading file names from stdin, no file name of '-' allowed"
                    );
                    failed = true;
                    continue;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStringExt;
                    files.push(std::ffi::OsString::from_vec(f.to_vec()));
                }
                #[cfg(not(unix))]
                files.push(String::from_utf8_lossy(f).into_owned().into());
            }
        }
    } else if files.is_empty() {
        files.push("-".into());
        input_count = 1;
    }
    if !flags.iter().any(|v| *v) {
        flags = [true, true, false, true, false];
    }
    let single = flags.iter().filter(|v| **v).count() == 1;
    let mut stat_total = 0u64;
    let mut nonregular = false;
    for f in &files {
        match std::fs::metadata(f) {
            Ok(meta) if meta.is_file() && f != "-" => {
                stat_total = stat_total.saturating_add(meta.len())
            }
            _ if f == "-" || std::fs::metadata(f).is_ok() => nonregular = true,
            _ => {}
        }
    }
    let width = if total == "only" || (single && input_count == 1) {
        0
    } else {
        (stat_total.to_string().len()).max(if nonregular { 7 } else { 1 })
    };
    let code = io::output("wc", |out| {
        let mut sum = Counts::default();
        for f in &files {
            let display = f.to_string_lossy();
            let result = match f.to_str() {
                Some(name @ ("-" | "/dev/stdin" | "/dev/null")) => {
                    io::input(name).and_then(|mut r| count(&mut r))
                }
                _ => std::fs::File::open(f).and_then(|mut r| count(&mut r)),
            };
            match result {
                Ok(c) => {
                    sum.lines += c.lines;
                    sum.words += c.words;
                    sum.chars += c.chars;
                    sum.bytes += c.bytes;
                    sum.longest = sum.longest.max(c.longest);
                    if total != "only" {
                        write_counts(
                            out,
                            &c,
                            flags,
                            width,
                            if implicit_stdin { None } else { Some(&display) },
                        )?;
                    }
                }
                Err(e) => {
                    eprintln!("wc: {display}: {}", error::io_text(&e));
                    failed = true;
                }
            }
        }
        if total == "always" || total == "only" || (total == "auto" && input_count > 1) {
            write_counts(
                out,
                &sum,
                flags,
                width,
                if total == "only" { None } else { Some("total") },
            )?;
        }
        Ok(())
    });
    if code != 0 || failed {
        1
    } else {
        0
    }
}

fn write_counts(
    out: &mut dyn std::io::Write,
    c: &Counts,
    flags: [bool; 5],
    width: usize,
    name: Option<&str>,
) -> std::io::Result<()> {
    let mut first = true;
    for (value, selected) in [c.lines, c.words, c.chars, c.bytes, c.longest]
        .into_iter()
        .zip(flags)
    {
        if selected {
            if !first {
                write!(out, " ")?;
            }
            write!(out, "{value:width$}")?;
            first = false;
        }
    }
    if let Some(name) = name {
        write!(
            out,
            " {}",
            crate::common::path::display_path(std::path::Path::new(name))
        )?;
    }
    writeln!(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(data: &[u8]) -> Counts {
        count(&mut std::io::Cursor::new(data)).unwrap()
    }
    #[test]
    fn empty() {
        assert_eq!(c(b""), Counts::default());
    }
    #[test]
    fn words() {
        assert_eq!(c(b" a\tb\n\rc\x0bd\x0ce ").words, 5);
    }
    #[test]
    fn lines() {
        assert_eq!(c(b"a\nb\nc").lines, 2);
    }
    #[test]
    fn utf8_invalid() {
        assert_eq!(c(b"\xc3\xa9\xff\xf0").chars, 3);
    }
    #[test]
    fn tabs() {
        assert_eq!(c(b"a\tb\n123").longest, 9);
    }
    #[test]
    fn utf8_block_boundary() {
        let s = format!("{}\u{e9}", "a".repeat(io::BUFFER_SIZE - 1));
        assert_eq!(c(s.as_bytes()).chars, io::BUFFER_SIZE as u64);
    }
}
