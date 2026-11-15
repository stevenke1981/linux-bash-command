use crate::common::{
    args::{self, Item, OptSpec},
    color::visible_width,
    error,
    external::TemporaryFile,
    io,
};
use std::io::{Read, Seek, SeekFrom, Write};
const HELP:&str="Usage: column [OPTION]... [FILE]...\nArrange input in columns or a table.\n  -t, --table                 create a table\n  -s, --separator=SEP         input separator characters\n  -o, --output-separator=STR  separate table columns (default two spaces)\n  -c, --output-width=N        output width (default COLUMNS or 80)\n  -x, --fillrows              fill rows before columns\n  -N, --table-columns=NAMES  comma-separated column headers\n  -R, --table-right=COLS      right-align named or numbered columns\n  -H, --table-hide=COLS       hide named or numbered columns\n  -l, --table-columns-limit=N limit the number of parsed columns\n  -L, --keep-empty-lines     retain empty input lines\n  -n, --table-name=NAME      name the JSON table\n  -J, --json                 print a JSON table (requires column names)\n      --help                display help\n      --version             output version\nExit status: 0 success, 1 failure.\n";
struct Options {
    table: bool,
    separator: Option<Vec<u8>>,
    output: Vec<u8>,
    width: usize,
    across: bool,
    names: Vec<String>,
    right: Vec<String>,
    hide: Vec<String>,
    limit: Option<usize>,
    empty: bool,
    json: bool,
    name: String,
}
fn parse(arguments: &[String]) -> Result<(Options, Vec<String>), String> {
    let specs = [
        OptSpec::flag("table", 't'),
        OptSpec::value("separator", 's'),
        OptSpec::value("output-separator", 'o'),
        OptSpec::value("output-width", 'c'),
        OptSpec::flag("fillrows", 'x'),
        OptSpec::value("table-columns", 'N'),
        OptSpec::value("table-right", 'R'),
        OptSpec::value("table-hide", 'H'),
        OptSpec::value("table-columns-limit", 'l'),
        OptSpec::flag("keep-empty-lines", 'L'),
        OptSpec::value("table-name", 'n'),
        OptSpec::flag("json", 'J'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let mut o = Options {
        table: false,
        separator: None,
        output: b"  ".to_vec(),
        width: std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|n| *n > 0)
            .unwrap_or(80),
        across: false,
        names: vec![],
        right: vec![],
        hide: vec![],
        limit: None,
        empty: false,
        json: false,
        name: "table".into(),
    };
    let mut files = vec![];
    for item in args::parse(arguments, &specs, true)? {
        match item {
            Item::Operand(s) => files.push(s),
            Item::Opt(id, v) => match id {
                "table" => o.table = true,
                "separator" => o.separator = Some(v.unwrap().into_bytes()),
                "output-separator" => o.output = v.unwrap().into_bytes(),
                "fillrows" => o.across = true,
                "output-width" => o.width = positive(&v.unwrap())?,
                "table-columns-limit" => o.limit = Some(positive(&v.unwrap())?),
                "table-columns" => {
                    o.names = v.unwrap().split(',').map(str::to_owned).collect();
                }
                "table-right" => o.right = v.unwrap().split(',').map(str::to_owned).collect(),
                "table-hide" => o.hide = v.unwrap().split(',').map(str::to_owned).collect(),
                "keep-empty-lines" => o.empty = true,
                "json" => {
                    o.json = true;
                    o.table = true;
                }
                "table-name" => o.name = v.unwrap(),
                "help" | "version" => return Ok((o, vec![format!("--{id}")])),
                _ => unreachable!(),
            },
        }
    }
    if o.json && o.names.is_empty() {
        return Err("JSON output requires column names".into());
    }
    if !o.table
        && (!o.names.is_empty()
            || !o.right.is_empty()
            || !o.hide.is_empty()
            || o.limit.is_some()
            || o.name != "table")
    {
        return Err("option --table required for all --table-*".into());
    }
    for name in o.right.iter().chain(&o.hide) {
        if name.parse::<usize>().ok().filter(|n| *n > 0).is_none() && !o.names.contains(name) {
            return Err(format!("undefined column name: {name}"));
        }
    }
    Ok((o, files))
}
fn positive(s: &str) -> Result<usize, String> {
    s.parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid number: '{s}'"))
}
fn width(s: &[u8]) -> usize {
    visible_width(&String::from_utf8_lossy(s))
}
fn fields<'a>(line: &'a [u8], o: &Options) -> Vec<&'a [u8]> {
    if line.is_empty() {
        return vec![];
    }
    if let Some(separator) = &o.separator {
        if let Some(limit) = o.limit {
            line.splitn(limit, |b| separator.contains(b)).collect()
        } else {
            line.split(|b| separator.contains(b)).collect()
        }
    } else {
        let mut parts = Vec::new();
        let mut start = 0;
        while start < line.len() {
            while start < line.len() && line[start].is_ascii_whitespace() {
                start += 1;
            }
            if start == line.len() {
                break;
            }
            let end = if o.limit == Some(parts.len() + 1) {
                line.len()
            } else {
                start
                    + line[start..]
                        .iter()
                        .position(u8::is_ascii_whitespace)
                        .unwrap_or(line.len() - start)
            };
            parts.push(&line[start..end]);
            start = end;
        }
        parts
    }
}
struct Input {
    file: TemporaryFile,
    offsets: Vec<u64>,
    widths: Vec<usize>,
    max: usize,
}
impl Input {
    fn collect(files: &[String], o: &Options) -> std::io::Result<(Self, bool)> {
        let mut data = Self {
            file: TemporaryFile::new(&std::env::temp_dir())?,
            offsets: vec![],
            widths: vec![],
            max: 0,
        };
        let failed = io::for_inputs("column", files, |_, input| {
            let mut line = Vec::new();
            while input.read_until(b'\n', &mut line)? != 0 {
                if line.last() == Some(&b'\n') {
                    line.pop();
                }
                if o.empty || !line.iter().all(u8::is_ascii_whitespace) {
                    data.max = data.max.max(width(&line));
                    for (i, field) in fields(&line, o).iter().enumerate() {
                        if i >= data.widths.len() {
                            data.widths.resize(i + 1, 0);
                        }
                        data.widths[i] = data.widths[i].max(width(field));
                    }
                    data.offsets.push(data.file.file().stream_position()?);
                    data.file
                        .file()
                        .write_all(&(line.len() as u64).to_le_bytes())?;
                    data.file.file().write_all(&line)?;
                }
                line.clear();
            }
            Ok(())
        })?;
        Ok((data, failed))
    }
    fn line(&mut self, index: usize) -> std::io::Result<Vec<u8>> {
        self.file
            .file()
            .seek(SeekFrom::Start(self.offsets[index]))?;
        let mut len = [0; 8];
        self.file.file().read_exact(&mut len)?;
        let mut line = vec![0; u64::from_le_bytes(len) as usize];
        self.file.file().read_exact(&mut line)?;
        Ok(line)
    }
}
fn selected(list: &[String], index: usize, names: &[String]) -> bool {
    list.iter()
        .any(|s| s.parse::<usize>().ok() == Some(index + 1) || names.get(index) == Some(s))
}
fn row(
    out: &mut dyn Write,
    values: &[&[u8]],
    widths: &[usize],
    o: &Options,
) -> std::io::Result<()> {
    let columns: Vec<_> = (0..widths.len())
        .filter(|i| !selected(&o.hide, *i, &o.names))
        .collect();
    for (at, i) in columns.iter().enumerate() {
        let value = values.get(*i).copied().unwrap_or(b"");
        let padding = widths[*i].saturating_sub(width(value));
        let right = selected(&o.right, *i, &o.names);
        if right {
            for _ in 0..padding {
                out.write_all(b" ")?;
            }
        }
        out.write_all(value)?;
        if at + 1 < columns.len() {
            if !right {
                for _ in 0..padding {
                    out.write_all(b" ")?;
                }
            }
            out.write_all(&o.output)?;
        }
    }
    out.write_all(b"\n")
}
fn table(data: &mut Input, o: &Options, out: &mut dyn Write) -> std::io::Result<()> {
    data.widths.resize(data.widths.len().max(o.names.len()), 0);
    for (i, name) in o.names.iter().enumerate() {
        data.widths[i] = data.widths[i].max(width(name.as_bytes()));
    }
    if !o.names.is_empty() {
        row(
            out,
            &o.names.iter().map(|s| s.as_bytes()).collect::<Vec<_>>(),
            &data.widths,
            o,
        )?;
    }
    for i in 0..data.offsets.len() {
        let line = data.line(i)?;
        row(out, &fields(&line, o), &data.widths, o)?;
    }
    Ok(())
}
fn json(data: &mut Input, o: &Options, out: &mut dyn Write) -> std::io::Result<()> {
    out.write_all(b"{\n   ")?;
    serde_json::to_writer(&mut *out, &o.name)?;
    out.write_all(b": [\n")?;
    for i in 0..data.offsets.len() {
        let line = data.line(i)?;
        let values = fields(&line, o);
        let mut object = serde_json::Map::new();
        for (col, name) in o.names.iter().enumerate() {
            if !selected(&o.hide, col, &o.names) {
                object.insert(
                    name.to_ascii_lowercase(),
                    serde_json::Value::String(
                        String::from_utf8_lossy(values.get(col).copied().unwrap_or(b""))
                            .into_owned(),
                    ),
                );
            }
        }
        if i > 0 {
            out.write_all(b",\n")?;
        }
        out.write_all(b"      ")?;
        serde_json::to_writer(&mut *out, &object)?;
    }
    out.write_all(b"\n   ]\n}\n")
}
fn columns(data: &mut Input, o: &Options, out: &mut dyn Write) -> std::io::Result<()> {
    let cell = (data.max / 8 + 1).saturating_mul(8);
    let cols = (o.width / cell.max(1))
        .max(1)
        .min(data.offsets.len().max(1));
    let rows = data.offsets.len().div_ceil(cols);
    for r in 0..rows {
        let mut position = 0;
        for c in 0..cols {
            let index = if o.across { r * cols + c } else { c * rows + r };
            if index >= data.offsets.len() {
                break;
            }
            let line = data.line(index)?;
            out.write_all(&line)?;
            position += width(&line);
            let next = if o.across { index + 1 } else { index + rows };
            if c + 1 < cols && next < data.offsets.len() {
                let target = (c + 1) * cell;
                while position < target {
                    out.write_all(b"\t")?;
                    position = (position / 8 + 1) * 8;
                }
            }
        }
        out.write_all(b"\n")?;
    }
    Ok(())
}
pub fn run(arguments: &[String]) -> i32 {
    let (options, files) = match parse(arguments) {
        Ok(v) => v,
        Err(e) => return error::usage("column", e, 1),
    };
    if files.len() == 1 && matches!(files[0].as_str(), "--help" | "--version") {
        return args::help_version("column", &files, HELP, true).unwrap();
    }
    let (mut data, failed) = match Input::collect(&files, &options) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("column: {}", error::io_text(&e));
            return 1;
        }
    };
    io::output_status("column", i32::from(failed), 1, |out| {
        if options.json {
            json(&mut data, &options, out)
        } else if options.table {
            table(&mut data, &options, out)
        } else {
            columns(&mut data, &options, out)
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delimiters_preserve_empty_fields() {
        let (o, _) = parse(&["-s,".into()]).unwrap();
        assert_eq!(fields(b"a,,c", &o), [b"a".as_slice(), b"", b"c"]);
    }
    #[test]
    fn limit_keeps_the_remaining_text() {
        let (o, _) = parse(&["-t".into(), "-l2".into()]).unwrap();
        assert_eq!(fields(b"a b  c", &o), [b"a".as_slice(), b"b  c"]);
    }
    #[test]
    fn names_and_numbers_select_columns() {
        let names = vec!["NAME".into(), "COUNT".into()];
        assert!(selected(&["2".into()], 1, &names));
        assert!(selected(&["NAME".into()], 0, &names));
        assert_eq!(width("界".as_bytes()), 2);
    }
}
