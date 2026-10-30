//! `kill`: sends signals to processes, and lists signal names. The signal table and the
//! `send_signal` primitive are shared with `timeout` and `pkill`.
//!
//! Numbers follow the Linux x86-64 table on Linux and Windows. macOS uses the libc constants
//! for the signals it has. Windows has no POSIX signals: any signal other than 0 terminates
//! the process with `TerminateProcess`, and 0 only checks that the process exists.

use std::io::{self, Write};

use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const HELP: &str = r#"Usage: kill [-s SIGNAL | -SIGNAL | -n NUMBER] PID...
  or:  kill -l [SIGNAL|NUMBER]
  or:  kill -L
Send a signal to each process PID (default TERM).

  -s, --signal=SIGNAL  name or number of the signal to send
  -n NUMBER            number of the signal to send
  -l [SIGNAL|NUMBER]   list signal names, or convert a name to a number and back
  -L                   list signal names in a table
      --help           display this help and exit
      --version        output version information and exit

A negative PID addresses a process group (Unix only).
Signal names are accepted with or without the SIG prefix, in any letter case.
"#;

/// Linux x86-64 numbering. Used on Linux and Windows.
#[cfg(any(target_os = "linux", windows))]
const SIGNALS: &[(&str, i32)] = &[
    ("HUP", 1),
    ("INT", 2),
    ("QUIT", 3),
    ("ILL", 4),
    ("TRAP", 5),
    ("ABRT", 6),
    ("BUS", 7),
    ("FPE", 8),
    ("KILL", 9),
    ("USR1", 10),
    ("SEGV", 11),
    ("USR2", 12),
    ("PIPE", 13),
    ("ALRM", 14),
    ("TERM", 15),
    ("STKFLT", 16),
    ("CHLD", 17),
    ("CONT", 18),
    ("STOP", 19),
    ("TSTP", 20),
    ("TTIN", 21),
    ("TTOU", 22),
    ("URG", 23),
    ("XCPU", 24),
    ("XFSZ", 25),
    ("VTALRM", 26),
    ("PROF", 27),
    ("WINCH", 28),
    ("POLL", 29),
    ("PWR", 30),
    ("SYS", 31),
];

/// The signals macOS and the BSDs have, with their platform numbers.
#[cfg(all(unix, not(target_os = "linux")))]
const SIGNALS: &[(&str, i32)] = &[
    ("HUP", libc::SIGHUP),
    ("INT", libc::SIGINT),
    ("QUIT", libc::SIGQUIT),
    ("ILL", libc::SIGILL),
    ("TRAP", libc::SIGTRAP),
    ("ABRT", libc::SIGABRT),
    ("BUS", libc::SIGBUS),
    ("FPE", libc::SIGFPE),
    ("KILL", libc::SIGKILL),
    ("USR1", libc::SIGUSR1),
    ("SEGV", libc::SIGSEGV),
    ("USR2", libc::SIGUSR2),
    ("PIPE", libc::SIGPIPE),
    ("ALRM", libc::SIGALRM),
    ("TERM", libc::SIGTERM),
    ("CHLD", libc::SIGCHLD),
    ("CONT", libc::SIGCONT),
    ("STOP", libc::SIGSTOP),
    ("TSTP", libc::SIGTSTP),
    ("TTIN", libc::SIGTTIN),
    ("TTOU", libc::SIGTTOU),
    ("URG", libc::SIGURG),
    ("XCPU", libc::SIGXCPU),
    ("XFSZ", libc::SIGXFSZ),
    ("VTALRM", libc::SIGVTALRM),
    ("PROF", libc::SIGPROF),
    ("WINCH", libc::SIGWINCH),
];

/// Looks up a signal by name (`TERM`, `SIGTERM`, any case) or by number (`15`).
pub(crate) fn signal_number(text: &str) -> Option<i32> {
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        let number: i32 = text.parse().ok()?;
        return (number == 0 || signal_name(number).is_some()).then_some(number);
    }
    let bare = match text.get(..3) {
        Some(prefix) if prefix.eq_ignore_ascii_case("SIG") => &text[3..],
        _ => text,
    };
    SIGNALS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(bare))
        .map(|&(_, number)| number)
}

/// The name of signal `number`, without the SIG prefix.
pub(crate) fn signal_name(number: i32) -> Option<&'static str> {
    SIGNALS
        .iter()
        .find(|&&(_, n)| n == number)
        .map(|&(name, _)| name)
}

/// Sends `sig` to `pid`. A negative `pid` addresses a process group (Unix only). Signal 0
/// only checks that the target exists.
#[cfg(unix)]
pub(crate) fn send_signal(pid: i32, sig: i32) -> io::Result<()> {
    // SAFETY: `kill` takes plain integers and has no memory-safety preconditions.
    let rc = unsafe { libc::kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Windows form of [`send_signal`]: signal 0 opens the process, any other signal
/// terminates it with exit code 1.
#[cfg(windows)]
pub(crate) fn send_signal(pid: i32, sig: i32) -> io::Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };
    if pid <= 0 {
        return Err(io::Error::other("operation not supported for this PID"));
    }
    let access = if sig == 0 {
        PROCESS_QUERY_LIMITED_INFORMATION
    } else {
        PROCESS_TERMINATE
    };
    // SAFETY: OpenProcess has no memory-safety preconditions; the handle is closed below.
    let handle = unsafe { OpenProcess(access, 0, pid as u32) };
    if handle.is_null() {
        return Err(windows_open_error());
    }
    let result = if sig == 0 {
        Ok(())
    } else {
        // SAFETY: `handle` is a valid process handle opened with PROCESS_TERMINATE.
        if unsafe { TerminateProcess(handle, 1) } != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    };
    // SAFETY: `handle` came from OpenProcess and is closed exactly once.
    unsafe { CloseHandle(handle) };
    result
}

#[cfg(windows)]
fn windows_open_error() -> io::Error {
    use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
        io::Error::from(io::ErrorKind::NotFound)
    } else {
        error
    }
}

/// Reason text for a failed signal, in procps wording for the two errors that matter most.
pub(crate) fn signal_failure_reason(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "No such process".to_string(),
        io::ErrorKind::PermissionDenied => "Operation not permitted".to_string(),
        _ => io_error_text(error),
    }
}

fn print_help() -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(HELP.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("kill: write error: {error}");
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output("kill", &mut out)
}

fn usage_error(message: &str) -> i32 {
    eprintln!("kill: {message}\nTry 'kill --help' for more information.");
    EXIT_FAIL
}

/// `kill -l [SIG|NUM]`: with an operand, converts it; without one, lists every name.
fn list_signals(operand: Option<&str>) -> i32 {
    let Some(text) = operand else {
        let names: Vec<&str> = SIGNALS.iter().map(|&(name, _)| name).collect();
        return emit(&format!("{}\n", wrap_names(&names, 80)));
    };
    if let Ok(number) = text.parse::<i32>() {
        if let Some(name) = signal_name(number) {
            return emit(&format!("{name}\n"));
        }
    } else if let Some(number) = signal_number(text).filter(|&n| n != 0) {
        return emit(&format!("{number}\n"));
    }
    eprintln!("kill: unknown signal: {text}");
    EXIT_FAIL
}

/// Joins names with single spaces, starting a new line before a name would pass `width`.
fn wrap_names(names: &[&str], width: usize) -> String {
    let mut out = String::new();
    let mut line_len = 0;
    for name in names {
        if line_len > 0 && line_len + 1 + name.len() > width {
            out.push('\n');
            line_len = 0;
        } else if line_len > 0 {
            out.push(' ');
            line_len += 1;
        }
        out.push_str(name);
        line_len += name.len();
    }
    out
}

/// `kill -L`: a table of number and name pairs, seven per line. Every cell but the seventh
/// of a row is followed by a space, so a partial last row keeps its trailing space, as
/// procps prints it.
fn signal_table() -> i32 {
    let mut out = String::new();
    for (index, &(name, number)) in SIGNALS.iter().enumerate() {
        if index % 7 == 6 {
            out.push_str(&format!("{number:>2} {name}\n"));
        } else {
            out.push_str(&format!("{number:>2} {name:<7} "));
        }
    }
    if !SIGNALS.len().is_multiple_of(7) {
        out.push('\n');
    }
    emit(&out)
}

/// Writes `text` to stdout through the shared buffered writer, with the usual BrokenPipe
/// and write-error handling.
fn emit(text: &str) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("kill: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output("kill", &mut out)
}

/// What one command line asks `kill` to do.
#[derive(Debug, PartialEq)]
enum Action {
    Help,
    Version,
    Table,
    List(Option<String>),
    Send { signal: i32, pids: Vec<String> },
}

fn unknown_signal(text: &str) -> i32 {
    eprintln!("kill: unknown signal: {text}\nTry 'kill --help' for more information.");
    EXIT_FAIL
}

fn signal_value(text: &str) -> Result<i32, i32> {
    signal_number(text).ok_or_else(|| unknown_signal(text))
}

/// Parses the command line. Option parsing stops at the first PID, as `kill` does.
fn parse(args: &[String]) -> Result<Action, i32> {
    let mut signal = 15;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        match arg {
            "--" => return Ok(send(signal, args[index..].to_vec())),
            "--help" => return Ok(Action::Help),
            "--version" => return Ok(Action::Version),
            "-L" => return Ok(Action::Table),
            "-l" => return Ok(Action::List(args.get(index).cloned())),
            "-s" | "-n" => {
                let Some(value) = args.get(index) else {
                    return Err(usage_error(&format!(
                        "option requires an argument -- '{}'",
                        &arg[1..]
                    )));
                };
                index += 1;
                signal = signal_value(value)?;
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--signal=") {
                    signal = signal_value(value)?;
                } else if arg == "--signal" {
                    let Some(value) = args.get(index) else {
                        return Err(usage_error("option '--signal' requires an argument"));
                    };
                    index += 1;
                    signal = signal_value(value)?;
                } else if let Some(sig) = dash_signal(arg) {
                    signal = sig;
                } else if arg.starts_with('-') && arg.len() > 1 {
                    return Err(unknown_signal(&arg[1..]));
                } else {
                    return Ok(send(signal, args[index - 1..].to_vec()));
                }
            }
        }
    }
    Ok(Action::Send {
        signal,
        pids: Vec::new(),
    })
}

fn send(signal: i32, pids: Vec<String>) -> Action {
    Action::Send { signal, pids }
}

/// Accepts the `-SIG` and `-NUM` short forms (`-9`, `-TERM`).
fn dash_signal(arg: &str) -> Option<i32> {
    let rest = arg.strip_prefix('-')?;
    if rest.is_empty() || rest.starts_with('-') {
        return None;
    }
    signal_number(rest)
}

/// Sends `signal` to each PID. Failures are reported and the remaining PIDs still get the
/// signal; the result is 1 when any PID failed.
fn send_all(signal: i32, pids: &[String]) -> i32 {
    let mut status = EXIT_OK;
    for text in pids {
        let Ok(pid) = text.parse::<i32>() else {
            eprintln!("kill: ({text}) - No such process");
            status = EXIT_FAIL;
            continue;
        };
        if let Err(error) = send_signal(pid, signal) {
            eprintln!("kill: ({text}) - {}", signal_failure_reason(&error));
            status = EXIT_FAIL;
        }
    }
    status
}

pub fn run(args: &[String]) -> i32 {
    match parse(args) {
        Err(code) => code,
        Ok(Action::Help) => print_help(),
        Ok(Action::Version) => print_version("kill"),
        Ok(Action::Table) => signal_table(),
        Ok(Action::List(operand)) => list_signals(operand.as_deref()),
        Ok(Action::Send { pids, .. }) if pids.is_empty() => usage_error("not enough arguments"),
        Ok(Action::Send { signal, pids }) => send_all(signal, &pids),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn names_and_numbers_resolve_both_ways() {
        assert_eq!(signal_number("TERM"), Some(15));
        assert_eq!(signal_number("sigkill"), Some(9));
        assert_eq!(signal_number("15"), Some(15));
        assert_eq!(signal_number("0"), Some(0));
        assert_eq!(signal_number("99"), None);
        assert_eq!(signal_number("BOGUS"), None);
        assert_eq!(signal_name(15), Some("TERM"));
        assert_eq!(signal_name(0), None);
    }

    #[test]
    fn wrap_breaks_long_lists_at_width() {
        assert_eq!(wrap_names(&["AA", "BB", "CC"], 80), "AA BB CC");
        assert_eq!(wrap_names(&["AAAA", "BBBB", "CC"], 9), "AAAA BBBB\nCC");
        assert_eq!(wrap_names(&["AAAA", "BBBB", "CC"], 8), "AAAA\nBBBB CC");
    }

    #[test]
    fn dash_forms_are_signals() {
        assert_eq!(dash_signal("-9"), Some(9));
        assert_eq!(dash_signal("-TERM"), Some(15));
        assert_eq!(dash_signal("-x"), None);
        assert_eq!(dash_signal("--x"), None);
        assert_eq!(dash_signal("9"), None);
    }

    #[test]
    fn parse_defaults_to_term_and_takes_signal_options() {
        assert_eq!(
            parse(&strings(&["12", "34"])).ok(),
            Some(Action::Send {
                signal: 15,
                pids: strings(&["12", "34"])
            })
        );
        assert_eq!(
            parse(&strings(&["-s", "KILL", "12"])).ok(),
            Some(Action::Send {
                signal: 9,
                pids: strings(&["12"])
            })
        );
        assert_eq!(
            parse(&strings(&["-2", "7"])).ok(),
            Some(Action::Send {
                signal: 2,
                pids: strings(&["7"])
            })
        );
    }

    #[test]
    fn parse_recognises_list_table_and_help_forms() {
        assert_eq!(
            parse(&strings(&["-l", "15"])).ok(),
            Some(Action::List(Some("15".into())))
        );
        assert_eq!(parse(&strings(&["-l"])).ok(), Some(Action::List(None)));
        assert_eq!(parse(&strings(&["-L"])).ok(), Some(Action::Table));
        assert_eq!(parse(&strings(&["--help"])).ok(), Some(Action::Help));
        assert_eq!(parse(&strings(&["-s", "BOGUS", "1"])).err(), Some(1));
    }

    #[test]
    fn zero_signal_checks_the_current_process() {
        assert!(send_signal(std::process::id() as i32, 0).is_ok());
    }

    #[test]
    fn missing_process_reports_no_such_process() {
        let error = send_signal(2_000_000_000, 0).unwrap_err();
        assert_eq!(signal_failure_reason(&error), "No such process");
    }
}
