use crate::common::args::{self, Item, OptSpec};
mod stream;

const HELP: &str = "Usage: split [OPTION]... [FILE [PREFIX]]\nSplit FILE into pieces.\n  -a, --suffix-length=N       suffix length\n      --additional-suffix=S   append S to each suffix\n  -b, --bytes=SIZE            bytes per output file\n  -C, --line-bytes=SIZE       at most SIZE bytes, preserving lines where possible\n  -d, --numeric-suffixes[=N]  numeric suffixes\n  -x, --hex-suffixes[=N]      hexadecimal suffixes\n  -e, --elide-empty-files     omit empty files\n  -l, --lines=N               lines per file\n  -n, --number=CHUNKS         split into chunks\n  -t, --separator=SEP         record separator\n  -u, --unbuffered            unbuffered round robin output\n      --verbose               report output files\n      --help                  display this help\n      --version               output version information\n";

#[derive(Clone, Copy)]
enum Mode {
    Lines(usize),
    Bytes(usize),
    LineBytes(usize),
    Number(char, Option<usize>, usize),
}
fn chunk_len(length: u64, total: usize, index: usize) -> u64 {
    let total = total as u64;
    length / total + u64::from((index as u64) < length % total)
}
fn chunk_at(length: u64, total: usize, offset: u64) -> usize {
    let total = total as u64;
    let base = length / total;
    let extra = length % total;
    let larger_region = (base + 1) * extra;
    if offset < larger_region {
        (offset / (base + 1)) as usize
    } else {
        (offset - larger_region)
            .checked_div(base)
            .map_or(total - 1, |part| (extra + part).min(total - 1)) as usize
    }
}

fn positive(s: &str) -> Result<usize, String> {
    let n = s
        .parse::<usize>()
        .map_err(|_| format!("invalid number: '{s}'"))?;
    if n == 0 {
        Err(format!("invalid number: '{s}'"))
    } else {
        Ok(n)
    }
}
fn size(s: &str) -> Result<usize, String> {
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let n = positive(&s[..split])?;
    let suffix = &s[split..];
    let (base, exp) = match suffix {
        "" => (1usize, 0),
        "b" => (512, 1),
        "K" | "k" | "KiB" => (1024, 1),
        "KB" | "kB" => (1000, 1),
        "M" | "MiB" => (1024, 2),
        "MB" => (1000, 2),
        "G" | "GiB" => (1024, 3),
        "GB" => (1000, 3),
        "T" | "TiB" => (1024, 4),
        "TB" => (1000, 4),
        "P" | "PiB" => (1024, 5),
        "PB" => (1000, 5),
        "E" | "EiB" => (1024, 6),
        "EB" => (1000, 6),
        _ => return Err(format!("invalid number: '{s}'")),
    };
    n.checked_mul(
        base.checked_pow(exp)
            .ok_or_else(|| format!("invalid number: '{s}'"))?,
    )
    .ok_or_else(|| format!("invalid number: '{s}'"))
}
fn number(s: &str) -> Result<Mode, String> {
    let (kind, s) = if let Some(x) = s.strip_prefix("l/") {
        ('l', x)
    } else if let Some(x) = s.strip_prefix("r/") {
        ('r', x)
    } else {
        ('b', s)
    };
    let (which, total) = if let Some((a, b)) = s.split_once('/') {
        (Some(positive(a)?), positive(b)?)
    } else {
        (None, positive(s)?)
    };
    if which.is_some_and(|x| x > total) {
        return Err(format!("invalid number of chunks: '{s}'"));
    }
    Ok(Mode::Number(kind, which, total))
}
fn suffix(mut index: usize, width: usize, base: u8) -> Option<String> {
    let alphabet = match base {
        10 => b"0123456789".as_slice(),
        16 => b"0123456789abcdef".as_slice(),
        _ => b"abcdefghijklmnopqrstuvwxyz".as_slice(),
    };
    let mut output = vec![alphabet[0]; width];
    for digit in output.iter_mut().rev() {
        *digit = alphabet[index % alphabet.len()];
        index /= alphabet.len();
    }
    if index != 0 {
        return None;
    }
    String::from_utf8(output).ok()
}
fn automatic_suffix(mut index: usize, mut width: usize, base: u8) -> Option<String> {
    let last = match base {
        10 => '9',
        16 => 'f',
        _ => 'z',
    };
    let mut prefix = String::new();
    loop {
        let boundary = (base as usize)
            .checked_pow((width - 1) as u32)
            .and_then(|n| n.checked_mul((base - 1) as usize));
        match boundary {
            Some(n) if index >= n => {
                index -= n;
                prefix.push(last);
                width += 1;
            }
            _ => {
                prefix.push_str(&suffix(index, width, base)?);
                return Some(prefix);
            }
        }
    }
}

pub fn run(arguments: &[String]) -> i32 {
    let specs = [
        OptSpec::value("suffix-length", 'a'),
        OptSpec::value("additional-suffix", '\0'),
        OptSpec::value("bytes", 'b'),
        OptSpec::value("line-bytes", 'C'),
        OptSpec::optional("numeric-suffixes", 'd'),
        OptSpec::optional("hex-suffixes", 'x'),
        OptSpec::flag("elide-empty-files", 'e'),
        OptSpec::value("lines", 'l'),
        OptSpec::value("number", 'n'),
        OptSpec::value("separator", 't'),
        OptSpec::flag("verbose", '\0'),
        OptSpec::flag("unbuffered", 'u'),
        OptSpec::value("filter", '\0'),
    ];
    let items = match args::parse_command("split", arguments, HELP, &specs, true) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut mode = Mode::Lines(1000);
    let mut chosen = false;
    let mut width = 2usize;
    let mut width_set = false;
    let mut first_set = false;
    let mut base = 26u8;
    let mut first = 0usize;
    let mut extra = String::new();
    let mut sep = b'\n';
    let mut elide = false;
    let mut verbose = false;
    let mut unbuffered = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt("suffix-length", Some(v)) => match positive(&v) {
                Ok(n) if n < 1000 => {
                    width = n;
                    width_set = true;
                }
                _ => {
                    eprintln!("split: invalid suffix length: '{v}'");
                    return 1;
                }
            },
            Item::Opt("additional-suffix", Some(v)) => extra = v,
            Item::Opt(id @ ("bytes" | "line-bytes" | "lines" | "number"), Some(v)) => {
                if chosen {
                    eprintln!("split: cannot split in more than one way");
                    return 1;
                }
                chosen = true;
                let parsed = match id {
                    "bytes" => size(&v).map(Mode::Bytes),
                    "line-bytes" => size(&v).map(Mode::LineBytes),
                    "lines" => positive(&v).map(Mode::Lines),
                    _ => number(&v),
                };
                match parsed {
                    Ok(m) => mode = m,
                    Err(e) => {
                        eprintln!("split: {e}");
                        return 1;
                    }
                }
            }
            Item::Opt(id @ ("numeric-suffixes" | "hex-suffixes"), v) => {
                base = if id == "numeric-suffixes" { 10 } else { 16 };
                if let Some(v) = v {
                    first_set = true;
                    match usize::from_str_radix(&v, u32::from(base)) {
                        Ok(x) => first = x,
                        Err(_) => {
                            eprintln!("split: invalid suffix: '{v}'");
                            return 1;
                        }
                    }
                }
            }
            Item::Opt("elide-empty-files", _) => elide = true,
            Item::Opt("verbose", _) => verbose = true,
            Item::Opt("unbuffered", _) => unbuffered = true,
            Item::Opt("separator", Some(v)) => {
                let v = if v == "\\0" { "\0" } else { &v };
                if v.len() != 1 {
                    eprintln!("split: invalid separator");
                    return 1;
                }
                sep = v.as_bytes()[0];
            }
            Item::Opt("filter", _) => {
                eprintln!("split: --filter is unsupported");
                return 1;
            }
            Item::Operand(v) => operands.push(v),
            _ => {}
        }
    }
    if operands.len() > 2 {
        return crate::common::error::usage("split", "extra operand", 1);
    }
    if first_set && suffix(first, width, base).is_none() {
        return crate::common::error::usage(
            "split",
            "numerical suffix start value is too large for the suffix length",
            1,
        );
    }
    let mut fixed = width_set || first_set;
    if let Mode::Number(_, _, total) = mode {
        let Some(last) = first.checked_add(total - 1) else {
            return crate::common::error::usage("split", "too many output files", 1);
        };
        if width_set && suffix(last, width, base).is_none() {
            return crate::common::error::usage("split", "the suffix length is too short", 1);
        }
        while suffix(last, width, base).is_none() {
            width += 1;
        }
        fixed = true;
    }
    let input = operands.first().map_or("-", String::as_str);
    let prefix = operands.get(1).map_or("x", String::as_str);
    stream::run(
        input, prefix, mode, sep, width, fixed, base, first, &extra, elide, verbose, unbuffered,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes() {
        assert_eq!(size("1K"), Ok(1024));
        assert_eq!(size("2MB"), Ok(2_000_000));
        assert!(size("0").is_err());
        for (text, expected) in [
            ("1b", 512),
            ("1kB", 1000),
            ("1MiB", 1_048_576),
            ("1G", 1_073_741_824),
            ("1GB", 1_000_000_000),
        ] {
            assert_eq!(size(text), Ok(expected), "{text}");
        }
        #[cfg(target_pointer_width = "64")]
        for text in ["1T", "1TB", "1P", "1PB", "1E", "1EB"] {
            assert!(size(text).is_ok(), "{text}");
        }
        for text in ["", "bad", "1bad", "18446744073709551615K"] {
            assert!(size(text).is_err(), "{text}");
        }
    }
    #[test]
    fn suffixes() {
        assert_eq!(suffix(27, 2, 26).as_deref(), Some("bb"));
        assert_eq!(suffix(16, 2, 16).as_deref(), Some("10"));
        assert!(suffix(100, 2, 10).is_none());
        assert_eq!(automatic_suffix(649, 2, 26).as_deref(), Some("yz"));
        assert_eq!(automatic_suffix(650, 2, 26).as_deref(), Some("zaaa"));
    }
    #[test]
    fn split_modes() {
        assert!(matches!(number("3"), Ok(Mode::Number('b', None, 3))));
        assert!(matches!(number("2/3"), Ok(Mode::Number('b', Some(2), 3))));
        assert!(matches!(number("l/2/3"), Ok(Mode::Number('l', Some(2), 3))));
        assert!(matches!(number("r/3"), Ok(Mode::Number('r', None, 3))));
        assert!(number("4/3").is_err());
        assert!(number("0").is_err());
    }
    #[test]
    fn balanced_chunk_boundaries() {
        assert_eq!(
            (0..3).map(|i| chunk_len(14, 3, i)).collect::<Vec<_>>(),
            [5, 5, 4]
        );
        assert_eq!(
            (0..3).map(|i| chunk_len(7, 3, i)).collect::<Vec<_>>(),
            [3, 2, 2]
        );
        assert_eq!(
            (0..5).map(|i| chunk_len(2, 5, i)).collect::<Vec<_>>(),
            [1, 1, 0, 0, 0]
        );
        for (offset, expected) in [(0, 0), (4, 0), (5, 1), (9, 1), (10, 2), (13, 2)] {
            assert_eq!(chunk_at(14, 3, offset), expected);
        }
    }
}
