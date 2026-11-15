use super::{
    engine::{self, Algorithm},
    Options,
};
use crate::common::{error::io_text, io::input, path::display_path};
use std::{
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

struct Record {
    algorithm: Algorithm,
    digest: Vec<u8>,
    name: PathBuf,
}

pub fn verify(command: &str, options: &Options, out: &mut dyn Write) -> io::Result<bool> {
    let mut failed = false;
    for list in options.inputs() {
        match input(&list) {
            Ok(mut reader) => failed |= verify_list(command, &list, options, &mut reader, out)?,
            Err(error) => {
                eprintln!("{command}: {list}: {}", io_text(&error));
                failed = true;
            }
        }
    }
    Ok(failed)
}

fn verify_list(
    command: &str,
    list: &str,
    options: &Options,
    reader: &mut dyn BufRead,
    out: &mut dyn Write,
) -> io::Result<bool> {
    let (mut valid, mut malformed, mut mismatches, mut unreadable, mut checked) = (0, 0, 0, 0, 0);
    let mut line = Vec::new();
    let mut number = 0;
    loop {
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                if !options.status {
                    eprintln!("{command}: {list}: {}", io_text(&error));
                }
                return Ok(true);
            }
        }
        number += 1;
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() || line.first() == Some(&b'#') {
            line.clear();
            continue;
        }
        let Some(record) = parse(
            &line,
            options.algorithm,
            options.length_given,
            options.auto_algorithm,
        )
        .filter(|record| {
            !(matches!(list, "-" | "/dev/stdin")
                && matches!(record.name.to_str(), Some("-" | "/dev/stdin")))
        }) else {
            malformed += 1;
            if options.warn && !options.status {
                eprintln!(
                    "{command}: {list}:{number}: improperly formatted {} checksum line",
                    options.algorithm.tag()
                );
            }
            line.clear();
            continue;
        };
        valid += 1;
        let name = display_path(&record.name);
        match record_input(&record.name) {
            Err(error) if options.ignore_missing && error.kind() == io::ErrorKind::NotFound => {}
            result => {
                checked += 1;
                match result.and_then(|mut input| engine::hash(record.algorithm, &mut input)) {
                    Ok(actual) => {
                        let matches = actual.bytes == record.digest;
                        if !matches {
                            mismatches += 1;
                        }
                        if !options.status && (!matches || !options.quiet) {
                            writeln!(out, "{name}: {}", if matches { "OK" } else { "FAILED" })?;
                        }
                    }
                    Err(error) => {
                        unreadable += 1;
                        if !options.status {
                            eprintln!("{command}: {name}: {}", io_text(&error));
                            writeln!(out, "{name}: FAILED open or read")?;
                        }
                    }
                }
            }
        }
        line.clear();
    }
    if !options.status {
        if valid == 0 {
            eprintln!("{command}: {list}: no properly formatted checksum lines found");
        } else {
            if malformed != 0 {
                eprintln!(
                    "{command}: WARNING: {malformed} {} improperly formatted",
                    if malformed == 1 {
                        "line is"
                    } else {
                        "lines are"
                    }
                );
            }
            if mismatches != 0 {
                eprintln!(
                    "{command}: WARNING: {mismatches} computed checksum{} did NOT match",
                    if mismatches == 1 { "" } else { "s" }
                );
            }
            if unreadable != 0 {
                eprintln!(
                    "{command}: WARNING: {unreadable} listed file{} could not be read",
                    if unreadable == 1 { "" } else { "s" }
                );
            }
            if checked == 0 {
                eprintln!("{command}: {list}: no file was verified");
            }
        }
    }
    Ok(valid == 0
        || checked == 0
        || mismatches != 0
        || unreadable != 0
        || (options.strict && malformed != 0))
}

fn parse(line: &[u8], default: Algorithm, length_given: bool, auto: bool) -> Option<Record> {
    let first = line.iter().position(|b| !matches!(b, b' ' | b'\t'))?;
    let line = &line[first..];
    let escaped = line.first() == Some(&b'\\');
    let line = if escaped { &line[1..] } else { line };
    if let Some(open) = line.windows(2).position(|w| w == b" (") {
        let algorithm = std::str::from_utf8(&line[..open])
            .ok()
            .and_then(tag_algorithm);
        if let Some(algorithm) = algorithm {
            let close = line.windows(4).rposition(|w| w == b") = ")?;
            if !algorithm.crypto() {
                return None;
            }
            let digest = unhex(&line[close + 4..]).or_else(|| {
                auto.then(|| super::super::basenc::decode_bytes(&line[close + 4..]))
                    .flatten()
            })?;
            if !auto && algorithm != default {
                if matches!(
                    (algorithm, default),
                    (Algorithm::Blake2b(_), Algorithm::Blake2b(_))
                ) && !length_given
                {
                } else {
                    return None;
                }
            }
            if digest.len() != algorithm.bytes() {
                return None;
            }
            return Some(Record {
                algorithm,
                digest,
                name: filename(&line[open + 2..close], escaped)?,
            });
        }
    }
    if !default.crypto() {
        return None;
    }
    let space = line.iter().position(|b| matches!(b, b' ' | b'\t'))?;
    let digest = unhex(&line[..space]).or_else(|| {
        auto.then(|| super::super::basenc::decode_bytes(&line[..space]))
            .flatten()
    })?;
    let algorithm = if !length_given
        && matches!(default, Algorithm::Blake2b(_))
        && !digest.is_empty()
        && digest.len() <= 64
    {
        Algorithm::Blake2b(digest.len())
    } else {
        default
    };
    if digest.len() != algorithm.bytes() {
        return None;
    }
    let rest = &line[space + 1..];
    let rest = if matches!(rest.first(), Some(b' ' | b'*')) {
        &rest[1..]
    } else {
        rest
    };
    Some(Record {
        algorithm,
        digest,
        name: filename(rest, escaped)?,
    })
}

fn tag_algorithm(tag: &str) -> Option<Algorithm> {
    Algorithm::named(tag).or_else(|| {
        let bits = tag.strip_prefix("BLAKE2b-")?.parse::<usize>().ok()?;
        (bits > 0 && bits <= 512 && bits.is_multiple_of(8)).then_some(Algorithm::Blake2b(bits / 8))
    })
}
fn unhex(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return None;
    }
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            Some((char::from(pair[0]).to_digit(16)? * 16 + char::from(pair[1]).to_digit(16)?) as u8)
        })
        .collect()
}
fn record_input(path: &Path) -> io::Result<Box<dyn BufRead>> {
    if let Some(name) = path.to_str() {
        input(name)
    } else {
        Ok(Box::new(BufReader::with_capacity(
            crate::common::io::BUFFER_SIZE,
            std::fs::File::open(path)?,
        )))
    }
}
fn filename(bytes: &[u8], escaped: bool) -> Option<PathBuf> {
    if bytes.is_empty() {
        return None;
    }
    if !escaped {
        return Some(byte_path(bytes.to_vec()));
    }
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
            result.push(match bytes.get(i)? {
                b'\\' => b'\\',
                b'n' => b'\n',
                b'r' => b'\r',
                _ => return None,
            });
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    Some(byte_path(result))
}
#[cfg(unix)]
fn byte_path(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes).into()
}
#[cfg(not(unix))]
fn byte_path(bytes: Vec<u8>) -> PathBuf {
    String::from_utf8_lossy(&bytes).into_owned().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(s: &str) -> Option<Record> {
        parse(s.as_bytes(), Algorithm::Md5, false, false)
    }
    #[test]
    fn text() {
        assert_eq!(
            p("900150983cd24fb0d6963f7d28e17f72  file").unwrap().name,
            Path::new("file")
        );
    }
    #[test]
    fn binary() {
        assert_eq!(
            p("900150983cd24fb0d6963f7d28e17f72 *file")
                .unwrap()
                .digest
                .len(),
            16
        );
    }
    #[test]
    fn tagged() {
        assert!(p("MD5 (file) = 900150983cd24fb0d6963f7d28e17f72").is_some());
    }
    #[test]
    fn escapes() {
        assert_eq!(
            p("\\900150983cd24fb0d6963f7d28e17f72  line\\nname\\\\x")
                .unwrap()
                .name,
            Path::new("line\nname\\x")
        );
    }
    #[test]
    fn rejects_malformed() {
        for s in [
            "",
            "zz  file",
            "00  file",
            "900150983cd24fb0d6963f7d28e17f72  ",
            "SHA1 (f) = 900150983cd24fb0d6963f7d28e17f72",
        ] {
            assert!(p(s).is_none(), "{s}");
        }
    }
    #[test]
    fn horizontal_whitespace_is_allowed_but_other_control_prefixes_are_not() {
        for prefix in [" ", "\t", " \t "] {
            assert_eq!(
                p(&format!("{prefix}900150983cd24fb0d6963f7d28e17f72\tfile"))
                    .unwrap()
                    .name,
                Path::new("file")
            );
        }
        for prefix in ["\r", "\x0b", "\x0c"] {
            assert!(p(&format!("{prefix}900150983cd24fb0d6963f7d28e17f72  file")).is_none());
        }
    }
    #[test]
    fn variable_blake_length() {
        assert!(parse(b"aabb  file", Algorithm::Blake2b(64), false, false).is_some());
    }
    #[test]
    fn untagged_names_can_resemble_tagged_records() {
        assert_eq!(
            p("900150983cd24fb0d6963f7d28e17f72  a (b) = c")
                .unwrap()
                .name,
            Path::new("a (b) = c")
        );
    }
    #[cfg(unix)]
    #[test]
    fn checksum_records_preserve_non_utf8_filename_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let mut line = b"900150983cd24fb0d6963f7d28e17f72  ".to_vec();
        line.extend_from_slice(b"name-\xff");
        let record = parse(&line, Algorithm::Md5, false, false).unwrap();
        assert_eq!(record.name.as_os_str().as_bytes(), b"name-\xff");
        #[cfg(target_os = "linux")]
        {
            let root = std::env::temp_dir().join(format!(
                "agentcmd-hash-bytes-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            let path = root.join(record.name);
            std::fs::write(&path, b"abc").unwrap();
            let actual = engine::hash(Algorithm::Md5, &mut record_input(&path).unwrap()).unwrap();
            std::fs::remove_dir_all(root).unwrap();
            assert_eq!(actual.bytes, record.digest);
        }
    }
}
