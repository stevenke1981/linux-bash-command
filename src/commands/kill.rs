use crate::common::{args, error, io};
use std::io as stdio;

const HELP: &str = "Usage: kill [-s SIGNAL | -SIGNAL | -n NUMBER] PID...\n       kill -l [SIGNAL] | -L\nSend a signal to each specified process.\n  -s, --signal=SIGNAL  signal name or number\n  -n NUMBER            signal number\n  -l, --list[=SIGNAL]  list signal names or convert one value\n  -L, --table          list names and numbers\n      --help           display this help\n      --version        output version information\n";

const SIGNALS: [&str; 32] = [
    "0", "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "KILL", "USR1", "SEGV", "USR2",
    "PIPE", "ALRM", "TERM", "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN", "TTOU", "URG",
    "XCPU", "XFSZ", "VTALRM", "PROF", "WINCH", "IO", "PWR", "SYS",
];

pub(crate) fn signal_number(text: &str) -> Option<i32> {
    let normalized = text.to_ascii_uppercase();
    let text = normalized.strip_prefix("SIG").unwrap_or(&normalized);
    if let Ok(n) = text.parse::<i32>() {
        #[cfg(target_os = "macos")]
        return if n == 0 {
            Some(0)
        } else {
            (1..=32).find(|&linux| native_signal(linux) == Some(n))
        };
        #[cfg(not(target_os = "macos"))]
        return (0..=64).contains(&n).then_some(n);
    }
    let name = match text {
        "IOT" => "ABRT",
        "CLD" => "CHLD",
        "POLL" => "IO",
        other => other,
    };
    #[cfg(target_os = "macos")]
    if name == "INFO" {
        return Some(32);
    }
    if let Some(index) = SIGNALS.iter().position(|s| *s == name) {
        #[cfg(target_os = "macos")]
        if native_signal(index as i32).is_none() {
            return None;
        }
        return Some(index as i32);
    }
    #[cfg(target_os = "macos")]
    return None;
    #[cfg(not(target_os = "macos"))]
    realtime_signal(name)
}

#[cfg(not(target_os = "macos"))]
fn realtime_signal(name: &str) -> Option<i32> {
    if name == "RTMIN" {
        return Some(34);
    }
    if name == "RTMAX" {
        return Some(64);
    }
    if let Some(n) = name
        .strip_prefix("RTMIN+")
        .and_then(|s| s.parse::<i32>().ok())
    {
        return (0..=30).contains(&n).then_some(34 + n);
    }
    if let Some(n) = name
        .strip_prefix("RTMAX-")
        .and_then(|s| s.parse::<i32>().ok())
    {
        return (0..=30).contains(&n).then_some(64 - n);
    }
    None
}

pub(crate) fn signal_name(number: i32) -> Option<String> {
    #[cfg(target_os = "macos")]
    if number == 32 {
        return Some("INFO".into());
    }
    if (1..=31).contains(&number) {
        return Some(SIGNALS[number as usize].into());
    }
    if (34..=64).contains(&number) {
        return Some(if number == 34 {
            "RTMIN".into()
        } else if number == 64 {
            "RTMAX".into()
        } else if number <= 49 {
            format!("RTMIN+{}", number - 34)
        } else {
            format!("RTMAX-{}", 64 - number)
        });
    }
    None
}

#[cfg(unix)]
fn native_signal(number: i32) -> Option<i32> {
    Some(match number {
        0 => 0,
        1 => libc::SIGHUP,
        2 => libc::SIGINT,
        3 => libc::SIGQUIT,
        4 => libc::SIGILL,
        5 => libc::SIGTRAP,
        6 => libc::SIGABRT,
        7 => libc::SIGBUS,
        8 => libc::SIGFPE,
        9 => libc::SIGKILL,
        10 => libc::SIGUSR1,
        11 => libc::SIGSEGV,
        12 => libc::SIGUSR2,
        13 => libc::SIGPIPE,
        14 => libc::SIGALRM,
        15 => libc::SIGTERM,
        17 => libc::SIGCHLD,
        18 => libc::SIGCONT,
        19 => libc::SIGSTOP,
        20 => libc::SIGTSTP,
        21 => libc::SIGTTIN,
        22 => libc::SIGTTOU,
        23 => libc::SIGURG,
        24 => libc::SIGXCPU,
        25 => libc::SIGXFSZ,
        26 => libc::SIGVTALRM,
        27 => libc::SIGPROF,
        28 => libc::SIGWINCH,
        29 => libc::SIGIO,
        31 => libc::SIGSYS,
        #[cfg(target_os = "macos")]
        32 => libc::SIGINFO,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        16 => libc::SIGSTKFLT,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        30 => libc::SIGPWR,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        34..=64 => number,
        _ => return None,
    })
}

fn signal_catalog() -> Vec<(i32, String)> {
    #[cfg(target_os = "macos")]
    {
        (1..=32)
            .filter_map(|internal| Some((native_signal(internal)?, signal_name(internal)?)))
            .collect::<std::collections::BTreeMap<_, _>>()
            .into_iter()
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        (1..=31)
            .chain(34..=64)
            .map(|n| (n, signal_name(n).unwrap()))
            .collect()
    }
}

#[cfg(unix)]
pub(crate) fn send(pid: i32, signal: i32) -> stdio::Result<()> {
    let native =
        native_signal(signal).ok_or_else(|| stdio::Error::from(stdio::ErrorKind::InvalidInput))?;
    // SAFETY: kill receives a numeric PID and a platform signal number.
    if unsafe { libc::kill(pid, native) } == 0 {
        Ok(())
    } else {
        Err(stdio::Error::last_os_error())
    }
}

#[cfg(windows)]
pub(crate) fn send(pid: i32, signal: i32) -> stdio::Result<()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        },
    };
    if pid <= 0 {
        return Err(stdio::Error::new(
            stdio::ErrorKind::Unsupported,
            "process groups are unsupported",
        ));
    }
    let access = if signal == 0 {
        PROCESS_QUERY_LIMITED_INFORMATION
    } else {
        PROCESS_TERMINATE
    };
    // SAFETY: OpenProcess returns an owned handle, which is closed on every path.
    let handle = unsafe { OpenProcess(access, 0, pid as u32) };
    if handle.is_null() {
        return Err(stdio::Error::last_os_error());
    }
    // SAFETY: the handle belongs to this process and is valid until CloseHandle.
    let result = if signal == 0 || unsafe { TerminateProcess(handle, 1) } != 0 {
        Ok(())
    } else {
        Err(stdio::Error::last_os_error())
    };
    unsafe { CloseHandle(handle) };
    result
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn send(_: i32, _: i32) -> stdio::Result<()> {
    Err(stdio::Error::from(stdio::ErrorKind::Unsupported))
}

enum Action {
    Send(i32, Vec<String>),
    List(Vec<String>),
    Table,
}
fn list_option(argv: &[String], i: usize) -> Option<Result<Action, String>> {
    let arg = &argv[i];
    if arg == "-l" || arg == "--list" {
        return Some(Ok(Action::List(argv[i + 1..].to_vec())));
    }
    if let Some(value) = arg.strip_prefix("--list=") {
        return Some(Ok(Action::List(vec![value.into()])));
    }
    if arg == "-L" || arg == "--table" {
        return Some(if i + 1 == argv.len() {
            Ok(Action::Table)
        } else {
            Err("too many arguments".into())
        });
    }
    None
}
fn attached_signal(arg: &str) -> Option<&str> {
    arg.strip_prefix("-s")
        .or_else(|| arg.strip_prefix("-n"))
        .filter(|s| !s.is_empty())
        .or_else(|| arg.strip_prefix("--signal="))
}
fn parse(argv: &[String]) -> Result<Action, String> {
    let mut i = 0;
    let mut signal = 15;
    let mut signal_selected = false;
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--" {
            i += 1;
            break;
        }
        if let Some(action) = list_option(argv, i) {
            return action;
        }
        if arg == "-s" || arg == "-n" || arg == "--signal" {
            i += 1;
            let value = argv
                .get(i)
                .ok_or_else(|| format!("option '{arg}' requires an argument"))?;
            signal = signal_number(value).ok_or_else(|| format!("unknown signal '{value}'"))?;
            signal_selected = true;
            i += 1;
            continue;
        }
        if let Some(value) = attached_signal(arg) {
            signal = signal_number(value).ok_or_else(|| format!("unknown signal '{value}'"))?;
            signal_selected = true;
            i += 1;
            continue;
        }
        if signal_selected {
            break;
        }
        if let Some(value) = arg.strip_prefix('-').filter(|s| !s.is_empty()) {
            signal = signal_number(value).ok_or_else(|| format!("unknown signal '{value}'"))?;
            i += 1;
            break;
        }
        break;
    }
    if i == argv.len() {
        return Err("not enough arguments".into());
    }
    Ok(Action::Send(signal, argv[i..].to_vec()))
}

fn print_list(values: &[String], table: bool) -> i32 {
    if !values.is_empty() {
        if values.len() != 1 {
            return error::usage("kill", "too many arguments", 1);
        }
        let value = &values[0];
        let parsed = signal_number(value).or_else(|| {
            value
                .parse::<i32>()
                .ok()
                .and_then(|n| n.checked_sub(128))
                .and_then(|n| signal_number(&n.to_string()))
        });
        let Some(number) = parsed else {
            return error::usage("kill", format!("unknown signal '{value}'"), 1);
        };
        return io::output("kill", |out| {
            if value.chars().all(|c| c.is_ascii_digit()) {
                writeln!(
                    out,
                    "{}",
                    signal_name(number).unwrap_or_else(|| number.to_string())
                )
            } else {
                #[cfg(target_os = "macos")]
                let number = native_signal(number).unwrap_or(number);
                writeln!(out, "{number}")
            }
        });
    }
    io::output("kill", |out| {
        for (n, name) in signal_catalog() {
            if table {
                writeln!(out, "{n:2} {name}")?;
            } else {
                write!(out, "{}{name}", if n == 1 { "" } else { " " })?;
            }
        }
        if !table {
            writeln!(out)?;
        }
        Ok(())
    })
}

pub fn run(argv: &[String]) -> i32 {
    if argv
        .first()
        .is_some_and(|s| s == "--help" || s == "--version")
    {
        return args::help_version("kill", argv, HELP, true).unwrap_or(0);
    }
    match parse(argv) {
        Err(e) => error::usage("kill", e, 1),
        Ok(Action::List(values)) => print_list(&values, false),
        Ok(Action::Table) => print_list(&[], true),
        Ok(Action::Send(signal, pids)) => {
            let mut failed = false;
            for value in pids {
                match value.parse::<i32>() {
                    Ok(pid) => {
                        if let Err(e) = send(pid, signal) {
                            eprintln!("kill: ({pid}) - {}", error::io_text(&e));
                            failed = true;
                        }
                    }
                    Err(_) => {
                        eprintln!("kill: '{value}': not a process ID");
                        failed = true;
                    }
                }
            }
            i32::from(failed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_names() {
        for (s, n) in [
            ("HUP", 1),
            ("INT", 2),
            ("QUIT", 3),
            ("KILL", 9),
            ("USR1", 10),
            ("TERM", 15),
            ("CHLD", 17),
            ("STOP", 19),
            ("SYS", 31),
        ] {
            assert_eq!(signal_number(s), Some(n));
        }
    }
    #[test]
    fn aliases() {
        for (s, n) in [
            ("SIGTERM", 15),
            ("IOT", 6),
            ("CLD", 17),
            ("POLL", 29),
            ("0", 0),
            ("9", 9),
        ] {
            assert_eq!(signal_number(s), Some(n));
        }
        #[cfg(not(target_os = "macos"))]
        for (s, n) in [
            ("RTMIN", 34),
            ("RTMAX", 64),
            ("RTMIN+1", 35),
            ("RTMAX-1", 63),
        ] {
            assert_eq!(signal_number(s), Some(n));
        }
    }
    #[test]
    fn invalid_names() {
        for s in ["bad", "65", "-1", "RTMIN+31", "RTMAX-31", ""] {
            assert_eq!(signal_number(s), None);
        }
    }
    #[test]
    fn names_from_numbers() {
        for n in 1..=31 {
            #[cfg(target_os = "macos")]
            if native_signal(n).is_none() {
                continue;
            }
            assert_eq!(signal_number(&signal_name(n).unwrap()), Some(n));
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn mac_catalog_uses_native_numbers_and_includes_info() {
        let catalog = signal_catalog();
        for (native, name) in &catalog {
            let internal = signal_number(name).unwrap();
            assert_eq!(native_signal(internal), Some(*native));
            assert_eq!(signal_number(&native.to_string()), Some(internal));
        }
        assert!(catalog.contains(&(libc::SIGINFO, "INFO".into())));
        assert!(catalog.contains(&(libc::SIGUSR1, "USR1".into())));
    }
    #[test]
    fn attached_signal_and_group_operands() {
        for option in ["-sTERM", "-n15", "--signal=TERM"] {
            let input = [option.to_owned(), "-123".to_owned()];
            assert!(matches!(parse(&input), Ok(Action::Send(15, pids)) if pids == ["-123"]));
        }
        let input = ["-TERM".to_owned(), "-123".to_owned()];
        assert!(matches!(parse(&input), Ok(Action::Send(15, pids)) if pids == ["-123"]));
    }
}
