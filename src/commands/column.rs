//! `column`: fills lines into columns (default), or formats delimiter-separated input as a
//! table (`-t`), optionally as JSON (`-J`). Specification: docs/spec/coreutils-alignment/
//! 04-commands-extra.md (column, util-linux 2.39 subset).

use std::io::{Read, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, open_input, stdout_writer};

const CMD: &str = "column";

const OPTS: &[OptSpec] = &[
    OptSpec::flag("table", Some('t'), Some("table")),
    OptSpec::req("separator", Some('s'), Some("separator")),
    OptSpec::req("output-separator", Some('o'), Some("output-separator")),
    OptSpec::req("output-width", Some('c'), Some("output-width")),
    OptSpec::flag("fillrows", Some('x'), Some("fillrows")),
    OptSpec::req("table-columns", Some('N'), Some("table-columns")),
    OptSpec::req("table-right", Some('R'), Some("table-right")),
    OptSpec::req("table-hide", Some('H'), Some("table-hide")),
    OptSpec::req(
        "table-columns-limit",
        Some('l'),
        Some("table-columns-limit"),
    ),
    OptSpec::flag("keep-empty-lines", Some('L'), Some("keep-empty-lines")),
    OptSpec::req("table-name", Some('n'), Some("table-name")),
    OptSpec::flag("json", Some('J'), Some("json")),
    OptSpec::flag("help", Some('h'), Some("help")),
    OptSpec::flag("version", Some('V'), Some("version")),
    // Accepted by GNU column but not implemented here; they fail with `unsupported`.
    OptSpec::req("table-order", Some('O'), Some("table-order")),
    OptSpec::req("table-column", Some('C'), Some("table-column")),
    OptSpec::req("table-noextreme", Some('E'), Some("table-noextreme")),
    OptSpec::flag("table-noheadings", Some('d'), Some("table-noheadings")),
    OptSpec::flag("table-maxout", Some('m'), Some("table-maxout")),
    OptSpec::flag(
        "table-header-repeat",
        Some('e'),
        Some("table-header-repeat"),
    ),
    OptSpec::req("table-truncate", Some('T'), Some("table-truncate")),
    OptSpec::req("table-wrap", Some('W'), Some("table-wrap")),
    OptSpec::req("tree", Some('r'), Some("tree")),
    OptSpec::req("tree-id", Some('i'), Some("tree-id")),
    OptSpec::req("tree-parent", Some('p'), Some("tree-parent")),
];

/// A column named by its 1-based number or, with `-N`, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ColumnRef {
    Number(usize),
    Name(String),
}

#[derive(Debug)]
struct Options {
    table: bool,
    json: bool,
    /// Separator characters for table input; `None` splits on runs of whitespace.
    separator: Option<Vec<u8>>,
    output_separator: Vec<u8>,
    width: Option<usize>,
    fillrows: bool,
    names: Vec<String>,
    right: Vec<ColumnRef>,
    hidden: Vec<ColumnRef>,
    limit: Option<usize>,
    keep_empty: bool,
    table_name: String,
}

/// Entry point for `column` and the `agentcmd column` multi-call form.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, OPTS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage(CMD, &err, EXIT_FAIL),
    };
    let mut options = Options {
        table: false,
        json: false,
        separator: None,
        output_separator: b"  ".to_vec(),
        width: None,
        fillrows: false,
        names: Vec::new(),
        right: Vec::new(),
        hidden: Vec::new(),
        limit: None,
        keep_empty: false,
        table_name: "table".to_string(),
    };
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => {
                if let Some(code) = apply_option(id, value, &mut options) {
                    return code;
                }
            }
        }
    }
    if options.table && options.fillrows {
        eprintln!("{CMD}: mutually exclusive arguments: --table --fillrows");
        return EXIT_FAIL;
    }
    if options.json && options.names.is_empty() {
        eprintln!("{CMD}: option --table-columns or --table-column required for --json");
        return EXIT_FAIL;
    }
    if operands.is_empty() {
        operands.push("-".to_string());
    }
    let (lines, failed) = read_lines(&operands);
    let mut out = stdout_writer();
    let status = if options.table || options.json {
        match format_table(&lines, &options) {
            Ok(text) => write_text(&mut out, &text),
            Err(message) => {
                eprintln!("{CMD}: {message}");
                EXIT_FAIL
            }
        }
    } else {
        let width = options.width.unwrap_or_else(default_width);
        let items: Vec<&[u8]> = lines
            .iter()
            .map(Vec::as_slice)
            .filter(|line| options.keep_empty || !line.is_empty())
            .collect();
        write_text(&mut out, &fill_columns(&items, width, options.fillrows))
    };
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    if failed || status != EXIT_OK {
        EXIT_FAIL
    } else {
        EXIT_OK
    }
}

/// Applies one option. Returns `Some(code)` when the program must stop right away.
fn apply_option(id: &str, value: Option<String>, options: &mut Options) -> Option<i32> {
    let value = value.unwrap_or_default();
    match id {
        "help" => return Some(write_help()),
        "version" => return Some(print_version(CMD)),
        "table" => options.table = true,
        "json" => {
            options.json = true;
            options.table = true;
        }
        "separator" => options.separator = Some(value.into_bytes()),
        "output-separator" => options.output_separator = value.into_bytes(),
        "output-width" => match value.parse::<usize>() {
            Ok(width) => options.width = Some(width),
            Err(_) => {
                eprintln!("{CMD}: invalid columns argument: '{value}'");
                return Some(EXIT_FAIL);
            }
        },
        "fillrows" => options.fillrows = true,
        "table-columns" => options.names = value.split(',').map(str::to_string).collect(),
        "table-right" => options.right = column_refs(&value),
        "table-hide" => options.hidden = column_refs(&value),
        "table-columns-limit" => match value.parse::<usize>() {
            Ok(limit) if limit > 0 => options.limit = Some(limit),
            _ => {
                eprintln!("{CMD}: invalid limit argument: '{value}'");
                return Some(EXIT_FAIL);
            }
        },
        "keep-empty-lines" => options.keep_empty = true,
        "table-name" => options.table_name = value,
        other => return Some(unsupported(CMD, &format!("--{other}"))),
    }
    None
}

fn column_refs(text: &str) -> Vec<ColumnRef> {
    text.split(',')
        .filter(|part| !part.is_empty())
        .map(|part| match part.parse::<usize>() {
            Ok(number) => ColumnRef::Number(number),
            Err(_) => ColumnRef::Name(part.to_string()),
        })
        .collect()
}

/// The width used by fill mode: `COLUMNS` when it is a positive number, else 80.
fn default_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|width| *width > 0)
        .unwrap_or(80)
}

fn write_text(out: &mut dyn Write, text: &[u8]) -> i32 {
    match out.write_all(text) {
        Ok(()) => EXIT_OK,
        Err(error) if crate::common::io::is_broken_pipe(&error) => EXIT_OK,
        Err(error) => {
            eprintln!("{CMD}: write error: {}", io_error_text(&error));
            EXIT_FAIL
        }
    }
}

/// Reads every operand as lines, in order. A failing operand is reported and skipped.
fn read_lines(operands: &[String]) -> (Vec<Vec<u8>>, bool) {
    let mut lines = Vec::new();
    let mut failed = false;
    for name in operands {
        let mut data = Vec::new();
        let result = match open_input(name) {
            Ok(mut reader) => reader.read_to_end(&mut data).map(|_| ()),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            eprintln!("{CMD}: {name}: {}", io_error_text(&error));
            failed = true;
            continue;
        }
        if data.is_empty() {
            continue;
        }
        let body = data.strip_suffix(b"\n").unwrap_or(&data);
        lines.extend(body.split(|byte| *byte == b'\n').map(<[u8]>::to_vec));
    }
    (lines, failed)
}

/// Fill mode: items are laid out in columns of `roundup8(longest + 1)` and filled down the
/// columns, or across the rows with `-x`.
fn fill_columns(items: &[&[u8]], width: usize, fillrows: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if items.is_empty() {
        return out;
    }
    let longest = items
        .iter()
        .map(|item| expanded_width(item))
        .max()
        .unwrap_or(0);
    let column_width = round_to_tab(longest + 1);
    let columns = (width / column_width).max(1);
    let count = items.len();
    let rows = count.div_ceil(columns);
    for row in 0..rows {
        // Down the columns (default) the items of a row are `rows` apart; with -x they are adjacent.
        let indexes: Vec<usize> = (0..columns)
            .map(|column| {
                if fillrows {
                    row * columns + column
                } else {
                    column * rows + row
                }
            })
            .filter(|index| *index < count)
            .collect();
        let mut col = 0;
        for (position, index) in indexes.iter().enumerate() {
            push_expanded(&mut out, &mut col, items[*index]);
            if position + 1 < indexes.len() {
                pad_to(&mut out, &mut col, (position + 1) * column_width);
            }
        }
        out.push(b'\n');
    }
    out
}

/// Table mode: splits the lines into cells, aligns the columns, and prints them (or prints
/// JSON with `-J`).
fn format_table(lines: &[Vec<u8>], options: &Options) -> Result<Vec<u8>, String> {
    let mut rows: Vec<Vec<Vec<u8>>> = Vec::new();
    for line in lines {
        let blank = match options.separator {
            Some(_) => line.is_empty(),
            None => line.iter().all(|byte| *byte == b' ' || *byte == b'\t'),
        };
        if blank {
            if options.keep_empty {
                rows.push(Vec::new());
            }
            continue;
        }
        rows.push(split_cells(line, options));
    }
    let data_columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let columns = data_columns.max(options.names.len());
    let mut hidden = resolve(&options.hidden, options, columns)?;
    let right = resolve(&options.right, options, columns)?;
    hidden.sort_unstable();
    let visible: Vec<usize> = (0..columns)
        .filter(|index| !hidden.contains(index))
        .collect();
    if options.json {
        return Ok(json_table(&rows, &visible, options));
    }
    let mut table: Vec<Vec<Vec<u8>>> = Vec::new();
    if !options.names.is_empty() {
        table.push(
            options
                .names
                .iter()
                .map(|name| name.as_bytes().to_vec())
                .collect(),
        );
    }
    table.extend(rows);
    let mut widths = vec![0usize; columns];
    for row in &table {
        for &index in &visible {
            let cell = row.get(index).map_or(&[][..], Vec::as_slice);
            widths[index] = widths[index].max(display_width(cell));
        }
    }
    let mut out = Vec::new();
    for row in &table {
        for (position, &index) in visible.iter().enumerate() {
            let cell = row.get(index).map_or(&[][..], Vec::as_slice);
            let padding = widths[index].saturating_sub(display_width(cell));
            let last = position + 1 == visible.len();
            if right.contains(&index) {
                out.extend(std::iter::repeat_n(b' ', padding));
                out.extend_from_slice(cell);
            } else {
                out.extend_from_slice(cell);
                if !last {
                    out.extend(std::iter::repeat_n(b' ', padding));
                }
            }
            if !last {
                out.extend_from_slice(&options.output_separator);
            }
        }
        out.push(b'\n');
    }
    Ok(out)
}

/// Splits one line into cells: on runs of whitespace by default, or at each character of
/// `-s` (keeping empty cells). `-l` caps the count; the last cell takes the rest of the line.
fn split_cells(line: &[u8], options: &Options) -> Vec<Vec<u8>> {
    let limit = options.limit.unwrap_or(usize::MAX);
    match &options.separator {
        Some(set) => {
            let mut cells = Vec::new();
            let mut start = 0;
            for (index, byte) in line.iter().enumerate() {
                if cells.len() + 1 < limit && set.contains(byte) {
                    cells.push(line[start..index].to_vec());
                    start = index + 1;
                }
            }
            cells.push(line[start..].to_vec());
            cells
        }
        None => {
            let is_space = |byte: &u8| *byte == b' ' || *byte == b'\t';
            let mut cells = Vec::new();
            let mut index = 0;
            loop {
                while index < line.len() && is_space(&line[index]) {
                    index += 1;
                }
                if index >= line.len() {
                    break;
                }
                if cells.len() + 1 == limit {
                    cells.push(line[index..].to_vec());
                    break;
                }
                let start = index;
                while index < line.len() && !is_space(&line[index]) {
                    index += 1;
                }
                cells.push(line[start..index].to_vec());
            }
            cells
        }
    }
}

/// Resolves column references to 0-based indexes. A number beyond the column count, or an
/// unknown name, is an error.
fn resolve(refs: &[ColumnRef], options: &Options, columns: usize) -> Result<Vec<usize>, String> {
    let mut indexes = Vec::new();
    for reference in refs {
        let index = match reference {
            ColumnRef::Number(number) if *number >= 1 && *number <= columns => number - 1,
            ColumnRef::Number(number) => {
                return Err(format!("undefined column name '{number}'"));
            }
            ColumnRef::Name(name) => match options.names.iter().position(|known| known == name) {
                Some(index) => index,
                None => return Err(format!("undefined column name '{name}'")),
            },
        };
        indexes.push(index);
    }
    Ok(indexes)
}

/// The `-J` output: one object per data row, keyed by the `-N` names.
fn json_table(rows: &[Vec<Vec<u8>>], visible: &[usize], options: &Options) -> Vec<u8> {
    let mut out = format!("{{\n   \"{}\": [\n", options.table_name).into_bytes();
    for (row_index, row) in rows.iter().enumerate() {
        if row_index == 0 {
            out.extend_from_slice(b"      ");
        }
        out.extend_from_slice(b"{\n");
        let mut fields = Vec::new();
        for &index in visible {
            let Some(name) = options.names.get(index) else {
                continue;
            };
            let cell = row.get(index).map_or(&[][..], Vec::as_slice);
            let mut field = b"         \"".to_vec();
            field.extend_from_slice(name.as_bytes());
            field.extend_from_slice(b"\": \"");
            field.extend(json_escape(cell));
            field.push(b'"');
            fields.push(field);
        }
        out.extend(fields.join(&b",\n"[..]));
        out.extend_from_slice(b"\n      }");
        if row_index + 1 < rows.len() {
            out.push(b',');
        }
    }
    out.extend_from_slice(b"\n   ]\n}\n");
    out
}

fn json_escape(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &byte in text {
        match byte {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\r' => out.extend_from_slice(b"\\r"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            0..=0x1f => out.extend_from_slice(format!("\\u{:04x}", byte).as_bytes()),
            _ => out.push(byte),
        }
    }
    out
}

/// Display width of `text` with tabs expanded to 8-column stops and wide characters as 2.
fn expanded_width(text: &[u8]) -> usize {
    let mut col = 0;
    push_expanded(&mut Vec::new(), &mut col, text);
    col
}

/// Display width of a table cell (tabs as in [`expanded_width`]).
fn display_width(text: &[u8]) -> usize {
    expanded_width(text)
}

/// Appends `text` and advances `col` by its display width.
fn push_expanded(out: &mut Vec<u8>, col: &mut usize, text: &[u8]) {
    let decoded = String::from_utf8_lossy(text);
    for ch in decoded.chars() {
        if ch == '\t' {
            *col = next_stop(*col);
        } else {
            *col += char_width(ch);
        }
    }
    out.extend_from_slice(text);
}

fn char_width(ch: char) -> usize {
    let code = ch as u32;
    let wide = matches!(
        code,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD
    );
    if wide {
        2
    } else {
        1
    }
}

fn next_stop(col: usize) -> usize {
    (col / 8 + 1) * 8
}

fn round_to_tab(value: usize) -> usize {
    value.div_ceil(8) * 8
}

/// Pads with tabs (and spaces when no tab stop fits) up to the column `target`.
fn pad_to(out: &mut Vec<u8>, col: &mut usize, target: usize) {
    while next_stop(*col) <= target {
        out.push(b'\t');
        *col = next_stop(*col);
    }
    while *col < target {
        out.push(b' ');
        *col += 1;
    }
}

fn write_help() -> i32 {
    let mut out = stdout_writer();
    if out.write_all(HELP.as_bytes()).is_err() {
        return EXIT_OK;
    }
    finish_output(CMD, &mut out)
}

const HELP: &str = "\
Usage: column [options] [file ...]

Columnate lists.

Options:
 -t, --table                      create a table
 -N, --table-columns <names>      comma separated columns names
 -l, --table-columns-limit <num>  maximal number of input columns
 -L, --keep-empty-lines           don't ignore empty lines
 -J, --json                       use JSON output format for table
 -n, --table-name <name>          table name for JSON output
 -H, --table-hide <columns>       don't print the columns
 -R, --table-right <columns>      right align text in these columns
 -s, --separator <string>         possible table delimiters
 -o, --output-separator <string>  columns separator for table output (default is two spaces)
 -c, --output-width <width>       width of output in number of characters
 -x, --fillrows                   fill rows before columns

 -h, --help                       display this help
 -V, --version                    display version

For more details see column(1).
";

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(items: &[&str], width: usize, fillrows: bool) -> String {
        let items: Vec<&[u8]> = items.iter().map(|item| item.as_bytes()).collect();
        String::from_utf8(fill_columns(&items, width, fillrows)).unwrap()
    }

    fn table(lines: &[&str], configure: impl FnOnce(&mut Options)) -> String {
        let mut options = Options {
            table: true,
            json: false,
            separator: None,
            output_separator: b"  ".to_vec(),
            width: None,
            fillrows: false,
            names: Vec::new(),
            right: Vec::new(),
            hidden: Vec::new(),
            limit: None,
            keep_empty: false,
            table_name: "table".to_string(),
        };
        configure(&mut options);
        let lines: Vec<Vec<u8>> = lines.iter().map(|line| line.as_bytes().to_vec()).collect();
        String::from_utf8(format_table(&lines, &options).unwrap()).unwrap()
    }

    #[test]
    fn fill_uses_tab_stops_and_fills_down_columns() {
        assert_eq!(
            fill(&["a", "bb", "ccc", "dddd"], 80, false),
            "a\tbb\tccc\tdddd\n"
        );
        assert_eq!(
            fill(&["a", "bb", "ccc", "dddd", "e", "ff", "g"], 20, false),
            "a\te\nbb\tff\nccc\tg\ndddd\n"
        );
    }

    #[test]
    fn fill_width_rounds_the_column_to_a_tab_stop() {
        assert_eq!(
            fill(&["abcdefg", "abcdefg", "abcdefg"], 16, false),
            "abcdefg\tabcdefg\nabcdefg\n"
        );
        assert_eq!(
            fill(&["abcdefg", "abcdefg", "abcdefg"], 8, false),
            "abcdefg\nabcdefg\nabcdefg\n"
        );
    }

    #[test]
    fn fillrows_lays_items_out_across_rows() {
        assert_eq!(fill(&["a", "b", "c"], 16, true), "a\tb\nc\n");
    }

    #[test]
    fn table_aligns_columns_with_the_output_separator() {
        assert_eq!(
            table(&["name age", "alice 30"], |_| {}),
            "name   age\nalice  30\n"
        );
    }

    #[test]
    fn table_right_alignment_pads_on_the_left() {
        assert_eq!(
            table(&["name age", "alice 30", "bob 4"], |options| options
                .right =
                vec![ColumnRef::Number(2)]),
            "name   age\nalice   30\nbob      4\n"
        );
    }

    #[test]
    fn table_hides_named_columns_and_keeps_empty_fields_with_separator() {
        let names = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        assert_eq!(
            table(&["1,2,3"], |options| {
                options.separator = Some(b",".to_vec());
                options.names = names.clone();
                options.hidden = vec![ColumnRef::Name("y".to_string())];
            }),
            "x  z\n1  3\n"
        );
        assert_eq!(
            table(&["a,,c"], |options| options.separator = Some(b",".to_vec())),
            "a    c\n"
        );
    }

    #[test]
    fn json_wraps_rows_in_a_table_array() {
        let rows = vec![
            vec![b"name".to_vec(), b"age".to_vec()],
            vec![b"bob".to_vec(), b"4".to_vec()],
        ];
        let options = Options {
            table: true,
            json: true,
            separator: None,
            output_separator: b"  ".to_vec(),
            width: None,
            fillrows: false,
            names: vec!["n".to_string(), "a".to_string()],
            right: Vec::new(),
            hidden: Vec::new(),
            limit: None,
            keep_empty: false,
            table_name: "table".to_string(),
        };
        let text = String::from_utf8(json_table(&rows, &[0, 1], &options)).unwrap();
        assert_eq!(
            text,
            "{\n   \"table\": [\n      {\n         \"n\": \"name\",\n         \"a\": \"age\"\n      },{\n         \"n\": \"bob\",\n         \"a\": \"4\"\n      }\n   ]\n}\n"
        );
    }

    #[test]
    fn wide_characters_count_as_two_columns() {
        assert_eq!(display_width("日本".as_bytes()), 4);
        assert_eq!(display_width(b"a\tb"), 9);
    }

    #[test]
    fn split_collapses_whitespace_unless_a_separator_is_given() {
        let options = Options {
            table: true,
            json: false,
            separator: None,
            output_separator: b"  ".to_vec(),
            width: None,
            fillrows: false,
            names: Vec::new(),
            right: Vec::new(),
            hidden: Vec::new(),
            limit: None,
            keep_empty: false,
            table_name: "table".to_string(),
        };
        assert_eq!(
            split_cells(b"  a   b  ", &options),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
    }
}
