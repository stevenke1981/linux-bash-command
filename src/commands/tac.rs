use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
    regex::bre_to_regex,
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

fn last_regex(regex: &regex::bytes::Regex, bytes: &[u8]) -> Option<(usize, usize)> {
    let (mut next, mut last) = (0, None);
    while next < bytes.len() {
        let Some(found) = regex.find_at(bytes, next) else {
            break;
        };
        if !found.is_empty() {
            last = Some((found.start(), found.end()));
        }
        next = found.start() + 1;
    }
    last
}

fn reverse(
    data: &[u8],
    separator: &[u8],
    before: bool,
    regex: Option<&regex::bytes::Regex>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    if let Some(regex) = regex {
        let mut limit = data.len();
        let mut end = data.len();
        while let Some((start, stop)) = last_regex(regex, &data[..limit]) {
            let boundary = if before { start } else { stop };
            out.write_all(&data[boundary..end])?;
            end = boundary;
            limit = start;
        }
        return out.write_all(&data[..end]);
    }
    let mut boundaries = Vec::new();
    let mut pos = 0;
    while pos + separator.len() <= data.len() {
        if &data[pos..pos + separator.len()] == separator {
            boundaries.push(if before { pos } else { pos + separator.len() });
            pos += separator.len();
        } else {
            pos += 1;
        }
    }
    let mut end = data.len();
    for start in boundaries.into_iter().rev() {
        out.write_all(&data[start..end])?;
        end = start;
    }
    out.write_all(&data[..end])
}

fn reverse_file(
    file: &mut File,
    separator: &[u8],
    before: bool,
    regex: Option<&regex::bytes::Regex>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let mut offset = file.seek(SeekFrom::End(0))?;
    let mut pending = Vec::new();
    let mut search_limit = 0;
    let mut block = vec![0; io::BUFFER_SIZE];
    while offset > 0 {
        let count = (offset as usize).min(block.len());
        offset -= count as u64;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut block[..count])?;
        let mut joined = Vec::with_capacity(count + pending.len());
        joined.extend_from_slice(&block[..count]);
        joined.extend_from_slice(&pending);
        pending = joined;
        search_limit += count;
        let mut end = pending.len();
        loop {
            let found = if let Some(regex) = regex {
                last_regex(regex, &pending[..search_limit])
            } else {
                (0..search_limit
                    .saturating_sub(separator.len())
                    .saturating_add(1))
                    .rev()
                    .find_map(|pos| {
                        (pending.get(pos..pos + separator.len()) == Some(separator))
                            .then_some((pos, pos + separator.len()))
                    })
            };
            let Some((start, stop)) = found else {
                break;
            };
            if offset > 0 && start == 0 {
                break;
            }
            let boundary = if before { start } else { stop };
            out.write_all(&pending[boundary..end])?;
            end = boundary;
            search_limit = start;
        }
        pending.truncate(end);
    }
    out.write_all(&pending)
}

pub fn run(args: &[String]) -> i32 {
    let help = "Usage: tac [OPTION]... [FILE]...\n  -b, --before\n  -r, --regex\n  -s, --separator=STRING\n  --help --version\n";
    let parsed = match args::parse_command(
        "tac",
        args,
        help,
        &[
            OptSpec::flag("before", 'b'),
            OptSpec::flag("regex", 'r'),
            OptSpec::value("separator", 's'),
        ],
        true,
    ) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let (mut before, mut regex, mut separator, mut files) =
        (false, false, b"\n".to_vec(), Vec::new());
    for item in parsed {
        match item {
            Item::Opt("before", _) => before = true,
            Item::Opt("regex", _) => regex = true,
            Item::Opt("separator", Some(s)) => separator = s.into_bytes(),
            Item::Operand(s) => files.push(s),
            _ => unreachable!(),
        }
    }
    if separator.is_empty() {
        separator.push(0);
    }
    let regex = if regex {
        let pattern = String::from_utf8_lossy(&separator);
        let compiled = bre_to_regex(&pattern).and_then(|s| {
            regex::bytes::RegexBuilder::new(&s)
                .unicode(false)
                .build()
                .map_err(|e| e.to_string())
        });
        match compiled {
            Ok(r) => Some(r),
            Err(e) => return error::usage("tac", &e, 1),
        }
    } else {
        None
    };
    let mut failed = false;
    let code = io::output("tac", |out| {
        let default = ["-".to_owned()];
        for name in if files.is_empty() {
            &default[..]
        } else {
            &files[..]
        } {
            if name != "-" && name != "/dev/stdin" {
                match File::open(name) {
                    Ok(mut f) if f.metadata()?.is_file() => {
                        reverse_file(&mut f, &separator, before, regex.as_ref(), out)?;
                        continue;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("tac: {name}: {}", error::io_text(&e));
                        failed = true;
                        continue;
                    }
                }
            }
            match io::input(name) {
                Ok(mut r) => {
                    let mut bytes = Vec::new();
                    r.read_to_end(&mut bytes)?;
                    reverse(&bytes, &separator, before, regex.as_ref(), out)?;
                }
                Err(e) => {
                    eprintln!("tac: {name}: {}", error::io_text(&e));
                    failed = true;
                }
            }
        }
        Ok(())
    });
    code.max(i32::from(failed))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(s: &[u8], sep: &[u8], before: bool) -> Vec<u8> {
        let mut out = Vec::new();
        reverse(s, sep, before, None, &mut out).unwrap();
        out
    }
    #[test]
    fn terminated() {
        assert_eq!(r(b"a\nb\n", b"\n", false), b"b\na\n");
    }
    #[test]
    fn partial() {
        assert_eq!(r(b"a\nb", b"\n", false), b"ba\n");
    }
    #[test]
    fn before() {
        assert_eq!(r(b"a\nb\n", b"\n", true), b"\n\nba");
    }
    #[test]
    fn multi() {
        assert_eq!(r(b"a::b::", b"::", false), b"b::a::");
    }
    #[test]
    fn empty() {
        assert!(r(b"", b"\n", false).is_empty());
    }
    #[test]
    fn regex() {
        let mut out = Vec::new();
        reverse(
            b"a1b22c",
            b"",
            false,
            Some(&regex::bytes::Regex::new("[0-9]+").unwrap()),
            &mut out,
        )
        .unwrap();
        assert_eq!(out, b"c2b2a1");
    }
}
