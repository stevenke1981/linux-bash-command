//! `pr` option settings: page geometry, columns, numbering, headers and tab handling.

use crate::common::args::Item;

use super::columns::Separator;
use super::{DEFAULT_DATE, DEFAULT_LENGTH, DEFAULT_WIDTH};

/// Line numbering settings (`-n`).
#[derive(Clone, Debug)]
pub(super) struct Numbering {
    pub(super) separator: Vec<u8>,
    pub(super) width: usize,
}

/// Everything that shapes the output.
#[derive(Clone, Debug)]
pub(super) struct Settings {
    pub(super) length: usize,
    pub(super) width: usize,
    pub(super) margin: usize,
    pub(super) omit_header: bool,
    pub(super) omit_pages: bool,
    pub(super) double: bool,
    pub(super) columns: usize,
    pub(super) across: bool,
    pub(super) merge: bool,
    pub(super) join: bool,
    pub(super) separator: Separator,
    pub(super) numbering: Option<Numbering>,
    pub(super) first_number: i64,
    pub(super) expand: Option<(u8, usize)>,
    pub(super) form_feed: bool,
    pub(super) header: Option<String>,
    pub(super) date_format: String,
    pub(super) first_page: usize,
    pub(super) last_page: usize,
    pub(super) nonprint: bool,
    pub(super) control: bool,
    pub(super) unexpand: Option<(u8, usize)>,
}

/// `-COLUMN` (digits after a dash) is the same as `--columns=COLUMN`.
pub(super) fn column_option(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if arg.len() > 1 && arg.starts_with('-') && arg[1..].bytes().all(|b| b.is_ascii_digit())
            {
                format!("--columns={}", &arg[1..])
            } else {
                arg.clone()
            }
        })
        .collect()
}

/// `+FIRST` or `+FIRST:LAST` page ranges.
pub(super) fn page_range(text: &str) -> Option<(usize, usize)> {
    let body = text.strip_prefix('+')?;
    let (first, last) = match body.split_once(':') {
        Some((first, last)) => (first, Some(last)),
        None => (body, None),
    };
    let first: usize = first.parse().ok()?;
    let last = match last {
        Some(text) => text.parse().ok()?,
        None => usize::MAX,
    };
    Some((first.max(1), last))
}

pub(super) fn build_settings(items: &[Item]) -> Result<Settings, String> {
    let mut settings = Settings {
        length: DEFAULT_LENGTH,
        width: DEFAULT_WIDTH,
        margin: 0,
        omit_header: false,
        omit_pages: false,
        double: false,
        columns: 1,
        across: false,
        merge: false,
        join: false,
        separator: Separator::Aligned,
        numbering: None,
        first_number: 1,
        expand: None,
        form_feed: false,
        header: None,
        date_format: DEFAULT_DATE.to_owned(),
        first_page: 1,
        last_page: usize::MAX,
        nonprint: false,
        control: false,
        unexpand: None,
    };
    let mut plain: Option<Vec<u8>> = None;
    for item in items {
        let Item::Opt { id, value } = item else {
            continue;
        };
        let value = value.as_deref().unwrap_or("");
        match *id {
            "length" => settings.length = positive(value, "page length")?,
            "width" | "page_width" => settings.width = positive(value, "page width")?,
            "margin" => settings.margin = count(value, "margin")?,
            "header" => settings.header = Some(value.to_owned()),
            "omit_header" => settings.omit_header = true,
            "omit_pages" => {
                settings.omit_header = true;
                settings.omit_pages = true;
            }
            "double" => settings.double = true,
            "number" => settings.numbering = Some(parse_numbering(value)?),
            "first_number" => settings.first_number = count(value, "line number")? as i64,
            "plain_sep" => {
                plain = Some(if value.is_empty() {
                    b"\t".to_vec()
                } else {
                    value.as_bytes().to_vec()
                });
            }
            "string_sep" => {
                settings.separator = Separator::AlignedString(if value.is_empty() {
                    b"\t".to_vec()
                } else {
                    value.as_bytes().to_vec()
                });
            }
            "expand" => settings.expand = Some(parse_tabs(value)?),
            "unexpand" => settings.unexpand = Some(parse_tabs(value)?),
            "across" => settings.across = true,
            "merge" => settings.merge = true,
            "join" => settings.join = true,
            "columns" => settings.columns = positive(value, "column count")?,
            "form" | "form_f" => settings.form_feed = true,
            "date" => settings.date_format = value.to_owned(),
            "nonprint" => settings.nonprint = true,
            "control" => settings.control = true,
            _ => {}
        }
    }
    if let Some(text) = plain {
        settings.separator = Separator::Plain(text);
    }
    if settings.join {
        settings.separator = Separator::Plain(b"\t".to_vec());
    }
    if settings.merge && settings.columns == 1 {
        settings.columns = 2;
    }
    Ok(settings)
}

pub(super) fn positive(value: &str, what: &str) -> Result<usize, String> {
    match value.parse::<usize>() {
        Ok(number) if number > 0 => Ok(number),
        _ => Err(format!("invalid {what}: '{value}'")),
    }
}

pub(super) fn count(value: &str, what: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("invalid {what}: '{value}'"))
}

/// `-n[SEP[DIGITS]]`: a non-digit first character is the separator.
pub(super) fn parse_numbering(value: &str) -> Result<Numbering, String> {
    let (separator, digits) = match value.chars().next() {
        Some(first) if !first.is_ascii_digit() => {
            (first.to_string().into_bytes(), &value[first.len_utf8()..])
        }
        _ => (b"\t".to_vec(), value),
    };
    let width = if digits.is_empty() {
        5
    } else {
        count(digits, "line number width")?
    };
    Ok(Numbering { separator, width })
}

/// `-e` / `-i`: `[CHAR[WIDTH]]`, with a tab width default of 8.
pub(super) fn parse_tabs(value: &str) -> Result<(u8, usize), String> {
    let mut chars = value.chars();
    let character = match chars.next() {
        Some(ch) if ch.is_ascii() && !ch.is_ascii_digit() => ch as u8,
        _ => b'\t',
    };
    let rest = if value.is_empty() || value.starts_with(|c: char| c.is_ascii_digit()) {
        value
    } else {
        &value[1..]
    };
    let width = if rest.is_empty() {
        8
    } else {
        count(rest, "tab width")?
    };
    Ok((character, width.max(1)))
}
