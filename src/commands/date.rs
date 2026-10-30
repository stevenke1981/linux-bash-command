//! `date`: prints the current time, or a time given by `-d`, `-f` or `-r`, in a format
//! given by `+FORMAT`, `-I`, `-R` or `--rfc-3339`. Setting the clock is not supported.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use jiff::{tz::TimeZone, Timestamp};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::datetime::{self, format_in, parse, InvalidDate};
use crate::common::error::{print_version, report, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

const DEFAULT_FORMAT: &str = "%a %b %e %H:%M:%S %Z %Y";

const HELP: &str = r#"Usage: date [OPTION]... [+FORMAT]
  or:  date [-u|--utc|--universal] [MMDDhhmm[[CC]YY][.ss]]
Display date and time in the given FORMAT.

  -d, --date=STRING          display time described by STRING, not 'now'
  -f, --file=DATEFILE        like --date; once for each line of DATEFILE
  -I[FMT], --iso-8601[=FMT]  output date/time in ISO 8601 format.
                               FMT='date' for date only (the default),
                               'hours', 'minutes', 'seconds', or 'ns'
                               for date and time to the indicated precision.
                               Example: 2006-08-14T02:34:56-06:00
  -R, --rfc-email            output date and time in RFC 5322 format.
                               Example: Mon, 14 Aug 2006 02:34:56 -0600
      --rfc-3339=FMT         output date and time in RFC 3339 format.
                               FMT='date', 'seconds', or 'ns' for date and time
                               to the indicated precision.
                               Example: 2006-08-14 02:34:56-06:00
  -r, --reference=FILE       display the last modification time of FILE
  -u, --utc, --universal     print or set Coordinated Universal Time (UTC)
      --help     display this help and exit
      --version  output version information and exit

FORMAT controls the output.  Interpreted sequences are:

  %%   a literal %          %n   a newline          %t   a horizontal tab
  %a   locale's abbreviated weekday name (e.g., Sun)
  %A   locale's full weekday name (e.g., Sunday)
  %b   locale's abbreviated month name (e.g., Jan)
  %B   locale's full month name (e.g., January)
  %c   locale's date and time (e.g., Thu Mar  3 23:05:25 2005)
  %C   century; like %Y, except omit last two digits (e.g., 20)
  %d   day of month (e.g., 01)
  %D   date; same as %m/%d/%y
  %e   day of month, space padded; same as %_d
  %F   full date; like %+4Y-%m-%d
  %g   last two digits of year of ISO week number (see %G)
  %G   year of ISO week number (see %V); normally useful only with %V
  %h   same as %b
  %H   hour (00..23)
  %I   hour (01..12)
  %j   day of year (001..366)
  %k   hour, space padded ( 0..23); same as %_H
  %l   hour, space padded ( 1..12); same as %_I
  %m   month (01..12)
  %M   minute (00..59)
  %N   nanoseconds (000000000..999999999)
  %p   locale's equivalent of either AM or PM; blank if not known
  %P   like %p, but lower case
  %q   quarter of year (1..4)
  %r   locale's 12-hour clock time (e.g., 11:11:04 PM)
  %R   24-hour hour and minute; same as %H:%M
  %s   seconds since the Epoch (1970-01-01 00:00 UTC)
  %S   second (00..60)
  %T   time; same as %H:%M:%S
  %u   day of week (1..7); 1 is Monday
  %U   week number of year, with Sunday as first day of week (00..53)
  %V   ISO week number, with Monday as first day of week (01..53)
  %w   day of week (0..6); 0 is Sunday
  %W   week number of year, with Monday as first day of week (00..53)
  %x   locale's date representation (e.g., 12/31/99)
  %X   locale's time representation (e.g., 23:13:48)
  %y   last two digits of year (00..99)
  %Y   year
  %z   +hhmm numeric time zone (e.g., -0400)
  %:z  +hh:mm numeric time zone (e.g., -04:00)
  %::z +hh:mm:ss numeric time zone (e.g., -04:00:00)
  %:::z numeric time zone with : to necessary precision (e.g., -04, +05:30)
  %Z   alphabetic time zone abbreviation (e.g., EDT)

By default, date pads numeric fields with zeroes.  The following optional
flags may follow '%':

  -  (hyphen) do not pad the field
  _  (underscore) pad with spaces
  0  (zero) pad with zeros
  ^  use upper case if possible
  #  use opposite case if possible

After any flags comes an optional field width, as a decimal number;
then an optional modifier, which is either
E to use the locale's alternate representations if available, or
O to use the locale's alternate numeric symbols if available.

      --help     display this help and exit
      --version  output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::req("date", Some('d'), Some("date")),
    OptSpec::req("file", Some('f'), Some("file")),
    OptSpec::opt("iso", Some('I'), Some("iso-8601")),
    OptSpec::flag("rfc-email", Some('R'), Some("rfc-email")),
    OptSpec::req("rfc3339", None, Some("rfc-3339")),
    OptSpec::req("reference", Some('r'), Some("reference")),
    OptSpec::flag("utc", Some('u'), Some("utc")),
    OptSpec::flag("utc", None, Some("universal")),
    OptSpec::req("set", Some('s'), Some("set")),
    OptSpec::flag("debug", None, Some("debug")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Where the instants to print come from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Now,
    Date(String),
    File(String),
    Reference(String),
}

/// Parsed command line.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    source: Source,
    format: String,
    utc: bool,
}

/// Entry point.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("date", SPECS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage("date", &err, EXIT_FAIL),
    };
    let options = match collect(&items) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let zone = if options.utc {
        TimeZone::UTC
    } else {
        datetime::zone_from_env()
    };
    let mut out = stdout_writer();
    let status = match options.source {
        Source::Now => print_one(&mut out, datetime::now(), &options.format, &zone),
        Source::Date(ref text) => print_parsed(&mut out, text, &options.format, &zone),
        Source::File(ref path) => print_file(&mut out, path, &options.format, &zone),
        Source::Reference(ref path) => print_reference(&mut out, path, &options.format, &zone),
    };
    if status != EXIT_OK {
        return status;
    }
    finish_output("date", &mut out)
}

/// Handles the options that end the run early and gathers the rest. `Err` carries the exit
/// status to return at once.
fn collect(items: &[Item]) -> Result<Options, i32> {
    let mut source = Source::Now;
    let mut format: Option<String> = None;
    let mut output_flags = 0;
    let mut utc = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text.clone()),
            Item::Opt { id, value } => match *id {
                "help" => {
                    print!("{HELP}");
                    return Err(EXIT_OK);
                }
                "version" => return Err(print_version("date")),
                "debug" => return Err(unsupported("date", "--debug")),
                "set" => return Err(cannot_set()),
                "utc" => utc = true,
                "date" | "file" | "reference" => {
                    source = source_from(id, value.clone().unwrap_or_default());
                }
                "iso" => {
                    output_flags += 1;
                    format = Some(iso_format(value.as_deref().unwrap_or("date"))?);
                }
                "rfc-email" => {
                    output_flags += 1;
                    format = Some("%a, %d %b %Y %H:%M:%S %z".to_string());
                }
                "rfc3339" => {
                    output_flags += 1;
                    format = Some(rfc3339_format(value.as_deref().unwrap_or(""))?);
                }
                _ => {}
            },
        }
    }
    if !same_source_kind(items) {
        eprintln!(
            "date: the options to specify dates for printing are mutually exclusive\nTry 'date --help' for more information."
        );
        return Err(EXIT_FAIL);
    }
    let plus_format = take_format_operand(&operands)?;
    if plus_format.is_some() {
        output_flags += 1;
        format = plus_format;
    }
    if output_flags > 1 {
        eprintln!(
            "date: multiple output formats specified\nTry 'date --help' for more information."
        );
        return Err(EXIT_FAIL);
    }
    Ok(Options {
        source,
        format: format.unwrap_or_else(|| DEFAULT_FORMAT.to_string()),
        utc,
    })
}

/// True when every source option names the same kind (`-d -d` is fine, `-d -f` is not).
fn same_source_kind(items: &[Item]) -> bool {
    let mut kind: Option<&str> = None;
    for item in items {
        if let Item::Opt { id, .. } = item {
            if matches!(*id, "date" | "file" | "reference") {
                match kind {
                    None => kind = Some(id),
                    Some(previous) if previous != *id => return false,
                    Some(_) => {}
                }
            }
        }
    }
    true
}

fn source_from(id: &str, value: String) -> Source {
    match id {
        "file" => Source::File(value),
        "reference" => Source::Reference(value),
        _ => Source::Date(value),
    }
}

/// The single `+FORMAT` operand, if any. Other operands cannot set the clock here.
fn take_format_operand(operands: &[String]) -> Result<Option<String>, i32> {
    match operands {
        [] => Ok(None),
        [single] if single.starts_with('+') => Ok(Some(single[1..].to_string())),
        [_] => Err(cannot_set()),
        [_, extra, ..] => {
            eprintln!("date: extra operand '{extra}'\nTry 'date --help' for more information.");
            Err(EXIT_FAIL)
        }
    }
}

fn cannot_set() -> i32 {
    eprintln!("date: cannot set date: Operation not permitted");
    EXIT_FAIL
}

fn iso_format(value: &str) -> Result<String, i32> {
    Ok(match value {
        "date" => "%Y-%m-%d".to_string(),
        "hours" => "%Y-%m-%dT%H%:z".to_string(),
        "minutes" => "%Y-%m-%dT%H:%M%:z".to_string(),
        "seconds" => "%Y-%m-%dT%H:%M:%S%:z".to_string(),
        "ns" => "%Y-%m-%dT%H:%M:%S,%N%:z".to_string(),
        other => {
            return Err(invalid_argument(
                other,
                "--iso-8601",
                &["hours", "minutes", "date", "seconds", "ns"],
            ))
        }
    })
}

fn rfc3339_format(value: &str) -> Result<String, i32> {
    Ok(match value {
        "date" => "%Y-%m-%d".to_string(),
        "seconds" => "%Y-%m-%d %H:%M:%S%:z".to_string(),
        "ns" => "%Y-%m-%d %H:%M:%S.%N%:z".to_string(),
        other => {
            return Err(invalid_argument(
                other,
                "--rfc-3339",
                &["date", "seconds", "ns"],
            ))
        }
    })
}

fn invalid_argument(value: &str, option: &str, valid: &[&str]) -> i32 {
    eprintln!("date: invalid argument '{value}' for '{option}'");
    eprintln!("Valid arguments are:");
    for name in valid {
        eprintln!("  - '{name}'");
    }
    eprintln!("Try 'date --help' for more information.");
    EXIT_FAIL
}

/// Writes one formatted instant and its newline.
fn write_one(out: &mut impl Write, ts: Timestamp, format: &str, zone: &TimeZone) -> io::Result<()> {
    writeln!(out, "{}", format_in(format, ts, zone))
}

/// Maps a write result to an exit status. A closed pipe is silent success.
fn write_status(result: io::Result<()>) -> i32 {
    match result {
        Ok(()) => EXIT_OK,
        Err(error) if is_broken_pipe(&error) => EXIT_OK,
        Err(error) => {
            report("date", "write error", &error);
            EXIT_FAIL
        }
    }
}

/// Prints the single instant `ts`.
fn print_one(out: &mut impl Write, ts: Timestamp, format: &str, zone: &TimeZone) -> i32 {
    write_status(write_one(out, ts, format, zone))
}

/// `-d STRING`: an invalid string is fatal.
fn print_parsed(out: &mut impl Write, text: &str, format: &str, zone: &TimeZone) -> i32 {
    match parse(text, datetime::now(), zone) {
        Ok(ts) => print_one(out, ts, format, zone),
        Err(error) => invalid(&error),
    }
}

/// `-f FILE`: one date per line. An invalid line is reported and skipped, and the run fails
/// at the end. A write failure stops the run at once.
fn print_file(out: &mut impl Write, path: &str, format: &str, zone: &TimeZone) -> i32 {
    let mut reader = match open_input(path) {
        Ok(reader) => reader,
        Err(error) => {
            report("date", path, &error);
            return EXIT_FAIL;
        }
    };
    let mut buf = Vec::new();
    let mut status = EXIT_OK;
    loop {
        match read_record(reader.as_mut(), b'\n', &mut buf) {
            Ok(false) => return status,
            Ok(true) => {}
            Err(error) => {
                report("date", path, &error);
                return EXIT_FAIL;
            }
        }
        let line = String::from_utf8_lossy(&buf);
        let text = line.trim_end_matches(['\n', '\r']);
        match parse(text, datetime::now(), zone) {
            Ok(ts) => {
                let written = write_status(write_one(out, ts, format, zone));
                if written != EXIT_OK {
                    return written;
                }
            }
            Err(error) => {
                eprintln!("date: {error}");
                status = EXIT_FAIL;
            }
        }
    }
}

/// `-r FILE`: the modification time of FILE.
fn print_reference(out: &mut impl Write, path: &str, format: &str, zone: &TimeZone) -> i32 {
    match modified_time(Path::new(path)) {
        Ok(ts) => print_one(out, ts, format, zone),
        Err(error) => {
            report("date", path, &error);
            EXIT_FAIL
        }
    }
}

fn modified_time(path: &Path) -> io::Result<Timestamp> {
    let meta = fs::metadata(path)?;
    Ok(datetime::from_system_time(meta.modified()?))
}

fn invalid(error: &InvalidDate) -> i32 {
    eprintln!("date: {error}");
    EXIT_FAIL
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn options(list: &[&str]) -> Result<Options, i32> {
        let items = Parser::new("date", SPECS).parse(&args(list)).unwrap();
        collect(&items)
    }

    #[test]
    fn default_format_when_no_options() {
        let opts = options(&[]).unwrap();
        assert_eq!(opts.source, Source::Now);
        assert_eq!(opts.format, DEFAULT_FORMAT);
        assert!(!opts.utc);
    }

    #[test]
    fn plus_operand_is_the_format() {
        let opts = options(&["+%Y-%m"]).unwrap();
        assert_eq!(opts.format, "%Y-%m");
    }

    #[test]
    fn iso_defaults_to_date() {
        assert_eq!(options(&["-I"]).unwrap().format, "%Y-%m-%d");
        assert_eq!(options(&["-Ihours"]).unwrap().format, "%Y-%m-%dT%H%:z");
        assert_eq!(
            options(&["--iso-8601=ns"]).unwrap().format,
            "%Y-%m-%dT%H:%M:%S,%N%:z"
        );
    }

    #[test]
    fn rfc_formats() {
        assert_eq!(options(&["-R"]).unwrap().format, "%a, %d %b %Y %H:%M:%S %z");
        assert_eq!(
            options(&["--rfc-3339=seconds"]).unwrap().format,
            "%Y-%m-%d %H:%M:%S%:z"
        );
    }

    #[test]
    fn invalid_iso_argument_is_rejected() {
        assert_eq!(options(&["-Ix"]), Err(EXIT_FAIL));
        assert_eq!(options(&["--rfc-3339=minutes"]), Err(EXIT_FAIL));
    }

    #[test]
    fn last_date_string_wins_and_kinds_must_match() {
        let opts = options(&["-d", "2024-01-02", "-d", "2025-01-01"]).unwrap();
        assert_eq!(opts.source, Source::Date("2025-01-01".to_string()));
        assert_eq!(options(&["-d", "2024-01-02", "-f", "x"]), Err(EXIT_FAIL));
        assert_eq!(options(&["-r", "a", "-d", "x"]), Err(EXIT_FAIL));
    }

    #[test]
    fn multiple_output_formats_conflict() {
        assert_eq!(options(&["-I", "-R"]), Err(EXIT_FAIL));
        assert_eq!(options(&["-I", "+%Y"]), Err(EXIT_FAIL));
    }

    #[test]
    fn extra_operands_and_set_are_errors() {
        assert_eq!(options(&["+%Y", "+%m"]), Err(EXIT_FAIL));
        assert_eq!(options(&["2024"]), Err(EXIT_FAIL));
        assert_eq!(options(&["-s", "2024-01-02"]), Err(EXIT_FAIL));
        assert_eq!(options(&["--debug"]), Err(EXIT_FAIL));
    }

    #[test]
    fn utc_flags_both_spellings() {
        assert!(options(&["-u"]).unwrap().utc);
        assert!(options(&["--universal"]).unwrap().utc);
        assert!(options(&["--utc"]).unwrap().utc);
    }

    #[test]
    fn print_parsed_formats_a_fixed_instant() {
        let mut out = Vec::new();
        let status = print_parsed(&mut out, "2024-01-02 10:00 UTC", "%F %T", &TimeZone::UTC);
        assert_eq!(status, EXIT_OK);
        assert_eq!(String::from_utf8(out).unwrap(), "2024-01-02 10:00:00\n");
    }

    #[test]
    fn print_parsed_rejects_bad_text() {
        let mut out = Vec::new();
        assert_eq!(
            print_parsed(&mut out, "bogus", "%F", &TimeZone::UTC),
            EXIT_FAIL
        );
        assert!(out.is_empty());
    }

    #[test]
    fn modified_time_of_missing_file_fails() {
        assert!(modified_time(Path::new("/definitely/not/here/agentcmd")).is_err());
    }
}
