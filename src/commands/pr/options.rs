use crate::common::args::{self, Item, OptSpec};
use crate::common::error::usage;
pub(super) const HELP:&str="Usage: pr [OPTION]... [FILE]...\nPaginate or columnate files for printing; - reads standard input.\n  +FIRST[:LAST], --pages=FIRST[:LAST] select pages\n  -COLUMN, --columns=N    number of columns\n  -a, --across            fill columns across\n  -c, --show-control-chars use hat notation for controls\n  -d, --double-space      double space body lines\n  -D, --date-format=FORMAT header timestamp format\n  -e, --expand-tabs[=CHAR[WIDTH]] expand input tabs\n  -F, -f, --form-feed     separate pages with form feeds\n  -h, --header=HEADER     replace filename in header\n  -i, --output-tabs[=CHAR[WIDTH]] compress output spaces\n  -J, --join-lines        join full lines without alignment\n  -l, --length=N          page length (default 66)\n  -m, --merge             print files in parallel columns\n  -n, --number-lines[=SEP[DIGITS]] number lines\n  -N, --first-line-number=N first printed line number\n  -o, --indent=N          left margin\n  -r, --no-file-warnings  suppress file open diagnostics\n  -s, --separator[=CHAR]  separate columns without alignment\n  -S, --sep-string[=STRING] column separator string\n  -t, --omit-header       omit headers and trailers\n  -T, --omit-pagination   omit all pagination\n  -v, --show-nonprinting  display controls as octal escapes\n  -w, --width=N           page width for multiple columns\n  -W, --page-width=N      page width and line truncation\n      --help             display this help\n      --version          output version information\n";
pub(super) struct Options {
    pub columns: usize,
    pub across: bool,
    pub controls: bool,
    pub octal: bool,
    pub double: bool,
    pub date: String,
    pub header: Option<String>,
    pub expand: Option<(u8, usize)>,
    pub tabs: Option<(u8, usize)>,
    pub form: bool,
    pub join: bool,
    pub length: usize,
    pub merge: bool,
    pub numbers: Option<(u8, usize)>,
    pub first_number: Option<i64>,
    pub margin: usize,
    pub quiet: bool,
    pub separator: Option<Vec<u8>>,
    pub unaligned: bool,
    pub omit: bool,
    pub no_pages: bool,
    pub width: usize,
    pub wide: bool,
    pub width_set: bool,
    pub first: usize,
    pub last: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            columns: 1,
            across: false,
            controls: false,
            octal: false,
            double: false,
            date: "%Y-%m-%d %H:%M".into(),
            header: None,
            expand: None,
            tabs: None,
            form: false,
            join: false,
            length: 66,
            merge: false,
            numbers: None,
            first_number: None,
            margin: 0,
            quiet: false,
            separator: None,
            unaligned: false,
            omit: false,
            no_pages: false,
            width: 72,
            wide: false,
            width_set: false,
            first: 1,
            last: usize::MAX,
        }
    }
}
fn count(text: &str) -> Result<usize, String> {
    text.parse::<usize>()
        .ok()
        .filter(|n| *n > 0 && *n <= 1_000_000)
        .ok_or_else(|| format!("invalid number '{text}'"))
}
fn tab(text: Option<String>, default: usize) -> Result<(u8, usize), String> {
    let text = text.unwrap_or_default();
    let bytes = text.as_bytes();
    if bytes.first().is_some_and(|byte| !byte.is_ascii()) {
        return Err(format!("invalid tab or number separator argument '{text}'"));
    }
    let (character, digits) = if bytes.first().is_some_and(|b| !b.is_ascii_digit()) {
        (bytes[0], &text[1..])
    } else {
        (b'\t', text.as_str())
    };
    Ok((
        character,
        if digits.is_empty() {
            default
        } else {
            count(digits)?
        },
    ))
}
fn rewrite(arguments: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    let mut value = false;
    let mut stopped = false;
    for arg in arguments {
        if value || stopped {
            args.push(arg.clone());
            value = false;
            continue;
        }
        if arg == "--" {
            stopped = true;
            args.push(arg.clone());
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 && arg[1..].bytes().all(|b| b.is_ascii_digit()) {
            args.push(format!("--columns={}", &arg[1..]));
        } else if arg.starts_with('+') && arg.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
            args.push(format!("--pages={}", &arg[1..]));
        } else {
            value = matches!(
                arg.as_str(),
                "-D" | "-h"
                    | "-l"
                    | "-N"
                    | "-o"
                    | "-w"
                    | "-W"
                    | "--date-format"
                    | "--header"
                    | "--length"
                    | "--first-line-number"
                    | "--indent"
                    | "--width"
                    | "--page-width"
                    | "--columns"
                    | "--pages"
            );
            if arg.starts_with("--") && !arg.contains('=') {
                value = args::expand_long(
                    arg,
                    &[
                        "date-format",
                        "header",
                        "length",
                        "first-line-number",
                        "indent",
                        "width",
                        "page-width",
                        "columns",
                        "pages",
                    ],
                )
                .is_ok();
            } else if let Some(short_options) = arg.strip_prefix('-') {
                let chars = short_options.char_indices().collect::<Vec<_>>();
                for (index, ch) in chars {
                    if "DhlNowW".contains(ch) {
                        value = index + ch.len_utf8() == arg.len() - 1;
                        break;
                    }
                }
            }
            if !arg.starts_with('-') && std::env::var_os("POSIXLY_CORRECT").is_some() {
                stopped = true
            }
            args.push(arg.clone());
        }
    }
    args
}
pub(super) fn parse(arguments: &[String]) -> Result<(Options, Vec<String>), i32> {
    let specs = [
        OptSpec::value("pages", '\0'),
        OptSpec::value("columns", '\0'),
        OptSpec::flag("across", 'a'),
        OptSpec::flag("show-control-chars", 'c'),
        OptSpec::flag("double-space", 'd'),
        OptSpec::value("date-format", 'D'),
        OptSpec::optional("expand-tabs", 'e'),
        OptSpec::flag("form-feed", 'F'),
        OptSpec {
            id: "form-feed",
            short: 'f',
            long: "",
            has_arg: args::HasArg::No,
        },
        OptSpec::value("header", 'h'),
        OptSpec::optional("output-tabs", 'i'),
        OptSpec::flag("join-lines", 'J'),
        OptSpec::value("length", 'l'),
        OptSpec::flag("merge", 'm'),
        OptSpec::optional("number-lines", 'n'),
        OptSpec::value("first-line-number", 'N'),
        OptSpec::value("indent", 'o'),
        OptSpec::flag("no-file-warnings", 'r'),
        OptSpec::optional("separator", 's'),
        OptSpec::optional("sep-string", 'S'),
        OptSpec::flag("omit-header", 't'),
        OptSpec::flag("omit-pagination", 'T'),
        OptSpec::flag("show-nonprinting", 'v'),
        OptSpec::value("width", 'w'),
        OptSpec::value("page-width", 'W'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = args::parse(&rewrite(arguments), &specs, true).map_err(|e| usage("pr", e, 1))?;
    let mut o = Options::default();
    let mut files = Vec::new();
    for item in items {
        if let Item::Opt(id @ ("help" | "version"), _) = &item {
            return Err(args::help_version("pr", &[format!("--{id}")], HELP, true).unwrap());
        }
        let result: Result<(), String> = (|| {
            match item {
                Item::Operand(file) => files.push(file),
                Item::Opt("columns", Some(v)) => o.columns = count(&v)?,
                Item::Opt("across", _) => o.across = true,
                Item::Opt("show-control-chars", _) => o.controls = true,
                Item::Opt("show-nonprinting", _) => o.octal = true,
                Item::Opt("double-space", _) => o.double = true,
                Item::Opt("date-format", Some(v)) => o.date = v,
                Item::Opt("header", v) => o.header = v,
                Item::Opt("expand-tabs", v) => o.expand = Some(tab(v, 8)?),
                Item::Opt("output-tabs", v) => o.tabs = Some(tab(v, 8)?),
                Item::Opt("form-feed", _) => o.form = true,
                Item::Opt("join-lines", _) => o.join = true,
                Item::Opt("length", Some(v)) => o.length = count(&v)?,
                Item::Opt("merge", _) => o.merge = true,
                Item::Opt("number-lines", v) => o.numbers = Some(tab(v, 5)?),
                Item::Opt("first-line-number", Some(v)) => {
                    o.first_number = Some(
                        v.parse::<i32>()
                            .map(i64::from)
                            .map_err(|_| format!("invalid line number '{v}'"))?,
                    )
                }
                Item::Opt("indent", Some(v)) => {
                    o.margin = v.parse().map_err(|_| format!("invalid margin '{v}'"))?
                }
                Item::Opt("no-file-warnings", _) => o.quiet = true,
                Item::Opt("separator", v) => {
                    let v = v.unwrap_or_else(|| "\t".into());
                    if v.len() > 1 {
                        return Err("separator must be a single character".into());
                    }
                    o.separator = Some(v.into_bytes());
                    o.unaligned = true;
                }
                Item::Opt("sep-string", v) => {
                    o.separator = Some(v.unwrap_or_default().into_bytes());
                }
                Item::Opt("omit-header", _) => o.omit = true,
                Item::Opt("omit-pagination", _) => {
                    o.omit = true;
                    o.no_pages = true;
                }
                Item::Opt(id @ ("width" | "page-width"), Some(v)) => {
                    o.width = count(&v)?;
                    o.width_set = true;
                    if id == "page-width" {
                        o.wide = true
                    }
                }
                Item::Opt("pages", Some(v)) => {
                    let (a, b) = v
                        .split_once(':')
                        .map_or((v.as_str(), None), |(a, b)| (a, Some(b)));
                    o.first = count(a)?;
                    o.last = b.map(count).transpose()?.unwrap_or(usize::MAX);
                    if o.first > o.last {
                        return Err("first page is larger than last page".into());
                    }
                }
                _ => unreachable!(),
            };
            Ok(())
        })();
        if let Err(error) = result {
            return Err(usage("pr", error, 1));
        }
    }
    if o.length <= 10 {
        o.omit = true
    }
    if o.margin > 1_000_000 {
        return Err(usage("pr", "margin too large", 1));
    }
    if o.across && o.merge {
        return Err(usage("pr", "cannot specify both --across and --merge", 1));
    }
    if files.is_empty() {
        files.push("-".into())
    }
    Ok((o, files))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tab_default() {
        assert_eq!(tab(None, 8).unwrap(), (b'\t', 8));
    }
    #[test]
    fn custom_tab() {
        assert_eq!(tab(Some(":3".into()), 5).unwrap(), (b':', 3));
    }
    #[test]
    fn required_negative_not_rewritten() {
        assert_eq!(rewrite(&["-l".into(), "-2".into()]), vec!["-l", "-2"]);
    }
    #[test]
    fn multibyte_tab_and_number_characters_fail_without_slicing() {
        assert!(tab(Some("é".into()), 8).is_err());
        assert!(tab(Some("é3".into()), 5).is_err());
    }
    #[test]
    fn first_line_number_accepts_only_signed_int_range() {
        assert_eq!(parse(&["-N-1".into()]).unwrap().0.first_number, Some(-1));
        assert!(parse(&["-N2147483648".into()]).is_err());
        assert!(parse(&["-N18446744073709551615".into()]).is_err());
    }
}
