use crate::common::{
    args::{self, Item, OptSpec},
    error,
};
use std::{
    fs::{self, OpenOptions},
    path::Path,
};
const HELP: &str = "Usage: truncate OPTION... FILE...\nShrink or extend the size of each FILE.\n  -c, --no-create        do not create any files\n  -o, --io-blocks        treat SIZE as number of I/O blocks\n  -r, --reference=FILE   base size on reference file\n  -s, --size=SIZE        set or adjust file size\n      --help             display this help\n      --version          output version information\n";

fn number(text: &str) -> Option<u64> {
    let end = text
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(text.len());
    let n = text[..end].parse::<u64>().ok()?;
    let suffix = &text[end..];
    let multiplier = match suffix {
        "" | "c" => 1,
        "w" => 2,
        "b" => 512,
        "K" | "k" => 1024,
        "M" => 1024_u64.pow(2),
        "G" => 1024_u64.pow(3),
        "T" => 1024_u64.pow(4),
        "P" => 1024_u64.pow(5),
        "E" => 1024_u64.pow(6),
        "KB" | "kB" => 1000,
        "MB" => 1000_u64.pow(2),
        "GB" => 1000_u64.pow(3),
        "TB" => 1000_u64.pow(4),
        "PB" => 1000_u64.pow(5),
        "EB" => 1000_u64.pow(6),
        _ => return None,
    };
    n.checked_mul(multiplier)
}
fn size(spec: &str, base: u64, block: u64) -> Option<u64> {
    let prefix = spec
        .as_bytes()
        .first()
        .copied()
        .filter(|c| b"+-<>/%".contains(c));
    let operand = number(if prefix.is_some() { &spec[1..] } else { spec })?.checked_mul(block)?;
    match prefix {
        None => Some(operand),
        Some(b'+') => base.checked_add(operand),
        Some(b'-') => Some(base.saturating_sub(operand)),
        Some(b'<') => Some(base.min(operand)),
        Some(b'>') => Some(base.max(operand)),
        Some(b'/') if operand != 0 => Some(base / operand * operand),
        Some(b'%') if operand != 0 => base.checked_add(operand - 1).map(|n| n / operand * operand),
        _ => None,
    }
}
#[cfg(unix)]
fn block_size(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blksize()
}
#[cfg(not(unix))]
fn block_size(_: &fs::Metadata) -> u64 {
    4096
}

pub fn run(argv: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("no-create", 'c'),
        OptSpec::flag("io-blocks", 'o'),
        OptSpec::value("reference", 'r'),
        OptSpec::value("size", 's'),
    ];
    let parsed = match args::parse_command("truncate", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let (mut no_create, mut blocks, mut reference, mut spec, mut files) =
        (false, false, None, None, Vec::new());
    for item in parsed {
        match item {
            Item::Opt("no-create", _) => no_create = true,
            Item::Opt("io-blocks", _) => blocks = true,
            Item::Opt("reference", v) => reference = v,
            Item::Opt("size", v) => spec = v,
            Item::Operand(v) => files.push(v),
            _ => unreachable!(),
        }
    }
    if reference.is_none() && spec.is_none() {
        return error::usage(
            "truncate",
            "you must specify either '--size' or '--reference'",
            1,
        );
    }
    if files.is_empty() {
        return error::usage("truncate", "missing file operand", 1);
    }
    if spec.as_deref().is_some_and(|s| {
        let numeric = s.strip_prefix(['+', '-', '<', '>', '/', '%']).unwrap_or(s);
        number(numeric).is_none()
            || ((s.starts_with('/') || s.starts_with('%')) && number(numeric) == Some(0))
    }) {
        return error::usage(
            "truncate",
            format!("invalid number: '{}'", spec.unwrap()),
            1,
        );
    }
    let reference_size = if let Some(path) = reference {
        match fs::metadata(&path) {
            Ok(m) => Some(m.len()),
            Err(e) => {
                eprintln!(
                    "truncate: failed to get attributes of '{}': {}",
                    path,
                    error::io_text(&e)
                );
                return 1;
            }
        }
    } else {
        None
    };
    let mut failed = false;
    for name in files {
        let path = Path::new(&name);
        let file = match OpenOptions::new().write(true).create(!no_create).open(path) {
            Ok(f) => f,
            Err(e) if no_create && e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!(
                    "truncate: cannot open '{}': {}",
                    crate::common::path::display_path(path),
                    error::io_text(&e)
                );
                failed = true;
                continue;
            }
        };
        let meta = match file.metadata() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("truncate: cannot stat '{}': {}", name, error::io_text(&e));
                failed = true;
                continue;
            }
        };
        let target = match spec.as_deref() {
            Some(s) => size(
                s,
                reference_size.unwrap_or(meta.len()),
                if blocks { block_size(&meta) } else { 1 },
            ),
            None => reference_size,
        };
        match target.and_then(|n| {
            file.set_len(n).err().map_or(Some(()), |e| {
                eprintln!(
                    "truncate: failed to truncate '{}': {}",
                    name,
                    error::io_text(&e)
                );
                None
            })
        }) {
            Some(()) => {}
            None => {
                failed = true;
            }
        }
    }
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suffix() {
        assert_eq!(number("2K"), Some(2048));
        assert_eq!(number("2KB"), Some(2000));
    }
    #[test]
    fn absolute() {
        assert_eq!(size("10", 20, 1), Some(10));
    }
    #[test]
    fn relative() {
        assert_eq!(size("+10", 20, 1), Some(30));
        assert_eq!(size("-30", 20, 1), Some(0));
    }
    #[test]
    fn clamp() {
        assert_eq!(size("<10", 20, 1), Some(10));
        assert_eq!(size(">30", 20, 1), Some(30));
    }
    #[test]
    fn round() {
        assert_eq!(size("/8", 18, 1), Some(16));
        assert_eq!(size("%8", 18, 1), Some(24));
        assert_eq!(size("/0", 18, 1), None);
    }
}
