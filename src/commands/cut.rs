use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
const HELP: &str = "Usage: cut OPTION... [FILE]...\nPrint selected parts of lines.\n  -b, --bytes=LIST          select bytes\n  -c, --characters=LIST     select characters (bytes)\n  -f, --fields=LIST         select fields\n  -d, --delimiter=DELIM     field delimiter (default TAB)\n      --complement         complement selection\n  -s, --only-delimited     suppress lines without delimiters\n      --output-delimiter=STR  output delimiter\n  -z, --zero-terminated    NUL line delimiter\n  -n                       ignored\n      --help               display help\n      --version            display version\n";
const SPECS: &[OptSpec] = &[
    OptSpec::value("bytes", 'b'),
    OptSpec::value("characters", 'c'),
    OptSpec::value("fields", 'f'),
    OptSpec::value("delimiter", 'd'),
    OptSpec::flag("complement", '\0'),
    OptSpec::flag("only-delimited", 's'),
    OptSpec::value("output-delimiter", '\0'),
    OptSpec::flag("zero-terminated", 'z'),
    OptSpec::flag("", 'n'),
    OptSpec::flag("help", '\0'),
    OptSpec::flag("version", '\0'),
];

fn ranges(list: &str) -> Result<Vec<(usize, usize)>, String> {
    let mut result = Vec::new();
    for part in list.split([',', ' ', '\t']) {
        if part.is_empty() {
            return Err("invalid byte, character or field list".into());
        }
        let parse = |v: &str| {
            v.parse::<usize>()
                .ok()
                .filter(|n| *n != 0)
                .ok_or_else(|| "fields and positions are numbered from 1".to_string())
        };
        let pair = if let Some((a, b)) = part.split_once('-') {
            (
                if a.is_empty() { 1 } else { parse(a)? },
                if b.is_empty() { usize::MAX } else { parse(b)? },
            )
        } else {
            let n = parse(part)?;
            (n, n)
        };
        if pair.0 > pair.1 {
            return Err("invalid decreasing range".into());
        }
        result.push(pair);
    }
    result.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in result {
        if let Some(last) = merged.last_mut() {
            if a <= last.1.saturating_add(1) {
                last.1 = last.1.max(b);
                continue;
            }
        }
        merged.push((a, b));
    }
    Ok(merged)
}

fn select(
    line: &[u8],
    ranges: &[(usize, usize)],
    fields: bool,
    delimiter: u8,
    complement: bool,
    suppress: bool,
    output_delimiter: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let selected = |n| ranges.iter().any(|&(a, b)| a <= n && n <= b) != complement;
    let mut out = Vec::new();
    if fields {
        if !line.contains(&delimiter) {
            return (!suppress).then(|| line.to_vec());
        }
        let mut first = true;
        for (i, field) in line.split(|&b| b == delimiter).enumerate() {
            if selected(i + 1) {
                if !first {
                    out.extend_from_slice(output_delimiter.unwrap_or(&[delimiter]));
                }
                out.extend_from_slice(field);
                first = false;
            }
        }
    } else {
        let mut previous = false;
        let mut any = false;
        for (i, byte) in line.iter().enumerate() {
            let keep = selected(i + 1);
            if keep {
                if any && !previous {
                    if let Some(d) = output_delimiter {
                        out.extend_from_slice(d);
                    }
                }
                out.push(*byte);
                any = true;
            }
            previous = keep;
        }
    }
    Some(out)
}

pub fn run(argv: &[String]) -> i32 {
    let items = match args::parse(argv, SPECS, true) {
        Ok(v) => v,
        Err(e) => return error::usage("cut", &e, 1),
    };
    if let Some(id) = items.iter().find_map(|item| match item {
        Item::Opt(id @ ("help" | "version"), _) => Some(*id),
        _ => None,
    }) {
        return args::help_version("cut", &[format!("--{id}")], HELP, true).unwrap();
    }
    let (mut files, mut mode, mut list, mut delimiter, mut output_delimiter) =
        (Vec::new(), None, String::new(), None, None);
    let (mut complement, mut suppress, mut zero) = (false, false, false);
    for item in items {
        match item {
            Item::Operand(f) => files.push(f),
            Item::Opt(id @ ("bytes" | "characters" | "fields"), value) => {
                if mode.is_some() {
                    return error::usage("cut", "only one list may be specified", 1);
                }
                mode = Some(id);
                list = value.unwrap();
            }
            Item::Opt("delimiter", v) => delimiter = v,
            Item::Opt("output-delimiter", v) => output_delimiter = v,
            Item::Opt("complement", _) => complement = true,
            Item::Opt("only-delimited", _) => suppress = true,
            Item::Opt("zero-terminated", _) => zero = true,
            _ => {}
        }
    }
    let Some(mode) = mode else {
        return error::usage(
            "cut",
            "you must specify a list of bytes, characters, or fields",
            1,
        );
    };
    let fields = mode == "fields";
    if !fields && (delimiter.is_some() || suppress) {
        return error::usage(
            "cut",
            "an input delimiter may be specified only when operating on fields",
            1,
        );
    }
    if delimiter.as_ref().is_some_and(|s| s.len() > 1) {
        return error::usage("cut", "the delimiter must be a single character", 1);
    }
    let delim = delimiter
        .as_ref()
        .map_or(b'\t', |s| s.as_bytes().first().copied().unwrap_or(0));
    let output_delimiter = output_delimiter.map(|s| {
        if s.is_empty() {
            vec![0]
        } else {
            s.into_bytes()
        }
    });
    let ranges = match ranges(&list) {
        Ok(v) => v,
        Err(e) => return error::usage("cut", &e, 1),
    };
    let mut failed = false;
    let record_delim = if zero { 0 } else { b'\n' };
    let code = io::output("cut", |out| {
        failed = io::for_inputs("cut", &files, |_, reader| {
            io::records(reader, record_delim, |line| {
                if let Some(bytes) = select(
                    line,
                    &ranges,
                    fields,
                    delim,
                    complement,
                    suppress,
                    output_delimiter.as_deref(),
                ) {
                    out.write_all(&bytes)?;
                    out.write_all(&[record_delim])?;
                }
                Ok(())
            })
        })?;
        Ok(())
    });
    if code != 0 || failed {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge() {
        assert_eq!(ranges("4,2-3,3-6,-1").unwrap(), [(1, 6)]);
    }
    #[test]
    fn open() {
        assert_eq!(ranges("3-").unwrap(), [(3, usize::MAX)]);
    }
    #[test]
    fn invalid() {
        for s in ["", "0", "3-1", "x", "1,,2"] {
            assert!(ranges(s).is_err());
        }
    }
    #[test]
    fn bytes() {
        assert_eq!(
            select(
                b"abcdef",
                &[(1, 2), (5, 6)],
                false,
                9,
                false,
                false,
                Some(b":")
            ),
            Some(b"ab:ef".to_vec())
        );
    }
    #[test]
    fn complement() {
        assert_eq!(
            select(b"abcd", &[(2, 3)], false, 9, true, false, None),
            Some(b"ad".to_vec())
        );
    }
    #[test]
    fn fields() {
        assert_eq!(
            select(b"a::c", &[(2, 3)], true, b':', false, false, None),
            Some(b":c".to_vec())
        );
    }
    #[test]
    fn undelimited() {
        assert!(select(b"a", &[(1, 1)], true, 9, false, true, None).is_none());
    }
}
