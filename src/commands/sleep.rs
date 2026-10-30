//! `sleep`: pauses for the sum of its operands, each a number with an optional `s`, `m`, `h`
//! or `d` suffix, or `inf`/`infinity` for an indefinite pause. Options are GNU-style
//! (`--help` and `--version` only, with permutation).

use std::io::Write;
use std::thread;
use std::time::Duration;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const HELP: &str = r#"Usage: sleep NUMBER[SUFFIX]...
  or:  sleep OPTION
Pause for NUMBER seconds.  SUFFIX may be 's' for seconds (the default),
'm' for minutes, 'h' for hours or 'd' for days.  NUMBER need not be an
integer.  Given two or more arguments, pause for the amount of time
specified by the sum of their values.

      --help     display this help and exit
      --version  output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// One parsed operand.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Interval {
    Seconds(f64),
    Forever,
}

/// Splits the optional unit letter off `text`, returning the number text and its scale in
/// seconds.
fn split_suffix(text: &str) -> (&str, f64) {
    let scale = match text.as_bytes().last() {
        Some(b's') => 1.0,
        Some(b'm') => 60.0,
        Some(b'h') => 3600.0,
        Some(b'd') => 86400.0,
        _ => return (text, 1.0),
    };
    (&text[..text.len() - 1], scale)
}

/// Parses one operand. Accepts decimal numbers (with an exponent), but not `nan`, signs of
/// negative values, or hexadecimal.
fn parse_interval(text: &str) -> Option<Interval> {
    if text.eq_ignore_ascii_case("inf") || text.eq_ignore_ascii_case("infinity") {
        return Some(Interval::Forever);
    }
    let (number, scale) = split_suffix(text);
    let plausible = !number.is_empty()
        && number
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !plausible {
        return None;
    }
    let value: f64 = number.parse().ok()?;
    if value.is_nan() || value < 0.0 {
        return None;
    }
    let seconds = value * scale;
    if seconds.is_infinite() {
        Some(Interval::Forever)
    } else {
        Some(Interval::Seconds(seconds))
    }
}

/// Sums the operands. Any invalid operand is returned as the error text.
fn total_interval(operands: &[String]) -> Result<Interval, String> {
    let mut total = 0.0;
    for operand in operands {
        match parse_interval(operand) {
            Some(Interval::Forever) => return Ok(Interval::Forever),
            Some(Interval::Seconds(seconds)) => total += seconds,
            None => return Err(format!("invalid time interval '{operand}'")),
        }
    }
    if total.is_infinite() {
        return Ok(Interval::Forever);
    }
    Ok(Interval::Seconds(total))
}

/// Blocks for `interval`; an unbounded or out-of-range duration never returns.
fn pause(interval: Interval) {
    let duration = match interval {
        Interval::Seconds(seconds) => Duration::try_from_secs_f64(seconds).ok(),
        Interval::Forever => None,
    };
    match duration {
        Some(duration) => thread::sleep(duration),
        None => loop {
            thread::sleep(Duration::from_secs(3600));
        },
    }
}

fn print_help() -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(HELP.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("sleep: write error: {error}");
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output("sleep", &mut out)
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("sleep", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("sleep", &error, EXIT_FAIL),
    };
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return print_help(),
            Item::Opt { id: "version", .. } => return print_version("sleep"),
            Item::Opt { .. } => {}
            Item::Operand(text) => operands.push(text),
        }
    }
    if operands.is_empty() {
        eprintln!("sleep: missing operand\nTry 'sleep --help' for more information.");
        return EXIT_FAIL;
    }
    match total_interval(&operands) {
        Ok(interval) => {
            pause(interval);
            EXIT_OK
        }
        Err(message) => report_usage("sleep", &UsageError { message }, EXIT_FAIL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unit_suffixes_scale_the_number() {
        assert_eq!(parse_interval("2"), Some(Interval::Seconds(2.0)));
        assert_eq!(parse_interval("2s"), Some(Interval::Seconds(2.0)));
        assert_eq!(parse_interval("1m"), Some(Interval::Seconds(60.0)));
        assert_eq!(parse_interval("1h"), Some(Interval::Seconds(3600.0)));
        assert_eq!(parse_interval("1d"), Some(Interval::Seconds(86400.0)));
    }

    #[test]
    fn decimals_and_exponents_are_accepted() {
        assert_eq!(parse_interval(".5"), Some(Interval::Seconds(0.5)));
        assert_eq!(parse_interval("0.25m"), Some(Interval::Seconds(15.0)));
        assert_eq!(parse_interval("1e-2"), Some(Interval::Seconds(0.01)));
    }

    #[test]
    fn invalid_operands_are_rejected() {
        for text in ["", "x", "1x", "1S", "nan", "-1", "1ms", "e", "0x1"] {
            assert_eq!(parse_interval(text), None, "{text:?}");
        }
    }

    #[test]
    fn infinity_is_forever() {
        assert_eq!(parse_interval("inf"), Some(Interval::Forever));
        assert_eq!(parse_interval("infinity"), Some(Interval::Forever));
        assert_eq!(parse_interval("1e400"), Some(Interval::Forever));
    }

    #[test]
    fn operands_are_summed() {
        assert_eq!(
            total_interval(&words(&["0.5", "1m", "0"])),
            Ok(Interval::Seconds(60.5))
        );
    }

    #[test]
    fn first_invalid_operand_is_reported() {
        assert_eq!(
            total_interval(&words(&["1", "bad", "2"])),
            Err("invalid time interval 'bad'".to_owned())
        );
    }

    #[test]
    fn infinite_operand_wins_over_the_sum() {
        assert_eq!(total_interval(&words(&["1", "inf"])), Ok(Interval::Forever));
    }

    #[test]
    fn zero_pause_returns_immediately() {
        pause(Interval::Seconds(0.0));
    }
}
