use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    datetime, error, io,
};
use std::io::BufRead;
const HELP: &str = "Usage: date [OPTION]... [+FORMAT]\nDisplay the date and time.\n  -d, --date=STRING        display time described by STRING\n  -f, --file=DATEFILE      process one date per line\n  -I[FMT], --iso-8601[=FMT]  ISO 8601 output (date, hours, minutes, seconds, ns)\n  -R, --rfc-email          RFC 5322 format\n      --rfc-3339=FMT       RFC 3339 format (date, seconds, ns)\n  -r, --reference=FILE     use FILE's modification time\n  -u, --utc, --universal   use UTC\n      --help               display this help\n      --version            output version information\n";
const DEFAULT: &str = "%a %b %e %H:%M:%S %Z %Y";

fn iso(kind: &str) -> Option<&'static str> {
    match kind {
        "date" => Some("%F"),
        "hours" => Some("%FT%H%:z"),
        "minutes" => Some("%FT%H:%M%:z"),
        "seconds" => Some("%FT%H:%M:%S%:z"),
        "ns" => Some("%FT%H:%M:%S,%N%:z"),
        _ => None,
    }
}
fn rfc(kind: &str) -> Option<&'static str> {
    match kind {
        "date" => Some("%F"),
        "seconds" => Some("%F %T%:z"),
        "ns" => Some("%F %T.%N%:z"),
        _ => None,
    }
}
fn render(date: &jiff::Zoned, pattern: &str, out: &mut dyn std::io::Write) -> std::io::Result<()> {
    let text = datetime::format(date, pattern).map_err(std::io::Error::other)?;
    writeln!(out, "{text}")
}

pub fn run(argv: &[String]) -> i32 {
    let specs = [
        OptSpec::value("date", 'd'),
        OptSpec::value("file", 'f'),
        OptSpec::optional("iso-8601", 'I'),
        OptSpec {
            id: "rfc-email",
            short: 'R',
            long: "rfc-email",
            has_arg: HasArg::No,
        },
        OptSpec::value("rfc-3339", '\0'),
        OptSpec::value("reference", 'r'),
        OptSpec::flag("utc", 'u'),
        OptSpec::flag("universal", '\0'),
        OptSpec::flag("debug", '\0'),
        OptSpec::value("set", 's'),
    ];
    let items = match args::parse_command("date", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let (mut text, mut file, mut reference, mut utc, mut pattern, mut operands) =
        (None, None, None, false, None, Vec::new());
    for item in items {
        match item {
            Item::Opt("date", v) => text = v,
            Item::Opt("file", v) => file = v,
            Item::Opt("reference", v) => reference = v,
            Item::Opt("utc" | "universal", _) => utc = true,
            Item::Opt("iso-8601", v) => match iso(v.as_deref().unwrap_or("date")) {
                Some(p) => pattern = Some(p.to_owned()),
                None => return error::usage("date", "invalid argument for --iso-8601", 1),
            },
            Item::Opt("rfc-email", _) => pattern = Some("%a, %d %b %Y %H:%M:%S %z".into()),
            Item::Opt("rfc-3339", Some(v)) => match rfc(&v) {
                Some(p) => pattern = Some(p.to_owned()),
                None => {
                    return error::usage(
                        "date",
                        format!("invalid argument '{v}' for '--rfc-3339'"),
                        1,
                    )
                }
            },
            Item::Opt("debug", _) => {
                return error::usage("date", "option '--debug' is not supported by agentcmd", 1)
            }
            Item::Opt("set", _) => {
                eprintln!("date: cannot set date: Operation not permitted");
                return 1;
            }
            Item::Operand(v) => operands.push(v),
            _ => unreachable!(),
        }
    }
    if operands.len() > 1 {
        return error::usage("date", format!("extra operand '{}'", operands[1]), 1);
    }
    if let Some(arg) = operands.first() {
        if let Some(fmt) = arg.strip_prefix('+') {
            pattern = Some(fmt.to_owned());
        } else {
            return error::usage("date", "cannot set date: Operation not permitted", 1);
        }
    }
    if [text.is_some(), file.is_some(), reference.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count()
        > 1
    {
        return error::usage("date", "multiple output dates specified", 1);
    }
    let zone = match datetime::timezone(utc) {
        Ok(z) => z,
        Err(e) => {
            eprintln!("date: {e}");
            return 1;
        }
    };
    let base = jiff::Timestamp::now().to_zoned(zone.clone());
    let pattern = pattern.as_deref().unwrap_or(DEFAULT);
    let mut failed = false;
    let status = io::output("date", |out| {
        if let Some(path) = file {
            let mut reader = match io::input(&path) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("date: {path}: {}", error::io_text(&e));
                    failed = true;
                    return Ok(());
                }
            };
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => match datetime::parse_at(line.trim_end_matches(['\n', '\r']), &base) {
                        Ok(date) => render(&date, pattern, out)?,
                        Err(_) => {
                            eprintln!(
                                "date: invalid date '{}'",
                                line.trim_end_matches(['\n', '\r'])
                            );
                            failed = true;
                        }
                    },
                    Err(e) => {
                        eprintln!("date: {path}: {}", error::io_text(&e));
                        failed = true;
                        break;
                    }
                }
            }
        } else if let Some(text) = text {
            match datetime::parse_at(&text, &base) {
                Ok(date) => render(&date, pattern, out)?,
                Err(_) => {
                    eprintln!("date: invalid date '{text}'");
                    failed = true;
                }
            }
        } else if let Some(path) = reference {
            match std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .and_then(|t| jiff::Timestamp::try_from(t).map_err(std::io::Error::other))
            {
                Ok(time) => render(&time.to_zoned(zone), pattern, out)?,
                Err(e) => {
                    eprintln!("date: {path}: {}", error::io_text(&e));
                    failed = true;
                }
            }
        } else {
            render(&base, pattern, out)?;
        }
        Ok(())
    });
    status.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iso_date() {
        assert_eq!(iso("date"), Some("%F"));
    }
    #[test]
    fn iso_nanos() {
        assert_eq!(iso("ns"), Some("%FT%H:%M:%S,%N%:z"));
    }
    #[test]
    fn iso_invalid() {
        assert_eq!(iso("bad"), None);
    }
    #[test]
    fn rfc_seconds() {
        assert_eq!(rfc("seconds"), Some("%F %T%:z"));
    }
    #[test]
    fn rfc_invalid() {
        assert_eq!(rfc("hours"), None);
    }
}
