//! `od`: dump files in octal and other formats, following GNU `od` output layout.
//!
//! Formats come from `-t` or the traditional letters (`-b -c -d -o -x -s -i -l -f -a`).
//! Each format prints its own line group per input line. Identical lines collapse to a
//! single `*` unless `-v` is given. Floats print in the shortest `%g` form that round-trips.
//!
//! Submodules: `parse` (type strings, counts, offsets), `render` (item text).

mod parse;
mod render;

use parse::{letter_format, parse_count, parse_formats, parse_offset, parse_radix};
use render::{column_widths, format_items};

use std::io::{self, BufRead, Read, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};

const CMD: &str = "od";
const DEFAULT_WIDTH: usize = 16;
const MAX_ITEM: usize = 16;

const SPECS: &[OptSpec] = &[
    OptSpec::req("radix", Some('A'), Some("address-radix")),
    OptSpec::req("skip", Some('j'), Some("skip-bytes")),
    OptSpec::req("count", Some('N'), Some("read-bytes")),
    OptSpec::req("strings", Some('S'), Some("strings")),
    OptSpec::req("format", Some('t'), Some("format")),
    OptSpec::flag("verbose", Some('v'), Some("output-duplicates")),
    OptSpec::opt("width", Some('w'), Some("width")),
    OptSpec::req("endian", None, Some("endian")),
    OptSpec::flag("traditional", None, Some("traditional")),
    OptSpec::flag("t_a", Some('a'), None),
    OptSpec::flag("t_b", Some('b'), None),
    OptSpec::flag("t_c", Some('c'), None),
    OptSpec::flag("t_d", Some('d'), None),
    OptSpec::flag("t_f", Some('f'), None),
    OptSpec::flag("t_i", Some('i'), None),
    OptSpec::flag("t_l", Some('l'), None),
    OptSpec::flag("t_o", Some('o'), None),
    OptSpec::flag("t_s", Some('s'), None),
    OptSpec::flag("t_x", Some('x'), None),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = r"Usage: od [OPTION]... [FILE]...
  or:  od [-abcdfilosx]... [FILE] [[+]OFFSET[.][b]]
  or:  od --traditional [OPTION]... [FILE] [[+]OFFSET[.][b] [+][LABEL][.][b]]
Write an unambiguous representation, octal bytes by default,
of FILE to standard output.  With more than one FILE argument,
concatenate them in the listed order to form the input.

With no FILE, or when FILE is -, read standard input.

  -A, --address-radix=RADIX   output format for file offsets; RADIX is one
                                of [doxn], for decimal, octal, hex or none
  -j, --skip-bytes=BYTES      skip BYTES input bytes first
  -N, --read-bytes=BYTES      limit dump to BYTES input bytes
  -S, --strings[=BYTES]       output strings of at least BYTES graphic chars
  -t, --format=TYPE           select output format or formats
  -v, --output-duplicates     do not use * to mark line suppression
  -w[BYTES], --width[=BYTES]  output BYTES bytes per output line; 32 is implied
                                when BYTES is not specified, 16 when omitted
      --endian=TYPE          swap input bytes according to the TYPE argument,
                                TYPE is big or little
      --traditional          accept arguments in traditional form
  -a                          same as -t a,  select named characters
  -b                          same as -t o1, select octal bytes
  -c                          same as -t c,  select ASCII characters or backslash escapes
  -d                          same as -t u2, select unsigned decimal 2-byte units
  -f                          same as -t fF, select floats
  -i                          same as -t dI, select decimal ints
  -l                          same as -t dL, select decimal longs
  -o                          same as -t o2, select octal 2-byte units
  -s                          same as -t d2, select decimal 2-byte units
  -x                          same as -t x2, select hexadecimal 2-byte units
      --help                  display this help and exit
      --version               output version information and exit

Traditional format: TYPE is a sequence of a, c, d[SIZE], f[SIZE], o[SIZE], u[SIZE],
x[SIZE], each optionally followed by z.  SIZE is a byte count or C, S, I, L (ints),
F, D, L (floats).  Note that long double (fL, f16) is printed through f64.
";

/// Output address radix. `None` omits the offset column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Radix {
    Octal,
    Decimal,
    Hex,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Octal,
    Hex,
    Unsigned,
    Signed,
    Float,
    Char,
    Named,
}

/// One output format: how items are read and printed, and whether the `z` ASCII column follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Format {
    kind: Kind,
    size: usize,
    ascii: bool,
}

struct Config {
    radix: Radix,
    formats: Vec<Format>,
    width: usize,
    verbose: bool,
    big_endian: bool,
    skip: u64,
    limit: Option<u64>,
    strings: Option<usize>,
}

pub fn run(args: &[String]) -> i32 {
    let args = bare_strings_default(args);
    let items = match Parser::new(CMD, SPECS).parse(&args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    for item in &items {
        if let Item::Opt { id, .. } = item {
            match *id {
                "help" => {
                    print!("{HELP}");
                    return EXIT_OK;
                }
                "version" => return print_version(CMD),
                _ => {}
            }
        }
    }
    let config = match Config::from_items(&items) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let (files, offset) = match split_operands(&items) {
        Ok(split) => split,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let mut config = config;
    config.skip += offset;
    let mut source = Source::new(files);
    let mut out = stdout_writer();
    let status = match dump(&config, &mut source, &mut out) {
        Ok(code) => code,
        Err(error) if is_broken_pipe(&error) => return EXIT_OK,
        Err(error) => {
            eprintln!("{CMD}: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
    };
    match finish_output(CMD, &mut out) {
        EXIT_OK => status,
        code => code,
    }
}

/// `--strings` without a value means 3; the shared parser would otherwise consume the next
/// argument. Arguments after `--` are left alone.
fn bare_strings_default(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if arg == "--" {
            out.push(arg.clone());
            out.extend(args[index..].iter().cloned());
            break;
        }
        if arg == "--strings" {
            out.push("--strings=3".to_owned());
        } else {
            out.push(arg.clone());
        }
    }
    out
}

impl Config {
    fn from_items(items: &[Item]) -> Result<Self, String> {
        let mut config = Config {
            radix: Radix::Octal,
            formats: Vec::new(),
            width: DEFAULT_WIDTH,
            verbose: false,
            big_endian: false,
            skip: 0,
            limit: None,
            strings: None,
        };
        for item in items {
            let Item::Opt { id, value } = item else {
                continue;
            };
            let value = value.as_deref().unwrap_or("");
            match *id {
                "radix" => config.radix = parse_radix(value)?,
                "skip" => {
                    config.skip = parse_count(value)
                        .ok_or_else(|| format!("invalid -j argument '{value}'"))?;
                }
                "count" => {
                    config.limit = Some(
                        parse_count(value)
                            .ok_or_else(|| format!("invalid -N argument '{value}'"))?,
                    );
                }
                "strings" => {
                    let min = parse_count(value)
                        .ok_or_else(|| format!("invalid -S argument '{value}'"))?;
                    config.strings = Some(usize::try_from(min).unwrap_or(usize::MAX).max(1));
                }
                "format" => config.formats.extend(parse_formats(value)?),
                "verbose" => config.verbose = true,
                "width" => {
                    config.width = if value.is_empty() {
                        32
                    } else {
                        parse_count(value)
                            .and_then(|n| usize::try_from(n).ok())
                            .filter(|n| *n > 0)
                            .ok_or_else(|| format!("invalid line size '{value}'"))?
                    };
                }
                "endian" => match value {
                    "big" => config.big_endian = true,
                    "little" => config.big_endian = false,
                    _ => {
                        return Err(format!(
                            "invalid argument '{value}' for '--endian'\n\
                             Valid arguments are:\n  - 'little'\n  - 'big'\n\
                             Try '{CMD} --help' for more information."
                        ))
                    }
                },
                "traditional" => {}
                other => {
                    if let Some(letter) = other.strip_prefix("t_") {
                        let spec = letter_format(letter);
                        config.formats.push(spec);
                    }
                }
            }
        }
        if config.formats.is_empty() {
            config.formats.push(Format {
                kind: Kind::Octal,
                size: 2,
                ascii: false,
            });
        }
        if config.strings.is_some() && has_explicit_format(items) {
            return Err("no type may be specified when dumping strings".to_owned());
        }
        config.width = fit_width(config.width, &config.formats)?;
        Ok(config)
    }
}

fn has_explicit_format(items: &[Item]) -> bool {
    items.iter().any(|item| match item {
        Item::Opt { id, .. } => *id == "format" || id.starts_with("t_"),
        Item::Operand(_) => false,
    })
}

/// Rounds the line width down to a multiple of every item size, as GNU does, warning when
/// it changes. A width smaller than that multiple becomes the multiple itself.
fn fit_width(width: usize, formats: &[Format]) -> Result<usize, String> {
    let unit = formats.iter().map(|f| f.size).fold(1, lcm);
    if width.is_multiple_of(unit) {
        return Ok(width);
    }
    let fitted = (width / unit * unit).max(unit);
    eprintln!("{CMD}: warning: invalid width {width}; using {fitted} instead");
    Ok(fitted)
}

fn lcm(a: usize, b: usize) -> usize {
    fn gcd(a: usize, b: usize) -> usize {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    a / gcd(a, b) * b
}

/// The format selected by a traditional single-letter option (`-b`, `-x`, ...).
fn split_operands(items: &[Item]) -> Result<(Vec<String>, u64), String> {
    let operands: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(text) => Some(text.clone()),
            Item::Opt { .. } => None,
        })
        .collect();
    let traditional_flag = items
        .iter()
        .any(|item| matches!(item, Item::Opt { id, .. } if *id == "traditional"));
    let gnu_only = items.iter().any(|item| match item {
        Item::Opt { id, .. } => !is_traditional_id(id),
        Item::Operand(_) => false,
    });
    let looks_like_offset = match operands.as_slice() {
        [_, second, ..] => {
            second.starts_with('+')
                || (operands.len() == 2 && second.starts_with(|c: char| c.is_ascii_digit()))
        }
        _ => false,
    };
    let traditional = traditional_flag || (!gnu_only && looks_like_offset);
    if !traditional || operands.len() < 2 {
        let files = if operands.is_empty() {
            vec!["-".to_owned()]
        } else {
            operands
        };
        return Ok((files, 0));
    }
    let offset = parse_offset(&operands[1])
        .ok_or_else(|| format!("invalid offset operand '{}'", operands[1]))?;
    Ok((vec![operands[0].clone()], offset))
}

fn is_traditional_id(id: &str) -> bool {
    id.starts_with("t_") || matches!(id, "traditional" | "verbose" | "help" | "version")
}

/// Concatenated input from the operand list. Files open lazily, and a file that cannot be
/// read is reported and skipped, as GNU does.
struct Source {
    names: Vec<String>,
    next: usize,
    current: Option<Box<dyn BufRead>>,
    failed: bool,
}

impl Source {
    fn new(names: Vec<String>) -> Self {
        Self {
            names,
            next: 0,
            current: None,
            failed: false,
        }
    }

    /// Reads up to `buf.len()` bytes. Returns 0 only at the end of all input.
    fn read_some(&mut self, buf: &mut [u8]) -> usize {
        loop {
            if self.current.is_none() {
                let Some(name) = self.names.get(self.next).cloned() else {
                    return 0;
                };
                self.next += 1;
                match open_input(&name) {
                    Ok(reader) => self.current = Some(reader),
                    Err(error) => {
                        report(CMD, &name, &error);
                        self.failed = true;
                        continue;
                    }
                }
            }
            let Some(reader) = self.current.as_mut() else {
                continue;
            };
            match reader.read(buf) {
                Ok(0) => self.current = None,
                Ok(count) => return count,
                Err(error) => {
                    report(CMD, &self.names[self.next - 1], &error);
                    self.failed = true;
                    self.current = None;
                }
            }
        }
    }

    /// Fills `buf` unless the input ends first. Returns the number of bytes read.
    fn fill(&mut self, buf: &mut [u8]) -> usize {
        let mut total = 0;
        while total < buf.len() {
            let count = self.read_some(&mut buf[total..]);
            if count == 0 {
                break;
            }
            total += count;
        }
        total
    }

    /// Discards `count` bytes. Returns false when the input ends first.
    fn skip(&mut self, mut count: u64) -> bool {
        let mut scratch = [0u8; 8192];
        while count > 0 {
            let want = scratch
                .len()
                .min(usize::try_from(count).unwrap_or(usize::MAX));
            let got = self.read_some(&mut scratch[..want]);
            if got == 0 {
                return false;
            }
            count -= got as u64;
        }
        true
    }
}

fn dump(config: &Config, source: &mut Source, out: &mut impl Write) -> io::Result<i32> {
    if !source.skip(config.skip) {
        eprintln!("{CMD}: cannot skip past end of combined input");
        return Ok(EXIT_FAIL);
    }
    if let Some(min) = config.strings {
        dump_strings(config, source, out, min)?;
    } else {
        dump_lines(config, source, out)?;
    }
    Ok(if source.failed { EXIT_FAIL } else { EXIT_OK })
}

fn dump_lines(config: &Config, source: &mut Source, out: &mut impl Write) -> io::Result<()> {
    let width = config.width;
    let widths = column_widths(&config.formats, width);
    let mut line = vec![0u8; width];
    let mut previous = vec![0u8; width];
    let mut have_previous = false;
    let mut starred = false;
    let mut offset = config.skip;
    let mut remaining = config.limit.unwrap_or(u64::MAX);
    loop {
        let want = usize::try_from(remaining.min(width as u64)).unwrap_or(width);
        if want == 0 {
            break;
        }
        let count = source.fill(&mut line[..want]);
        if count == 0 {
            break;
        }
        remaining -= count as u64;
        let bytes = &line[..count];
        let repeats = !config.verbose && count == width && have_previous && bytes == &previous[..];
        if repeats {
            if !starred {
                out.write_all(b"*\n")?;
                starred = true;
            }
        } else {
            starred = false;
            write_block(config, &widths, out, offset, bytes)?;
            previous[..count].copy_from_slice(bytes);
            have_previous = true;
        }
        offset += count as u64;
    }
    if config.radix != Radix::None {
        out.write_all(address(config.radix, offset).as_bytes())?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn write_block(
    config: &Config,
    widths: &[usize],
    out: &mut impl Write,
    offset: u64,
    bytes: &[u8],
) -> io::Result<()> {
    for (index, format) in config.formats.iter().enumerate() {
        let mut text = if index == 0 {
            address(config.radix, offset)
        } else {
            blank_address(config.radix)
        };
        format_items(
            &mut text,
            *format,
            widths[index],
            bytes,
            config.width,
            config.big_endian,
        );
        text.push('\n');
        out.write_all(text.as_bytes())?;
    }
    Ok(())
}

fn address(radix: Radix, offset: u64) -> String {
    match radix {
        Radix::Octal => format!("{offset:07o}"),
        Radix::Decimal => format!("{offset:07}"),
        Radix::Hex => format!("{offset:06x}"),
        Radix::None => String::new(),
    }
}

fn blank_address(radix: Radix) -> String {
    " ".repeat(address(radix, 0).len())
}

/// Appends the formatted items of one line. The `z` column pads a short final line to full
/// width before the quoted ASCII rendering.
fn dump_strings(
    config: &Config,
    source: &mut Source,
    out: &mut impl Write,
    min: usize,
) -> io::Result<()> {
    let mut run: Vec<u8> = Vec::new();
    let mut run_start = config.skip;
    let mut offset = config.skip;
    let mut remaining = config.limit.unwrap_or(u64::MAX);
    let mut chunk = vec![0u8; 8192];
    while remaining > 0 {
        let want = usize::try_from(remaining.min(chunk.len() as u64)).unwrap_or(chunk.len());
        let count = source.fill(&mut chunk[..want]);
        if count == 0 {
            break;
        }
        remaining -= count as u64;
        for &byte in &chunk[..count] {
            if byte == 0 {
                if run.len() >= min {
                    let prefix = address(config.radix, run_start);
                    let line = if prefix.is_empty() {
                        format!(" {}\n", String::from_utf8_lossy(&run))
                    } else {
                        format!("{prefix} {}\n", String::from_utf8_lossy(&run))
                    };
                    out.write_all(line.as_bytes())?;
                }
                run.clear();
            } else if (0x20..=0x7e).contains(&byte) {
                if run.is_empty() {
                    run_start = offset;
                }
                run.push(byte);
            } else {
                run.clear();
            }
            offset += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn fit_width_rounds_to_item_multiple() {
        let x4 = letter_format("l");
        assert_eq!(fit_width(5, &[x4]).unwrap(), 8);
        assert_eq!(
            fit_width(
                16,
                &[Format {
                    kind: Kind::Hex,
                    size: 2,
                    ascii: false
                }]
            )
            .unwrap(),
            16
        );
    }

    #[test]
    fn bare_strings_flag_gets_default_length() {
        let out = bare_strings_default(&args(&["--strings", "f"]));
        assert_eq!(out, args(&["--strings=3", "f"]));
        let kept = bare_strings_default(&args(&["--", "--strings"]));
        assert_eq!(kept, args(&["--", "--strings"]));
    }

    #[test]
    fn default_line_uses_octal_two_byte_units() {
        let format = Format {
            kind: Kind::Octal,
            size: 2,
            ascii: false,
        };
        let mut text = String::new();
        format_items(&mut text, format, 7, b"AB", 16, false);
        assert_eq!(text, " 041101");
    }
}
