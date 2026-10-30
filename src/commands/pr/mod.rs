//! `pr`: paginate or columnate files for printing.
//!
//! Each input is paginated separately (page numbers restart per file), except that `-m`
//! merges the files side by side. A page has a five-line header (date, name, page number),
//! a body of `length - 10` lines, and a five-line footer; `-t`/`-T` drop the header, footer
//! and padding. Multi-column output lays entries out down (default) or across (`-a`).

mod columns;
mod settings;

use std::fs;
use std::io::{self, BufRead, Write};
use std::time::SystemTime;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};
use crate::common::path::display_path;

use columns::{down_columns, write_row};
use settings::{build_settings, column_option, page_range, Settings};

const CMD: &str = "pr";
const DEFAULT_LENGTH: usize = 66;
const DEFAULT_WIDTH: usize = 72;
const HEADER_LINES: usize = 5;
const FOOTER_LINES: usize = 5;
const DEFAULT_DATE: &str = "%Y-%m-%d %H:%M";

const SPECS: &[OptSpec] = &[
    OptSpec::req("length", Some('l'), Some("length")),
    OptSpec::req("width", Some('w'), Some("width")),
    OptSpec::req("page_width", Some('W'), Some("page-width")),
    OptSpec::req("margin", Some('o'), Some("indent")),
    OptSpec::req("header", Some('h'), Some("header")),
    OptSpec::flag("omit_header", Some('t'), Some("omit-header")),
    OptSpec::flag("omit_pages", Some('T'), Some("omit-pagination")),
    OptSpec::flag("double", Some('d'), Some("double-space")),
    OptSpec::opt("number", Some('n'), Some("number-lines")),
    OptSpec::req("first_number", Some('N'), Some("first-line-number")),
    OptSpec::opt("plain_sep", Some('s'), Some("separator")),
    OptSpec::opt("string_sep", Some('S'), None),
    OptSpec::opt("expand", Some('e'), Some("expand-tabs")),
    OptSpec::opt("unexpand", Some('i'), Some("output-tabs")),
    OptSpec::flag("across", Some('a'), Some("across")),
    OptSpec::flag("merge", Some('m'), Some("merge")),
    OptSpec::flag("join", Some('J'), Some("join-lines")),
    OptSpec::req("columns", None, Some("columns")),
    OptSpec::flag("form", Some('F'), Some("form-feed")),
    OptSpec::flag("form_f", Some('f'), None),
    OptSpec::req("date", Some('D'), Some("date-format")),
    OptSpec::flag("quiet", Some('r'), Some("no-file-warnings")),
    OptSpec::flag("pause", Some('p'), Some("pause")),
    OptSpec::flag("control", Some('c'), Some("show-control-chars")),
    OptSpec::flag("nonprint", Some('v'), Some("show-nonprinting")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = r"Usage: pr [OPTION]... [FILE]...
Paginate or columnate FILE(s) for printing.

With no FILE, or when FILE is -, read standard input.

  +FIRST[:LAST]          output pages FIRST through LAST only
  -COLUMN                output COLUMN columns (print down, then across with -a)
  -a, --across           print columns across rather than down (with -COLUMN)
  -c, --show-control-chars   use hat notation (^G) and octal backslash notation
  -d, --double-space     double-space the output
  -D, --date-format=FORMAT   use FORMAT for the header date
  -e[CHAR[WIDTH]], --expand-tabs[=CHAR[WIDTH]]   expand input CHARs (TABs) to tab WIDTH (8)
  -F, -f, --form-feed    use form feeds instead of newlines to separate pages
  -h, --header=HEADER    use a centered HEADER instead of the file name
  -i[CHAR[WIDTH]], --output-tabs[=CHAR[WIDTH]]  replace spaces with CHARs at tab stops
  -J, --join-lines       merge full lines, turns off -W line truncation
  -l, --length=PAGE_LENGTH   set the page length to PAGE_LENGTH (66) lines
  -m, --merge            print all files in parallel, one in each column
  -n[SEP[DIGITS]], --number-lines[=SEP[DIGITS]]   number lines, SEP after DIGITS (5)
  -N, --first-line-number=NUMBER   start counting with NUMBER at first line
  -o, --indent=MARGIN    offset each line with MARGIN spaces
  -r, --no-file-warnings omit warning when a file cannot be opened
  -s[CHAR], --separator[=CHAR]   separate columns by a single CHAR (TAB)
  -S[STRING], --sep-string[=STRING]   separate columns by STRING
  -t, --omit-header      omit page headers and trailers
  -T, --omit-pagination  omit page headers and trailers, and form feeds
  -v, --show-nonprinting  use octal backslash notation
  -w, --width=PAGE_WIDTH   set page width to PAGE_WIDTH (72) characters
  -W, --page-width=PAGE_WIDTH   set page width to PAGE_WIDTH, no truncation
      --help             display this help and exit
      --version          output version information and exit
";

/// One input: its name for the header, its lines, and its modification time.
struct Source {
    name: String,
    lines: Vec<Vec<u8>>,
    mtime: Option<SystemTime>,
}

pub fn run(args: &[String]) -> i32 {
    let args = column_option(args);
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
    let mut settings = match build_settings(&items) {
        Ok(settings) => settings,
        Err(message) => {
            eprintln!("{CMD}: {message}");
            return EXIT_FAIL;
        }
    };
    let mut files: Vec<String> = Vec::new();
    for item in &items {
        if let Item::Operand(text) = item {
            if let Some((first, last)) = page_range(text) {
                settings.first_page = first;
                settings.last_page = last;
            } else {
                files.push(text.clone());
            }
        }
    }
    if files.is_empty() {
        files.push("-".to_owned());
    }
    let mut status = EXIT_OK;
    let mut sources = Vec::new();
    for file in &files {
        match read_source(file) {
            Ok(source) => sources.push(source),
            Err(message) => {
                eprintln!("{CMD}: {message}");
                status = EXIT_FAIL;
            }
        }
    }
    let mut out = stdout_writer();
    let result = if settings.merge {
        merge_sources(&settings, &sources, &mut out)
    } else {
        sources
            .iter()
            .try_for_each(|source| paginate(&settings, source, &mut out))
    };
    match result {
        Ok(()) => {}
        // GNU reports a page range that starts past the last page but still exits 0.
        Err(Failure::Message(message)) => {
            eprintln!("{CMD}: {message}");
            return EXIT_OK;
        }
        Err(Failure::Write(error)) => {
            if is_broken_pipe(&error) {
                return EXIT_OK;
            }
            eprintln!("{CMD}: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
    }
    match finish_output(CMD, &mut out) {
        EXIT_OK => status,
        code => code,
    }
}

enum Failure {
    Message(String),
    Write(io::Error),
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Failure::Write(error)
    }
}

fn read_source(file: &str) -> Result<Source, String> {
    let reader =
        open_input(file).map_err(|error| format!("cannot open {file}, {}", io_error_text(&error)));
    let mut reader = reader?;
    let mut lines = Vec::new();
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let count = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| format!("read error on {file}: {}", io_error_text(&error)))?;
        if count == 0 {
            break;
        }
        if buffer.last() == Some(&b'\n') {
            buffer.pop();
        }
        lines.push(buffer.clone());
    }
    let mtime = if file == "-" {
        None
    } else {
        fs::metadata(file)
            .ok()
            .and_then(|meta| meta.modified().ok())
    };
    let name = if file == "-" {
        String::new()
    } else {
        display_path(std::path::Path::new(file))
    };
    Ok(Source { name, lines, mtime })
}

fn now_or_mtime(source: &Source) -> SystemTime {
    source.mtime.unwrap_or_else(SystemTime::now)
}

/// Header date text in the requested format, in the local time zone.
fn header_date(source: &Source, format: &str) -> String {
    let time = now_or_mtime(source);
    let Ok(timestamp) = jiff::Timestamp::try_from(time) else {
        return String::new();
    };
    let zone = jiff::tz::TimeZone::system();
    timestamp.to_zoned(zone).strftime(format).to_string()
}

/// Header line: date, centered name, and `Page N` at the right.
fn header_line(settings: &Settings, source: &Source, page: usize) -> String {
    let date = header_date(source, &settings.date_format);
    let name = match &settings.header {
        Some(text) => text.clone(),
        None => source.name.clone(),
    };
    let page_text = format!("Page {page}");
    let used = date.len() + name.len() + page_text.len();
    let (left, right) = if settings.width > used {
        let space = settings.width - used;
        (space / 2, space - space / 2)
    } else {
        (1, 1)
    };
    format!(
        "{date}{}{name}{}{page_text}",
        " ".repeat(left),
        " ".repeat(right)
    )
}

/// Number of body rows on a page.
fn body_rows(settings: &Settings) -> usize {
    if settings.omit_header || settings.length <= HEADER_LINES + FOOTER_LINES {
        settings.length
    } else {
        settings.length - HEADER_LINES - FOOTER_LINES
    }
}

fn has_chrome(settings: &Settings) -> bool {
    !settings.omit_header && settings.length > HEADER_LINES + FOOTER_LINES
}

/// Writes the header lines of a page (or nothing when headers are omitted).
fn write_header(
    settings: &Settings,
    source: &Source,
    page: usize,
    out: &mut impl Write,
) -> io::Result<()> {
    if !has_chrome(settings) {
        return Ok(());
    }
    let margin = " ".repeat(settings.margin);
    out.write_all(margin.as_bytes())?;
    out.write_all(b"\n")?;
    out.write_all(b"\n")?;
    out.write_all(margin.as_bytes())?;
    out.write_all(header_line(settings, source, page).as_bytes())?;
    out.write_all(b"\n\n\n")
}

/// The footer: five blank lines, or with `-F` a bare form feed (GNU ends each page with it).
fn write_footer(settings: &Settings, out: &mut impl Write) -> io::Result<()> {
    if has_chrome(settings) {
        if settings.form_feed {
            out.write_all(b"\x0c")?;
        } else {
            out.write_all(&[b'\n'; FOOTER_LINES])?;
        }
    }
    Ok(())
}

/// Text of each input line after tab expansion and control-character notation.
fn prepare(settings: &Settings, line: &[u8]) -> Vec<u8> {
    let mut text = match settings.expand {
        Some((character, width)) => expand_tabs(line, character, width),
        None => line.to_vec(),
    };
    if settings.nonprint || settings.control {
        text = escape_controls(&text, settings.control);
    }
    text
}

fn expand_tabs(line: &[u8], character: u8, width: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    for &byte in line {
        if byte == character && character == b'\t' {
            let column = out.len();
            let stop = (column / width + 1) * width;
            out.extend(std::iter::repeat_n(b' ', stop - column));
        } else {
            out.push(byte);
        }
    }
    out
}

fn escape_controls(line: &[u8], hat: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    for &byte in line {
        if byte == b'\t' {
            out.push(byte);
        } else if byte < 32 && hat {
            out.push(b'^');
            out.push(byte + 64);
        } else if !(32..127).contains(&byte) {
            out.extend(format!("\\{byte:03o}").bytes());
        } else {
            out.push(byte);
        }
    }
    out
}

/// One single-column body row: text (numbered when `-n` is given) or a double-space blank.
enum Row {
    Text(Vec<u8>),
    Blank,
}

fn paginate(settings: &Settings, source: &Source, out: &mut impl Write) -> Result<(), Failure> {
    if settings.columns > 1 && !settings.merge {
        return columnate(settings, source, out);
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut number = settings.first_number;
    for line in &source.lines {
        let text = prepare(settings, line);
        rows.push(Row::Text(text));
        if settings.double {
            rows.push(Row::Blank);
        }
    }
    let per_page = body_rows(settings).max(1);
    let total_pages = rows.len().div_ceil(per_page).max(1);
    check_range(settings, total_pages)?;
    let mut index = 0;
    let mut page = 1;
    while index < rows.len() || page == 1 {
        let end = (index + per_page).min(rows.len());
        if page >= settings.first_page && page <= settings.last_page {
            write_header(settings, source, page, out)?;
            let mut written = 0;
            for row in &rows[index..end] {
                match row {
                    Row::Text(text) => {
                        write_numbered(settings, &mut number, text, out)?;
                    }
                    Row::Blank => out.write_all(b"\n")?,
                }
                written += 1;
            }
            if has_chrome(settings) && !settings.form_feed {
                out.write_all(&vec![b'\n'; per_page - written])?;
            }
            write_footer(settings, out)?;
        } else {
            for row in &rows[index..end] {
                if let Row::Text(_) = row {
                    number += 1;
                }
            }
        }
        index = end;
        page += 1;
        if index >= rows.len() {
            break;
        }
    }
    Ok(())
}

fn check_range(settings: &Settings, total_pages: usize) -> Result<(), Failure> {
    if settings.first_page > total_pages && settings.first_page != 1 {
        return Err(Failure::Message(format!(
            "starting page number {} exceeds page count {total_pages}",
            settings.first_page
        )));
    }
    Ok(())
}

/// Writes a single-column text row, with its line number when numbering is on.
fn write_numbered(
    settings: &Settings,
    number: &mut i64,
    text: &[u8],
    out: &mut impl Write,
) -> io::Result<()> {
    out.write_all(&vec![b' '; settings.margin])?;
    if let Some(numbering) = &settings.numbering {
        let digits = number.to_string();
        let padding = numbering.width.saturating_sub(digits.len());
        out.write_all(&vec![b' '; padding])?;
        out.write_all(digits.as_bytes())?;
        out.write_all(&numbering.separator)?;
        *number += 1;
    }
    let body = match settings.unexpand {
        Some((character, width)) => unexpand_spaces(text, character, width),
        None => text.to_vec(),
    };
    out.write_all(&body)?;
    out.write_all(b"\n")
}

/// `-i`: runs of spaces that reach a tab stop become one tab (or the chosen character).
fn unexpand_spaces(text: &[u8], character: u8, width: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut column = 0usize;
    let mut index = 0;
    while index < text.len() {
        if text[index] == b' ' {
            let start = index;
            let start_column = column;
            while index < text.len() && text[index] == b' ' {
                index += 1;
                column += 1;
            }
            let mut position = start_column;
            while (position / width + 1) * width <= column && column - position >= 2 {
                let stop = (position / width + 1) * width;
                out.push(character);
                position = stop;
            }
            out.extend(std::iter::repeat_n(b' ', column - position));
            let _ = start;
        } else {
            out.push(text[index]);
            column += 1;
            index += 1;
        }
    }
    out
}

/// Multi-column output for one file: entries go down (default) or across (`-a`).
fn columnate(settings: &Settings, source: &Source, out: &mut impl Write) -> Result<(), Failure> {
    let entries: Vec<Vec<u8>> = source
        .lines
        .iter()
        .map(|line| prepare(settings, line))
        .collect();
    let per_page = body_rows(settings).max(1) * settings.columns;
    let total_pages = entries.len().div_ceil(per_page).max(1);
    check_range(settings, total_pages)?;
    let column_width = settings.width / settings.columns;
    let mut index = 0;
    let mut page = 1;
    loop {
        let end = (index + per_page).min(entries.len());
        let chunk = &entries[index..end];
        if page >= settings.first_page && page <= settings.last_page {
            write_header(settings, source, page, out)?;
            let rows = column_rows(settings, chunk);
            let mut written = 0;
            for cells in &rows {
                let cells: Vec<Option<&[u8]>> = cells
                    .iter()
                    .map(|cell| cell.map(|text| truncate(settings, text, column_width)))
                    .collect();
                write_row(
                    out,
                    settings.margin,
                    &cells,
                    column_width,
                    &settings.separator,
                    false,
                )?;
                written += 1;
            }
            if has_chrome(settings) && !settings.form_feed {
                out.write_all(&vec![b'\n'; body_rows(settings).saturating_sub(written)])?;
            }
            write_footer(settings, out)?;
        }
        index = end;
        page += 1;
        if index >= entries.len() {
            break;
        }
    }
    Ok(())
}

/// Cell texts per row for one page of entries.
fn column_rows<'a>(settings: &Settings, chunk: &'a [Vec<u8>]) -> Vec<Vec<Option<&'a [u8]>>> {
    let columns = settings.columns;
    if settings.across {
        chunk
            .chunks(columns)
            .map(|row| {
                let mut cells: Vec<Option<&[u8]>> =
                    row.iter().map(|t| Some(t.as_slice())).collect();
                cells.resize(columns, None);
                cells
            })
            .collect()
    } else {
        let ranges = down_columns(chunk.len(), columns);
        let height = ranges
            .iter()
            .map(|(start, end)| end - start)
            .max()
            .unwrap_or(0);
        (0..height)
            .map(|row| {
                ranges
                    .iter()
                    .map(|(start, end)| {
                        let at = start + row;
                        (at < *end).then(|| chunk[at].as_slice())
                    })
                    .collect()
            })
            .collect()
    }
}

/// Truncates a cell to the column width (one column short of the next start), unless `-J`.
fn truncate<'a>(settings: &Settings, text: &'a [u8], column_width: usize) -> &'a [u8] {
    if settings.join || settings.columns == 1 {
        return text;
    }
    let limit = column_width.saturating_sub(1);
    &text[..text.len().min(limit)]
}

/// `-m`: every file in its own column, row by row.
fn merge_sources(
    settings: &Settings,
    sources: &[Source],
    out: &mut impl Write,
) -> Result<(), Failure> {
    let columns: Vec<Vec<Vec<u8>>> = sources
        .iter()
        .map(|source| {
            source
                .lines
                .iter()
                .map(|line| prepare(settings, line))
                .collect()
        })
        .collect();
    let per_page = body_rows(settings).max(1);
    let longest = columns.iter().map(Vec::len).max().unwrap_or(0);
    let total_pages = longest.div_ceil(per_page).max(1);
    check_range(settings, total_pages)?;
    let column_width = settings.width / columns.len().max(1);
    let mut cursors = vec![0usize; columns.len()];
    let mut page = 1;
    let header_source = sources.first();
    loop {
        let mut rows: Vec<Vec<Option<Vec<u8>>>> = Vec::new();
        for _ in 0..per_page {
            let cells: Vec<Option<Vec<u8>>> = columns
                .iter()
                .zip(cursors.iter_mut())
                .map(|(column, cursor)| {
                    let cell = column.get(*cursor).cloned();
                    if cell.is_some() {
                        *cursor += 1;
                    }
                    cell
                })
                .collect();
            if cells.iter().all(Option::is_none) {
                break;
            }
            rows.push(cells);
        }
        if page >= settings.first_page && page <= settings.last_page {
            if let Some(source) = header_source {
                write_header(settings, source, page, out)?;
            }
            for cells in &rows {
                let cells: Vec<Option<&[u8]>> = cells
                    .iter()
                    .map(|cell| {
                        cell.as_deref()
                            .map(|text| truncate(settings, text, column_width))
                    })
                    .collect();
                write_row(
                    out,
                    settings.margin,
                    &cells,
                    column_width,
                    &settings.separator,
                    true,
                )?;
            }
            if has_chrome(settings) && !settings.form_feed {
                out.write_all(&vec![b'\n'; per_page.saturating_sub(rows.len())])?;
            }
            write_footer(settings, out)?;
        }
        page += 1;
        let done = columns
            .iter()
            .zip(cursors.iter())
            .all(|(column, cursor)| *cursor >= column.len());
        if done {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::settings::parse_numbering;
    use super::*;

    #[test]
    fn page_ranges_parse_first_and_last() {
        assert_eq!(page_range("+2"), Some((2, usize::MAX)));
        assert_eq!(page_range("+2:3"), Some((2, 3)));
        assert_eq!(page_range("file"), None);
    }

    #[test]
    fn column_flags_become_column_option() {
        let args = vec!["-3".to_owned(), "x".to_owned()];
        assert_eq!(column_option(&args), vec!["--columns=3", "x"]);
    }

    #[test]
    fn numbering_separator_and_width() {
        let numbering = parse_numbering("x2").unwrap();
        assert_eq!(numbering.separator, b"x");
        assert_eq!(numbering.width, 2);
        let default = parse_numbering("").unwrap();
        assert_eq!(default.separator, b"\t");
        assert_eq!(default.width, 5);
    }

    #[test]
    fn tabs_expand_to_stops() {
        assert_eq!(expand_tabs(b"a\tb", b'\t', 8), b"a       b");
    }

    #[test]
    fn controls_use_octal_or_hat_notation() {
        assert_eq!(escape_controls(b"a\x01b", false), b"a\\001b");
        assert_eq!(escape_controls(b"a\x01b", true), b"a^Ab");
    }
}
