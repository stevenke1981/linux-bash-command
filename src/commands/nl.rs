//! `nl`: number the lines of each file.
//!
//! Logical pages are delimited by lines consisting of the section delimiter repeated three
//! times (header), twice (body) or once (footer). A delimiter line is printed as an empty
//! line. Numbering restarts at each delimiter unless `-p` is given.

use std::io::{self, Write};

use regex::bytes::Regex;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};
use crate::common::regex::bre_to_regex;

const CMD: &str = "nl";

const SPECS: &[OptSpec] = &[
    OptSpec::req("body", Some('b'), Some("body-numbering")),
    OptSpec::req("delim", Some('d'), Some("section-delimiter")),
    OptSpec::req("footer", Some('f'), Some("footer-numbering")),
    OptSpec::req("header", Some('h'), Some("header-numbering")),
    OptSpec::req("incr", Some('i'), Some("page-increment")),
    OptSpec::req("join", Some('l'), Some("join-blank-lines")),
    OptSpec::req("format", Some('n'), Some("number-format")),
    OptSpec::flag("norenum", Some('p'), Some("no-renumber")),
    OptSpec::req("sep", Some('s'), Some("number-separator")),
    OptSpec::req("start", Some('v'), Some("starting-line-number")),
    OptSpec::req("width", Some('w'), Some("number-width")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: nl [OPTION]... [FILE]...\n\
Write each FILE to standard output, with line numbers added. With no FILE,\n\
or when FILE is -, read standard input.\n\n\
  -b, --body-numbering=STYLE      use STYLE for numbering body lines\n\
  -d, --section-delimiter=CC      use CC for logical-page delimiters\n\
  -f, --footer-numbering=STYLE    use STYLE for numbering footer lines\n\
  -h, --header-numbering=STYLE    use STYLE for numbering header lines\n\
  -i, --page-increment=NUMBER     line number increment at each line\n\
  -l, --join-blank-lines=NUMBER   group of NUMBER empty lines counted as one\n\
  -n, --number-format=FORMAT      insert line numbers according to FORMAT\n\
  -p, --no-renumber               do not reset line numbers at logical pages\n\
  -s, --number-separator=STRING   add STRING after (possible) line number\n\
  -v, --starting-line-number=NUMBER  first line number on each logical page\n\
  -w, --number-width=NUMBER       use NUMBER columns for line numbers\n\
      --help                      display this help and exit\n\
      --version                   output version information and exit\n\n\
STYLE is one of: a (all lines), t (non-empty lines, the default for body),\n\
n (no lines; the default for header and footer), or pBRE (lines matching BRE).\n\
FORMAT is one of: ln (left justified), rn (right justified, the default),\n\
rz (right justified, leading zeros).\n\
Exit status: 0 on success, 1 if an input could not be read.\n";

#[derive(Debug)]
enum Style {
    All,
    NonEmpty,
    Never,
    Matching(Regex),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    LeftJustified,
    RightJustified,
    ZeroFilled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Header,
    Body,
    Footer,
}

#[derive(Debug)]
struct Config {
    body: Style,
    header: Style,
    footer: Style,
    delims: [Vec<u8>; 3],
    start: i64,
    incr: i64,
    width: usize,
    format: Format,
    sep: Vec<u8>,
    join: u64,
    renumber: bool,
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
    let config = match build_config(&items) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let files: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(name) => Some(name.clone()),
            Item::Opt { .. } => None,
        })
        .collect();
    run_files(&config, &files)
}

fn usage(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

fn build_config(items: &[Item]) -> Result<Config, i32> {
    let mut config = Config {
        body: Style::NonEmpty,
        header: Style::Never,
        footer: Style::Never,
        delims: delimiters("\\:")?,
        start: 1,
        incr: 1,
        width: 6,
        format: Format::RightJustified,
        sep: b"\t".to_vec(),
        join: 1,
        renumber: true,
    };
    for item in items {
        if let Item::Opt { id, value } = item {
            apply(&mut config, id, value.as_deref().unwrap_or(""))?;
        }
    }
    Ok(config)
}

fn apply(config: &mut Config, id: &str, value: &str) -> Result<(), i32> {
    match id {
        "body" => config.body = parse_style("body", value)?,
        "header" => config.header = parse_style("header", value)?,
        "footer" => config.footer = parse_style("footer", value)?,
        "delim" => config.delims = delimiters(value)?,
        "start" => {
            config.start = parse_signed(value)
                .ok_or_else(|| usage(&format!("invalid starting line number: '{value}'")))?
        }
        "incr" => {
            config.incr = parse_signed(value)
                .ok_or_else(|| usage(&format!("invalid line number increment: '{value}'")))?
        }
        "width" => config.width = parse_width(value)?,
        "format" => config.format = parse_format(value)?,
        "sep" => config.sep = value.as_bytes().to_vec(),
        "join" => config.join = parse_join(value)?,
        "norenum" => config.renumber = false,
        _ => {}
    }
    Ok(())
}

fn parse_style(which: &str, value: &str) -> Result<Style, i32> {
    match value {
        "a" => Ok(Style::All),
        "t" => Ok(Style::NonEmpty),
        "n" => Ok(Style::Never),
        _ => match value.strip_prefix('p') {
            Some(pattern) => compile_pattern(pattern),
            None => Err(usage(&format!(
                "invalid {which} numbering style: '{value}'"
            ))),
        },
    }
}

fn compile_pattern(pattern: &str) -> Result<Style, i32> {
    let translated = bre_to_regex(pattern).map_err(|error| usage(&error))?;
    Regex::new(&translated)
        .map(Style::Matching)
        .map_err(|error| usage(&format!("invalid regular expression: {error}")))
}

/// Builds the header, body and footer delimiter lines from a two-character `CC` spec. A
/// single character is completed with `:`.
fn delimiters(spec: &str) -> Result<[Vec<u8>; 3], i32> {
    let chars: Vec<char> = spec.chars().collect();
    let (first, second) = match chars.as_slice() {
        [one] => (*one, ':'),
        [one, two] => (*one, *two),
        _ => return Err(usage("invalid section delimiters")),
    };
    let pair: String = [first, second].iter().collect();
    Ok([
        pair.repeat(3).into_bytes(),
        pair.repeat(2).into_bytes(),
        pair.into_bytes(),
    ])
}

fn parse_signed(text: &str) -> Option<i64> {
    text.parse::<i64>().ok()
}

fn parse_width(value: &str) -> Result<usize, i32> {
    match value.parse::<usize>() {
        Ok(width) if width > 0 => Ok(width),
        _ => Err(usage(&format!(
            "invalid line number field width: '{value}'"
        ))),
    }
}

fn parse_format(value: &str) -> Result<Format, i32> {
    match value {
        "ln" => Ok(Format::LeftJustified),
        "rn" => Ok(Format::RightJustified),
        "rz" => Ok(Format::ZeroFilled),
        _ => Err(usage(&format!("invalid line numbering format: '{value}'"))),
    }
}

fn parse_join(value: &str) -> Result<u64, i32> {
    match value.parse::<u64>() {
        Ok(count) if count > 0 => Ok(count),
        _ => Err(usage(&format!(
            "invalid line number of blank lines: '{value}'"
        ))),
    }
}

/// Numbering state carried across the whole input (files are one stream, as in GNU).
struct State {
    section: Section,
    number: i64,
    blank_run: u64,
}

fn run_files(config: &Config, files: &[String]) -> i32 {
    let default = ["-".to_owned()];
    let names: &[String] = if files.is_empty() { &default } else { files };
    let mut out = stdout_writer();
    let mut state = State {
        section: Section::Body,
        number: config.start,
        blank_run: 0,
    };
    let mut status = EXIT_OK;
    for name in names {
        match number_file(config, &mut state, name, &mut out) {
            Ok(()) => {}
            Err(Failure::Input(error)) => {
                report(CMD, name, &error);
                status = EXIT_FAIL;
            }
            Err(Failure::Output(error)) => return output_failed(&error),
        }
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed == EXIT_OK {
        status
    } else {
        flushed
    }
}

enum Failure {
    Input(io::Error),
    Output(io::Error),
}

fn number_file(
    config: &Config,
    state: &mut State,
    name: &str,
    out: &mut impl Write,
) -> Result<(), Failure> {
    let mut reader = open_input(name).map_err(Failure::Input)?;
    let mut record = Vec::new();
    let mut line = Vec::new();
    while read_record(reader.as_mut(), b'\n', &mut record).map_err(Failure::Input)? {
        line.clear();
        number_line(config, state, &record, &mut line);
        out.write_all(&line).map_err(Failure::Output)?;
    }
    Ok(())
}

fn output_failed(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_FAIL
}

/// Produces the output for one record (its newline, if any, is kept).
fn number_line(config: &Config, state: &mut State, record: &[u8], out: &mut Vec<u8>) {
    let terminated = record.last() == Some(&b'\n');
    let text = if terminated {
        &record[..record.len() - 1]
    } else {
        record
    };
    if let Some(section) = delimiter_section(config, text) {
        state.section = section;
        if config.renumber {
            state.number = config.start;
        }
        state.blank_run = 0;
        out.push(b'\n');
        return;
    }
    let style = match state.section {
        Section::Header => &config.header,
        Section::Body => &config.body,
        Section::Footer => &config.footer,
    };
    let numbered = is_numbered(config, style, state, text);
    if numbered {
        out.extend_from_slice(format_number(config, state.number).as_bytes());
        out.extend_from_slice(&config.sep);
        state.number += config.incr;
    } else {
        out.extend(std::iter::repeat_n(b' ', config.width + config.sep.len()));
    }
    out.extend_from_slice(text);
    if terminated {
        out.push(b'\n');
    }
}

fn delimiter_section(config: &Config, text: &[u8]) -> Option<Section> {
    let [header, body, footer] = &config.delims;
    if text == header.as_slice() {
        Some(Section::Header)
    } else if text == body.as_slice() {
        Some(Section::Body)
    } else if text == footer.as_slice() {
        Some(Section::Footer)
    } else {
        None
    }
}

/// Decides whether a line gets a number, updating the blank-line counter.
fn is_numbered(config: &Config, style: &Style, state: &mut State, text: &[u8]) -> bool {
    let blank = text.is_empty();
    if blank {
        state.blank_run += 1;
    } else {
        state.blank_run = 0;
    }
    match style {
        Style::All => !blank || state.blank_run.is_multiple_of(config.join),
        Style::NonEmpty => !blank,
        Style::Never => false,
        Style::Matching(regex) => regex.is_match(text),
    }
}

fn format_number(config: &Config, number: i64) -> String {
    match config.format {
        Format::LeftJustified => format!("{number:<width$}", width = config.width),
        Format::RightJustified => format!("{number:>width$}", width = config.width),
        Format::ZeroFilled => format!("{number:0width$}", width = config.width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            body: Style::NonEmpty,
            header: Style::Never,
            footer: Style::Never,
            delims: delimiters("\\:").unwrap(),
            start: 1,
            incr: 1,
            width: 6,
            format: Format::RightJustified,
            sep: b"\t".to_vec(),
            join: 1,
            renumber: true,
        }
    }

    fn number(config: &Config, input: &[u8]) -> Vec<u8> {
        let mut state = State {
            section: Section::Body,
            number: config.start,
            blank_run: 0,
        };
        let mut out = Vec::new();
        for record in input.split_inclusive(|&b| b == b'\n') {
            number_line(config, &mut state, record, &mut out);
        }
        out
    }

    #[test]
    fn numbers_non_empty_lines_by_default() {
        assert_eq!(
            number(&config(), b"a\n\nb\n"),
            b"     1\ta\n       \n     2\tb\n"
        );
    }

    #[test]
    fn formats_numbers() {
        let mut cfg = config();
        cfg.format = Format::ZeroFilled;
        cfg.width = 4;
        assert_eq!(number(&cfg, b"x\n"), b"0001\tx\n");
        cfg.format = Format::LeftJustified;
        cfg.sep = b"|".to_vec();
        cfg.width = 3;
        assert_eq!(number(&cfg, b"x\n"), b"1  |x\n");
    }

    #[test]
    fn section_delimiters_restart_numbering() {
        let mut cfg = config();
        cfg.header = Style::All;
        let out = number(&cfg, b"\\:\\:\\:\nh1\nh2\n\\:\\:\nb1\n");
        assert_eq!(out, b"\n     1\th1\n     2\th2\n\n     1\tb1\n");
    }

    #[test]
    fn join_blank_lines_numbers_every_nth_blank() {
        let mut cfg = config();
        cfg.body = Style::All;
        cfg.join = 2;
        let out = number(&cfg, b"\n\n\n");
        assert_eq!(out, b"       \n     1\t\n       \n");
    }

    #[test]
    fn matching_style_uses_bre() {
        let mut cfg = config();
        cfg.body = compile_pattern("o").unwrap();
        assert_eq!(
            number(&cfg, b"one\ntwo\nxyz\n"),
            b"     1\tone\n     2\ttwo\n       xyz\n"
        );
    }

    #[test]
    fn rejects_bad_styles_and_formats() {
        assert!(parse_style("body", "x").is_err());
        assert!(parse_format("cc").is_err());
        assert!(parse_width("0").is_err());
        assert!(delimiters("abc").is_err());
    }
}
