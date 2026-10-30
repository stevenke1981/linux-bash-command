//! `timeout`: runs a command with a time limit. On expiry the command gets a signal (TERM by
//! default), optionally followed by KILL after `--kill-after`. Exit statuses follow GNU:
//! 124 when the limit expired, 137 when KILL was the last signal sent, 125 for timeout's own
//! failures, 126 and 127 when the command cannot be run, and otherwise the command's status.

#[cfg(unix)]
#[path = "timeout/unix.rs"]
mod platform;

#[cfg(windows)]
#[path = "timeout/windows.rs"]
mod platform;

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::common::args::{Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::process::{exit_status_code, resolve_command, spawn_error_status};
use std::process::ExitStatus;

use super::signal::{signal_name, signal_number};

const EXIT_TIMED_OUT: i32 = 124;
const EXIT_TIMEOUT_FAILED: i32 = 125;
const EXIT_KILLED: i32 = 137;
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

const HELP: &str = r#"Usage: timeout [OPTION] DURATION COMMAND [ARG]...
  or:  timeout [OPTION]
Start COMMAND, and kill it if still running after DURATION.

Options:
  -f, --foreground       run COMMAND in the foreground (Unix: do not signal its
                         process group)
  -k, --kill-after=DUR   also send KILL if COMMAND is still running DUR after the
                         first signal
  -p, --preserve-status  exit with the status of COMMAND, even on timeout
  -s, --signal=SIGNAL    signal to send on timeout (default TERM)
  -v, --verbose          diagnose to stderr any signal sent upon timeout
      --help             display this help and exit
      --version          output version information and exit

DURATION is a number with an optional suffix: 's' (seconds, the default), 'm',
'h' or 'd'. A DURATION of 0 disables the timeout.

Exit status:
  124  if COMMAND times out, and --preserve-status is not set
  125  if the timeout command itself fails
  126  if COMMAND is found but cannot be invoked
  127  if COMMAND cannot be found
  137  if COMMAND is sent the KILL signal (128+9)
  the exit status of COMMAND otherwise
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("foreground", Some('f'), Some("foreground")),
    OptSpec::req("kill-after", Some('k'), Some("kill-after")),
    OptSpec::flag("preserve", Some('p'), Some("preserve-status")),
    OptSpec::req("signal", Some('s'), Some("signal")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Options after the parsed ones.
struct Options {
    foreground: bool,
    preserve: bool,
    verbose: bool,
    signal: i32,
    kill_after: Option<Duration>,
}

/// Parses a sleep-style duration. `Ok(None)` means "no limit" (zero or infinite).
fn parse_duration(text: &str) -> Result<Option<Duration>, ()> {
    if text.eq_ignore_ascii_case("inf") || text.eq_ignore_ascii_case("infinity") {
        return Ok(None);
    }
    let (number, scale) = match text.as_bytes().last() {
        Some(b's') => (&text[..text.len() - 1], 1.0),
        Some(b'm') => (&text[..text.len() - 1], 60.0),
        Some(b'h') => (&text[..text.len() - 1], 3600.0),
        Some(b'd') => (&text[..text.len() - 1], 86400.0),
        _ => (text, 1.0),
    };
    let plausible = !number.is_empty()
        && number
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !plausible {
        return Err(());
    }
    let value: f64 = number.parse().map_err(|_| ())?;
    if value.is_nan() || value < 0.0 {
        return Err(());
    }
    let seconds = value * scale;
    if seconds == 0.0 || seconds.is_infinite() {
        return Ok(None);
    }
    Duration::try_from_secs_f64(seconds)
        .map(Some)
        .map_err(|_| ())
}

fn print_help() -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(HELP.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("timeout: write error: {error}");
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output("timeout", &mut out)
}

fn usage_failure(message: &str) -> i32 {
    eprintln!("timeout: {message}\nTry 'timeout --help' for more information.");
    EXIT_TIMEOUT_FAILED
}

/// Waits up to `limit` for the child. `Ok(None)` means it was still running.
fn wait_for(
    proc: &mut platform::Monitored,
    limit: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = proc.child().try_wait()? {
            return Ok(Some(status));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

fn wait_forever(proc: &mut platform::Monitored) -> std::io::Result<ExitStatus> {
    proc.child().wait()
}

fn send_verbose(options: &Options, command: &str, signal: i32) {
    if options.verbose {
        let name = signal_name(signal).unwrap_or("TERM");
        eprintln!("timeout: sending signal {name} to command '{command}'");
    }
}

/// Runs the command under the time limit and returns timeout's exit status.
fn supervise(
    proc: &mut platform::Monitored,
    limit: Duration,
    options: &Options,
    command: &str,
) -> i32 {
    match wait_for(proc, limit) {
        Ok(Some(status)) => return exit_status_code(status),
        Ok(None) => {}
        Err(error) => {
            eprintln!(
                "timeout: error waiting for command: {}",
                io_error_text(&error)
            );
            return EXIT_TIMEOUT_FAILED;
        }
    }
    send_verbose(options, command, options.signal);
    proc.send(options.signal);
    let mut last_signal = options.signal;
    if options.signal == 0 {
        return EXIT_TIMED_OUT;
    }
    let exit = match options.kill_after {
        Some(extra) => match wait_for(proc, extra) {
            Ok(Some(status)) => Some(status),
            Ok(None) => {
                send_verbose(options, command, SIGKILL);
                proc.send(SIGKILL);
                last_signal = SIGKILL;
                wait_forever(proc).ok()
            }
            Err(_) => None,
        },
        None => wait_forever(proc).ok(),
    };
    if options.preserve {
        return exit.map_or(EXIT_TIMEOUT_FAILED, exit_status_code);
    }
    if last_signal == SIGKILL {
        EXIT_KILLED
    } else {
        EXIT_TIMED_OUT
    }
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("timeout", SPECS).no_permute().parse(args) {
        Ok(items) => items,
        Err(error) => {
            eprintln!(
                "timeout: {}\nTry 'timeout --help' for more information.",
                error.message
            );
            return EXIT_TIMEOUT_FAILED;
        }
    };
    let mut options = Options {
        foreground: false,
        preserve: false,
        verbose: false,
        signal: SIGTERM,
        kill_after: None,
    };
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return print_help(),
            Item::Opt { id: "version", .. } => return print_version("timeout"),
            Item::Opt {
                id: "foreground", ..
            } => options.foreground = true,
            Item::Opt { id: "preserve", .. } => options.preserve = true,
            Item::Opt { id: "verbose", .. } => options.verbose = true,
            Item::Opt {
                id: "signal",
                value,
            } => {
                let text = value.unwrap_or_default();
                match signal_number(&text) {
                    Some(signal) => options.signal = signal,
                    None => {
                        eprintln!("timeout: '{text}': invalid signal\nTry 'timeout --help' for more information.");
                        return EXIT_TIMEOUT_FAILED;
                    }
                }
            }
            Item::Opt {
                id: "kill-after",
                value,
            } => {
                let text = value.unwrap_or_default();
                match parse_duration(&text) {
                    Ok(limit) => options.kill_after = limit,
                    Err(()) => return usage_failure(&format!("invalid time interval '{text}'")),
                }
            }
            Item::Opt { .. } => {}
            Item::Operand(text) => operands.push(text),
        }
    }
    if operands.len() < 2 {
        return usage_failure("missing operand");
    }
    let limit = match parse_duration(&operands[0]) {
        Ok(limit) => limit,
        Err(()) => return usage_failure(&format!("invalid time interval '{}'", operands[0])),
    };
    run_command(&operands[1], &operands[2..], limit, &options)
}

fn run_command(
    command: &str,
    arguments: &[String],
    limit: Option<Duration>,
    options: &Options,
) -> i32 {
    let program = resolve_command(command).unwrap_or_else(|| PathBuf::from(command));
    let mut proc = match platform::spawn(&program, arguments, options.foreground) {
        Ok(proc) => proc,
        Err(error) => {
            eprintln!(
                "timeout: failed to run command '{command}': {}",
                io_error_text(&error)
            );
            return spawn_error_status(&error);
        }
    };
    match limit {
        None => match wait_forever(&mut proc) {
            Ok(status) => exit_status_code(status),
            Err(error) => {
                eprintln!(
                    "timeout: error waiting for command: {}",
                    io_error_text(&error)
                );
                EXIT_TIMEOUT_FAILED
            }
        },
        Some(limit) => supervise(&mut proc, limit, options, command),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_sleep_suffixes() {
        assert_eq!(parse_duration("2"), Ok(Some(Duration::from_secs(2))));
        assert_eq!(
            parse_duration("1.5s"),
            Ok(Some(Duration::from_millis(1500)))
        );
        assert_eq!(parse_duration("1m"), Ok(Some(Duration::from_secs(60))));
        assert_eq!(parse_duration("0.5"), Ok(Some(Duration::from_millis(500))));
    }

    #[test]
    fn zero_and_infinity_disable_the_limit() {
        assert_eq!(parse_duration("0"), Ok(None));
        assert_eq!(parse_duration("0d"), Ok(None));
        assert_eq!(parse_duration("inf"), Ok(None));
    }

    #[test]
    fn invalid_durations_are_rejected() {
        for text in ["", "x", "-1", "nan", "1S", "1ms"] {
            assert_eq!(parse_duration(text), Err(()), "{text}");
        }
    }

    #[test]
    fn sigkill_exit_status_is_137() {
        assert_eq!(EXIT_KILLED, 128 + SIGKILL);
    }

    #[test]
    fn help_lists_every_option() {
        for option in [
            "--foreground",
            "--kill-after",
            "--preserve-status",
            "--signal",
            "--verbose",
        ] {
            assert!(HELP.contains(option), "{option}");
        }
    }
}
