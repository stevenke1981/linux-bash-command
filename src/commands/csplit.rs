use crate::common::{
    args::{self, Item, OptSpec},
    io,
    regex::bre_to_regex,
};
use regex::bytes::Regex;
mod data;

const HELP: &str = "Usage: csplit [OPTION]... FILE PATTERN...\nSplit FILE at line numbers or regular expression matches.\n  -b, --suffix-format=FORMAT  use printf-style integer suffix\n  -f, --prefix=PREFIX         use PREFIX instead of xx\n  -k, --keep-files            keep files on errors\n  -n, --digits=DIGITS         suffix width\n  -s, -q, --quiet, --silent   do not print byte counts\n  -z, --elide-empty-files     remove empty output files\n      --suppress-matched      omit matched lines\n      --help                  display this help\n      --version               output version information\n";

enum Pattern {
    Line(usize),
    Regex {
        regex: Matcher,
        offset: i64,
        discard: bool,
    },
}

enum Matcher {
    Fast(Regex),
    Backref(crate::common::bre::Matcher),
}
impl Matcher {
    fn is_match(&self, input: &[u8]) -> bool {
        match self {
            Self::Fast(regex) => regex.is_match(input),
            Self::Backref(regex) => regex.is_match(input),
        }
    }
}
fn has_backreference(pattern: &str) -> bool {
    let mut bytes = pattern.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            if let Some(next) = bytes.next() {
                if (b'1'..=b'9').contains(&next) {
                    return true;
                }
            }
        }
    }
    false
}

fn parse_pattern(s: &str) -> Result<Pattern, String> {
    if let Ok(n) = s.parse::<usize>() {
        return if n == 0 {
            Err(format!("{s}: line number must be greater than zero"))
        } else {
            Ok(Pattern::Line(n))
        };
    }
    let delimiter = s
        .as_bytes()
        .first()
        .copied()
        .ok_or_else(|| "empty pattern".to_string())?;
    if delimiter != b'/' && delimiter != b'%' {
        return Err(format!("{s}: invalid pattern"));
    }
    let bytes = s.as_bytes();
    let mut escaped = false;
    let mut end = None;
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        if !escaped && b == delimiter {
            end = Some(i);
            break;
        }
        if b == b'\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }
    let end = end.ok_or_else(|| format!("{s}: missing closing delimiter"))?;
    let expression = &s[1..end];
    let expression = expression.replace(
        if delimiter == b'/' { "\\/" } else { "\\%" },
        if delimiter == b'/' { "/" } else { "%" },
    );
    let regex = if has_backreference(&expression) {
        Matcher::Backref(crate::common::bre::Matcher::compile(expression.as_bytes())?)
    } else {
        Matcher::Fast(
            regex::bytes::RegexBuilder::new(&bre_to_regex(&expression)?)
                .unicode(false)
                .build()
                .map_err(|e| e.to_string())?,
        )
    };
    let offset = if end + 1 == s.len() {
        0
    } else {
        s[end + 1..]
            .parse::<i64>()
            .map_err(|_| format!("{s}: invalid offset"))?
    };
    Ok(Pattern::Regex {
        regex,
        offset,
        discard: delimiter == b'%',
    })
}

fn suffix(format: Option<&str>, index: usize, digits: usize) -> Result<String, String> {
    let Some(format) = format else {
        if index.to_string().len() > digits {
            return Err("output file suffixes exhausted".into());
        }
        return Ok(format!("{index:0digits$}"));
    };
    let bytes = format.as_bytes();
    let mut result = String::new();
    let mut cursor = 0;
    let mut converted = false;
    while cursor < bytes.len() {
        if bytes[cursor] != b'%' {
            let ch = format[cursor..].chars().next().unwrap();
            result.push(ch);
            cursor += ch.len_utf8();
            continue;
        }
        cursor += 1;
        if bytes.get(cursor) == Some(&b'%') {
            result.push('%');
            cursor += 1;
            continue;
        }
        if converted {
            return Err("invalid suffix format".into());
        }
        converted = true;
        let start = cursor;
        while bytes.get(cursor).is_some_and(|b| b"-#0".contains(b)) {
            cursor += 1;
        }
        let flags = &format[start..cursor];
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        let width = if cursor == start {
            0
        } else {
            format[start..cursor]
                .parse::<usize>()
                .map_err(|_| "invalid suffix format")?
        };
        let precision = if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            let start = cursor;
            while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                cursor += 1;
            }
            Some(if cursor == start {
                0
            } else {
                format[start..cursor]
                    .parse::<usize>()
                    .map_err(|_| "invalid suffix format")?
            })
        } else {
            None
        };
        let Some(&conversion) = bytes.get(cursor) else {
            return Err("invalid suffix format".into());
        };
        cursor += 1;
        let mut val = match conversion {
            b'o' => format!("{index:o}"),
            b'x' => format!("{index:x}"),
            b'X' => format!("{index:X}"),
            b'd' | b'i' | b'u' => index.to_string(),
            _ => return Err("invalid suffix format".into()),
        };
        if precision == Some(0) && index == 0 {
            val.clear();
        }
        if let Some(n) = precision {
            val = format!("{}{val}", "0".repeat(n.saturating_sub(val.len())));
        }
        let prefix = if flags.contains('#') && conversion == b'o' && !val.starts_with('0') {
            "0"
        } else if flags.contains('#') && index != 0 && conversion == b'x' {
            "0x"
        } else if flags.contains('#') && index != 0 && conversion == b'X' {
            "0X"
        } else {
            ""
        };
        let padding = width.saturating_sub(prefix.len() + val.len());
        if flags.contains('-') {
            result.push_str(prefix);
            result.push_str(&val);
            result.push_str(&" ".repeat(padding));
        } else if flags.contains('0') && precision.is_none() {
            result.push_str(prefix);
            result.push_str(&"0".repeat(padding));
            result.push_str(&val);
        } else {
            result.push_str(&" ".repeat(padding));
            result.push_str(prefix);
            result.push_str(&val);
        }
    }
    if converted {
        Ok(result)
    } else {
        Err("invalid suffix format".into())
    }
}

pub fn run(arguments: &[String]) -> i32 {
    let specs = [
        OptSpec::value("suffix-format", 'b'),
        OptSpec::value("prefix", 'f'),
        OptSpec::flag("keep-files", 'k'),
        OptSpec::value("digits", 'n'),
        OptSpec::flag("quiet", 's'),
        OptSpec::flag("silent", 'q'),
        OptSpec::flag("elide-empty-files", 'z'),
        OptSpec::flag("suppress-matched", '\0'),
    ];
    let items = match args::parse_command("csplit", arguments, HELP, &specs, true) {
        Ok(x) => x,
        Err(x) => return x,
    };
    let mut prefix = "xx".to_string();
    let mut digits = 2;
    let mut format = None;
    let mut keep = false;
    let mut quiet = false;
    let mut elide = false;
    let mut suppress = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt("prefix", Some(x)) => prefix = x,
            Item::Opt("suffix-format", Some(x)) => format = Some(x),
            Item::Opt("digits", Some(x)) => match x.parse::<usize>() {
                Ok(n) if n > 0 && n < 1000 => digits = n,
                _ => {
                    eprintln!("csplit: invalid number: '{x}'");
                    return 1;
                }
            },
            Item::Opt("keep-files", _) => keep = true,
            Item::Opt("quiet" | "silent", _) => quiet = true,
            Item::Opt("elide-empty-files", _) => elide = true,
            Item::Opt("suppress-matched", _) => suppress = true,
            Item::Operand(x) => operands.push(x),
            _ => {}
        }
    }
    if operands.len() < 2 {
        return crate::common::error::usage("csplit", "missing operand", 1);
    }
    if let Err(e) = suffix(format.as_deref(), 0, digits) {
        eprintln!("csplit: {e}");
        return 1;
    }
    let mut patterns: Vec<(Pattern, Option<usize>)> = Vec::new();
    for s in &operands[1..] {
        if s.starts_with('{') && s.ends_with('}') {
            let count = &s[1..s.len() - 1];
            let count = if count == "*" {
                Some(None)
            } else {
                count.parse::<usize>().ok().map(Some)
            };
            if let (Some((_, repeat)), Some(n)) = (patterns.last_mut(), count) {
                if repeat.is_some() {
                    eprintln!("csplit: '{s}': consecutive repeat counts are invalid");
                    return 1;
                }
                *repeat = n.or(Some(usize::MAX));
            } else {
                eprintln!("csplit: '{s}': invalid pattern");
                return 1;
            }
        } else {
            match parse_pattern(s) {
                Ok(p) => patterns.push((p, None)),
                Err(e) => {
                    eprintln!("csplit: {e}");
                    return 1;
                }
            }
        }
    }
    let input = match data::Data::open(&operands[0]) {
        Ok(input) => input,
        Err(e) => {
            io::report_open("csplit", &operands[0], &e);
            return 1;
        }
    };
    macro_rules! position {
        ($line:expr) => {
            match input.offset($line) {
                Ok(offset) => offset,
                Err(e) => {
                    eprintln!("csplit: {e}");
                    return 1;
                }
            }
        };
    }
    let mut chunks: Vec<(u64, u64)> = Vec::new();
    let mut start = 0usize;
    let mut last_match = None;
    let mut failure = None;
    let mut failure_discards = false;
    let mut exhausted = false;
    for (pattern, repeat) in patterns {
        let count = repeat.unwrap_or(0);
        for iteration in 0..=count {
            let (target, discard, matched) = match &pattern {
                Pattern::Line(n) => {
                    let line = n.checked_mul(iteration + 1).and_then(|x| x.checked_sub(1));
                    match line {
                        Some(t) if t < input.lines && t >= start => (t, false, None),
                        _ => {
                            if count != usize::MAX {
                                failure = Some("line number out of range".to_string());
                            } else {
                                exhausted = true;
                            }
                            break;
                        }
                    }
                }
                Pattern::Regex {
                    regex,
                    offset,
                    discard,
                } => {
                    let search_start =
                        last_match.map_or(start, |m: usize| start.max(m.saturating_add(1)));
                    let found = match input.find(|line| regex.is_match(line), search_start) {
                        Ok(found) => found,
                        Err(e) => {
                            eprintln!("csplit: {e}");
                            return 1;
                        }
                    };
                    let Some(found) = found else {
                        if count != usize::MAX {
                            failure = Some("match not found".to_string());
                            failure_discards = *discard;
                        } else {
                            exhausted = true;
                        }
                        break;
                    };
                    let target = (found as i128) + (*offset as i128);
                    if target < start as i128 || target > input.lines as i128 {
                        failure = Some("line number out of range".to_string());
                        if !discard {
                            let boundary =
                                target.clamp(start as i128, input.lines as i128) as usize;
                            chunks.push((position!(start), position!(boundary)));
                        }
                        failure_discards = true;
                        break;
                    }
                    (target as usize, *discard, Some(found))
                }
            };
            if !discard {
                chunks.push((position!(start), position!(target)));
            }
            start = target;
            if suppress && !discard {
                start = (start + 1).min(input.lines);
            }
            last_match = matched;
        }
        if failure.is_some() || exhausted {
            break;
        }
    }
    if failure.is_none() || !failure_discards {
        chunks.push((position!(start), position!(input.lines)));
    }
    if let Some(e) = &failure {
        eprintln!("csplit: {e}");
        if !keep {
            if !quiet {
                let _ = io::output("csplit", |out| {
                    for &(start, end) in &chunks {
                        if !elide || start != end {
                            writeln!(out, "{}", end - start)?;
                        }
                    }
                    Ok(())
                });
            }
            return 1;
        }
    }
    let mut counts = Vec::new();
    let mut created = Vec::new();
    for (index, (start, end)) in chunks
        .into_iter()
        .filter(|(start, end)| !elide || start != end)
        .enumerate()
    {
        let filename = match suffix(format.as_deref(), index, digits) {
            Ok(suffix) => format!("{prefix}{suffix}"),
            Err(e) => {
                eprintln!("csplit: {e}");
                if !keep {
                    for path in created {
                        let _ = std::fs::remove_file(path);
                    }
                }
                return 1;
            }
        };
        if operands[0] != "-"
            && operands[0] != "/dev/stdin"
            && crate::common::meta::same_file(
                std::path::Path::new(&operands[0]),
                std::path::Path::new(&filename),
            )
            .unwrap_or(false)
        {
            eprintln!("csplit: '{filename}' would overwrite input");
            if !keep {
                for path in created {
                    let _ = std::fs::remove_file(path);
                }
            }
            return 1;
        }
        let existed = std::fs::symlink_metadata(&filename).is_ok();
        if !existed {
            created.push(filename.clone());
        }
        match std::fs::File::create(&filename)
            .and_then(|mut f| input.copy_range(start, end, &mut f))
        {
            Ok(()) => counts.push(end - start),
            Err(e) => {
                eprintln!("csplit: '{filename}': {e}");
                if !keep {
                    for path in created {
                        let _ = std::fs::remove_file(path);
                    }
                }
                return 1;
            }
        }
    }
    if !quiet {
        let status = io::output("csplit", |out| {
            for count in counts {
                writeln!(out, "{count}")?;
            }
            Ok(())
        });
        if status != 0 {
            return status;
        }
    }
    if failure.is_some() {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers() {
        assert!(matches!(parse_pattern("2"), Ok(Pattern::Line(2))));
        assert!(parse_pattern("0").is_err());
    }
    #[test]
    fn expressions() {
        assert!(matches!(
            parse_pattern("/abc/+1"),
            Ok(Pattern::Regex { offset: 1, .. })
        ));
        assert!(parse_pattern("/abc").is_err());
    }
    #[test]
    fn formats() {
        assert_eq!(suffix(None, 3, 2).unwrap(), "03");
        assert_eq!(suffix(Some("p%03x.s"), 15, 2).unwrap(), "p00f.s");
        assert_eq!(suffix(Some("%#04x"), 1, 2).unwrap(), "0x01");
        assert_eq!(suffix(Some("%.3d"), 1, 2).unwrap(), "001");
        assert_eq!(suffix(Some("%-3d"), 1, 2).unwrap(), "1  ");
        assert_eq!(suffix(Some("x%%-%02d"), 1, 2).unwrap(), "x%-01");
        assert!(suffix(Some("x"), 0, 2).is_err());
    }
}
