//! Decimal field conversion and human-readable unit scaling.
use crate::common::{
    args::{self, Item, OptSpec},
    error::usage,
    io::output,
};
use std::io::{self, Write};
mod binary;
mod number;
use number::{Format, Round, Scale};
const HELP:&str="Usage: numfmt [OPTION]... [NUMBER]...\nConvert numbers or selected fields read from standard input.\n      --debug              accept diagnostic debugging\n  -d, --delimiter=X        field separator (default whitespace)\n      --field=FIELDS       cut-style field ranges (default 1)\n      --format=FORMAT      one printf floating-point %f conversion\n      --from=UNIT          none, auto, si, iec, iec-i\n      --from-unit=N        input unit size\n      --grouping           locale grouping (no effect in C locale)\n      --header[=N]         copy N header lines (default 1)\n      --invalid=MODE       abort, fail, warn, ignore\n      --padding=N          align to signed width N\n      --round=METHOD       up, down, from-zero, towards-zero, nearest\n      --suffix=SUFFIX      accept and append SUFFIX\n      --to=UNIT            none, si, iec, iec-i\n      --to-unit=N          output unit size\n  -z, --zero-terminated    use NUL record separators\n      --help              display this help\n      --version           output version information\nExit status: 0 on success, 1 on usage errors, 2 on conversion errors.\n";
#[derive(Clone, Copy, PartialEq)]
enum Invalid {
    Abort,
    Fail,
    Warn,
    Ignore,
}
struct Options {
    from: Scale,
    to: Scale,
    from_unit: i128,
    to_unit: i128,
    round: Round,
    format: Format,
    suffix: String,
    padding: Option<i32>,
    fields: Vec<(usize, usize)>,
    delimiter: Option<u8>,
    header: usize,
    invalid: Invalid,
    zero: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            from: Scale::None,
            to: Scale::None,
            from_unit: 1,
            to_unit: 1,
            round: Round::Away,
            format: Format::default(),
            suffix: String::new(),
            padding: None,
            fields: vec![(1, 1)],
            delimiter: None,
            header: 0,
            invalid: Invalid::Abort,
            zero: false,
        }
    }
}
fn fields(text: &str) -> Result<Vec<(usize, usize)>, String> {
    text.split(',')
        .map(|part| {
            let parse = |s: &str| {
                s.parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("invalid field value '{text}'"))
            };
            let (start, end) = if let Some((a, b)) = part.split_once('-') {
                (
                    if a.is_empty() { 1 } else { parse(a)? },
                    if b.is_empty() { usize::MAX } else { parse(b)? },
                )
            } else {
                let n = parse(part)?;
                (n, n)
            };
            if start > end {
                return Err(format!("invalid decreasing range '{part}'"));
            }
            Ok((start, end))
        })
        .collect()
}
fn parse(arguments: &[String]) -> Result<(Options, Vec<String>), i32> {
    let specs = [
        OptSpec::flag("debug", '\0'),
        OptSpec::value("delimiter", 'd'),
        OptSpec::value("field", '\0'),
        OptSpec::value("format", '\0'),
        OptSpec::value("from", '\0'),
        OptSpec::value("from-unit", '\0'),
        OptSpec::flag("grouping", '\0'),
        OptSpec::optional("header", '\0'),
        OptSpec::value("invalid", '\0'),
        OptSpec::value("padding", '\0'),
        OptSpec::value("round", '\0'),
        OptSpec::value("suffix", '\0'),
        OptSpec::value("to", '\0'),
        OptSpec::value("to-unit", '\0'),
        OptSpec::flag("zero-terminated", 'z'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = args::parse(arguments, &specs, true).map_err(|e| usage("numfmt", e, 1))?;
    let mut o = Options::default();
    let mut values = Vec::new();
    for item in items {
        if let Item::Opt(id @ ("help" | "version"), _) = &item {
            return Err(args::help_version("numfmt", &[format!("--{id}")], HELP, true).unwrap());
        }
        let result: Result<(), String> = (|| {
            match item {
                Item::Opt("from", Some(v)) => o.from = Scale::parse(&v, true)?,
                Item::Opt("to", Some(v)) => o.to = Scale::parse(&v, false)?,
                Item::Opt("from-unit", Some(v)) => o.from_unit = number::unit(&v)?,
                Item::Opt("to-unit", Some(v)) => o.to_unit = number::unit(&v)?,
                Item::Opt("round", Some(v)) => o.round = Round::parse(&v)?,
                Item::Opt("format", Some(v)) => o.format = Format::parse(&v)?,
                Item::Opt("field", Some(v)) => o.fields = fields(&v)?,
                Item::Opt("delimiter", Some(v)) => {
                    if v.len() != 1 {
                        return Err("the delimiter must be a single character".into());
                    }
                    o.delimiter = Some(v.as_bytes()[0]);
                }
                Item::Opt("padding", Some(v)) => {
                    o.padding = Some(
                        v.parse()
                            .map_err(|_| format!("invalid padding value '{v}'"))?,
                    )
                }
                Item::Opt("suffix", Some(v)) => o.suffix = v,
                Item::Opt("header", v) => {
                    o.header = v
                        .as_deref()
                        .unwrap_or("1")
                        .parse()
                        .map_err(|_| "invalid header value")?
                }
                Item::Opt("invalid", Some(v)) => {
                    o.invalid = match v.as_str() {
                        "abort" => Invalid::Abort,
                        "fail" => Invalid::Fail,
                        "warn" => Invalid::Warn,
                        "ignore" => Invalid::Ignore,
                        _ => return Err(format!("invalid argument '{v}' for 'invalid'")),
                    }
                }
                Item::Opt("zero-terminated", _) => o.zero = true,
                Item::Opt("debug" | "grouping", _) => {}
                Item::Operand(v) => values.push(v),
                _ => unreachable!(),
            };
            Ok(())
        })();
        if let Err(e) = result {
            return Err(usage("numfmt", e, 1));
        }
    }
    Ok((o, values))
}
fn convert(bytes: &[u8], o: &Options) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| format!("invalid number: '{}'", String::from_utf8_lossy(bytes)))?;
    let leading = text.len()
        - text
            .trim_start_matches(|c: char| c.is_ascii_whitespace())
            .len();
    let number = text[leading..]
        .strip_suffix(&o.suffix)
        .unwrap_or(&text[leading..]);
    let parsed = number::Number::parse(number, o.from)?;
    let rendered = parsed.render(o.from_unit, o.to_unit, o.to, o.round, &o.format)?;
    let mut result = format!(
        "{}{}{}{}",
        o.format.prefix, rendered, o.suffix, o.format.suffix
    );
    let padding = o
        .padding
        .unwrap_or(if leading > 0 { bytes.len() as i32 } else { 0 });
    let count = (padding.unsigned_abs() as usize).saturating_sub(result.len());
    if padding < 0 {
        result.push_str(&" ".repeat(count))
    } else if count > 0 {
        result = format!("{}{result}", " ".repeat(count))
    }
    Ok(result.into_bytes())
}
fn field(
    bytes: &[u8],
    number: usize,
    o: &Options,
    out: &mut dyn Write,
    failed: &mut bool,
) -> io::Result<bool> {
    if !o.fields.iter().any(|(a, b)| (*a..=*b).contains(&number)) {
        out.write_all(bytes)?;
        return Ok(true);
    }
    match convert(bytes, o) {
        Ok(value) => out.write_all(&value)?,
        Err(error) => {
            if o.invalid != Invalid::Ignore {
                eprintln!("numfmt: {error}")
            }
            if matches!(o.invalid, Invalid::Abort | Invalid::Fail) {
                *failed = true
            }
            if o.invalid == Invalid::Abort {
                return Ok(false);
            }
            out.write_all(bytes)?;
        }
    }
    Ok(true)
}
fn record(line: &[u8], o: &Options, out: &mut dyn Write, failed: &mut bool) -> io::Result<bool> {
    if let Some(separator) = o.delimiter {
        for (index, bytes) in line.split(|b| *b == separator).enumerate() {
            if index > 0 {
                out.write_all(&[separator])?
            }
            if !field(bytes, index + 1, o, out, failed)? {
                return Ok(false);
            }
        }
    } else {
        let (mut cursor, mut number) = (0, 0);
        loop {
            let start = cursor;
            while cursor < line.len() && line[cursor].is_ascii_whitespace() {
                cursor += 1
            }
            while cursor < line.len() && !line[cursor].is_ascii_whitespace() {
                cursor += 1
            }
            number += 1;
            if !field(&line[start..cursor], number, o, out, failed)? {
                return Ok(false);
            }
            if cursor == line.len() {
                break;
            }
            out.write_all(&line[cursor..cursor + 1])?;
            cursor += 1;
            if line[cursor..].iter().all(u8::is_ascii_whitespace) {
                out.write_all(&line[cursor..])?;
                break;
            }
        }
    }
    out.write_all(&[if o.zero { 0 } else { b'\n' }])?;
    Ok(true)
}
pub fn run(arguments: &[String]) -> i32 {
    let (o, values) = match parse(arguments) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let mut failed = false;
    let mut line = 0;
    let status = output("numfmt", |out| {
        let mut handle = |bytes: &[u8]| -> io::Result<bool> {
            line += 1;
            if line <= o.header {
                out.write_all(bytes)?;
                out.write_all(&[if o.zero { 0 } else { b'\n' }])?;
                Ok(true)
            } else {
                record(bytes, &o, out, &mut failed)
            }
        };
        if values.is_empty() {
            let mut reader = crate::common::io::input("-")?;
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                if reader.read_until(if o.zero { 0 } else { b'\n' }, &mut bytes)? == 0 {
                    break;
                }
                if bytes.last() == Some(&if o.zero { 0 } else { b'\n' }) {
                    bytes.pop();
                }
                if !handle(&bytes)? {
                    break;
                }
            }
        } else {
            for value in &values {
                if !handle(value.as_bytes())? {
                    break;
                }
            }
        }
        Ok(())
    });
    if status != 0 {
        status
    } else if failed {
        2
    } else {
        0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges() {
        assert_eq!(
            fields("1,3-5,8-").unwrap(),
            vec![(1, 1), (3, 5), (8, usize::MAX)]
        );
    }
    #[test]
    fn invalid_range() {
        assert!(fields("3-1").is_err());
    }
    #[test]
    fn padding() {
        let o = Options {
            padding: Some(-4),
            ..Default::default()
        };
        assert_eq!(convert(b"2", &o).unwrap(), b"2   ");
    }
}
