use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, escape, io,
};
use std::io::{BufRead, Write};

const HELP: &str = "Usage: join [OPTION]... FILE1 FILE2\nFor each pair of input lines with identical join fields, write a line to standard output.\n  -a FILENUM              also print unpairable lines from file FILENUM\n  -e EMPTY                replace missing input fields with EMPTY\n  -i, --ignore-case       ignore differences in case when comparing fields\n  -j FIELD                equivalent to '-1 FIELD -2 FIELD'\n  -o FORMAT               obey FORMAT while constructing output lines\n  -t CHAR                 use CHAR as field separator; '' means whole lines, \\0 means NUL\n  -v FILENUM              like -a FILENUM, but suppress joined output lines\n  -1 FIELD                join on this field of file 1\n  -2 FIELD                join on this field of file 2\n      --check-order       check that input is correctly sorted\n      --nocheck-order     do not check input order\n      --header            treat first line of each file as a header line\n  -z, --zero-terminated   line delimiter is NUL, not newline\n      --help              display this help\n      --version           output version information\nExit status: 0 success, 1 if an error occurred.\n";

mod group;
use group::Group;

#[derive(Clone)]
enum FieldMode {
    Blanks,
    Delimiter(u8),
    WholeLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FormatField {
    file: u8,
    field: usize,
}

struct Options {
    field_mode: FieldMode,
    delimiter: u8,
    join_fields: [usize; 2],
    only_unpaired: [bool; 2],
    include_unpaired: [bool; 2],
    ignore_case: bool,
    empty: Option<Vec<u8>>,
    output: Option<Vec<FormatField>>,
    auto_output: bool,
    header: bool,
    check_order: Option<bool>,
    files: Vec<String>,
}

#[derive(Clone)]
struct Record {
    raw: Vec<u8>,
    fields: Vec<Vec<u8>>,
    key: Vec<u8>,
    line_number: u64,
    disorder: bool,
}

impl Record {
    fn new(raw: Vec<u8>, line_number: u64, options: &Options, side: usize) -> Self {
        let fields = split_fields(&raw, &options.field_mode);
        let source = fields
            .get(options.join_fields[side])
            .cloned()
            .unwrap_or_default();
        let key = normalize_key(&source, options.ignore_case);
        Self {
            raw,
            fields,
            key,
            line_number,
            disorder: false,
        }
    }
}

fn normalize_key(key: &[u8], ignore_case: bool) -> Vec<u8> {
    if ignore_case {
        key.iter().map(u8::to_ascii_lowercase).collect()
    } else {
        key.to_vec()
    }
}

fn split_fields(raw: &[u8], mode: &FieldMode) -> Vec<Vec<u8>> {
    match mode {
        FieldMode::WholeLine => vec![raw.to_vec()],
        FieldMode::Delimiter(delimiter) => raw
            .split(|byte| byte == delimiter)
            .map(<[u8]>::to_vec)
            .collect(),
        FieldMode::Blanks => raw
            .split(|byte| matches!(byte, b' ' | b'\t'))
            .filter(|field| !field.is_empty())
            .map(<[u8]>::to_vec)
            .collect(),
    }
}

fn parse_field_number(value: &str) -> Result<usize, String> {
    let number = value
        .parse::<usize>()
        .map_err(|_| format!("invalid field number: '{value}'"))?;
    if number == 0 {
        return Err(format!("invalid field number: '{value}'"));
    }
    Ok(number - 1)
}

fn parse_output(value: &str) -> Result<(Vec<FormatField>, bool), String> {
    if value == "auto" {
        return Ok((Vec::new(), true));
    }
    let mut fields = Vec::new();
    for part in value.split(',') {
        if part == "0" {
            fields.push(FormatField { file: 0, field: 0 });
            continue;
        }
        let Some((file, field)) = part.split_once('.') else {
            return Err(format!("invalid field specifier: '{part}'"));
        };
        let file = file
            .parse::<u8>()
            .ok()
            .filter(|file| matches!(file, 1 | 2))
            .ok_or_else(|| format!("invalid field specifier: '{part}'"))?;
        fields.push(FormatField {
            file,
            field: parse_field_number(field)?,
        });
    }
    if fields.is_empty() {
        return Err("empty output format".into());
    }
    Ok((fields, false))
}

fn options(arguments: &[String]) -> Result<Options, i32> {
    let specs = [
        OptSpec {
            id: "include-one",
            short: 'a',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec {
            id: "empty",
            short: 'e',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec::flag("ignore-case", 'i'),
        OptSpec {
            id: "join-field",
            short: 'j',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec {
            id: "output",
            short: 'o',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec {
            id: "separator",
            short: 't',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec {
            id: "only-one",
            short: 'v',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec {
            id: "field-one",
            short: '1',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec {
            id: "field-two",
            short: '2',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec::flag("check-order", '\0'),
        OptSpec::flag("nocheck-order", '\0'),
        OptSpec::flag("header", '\0'),
        OptSpec::flag("zero-terminated", 'z'),
    ];
    let items = args::parse_command("join", arguments, HELP, &specs, true)?;
    let mut result = Options {
        field_mode: FieldMode::Blanks,
        delimiter: b'\n',
        join_fields: [0, 0],
        only_unpaired: [false; 2],
        include_unpaired: [false; 2],
        ignore_case: false,
        empty: None,
        output: None,
        auto_output: false,
        header: false,
        check_order: None,
        files: Vec::new(),
    };
    for item in items {
        match item {
            Item::Operand(path) => result.files.push(path),
            Item::Opt(id, value) => {
                let value = value.unwrap_or_default();
                match id {
                    "include-one" => {
                        let side = parse_side(&value).map_err(|e| error::usage("join", e, 1))?;
                        result.include_unpaired[side] = true;
                    }
                    "empty" => result.empty = Some(value.into_bytes()),
                    "ignore-case" => result.ignore_case = true,
                    "join-field" => {
                        let field =
                            parse_field_number(&value).map_err(|e| error::usage("join", e, 1))?;
                        result.join_fields = [field; 2];
                    }
                    "output" => {
                        let (format, auto) =
                            parse_output(&value).map_err(|e| error::usage("join", e, 1))?;
                        result.output = Some(format);
                        result.auto_output = auto;
                    }
                    "separator" => {
                        if value.is_empty() {
                            result.field_mode = FieldMode::WholeLine;
                        } else if value == "\\0" {
                            result.field_mode = FieldMode::Delimiter(0);
                        } else if value.len() == 1 {
                            result.field_mode = FieldMode::Delimiter(value.as_bytes()[0]);
                        } else {
                            return Err(error::usage("join", "separator must be a single byte", 1));
                        }
                    }
                    "only-one" => {
                        let side = parse_side(&value).map_err(|e| error::usage("join", e, 1))?;
                        result.only_unpaired[side] = true;
                        result.include_unpaired[side] = true;
                    }
                    "field-one" => {
                        result.join_fields[0] =
                            parse_field_number(&value).map_err(|e| error::usage("join", e, 1))?;
                    }
                    "field-two" => {
                        result.join_fields[1] =
                            parse_field_number(&value).map_err(|e| error::usage("join", e, 1))?;
                    }
                    "check-order" => result.check_order = Some(true),
                    "nocheck-order" => result.check_order = Some(false),
                    "header" => result.header = true,
                    "zero-terminated" => result.delimiter = 0,
                    _ => unreachable!(),
                }
            }
        }
    }
    if result.files.len() != 2 {
        return Err(error::usage(
            "join",
            if result.files.len() < 2 {
                "missing operand"
            } else {
                "extra operand"
            },
            1,
        ));
    }
    let stdin_names = ["-", "/dev/stdin"];
    if stdin_names.contains(&result.files[0].as_str())
        && stdin_names.contains(&result.files[1].as_str())
    {
        return Err(error::usage(
            "join",
            "both input files cannot be standard input",
            1,
        ));
    }
    Ok(result)
}

fn parse_side(value: &str) -> Result<usize, String> {
    match value {
        "1" => Ok(0),
        "2" => Ok(1),
        _ => Err(format!("invalid file number: '{value}'")),
    }
}

pub fn run(arguments: &[String]) -> i32 {
    match options(arguments) {
        Ok(options) => run_join(options),
        Err(code) => code,
    }
}

struct Reader {
    name: String,
    side: usize,
    input: Box<dyn BufRead>,
    line_number: u64,
    previous_key: Option<Vec<u8>>,
    current: Option<Record>,
}

impl Reader {
    fn open(name: &str, side: usize) -> Result<Self, String> {
        let input = io::input(name).map_err(|e| format!("{name}: {}", error::io_text(&e)))?;
        Ok(Self {
            name: name.to_owned(),
            side,
            input,
            line_number: 0,
            previous_key: None,
            current: None,
        })
    }

    fn advance(&mut self, options: &Options, check_now: bool) -> Result<(), String> {
        let mut raw = Vec::new();
        let count = self
            .input
            .read_until(options.delimiter, &mut raw)
            .map_err(|e| format!("{}: {}", self.name, error::io_text(&e)))?;
        if count == 0 {
            self.current = None;
            return Ok(());
        }
        if raw.last() == Some(&options.delimiter) {
            raw.pop();
        }
        self.line_number += 1;
        let mut record = Record::new(raw, self.line_number, options, self.side);
        record.disorder = self
            .previous_key
            .as_ref()
            .is_some_and(|previous| previous > &record.key);
        if check_now && record.disorder {
            return Err(disorder_message(&self.name, &record));
        }
        self.previous_key = Some(record.key.clone());
        self.current = Some(record);
        Ok(())
    }

    fn take_group(&mut self, options: &Options) -> Result<Group, String> {
        let key = self.current.as_ref().unwrap().key.clone();
        let mut group = Group::new(self.side);
        loop {
            group
                .push(self.current.take().unwrap())
                .map_err(|e| format!("{}: {}", self.name, error::io_text(&e)))?;
            self.advance(options, options.check_order == Some(true))?;
            if self.current.as_ref().is_none_or(|record| record.key != key) {
                break;
            }
        }
        group
            .seal()
            .map_err(|e| format!("{}: {}", self.name, error::io_text(&e)))?;
        Ok(group)
    }
}

fn disorder_message(file: &str, record: &Record) -> String {
    let quoted = escape::quote_name(&record.raw);
    let line = String::from_utf8_lossy(&quoted);
    format!("{file}:{}: is not sorted: {line}", record.line_number)
}

fn output_separator(options: &Options) -> u8 {
    match options.field_mode {
        FieldMode::Blanks | FieldMode::WholeLine => b' ',
        FieldMode::Delimiter(byte) => byte,
    }
}

fn auto_format(records: [&Option<Record>; 2], options: &Options) -> Vec<FormatField> {
    let mut format = vec![FormatField { file: 0, field: 0 }];
    for (side, record) in records.into_iter().enumerate() {
        let count = record.as_ref().map_or(0, |record| record.fields.len());
        for field in 0..count {
            if field != options.join_fields[side] {
                format.push(FormatField {
                    file: (side + 1) as u8,
                    field,
                });
            }
        }
    }
    format
}

fn format_row(
    left: Option<&Record>,
    right: Option<&Record>,
    options: &Options,
    format: &[FormatField],
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let separator = output_separator(options);
    if options.output.is_none() {
        if let (Some(left), Some(right)) = (left, right) {
            let key = left
                .fields
                .get(options.join_fields[0])
                .or_else(|| right.fields.get(options.join_fields[1]));
            let mut fields = Vec::new();
            if let Some(key) = key {
                fields.push(key);
            }
            fields.extend(
                left.fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| (i != options.join_fields[0]).then_some(f)),
            );
            fields.extend(
                right
                    .fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| (i != options.join_fields[1]).then_some(f)),
            );
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    out.write_all(&[separator])?;
                }
                out.write_all(field)?;
            }
            return out.write_all(&[options.delimiter]);
        }
        let record = left.or(right).unwrap();
        let fields = &record.fields;
        for (index, field) in fields.iter().enumerate() {
            if index != 0 {
                out.write_all(&[separator])?;
            }
            out.write_all(field)?;
        }
        return out.write_all(&[options.delimiter]);
    }
    for (index, spec) in format.iter().enumerate() {
        if index != 0 {
            out.write_all(&[separator])?;
        }
        let value = if spec.file == 0 {
            left.and_then(|record| record.fields.get(options.join_fields[0]))
                .or_else(|| right.and_then(|record| record.fields.get(options.join_fields[1])))
        } else if spec.file == 1 {
            left.and_then(|record| record.fields.get(spec.field))
        } else {
            right.and_then(|record| record.fields.get(spec.field))
        };
        if let Some(value) = value {
            out.write_all(value)?;
        } else if let Some(empty) = &options.empty {
            out.write_all(empty)?;
        }
    }
    out.write_all(&[options.delimiter])
}

fn emit_unpaired(
    side: usize,
    group: &Group,
    reader_name: &str,
    options: &Options,
    format: &[FormatField],
    out: &mut dyn Write,
    check_disorder: bool,
) -> std::io::Result<bool> {
    let selected = options.include_unpaired[side];
    let mut disorder = false;
    group.visit(options, |record| {
        if check_disorder && options.check_order.is_none() && record.disorder {
            eprintln!("join: {}", disorder_message(reader_name, &record));
            disorder = true;
        }
        if selected {
            if side == 0 {
                format_row(Some(&record), None, options, format, out)?;
            } else {
                format_row(None, Some(&record), options, format, out)?;
            }
        }
        Ok(())
    })?;
    Ok(disorder)
}

fn run_join(options: Options) -> i32 {
    let mut left = match Reader::open(&options.files[0], 0) {
        Ok(reader) => reader,
        Err(message) => {
            eprintln!("join: {message}");
            return 1;
        }
    };
    let mut right = match Reader::open(&options.files[1], 1) {
        Ok(reader) => reader,
        Err(message) => {
            eprintln!("join: {message}");
            return 1;
        }
    };
    let strict = options.check_order == Some(true) && !options.header;
    if let Err(message) = left
        .advance(&options, strict)
        .and_then(|()| right.advance(&options, strict))
    {
        eprintln!("join: {message}");
        return 1;
    }
    let mut header = None;
    if options.header {
        header = Some((left.current.take(), right.current.take()));
        left.previous_key = None;
        right.previous_key = None;
        if let Err(message) = left
            .advance(&options, options.check_order == Some(true))
            .and_then(|()| right.advance(&options, options.check_order == Some(true)))
        {
            eprintln!("join: {message}");
            return 1;
        }
    }
    let format = if options.auto_output {
        let records = if let Some((left_header, right_header)) = &header {
            [left_header, right_header]
        } else {
            [&left.current, &right.current]
        };
        auto_format(records, &options)
    } else {
        options.output.clone().unwrap_or_default()
    };
    let mut failed = false;
    let mut disorder = false;
    let status = io::output("join", |out| {
        if let Some((left_header, right_header)) = &header {
            if let (Some(left), Some(right)) = (left_header, right_header) {
                format_row(Some(left), Some(right), &options, &format, out)?;
            } else if let Some(record) = left_header.as_ref().or(right_header.as_ref()) {
                out.write_all(&record.raw)?;
                out.write_all(&[options.delimiter])?;
            }
        }
        while !failed && (left.current.is_some() || right.current.is_some()) {
            match (&left.current, &right.current) {
                (None, Some(_)) => {
                    let group = match right.take_group(&options) {
                        Ok(group) => group,
                        Err(message) => {
                            eprintln!("join: {message}");
                            failed = true;
                            break;
                        }
                    };
                    disorder |=
                        emit_unpaired(1, &group, &right.name, &options, &format, out, false)?;
                }
                (Some(_), None) => {
                    let group = match left.take_group(&options) {
                        Ok(group) => group,
                        Err(message) => {
                            eprintln!("join: {message}");
                            failed = true;
                            break;
                        }
                    };
                    disorder |=
                        emit_unpaired(0, &group, &left.name, &options, &format, out, false)?;
                }
                (Some(a), Some(b)) => match a.key.cmp(&b.key) {
                    std::cmp::Ordering::Equal => {
                        let left_group = match left.take_group(&options) {
                            Ok(group) => group,
                            Err(message) => {
                                eprintln!("join: {message}");
                                failed = true;
                                break;
                            }
                        };
                        let right_group = match right.take_group(&options) {
                            Ok(group) => group,
                            Err(message) => {
                                eprintln!("join: {message}");
                                failed = true;
                                break;
                            }
                        };
                        if !options.only_unpaired.iter().any(|only| *only) {
                            left_group.visit(&options, |left_record| {
                                right_group.visit(&options, |right_record| {
                                    format_row(
                                        Some(&left_record),
                                        Some(&right_record),
                                        &options,
                                        &format,
                                        out,
                                    )
                                })
                            })?;
                        }
                    }
                    std::cmp::Ordering::Less => {
                        let group = match left.take_group(&options) {
                            Ok(group) => group,
                            Err(message) => {
                                eprintln!("join: {message}");
                                failed = true;
                                break;
                            }
                        };
                        disorder |=
                            emit_unpaired(0, &group, &left.name, &options, &format, out, true)?;
                    }
                    std::cmp::Ordering::Greater => {
                        let group = match right.take_group(&options) {
                            Ok(group) => group,
                            Err(message) => {
                                eprintln!("join: {message}");
                                failed = true;
                                break;
                            }
                        };
                        disorder |=
                            emit_unpaired(1, &group, &right.name, &options, &format, out, true)?;
                    }
                },
                (None, None) => break,
            }
        }
        Ok(())
    });
    if status != 0 {
        status
    } else {
        if disorder && options.check_order.is_none() {
            eprintln!("join: input is not in sorted order");
        }
        i32::from(failed || disorder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_fields_ignore_leading_blanks_and_coalesce_runs() {
        assert_eq!(
            split_fields(b"  alpha\t beta   gamma", &FieldMode::Blanks),
            [b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()]
        );
    }

    #[test]
    fn delimiter_fields_preserve_empty_fields_and_whole_line_mode() {
        assert_eq!(
            split_fields(b"a,,b,", &FieldMode::Delimiter(b',')),
            [b"a".to_vec(), Vec::new(), b"b".to_vec(), Vec::new()]
        );
        assert_eq!(split_fields(b"a,b", &FieldMode::WholeLine), [b"a,b"]);
    }

    #[test]
    fn format_parser_accepts_auto_join_key_and_numbered_fields() {
        assert_eq!(parse_output("auto").unwrap(), (Vec::new(), true));
        assert_eq!(
            parse_output("0,1.2,2.3").unwrap(),
            (
                vec![
                    FormatField { file: 0, field: 0 },
                    FormatField { file: 1, field: 1 },
                    FormatField { file: 2, field: 2 }
                ],
                false
            )
        );
        assert!(parse_output("1.0").is_err());
    }
}
