//! `csplit`: split a file into sections determined by line numbers and regular expressions.
//!
//! The whole input is read first so that a regular expression can be searched forward and
//! a negative offset can place a split before the matching line. Each completed section is
//! written as `PREFIX` plus a `printf`-style suffix (default `%02d`) and its byte count is
//! printed unless `-s` is given.

use std::fs;
use std::io::{self, Write};

use regex::bytes::Regex;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{is_broken_pipe, open_input};
use crate::common::regex::bre_to_regex;

const CMD: &str = "csplit";

const SPECS: &[OptSpec] = &[
    OptSpec::req("suffix", Some('b'), Some("suffix-format")),
    OptSpec::req("prefix", Some('f'), Some("prefix")),
    OptSpec::flag("keep", Some('k'), Some("keep-files")),
    OptSpec::flag("suppress", None, Some("suppress-matched")),
    OptSpec::req("digits", Some('n'), Some("digits")),
    OptSpec::flag("quiet", Some('s'), Some("silent")),
    OptSpec::flag("quiet_q", Some('q'), Some("quiet")),
    OptSpec::flag("elide", Some('z'), Some("elide-empty-files")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = r"Usage: csplit [OPTION]... FILE PATTERN...
Output pieces of FILE separated by PATTERN(s) to files 'xx00', 'xx01', ..., and
output byte counts of each piece to standard output.

Mandatory arguments to long options are mandatory for short options too.
  -b, --suffix-format=FORMAT  use sprintf FORMAT instead of %02d
  -f, --prefix=PREFIX        use PREFIX instead of 'xx'
  -k, --keep-files           do not remove output files on errors
      --suppress-matched     suppress the lines matching PATTERN
  -n, --digits=DIGITS        use specified number of digits instead of 2
  -s, --quiet, --silent      do not print counts of output file sizes
  -z, --elide-empty-files    remove empty output files
      --help                 display this help and exit
      --version              output version information and exit

Read standard input if FILE is -. Each PATTERN may be:
  INTEGER            copy up to but not including specified line number
  /REGEXP/[OFFSET]   copy up to but not including a matching line
  %REGEXP%[OFFSET]   skip to, but not including a matching line
  {INTEGER}          repeat the previous pattern specified number of times
  {*}                repeat the previous pattern as many times as possible

A line OFFSET is a required '+' or '-' followed by a positive integer.
";

/// How many times a pattern runs: once, `{N}` extra times, or as often as it matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Repeat {
    Once,
    Extra(u64),
    Unbounded,
}

#[derive(Debug)]
enum Kind {
    Line(u64),
    Regex {
        regex: Regex,
        offset: i64,
        /// `%re%`: the section before the match is discarded instead of written.
        skip: bool,
    },
}

#[derive(Debug)]
struct Step {
    kind: Kind,
    repeat: Repeat,
    text: String,
}

/// The input split into lines: `starts[i]` is the byte offset of line `i + 1`.
struct Lines {
    data: Vec<u8>,
    starts: Vec<usize>,
}

impl Lines {
    fn new(data: Vec<u8>) -> Self {
        let mut starts = Vec::new();
        if !data.is_empty() {
            starts.push(0);
        }
        for (index, byte) in data.iter().enumerate() {
            if *byte == b'\n' && index + 1 < data.len() {
                starts.push(index + 1);
            }
        }
        Self { data, starts }
    }

    fn total(&self) -> usize {
        self.starts.len()
    }

    /// Byte offset where 1-based `line` starts; `total + 1` gives the end of the data.
    fn offset(&self, line: usize) -> usize {
        self.starts
            .get(line - 1)
            .copied()
            .unwrap_or(self.data.len())
    }

    fn line(&self, line: usize) -> &[u8] {
        let start = self.offset(line);
        let end = self.offset(line + 1);
        let text = &self.data[start..end];
        text.strip_suffix(b"\n").unwrap_or(text)
    }
}

/// Output settings shared by the splitting loop.
struct Output {
    prefix: String,
    suffix: SuffixFormat,
    quiet: bool,
    elide: bool,
    keep: bool,
    suppress: bool,
    created: Vec<String>,
    index: u64,
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
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
    let operands: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(text) => Some(text.clone()),
            Item::Opt { .. } => None,
        })
        .collect();
    let Some(file) = operands.first() else {
        return usage_error("missing operand");
    };
    if operands.len() < 2 {
        return usage_error(&format!("missing operand after '{file}'"));
    }
    let mut output = match build_output(&items) {
        Ok(output) => output,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let steps = match parse_patterns(&operands[1..]) {
        Ok(steps) => steps,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let data = match read_input(file) {
        Ok(data) => data,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let lines = Lines::new(data);
    let result = split(&lines, &steps, &mut output);
    match result {
        Ok(()) => EXIT_OK,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            if !output.keep {
                for name in &output.created {
                    let _ = fs::remove_file(name);
                }
            }
            EXIT_FAIL
        }
    }
}

fn usage_error(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

fn read_input(file: &str) -> Result<Vec<u8>, String> {
    let mut reader = open_input(file).map_err(|error| {
        format!(
            "cannot open '{file}' for reading: {}",
            io_error_text(&error)
        )
    })?;
    let mut data = Vec::new();
    io::Read::read_to_end(&mut reader, &mut data)
        .map_err(|error| format!("read error: {}", io_error_text(&error)))?;
    Ok(data)
}

fn build_output(items: &[Item]) -> Result<Output, String> {
    let mut suffix_format: Option<String> = None;
    let mut prefix = "xx".to_owned();
    let mut keep = false;
    let mut suppress = false;
    let mut digits = 2usize;
    let mut quiet = false;
    let mut elide = false;
    for item in items {
        let Item::Opt { id, value } = item else {
            continue;
        };
        let value = value.as_deref().unwrap_or("");
        match *id {
            "suffix" => suffix_format = Some(value.to_owned()),
            "prefix" => prefix = value.to_owned(),
            "keep" => keep = true,
            "suppress" => suppress = true,
            "digits" => {
                digits = value
                    .parse()
                    .map_err(|_| format!("invalid number: '{value}'"))?;
            }
            "quiet" | "quiet_q" => quiet = true,
            "elide" => elide = true,
            _ => {}
        }
    }
    let suffix = match suffix_format {
        Some(text) => SuffixFormat::parse(&text)?,
        None => SuffixFormat::parse(&format!("%0{digits}d"))?,
    };
    Ok(Output {
        prefix,
        suffix,
        quiet,
        elide,
        keep,
        suppress,
        created: Vec::new(),
        index: 0,
    })
}

/// Parses the PATTERN operands. A `{N}` or `{*}` operand repeats the pattern before it.
fn parse_patterns(args: &[String]) -> Result<Vec<Step>, String> {
    let mut steps: Vec<Step> = Vec::new();
    let mut last_line: Option<u64> = None;
    for arg in args {
        let arg = arg.as_str();
        if let Some(inner) = arg
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
        {
            let repeat = if inner == "*" {
                Repeat::Unbounded
            } else {
                match inner.parse::<u64>() {
                    Ok(count) => Repeat::Extra(count),
                    Err(_) => return Err(format!("'{arg}': invalid pattern")),
                }
            };
            let Some(previous) = steps.last_mut() else {
                return Err(format!("{arg}: no pattern to repeat"));
            };
            previous.repeat = repeat;
            continue;
        }
        if arg.bytes().all(|b| b.is_ascii_digit()) && !arg.is_empty() {
            let number: u64 = arg
                .parse()
                .map_err(|_| format!("'{arg}': line number out of range"))?;
            if number == 0 {
                return Err("0: line number must be greater than zero".to_owned());
            }
            if let Some(previous) = last_line {
                if number <= previous {
                    return Err(format!(
                        "line number '{number}' is smaller than preceding line number, {previous}"
                    ));
                }
            }
            last_line = Some(number);
            steps.push(Step {
                kind: Kind::Line(number),
                repeat: Repeat::Once,
                text: arg.to_owned(),
            });
            continue;
        }
        steps.push(parse_regex_step(arg)?);
    }
    Ok(steps)
}

/// `/REGEXP/[OFFSET]` or `%REGEXP%[OFFSET]`.
fn parse_regex_step(arg: &str) -> Result<Step, String> {
    let delimiter = match arg.chars().next() {
        Some('/') => '/',
        Some('%') => '%',
        _ => return Err(format!("'{arg}': invalid pattern")),
    };
    let body = &arg[1..];
    let Some(end) = find_closing(body, delimiter) else {
        return Err(format!("{arg}: closing delimiter '{delimiter}' missing"));
    };
    let pattern = body[..end].replace(&format!("\\{delimiter}"), &delimiter.to_string());
    let rest = &body[end + 1..];
    let offset = parse_offset(rest).ok_or_else(|| format!("'{arg}': invalid pattern"))?;
    let translated = bre_to_regex(&pattern).map_err(|error| format!("{arg}: {error}"))?;
    let regex = Regex::new(&translated).map_err(|error| format!("{arg}: {error}"))?;
    Ok(Step {
        kind: Kind::Regex {
            regex,
            offset,
            skip: delimiter == '%',
        },
        repeat: Repeat::Once,
        text: arg.to_owned(),
    })
}

/// Index of the first unescaped `delimiter` in `body`.
fn find_closing(body: &str, delimiter: char) -> Option<usize> {
    let mut escaped = false;
    for (index, ch) in body.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == delimiter {
            return Some(index);
        }
    }
    None
}

/// The optional `+N` or `-N` after a regular expression.
fn parse_offset(rest: &str) -> Option<i64> {
    if rest.is_empty() {
        return Some(0);
    }
    let (sign, digits) = match rest.as_bytes()[0] {
        b'+' => (1, &rest[1..]),
        b'-' => (-1, &rest[1..]),
        _ => return None,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(|value| sign * value)
}

/// Runs every step against the input, writing sections as they complete.
fn split(lines: &Lines, steps: &[Step], output: &mut Output) -> Result<(), String> {
    let total = lines.total();
    let mut start = 1usize;
    let mut last = 0usize;
    for step in steps {
        let mut repetition = 0u64;
        loop {
            let boundary = match &step.kind {
                Kind::Line(number) => {
                    let value = if repetition == 0 {
                        *number
                    } else {
                        (last as u64).saturating_add(*number)
                    };
                    if value as usize > total || value > usize::MAX as u64 {
                        if repetition == 0 {
                            flush_remainder(lines, start, output)?;
                            return Err(format!("'{}': line number out of range", step.text));
                        }
                        flush_remainder(lines, start, output)?;
                        return Err(format!(
                            "'{}': line number out of range on repetition {repetition}",
                            step.text
                        ));
                    }
                    value as usize
                }
                Kind::Regex { regex, offset, .. } => match find_match(lines, regex, last + 1) {
                    Some(found) => {
                        let target = found as i64 + offset;
                        usize::try_from(target.max(1)).unwrap_or(1)
                    }
                    None if step.repeat == Repeat::Unbounded && repetition > 0 => break,
                    None => {
                        flush_remainder(lines, start, output)?;
                        return Err(if repetition == 0 {
                            format!("'{}': match not found", step.text)
                        } else {
                            format!(
                                "'{}': match not found on repetition {repetition}",
                                step.text
                            )
                        });
                    }
                },
            };
            let skip = matches!(step.kind, Kind::Regex { skip: true, .. });
            let next = cut(lines, start, boundary, skip, output)?;
            start = next;
            last = boundary;
            repetition += 1;
            let more = match step.repeat {
                Repeat::Once => false,
                Repeat::Extra(count) => repetition <= count,
                Repeat::Unbounded => true,
            };
            if !more {
                break;
            }
        }
    }
    flush_remainder(lines, start, output)
}

/// The first line at or after `from` that matches, as a 1-based line number.
fn find_match(lines: &Lines, regex: &Regex, from: usize) -> Option<usize> {
    (from.max(1)..=lines.total()).find(|&line| regex.is_match(lines.line(line)))
}

/// Writes lines `start..boundary` as a section (or discards them for `%re%`). Returns the
/// line where the next section starts.
fn cut(
    lines: &Lines,
    start: usize,
    boundary: usize,
    skip: bool,
    output: &mut Output,
) -> Result<usize, String> {
    let end_line = boundary.max(start);
    if !skip {
        let from = lines.offset(start);
        let to = lines.offset(end_line);
        write_section(&lines.data[from..to], output)?;
    }
    if output.suppress && boundary <= lines.total() {
        Ok(boundary.max(start) + 1)
    } else {
        Ok(end_line)
    }
}

/// Writes whatever is left from `start` to the end of the input as the final section.
fn flush_remainder(lines: &Lines, start: usize, output: &mut Output) -> Result<(), String> {
    // An empty remainder is still a section (GNU keeps it when the last matched line was
    // suppressed); `-z` drops it.
    let from = lines.offset(start);
    write_section(&lines.data[from..], output)
}

fn write_section(bytes: &[u8], output: &mut Output) -> Result<(), String> {
    if bytes.is_empty() && output.elide {
        return Ok(());
    }
    let name = format!("{}{}", output.prefix, output.suffix.render(output.index));
    output.index += 1;
    output.created.push(name.clone());
    let mut file = fs::File::create(&name).map_err(|error| {
        format!(
            "cannot open '{name}' for writing: {}",
            io_error_text(&error)
        )
    })?;
    file.write_all(bytes)
        .map_err(|error| format!("write error: {}", io_error_text(&error)))?;
    if !output.quiet {
        let mut stdout = io::stdout().lock();
        if let Err(error) = writeln!(stdout, "{}", bytes.len()) {
            if !is_broken_pipe(&error) {
                return Err(format!("write error: {}", io_error_text(&error)));
            }
        }
    }
    Ok(())
}

/// A `printf`-style suffix format with exactly one integer conversion.
struct SuffixFormat {
    before: String,
    after: String,
    left: bool,
    zero: bool,
    width: usize,
    conversion: char,
}

impl SuffixFormat {
    fn parse(text: &str) -> Result<Self, String> {
        let chars: Vec<char> = text.chars().collect();
        let mut before = String::new();
        let mut index = 0;
        let mut found: Option<usize> = None;
        while index < chars.len() {
            if chars[index] == '%' {
                if chars.get(index + 1) == Some(&'%') {
                    before.push('%');
                    index += 2;
                    continue;
                }
                if found.is_some() {
                    return Err("too many % conversion specifications in suffix".to_owned());
                }
                found = Some(index);
                index += 1;
                continue;
            }
            if found.is_none() {
                before.push(chars[index]);
            }
            index += 1;
        }
        let Some(start) = found else {
            return Err("missing % conversion specification in suffix".to_owned());
        };
        let mut position = start + 1;
        let mut left = false;
        let mut zero = false;
        while let Some(&flag) = chars.get(position) {
            match flag {
                '-' => left = true,
                '0' => zero = true,
                _ => break,
            }
            position += 1;
        }
        let mut width = 0usize;
        while let Some(digit) = chars.get(position).and_then(|c| c.to_digit(10)) {
            width = width * 10 + digit as usize;
            position += 1;
        }
        let conversion = chars.get(position).copied().unwrap_or(' ');
        if !matches!(conversion, 'd' | 'i' | 'u' | 'o' | 'x' | 'X') {
            return Err(format!(
                "invalid conversion specifier in suffix: {conversion}"
            ));
        }
        let mut after = String::new();
        let mut tail = position + 1;
        while tail < chars.len() {
            if chars[tail] == '%' && chars.get(tail + 1) == Some(&'%') {
                after.push('%');
                tail += 2;
                continue;
            }
            if chars[tail] == '%' {
                return Err("too many % conversion specifications in suffix".to_owned());
            }
            after.push(chars[tail]);
            tail += 1;
        }
        Ok(Self {
            before,
            after,
            left,
            zero,
            width,
            conversion,
        })
    }

    fn render(&self, value: u64) -> String {
        let number = match self.conversion {
            'o' => format!("{value:o}"),
            'x' => format!("{value:x}"),
            'X' => format!("{value:X}"),
            _ => value.to_string(),
        };
        let padded = if number.len() >= self.width {
            number
        } else if self.left {
            format!("{number:<width$}", width = self.width)
        } else if self.zero {
            format!("{number:0>width$}", width = self.width)
        } else {
            format!("{number:>width$}", width = self.width)
        };
        format!("{}{padded}{}", self.before, self.after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_format_pads_and_keeps_literal_text() {
        let format = SuffixFormat::parse("%02d").unwrap();
        assert_eq!(format.render(3), "03");
        assert_eq!(format.render(123), "123");
        let format = SuffixFormat::parse("part-%03x.txt").unwrap();
        assert_eq!(format.render(255), "part-0ff.txt");
        let format = SuffixFormat::parse("%%d%-3d|").unwrap();
        assert_eq!(format.render(7), "%d7  |");
    }

    #[test]
    fn suffix_format_rejects_bad_conversions() {
        assert_eq!(
            SuffixFormat::parse("nopct").err().as_deref(),
            Some("missing % conversion specification in suffix")
        );
        assert_eq!(
            SuffixFormat::parse("%d%d").err().as_deref(),
            Some("too many % conversion specifications in suffix")
        );
        assert_eq!(
            SuffixFormat::parse("%s").err().as_deref(),
            Some("invalid conversion specifier in suffix: s")
        );
    }

    #[test]
    fn offsets_need_a_sign_and_digits() {
        assert_eq!(parse_offset(""), Some(0));
        assert_eq!(parse_offset("+2"), Some(2));
        assert_eq!(parse_offset("-3"), Some(-3));
        assert_eq!(parse_offset("+"), None);
        assert_eq!(parse_offset("2"), None);
    }

    #[test]
    fn patterns_validate_line_order_and_delimiters() {
        let args: Vec<String> = ["5", "3"].iter().map(|s| (*s).to_owned()).collect();
        let err = parse_patterns(&args).err().unwrap();
        assert_eq!(
            err,
            "line number '3' is smaller than preceding line number, 5"
        );
        let args: Vec<String> = ["/abc"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(
            parse_patterns(&args).err().unwrap(),
            "/abc: closing delimiter '/' missing"
        );
        let args: Vec<String> = ["0"].iter().map(|s| (*s).to_owned()).collect();
        assert!(parse_patterns(&args).is_err());
        let args: Vec<String> = ["/a/", "{*}"].iter().map(|s| (*s).to_owned()).collect();
        let steps = parse_patterns(&args).unwrap();
        assert_eq!(steps[0].repeat, Repeat::Unbounded);
    }

    #[test]
    fn lines_index_by_line_number() {
        let lines = Lines::new(b"a\nbb\nc".to_vec());
        assert_eq!(lines.total(), 3);
        assert_eq!(lines.line(2), b"bb");
        assert_eq!(lines.line(3), b"c");
        assert_eq!(lines.offset(4), 6);
        let trailing = Lines::new(b"x\n".to_vec());
        assert_eq!(trailing.total(), 1);
    }
}
