use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io,
};
use std::io::{BufRead, Write};

const HELP: &str = "Usage: uniq [OPTION]... [INPUT [OUTPUT]]\nFilter adjacent matching records.\n  -c, --count               prefix lines with counts\n  -d, --repeated            only repeated groups\n  -D                       print all repeated lines\n      --all-repeated[=none|prepend|separate]\n      --group[=separate|prepend|append|both] print all groups\n  -u, --unique              only unique groups\n  -i, --ignore-case         compare without ASCII case\n  -f, --skip-fields=N       skip N blank-separated fields (-N)\n  -s, --skip-chars=N        skip N bytes (+N)\n  -w, --check-chars=N       compare at most N bytes\n  -z, --zero-terminated     use NUL records\n      --help               display this help\n      --version            output version information\n";

#[derive(Default)]
struct Options {
    count: bool,
    repeated: bool,
    unique: bool,
    all: bool,
    group: Option<String>,
    method: String,
    ignore: bool,
    fields: usize,
    chars: usize,
    width: Option<usize>,
    delimiter: u8,
}

fn key<'a>(line: &'a [u8], options: &Options) -> &'a [u8] {
    let mut i = 0;
    for _ in 0..options.fields {
        while i < line.len() && matches!(line[i], b' ' | b'\t') {
            i += 1;
        }
        while i < line.len() && !matches!(line[i], b' ' | b'\t') {
            i += 1;
        }
    }
    i = i.saturating_add(options.chars).min(line.len());
    &line[i..options
        .width
        .map_or(line.len(), |w| i.saturating_add(w).min(line.len()))]
}

fn matches(a: &[u8], b: &[u8], options: &Options) -> bool {
    let (a, b) = (key(a, options), key(b, options));
    if options.ignore {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn filter(reader: &mut dyn BufRead, out: &mut dyn Write, o: &Options) -> std::io::Result<()> {
    let (mut first, mut count, mut emitted_group) = (Vec::new(), 0u64, false);
    let record = |out: &mut dyn Write, line: &[u8]| {
        out.write_all(line)?;
        out.write_all(&[o.delimiter])
    };
    let begin = |out: &mut dyn Write, method: &str, emitted: bool| {
        if method == "prepend" || method == "both" && !emitted || emitted && method == "separate" {
            out.write_all(&[o.delimiter])?;
        }
        Ok::<_, std::io::Error>(())
    };
    let finish = |out: &mut dyn Write, first: &[u8], count: u64| {
        if o.group.is_some() {
            if o.group
                .as_deref()
                .is_some_and(|s| matches!(s, "append" | "both"))
            {
                out.write_all(&[o.delimiter])?;
            }
        } else if !o.all && ((count == 1 && !o.repeated) || (count > 1 && !o.unique)) {
            if o.count {
                write!(out, "{count:7} ")?;
            }
            record(out, first)?;
        }
        Ok::<_, std::io::Error>(())
    };
    io::records(reader, o.delimiter, |line| {
        if count > 0 && matches(&first, line, o) {
            count += 1;
            if o.group.is_some() {
                record(out, line)?;
            } else if o.all && !o.unique {
                if count == 2 {
                    begin(out, &o.method, emitted_group)?;
                    record(out, &first)?;
                    emitted_group = true;
                }
                record(out, line)?;
            }
        } else {
            if count > 0 {
                finish(out, &first, count)?;
            }
            first.clear();
            first.extend_from_slice(line);
            count = 1;
            if let Some(method) = &o.group {
                begin(out, method, emitted_group)?;
                record(out, line)?;
                emitted_group = true;
            }
        }
        Ok(())
    })?;
    if count > 0 {
        finish(out, &first, count)?;
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> i32 {
    let arguments: Vec<_> = arguments
        .iter()
        .map(|s| {
            if s.len() > 1
                && s[1..].bytes().all(|b| b.is_ascii_digit())
                && s.starts_with(['-', '+'])
            {
                format!(
                    "--{}={}",
                    if s.starts_with('-') {
                        "skip-fields"
                    } else {
                        "skip-chars"
                    },
                    &s[1..]
                )
            } else {
                s.clone()
            }
        })
        .collect();
    let specs = [
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
        OptSpec::flag("count", 'c'),
        OptSpec::flag("repeated", 'd'),
        OptSpec {
            id: "all-repeated-short",
            long: "",
            short: 'D',
            has_arg: HasArg::No,
        },
        OptSpec::flag("unique", 'u'),
        OptSpec::flag("ignore-case", 'i'),
        OptSpec::optional("all-repeated", '\0'),
        OptSpec::optional("group", '\0'),
        OptSpec::value("skip-fields", 'f'),
        OptSpec::value("skip-chars", 's'),
        OptSpec::value("check-chars", 'w'),
        OptSpec::flag("zero-terminated", 'z'),
    ];
    let parsed = match args::parse(&arguments, &specs, true) {
        Ok(v) => v,
        Err(e) => return usage("uniq", &e),
    };
    let mut options = Options {
        delimiter: b'\n',
        method: "none".into(),
        ..Options::default()
    };
    let mut files = Vec::new();
    for item in parsed {
        match item {
            Item::Operand(s) => files.push(s),
            Item::Opt(id, value) => match id {
                "help" | "version" => {
                    return args::help_version("uniq", &[format!("--{id}")], HELP, true).unwrap()
                }
                "count" => options.count = true,
                "repeated" => options.repeated = true,
                "unique" => options.unique = true,
                "ignore-case" => options.ignore = true,
                "zero-terminated" => options.delimiter = 0,
                "all-repeated-short" => {
                    options.all = true;
                    options.method = "none".into();
                }
                "all-repeated" => {
                    options.all = true;
                    let value = value.unwrap_or("none".into());
                    if !matches!(value.as_str(), "none" | "prepend" | "separate") {
                        return usage("uniq", "invalid argument for '--all-repeated'");
                    }
                    options.method = value;
                }
                "group" => {
                    let value = value.unwrap_or("separate".into());
                    if !matches!(value.as_str(), "separate" | "prepend" | "append" | "both") {
                        return usage("uniq", "invalid argument for '--group'");
                    }
                    options.group = Some(value);
                }
                "skip-fields" | "skip-chars" | "check-chars" => {
                    let Some(n) = value.and_then(|v| v.parse::<usize>().ok()) else {
                        return usage("uniq", "invalid number of fields or characters");
                    };
                    match id {
                        "skip-fields" => options.fields = n,
                        "skip-chars" => options.chars = n,
                        _ => options.width = Some(n),
                    }
                }
                _ => unreachable!(),
            },
        }
    }
    if files.len() > 2 {
        return usage("uniq", &format!("extra operand '{}'", files[2]));
    }
    if options.count && options.all {
        return usage(
            "uniq",
            "printing all duplicated lines and repeat counts is meaningless",
        );
    }
    if options.group.is_some()
        && (options.count || options.repeated || options.unique || options.all)
    {
        return usage("uniq", "--group is mutually exclusive with -c/-d/-D/-u");
    }
    let name = files.first().map(String::as_str).unwrap_or("-");
    let mut reader = match io::input(name) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("uniq: {name}: {}", error::io_text(&e));
            return 1;
        }
    };
    if let Some(name) = files.get(1).filter(|s| s.as_str() != "-") {
        match std::fs::File::create(name)
            .and_then(|mut f| filter(&mut reader, &mut f, &options).and_then(|()| f.flush()))
        {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("uniq: {name}: {}", error::io_text(&e));
                1
            }
        }
    } else {
        io::output("uniq", |out| filter(&mut reader, out, &options))
    }
}

fn usage(command: &str, message: &str) -> i32 {
    error::usage(command, message, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(input: &[u8], options: Options) -> Vec<u8> {
        let mut out = Vec::new();
        filter(&mut std::io::Cursor::new(input), &mut out, &options).unwrap();
        out
    }
    fn o() -> Options {
        Options {
            delimiter: b'\n',
            method: "none".into(),
            ..Options::default()
        }
    }
    #[test]
    fn adjacent() {
        assert_eq!(f(b"a\na\nb\na\n", o()), b"a\nb\na\n");
    }
    #[test]
    fn counts() {
        assert_eq!(
            f(b"a\na\nb\n", Options { count: true, ..o() }),
            b"      2 a\n      1 b\n"
        );
    }
    #[test]
    fn unique() {
        assert_eq!(
            f(
                b"a\na\nb\n",
                Options {
                    unique: true,
                    ..o()
                }
            ),
            b"b\n"
        );
    }
    #[test]
    fn fields() {
        let options = Options { fields: 1, ..o() };
        assert!(matches(b"a x", b"b x", &options));
    }
    #[test]
    fn chars_width() {
        let options = Options {
            chars: 1,
            width: Some(1),
            ignore: true,
            ..o()
        };
        assert!(matches(b"aX1", b"bX2", &options));
    }
    #[test]
    fn repeated_groups() {
        assert_eq!(
            f(
                b"a\na\nb\nc\nc\n",
                Options {
                    all: true,
                    method: "separate".into(),
                    ..o()
                }
            ),
            b"a\na\n\nc\nc\n"
        );
    }
    #[test]
    fn group_both() {
        assert_eq!(
            f(
                b"a\nb\n",
                Options {
                    group: Some("both".into()),
                    ..o()
                }
            ),
            b"\na\n\nb\n\n"
        );
    }
}
