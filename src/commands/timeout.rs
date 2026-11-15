use crate::common::{
    args::{self, Item, OptSpec},
    error, process,
};
use std::{
    ffi::OsStr,
    process::{Child, Command},
    time::{Duration, Instant},
};

#[cfg(unix)]
#[path = "timeout/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "timeout/windows.rs"]
mod platform;

const HELP: &str = "Usage: timeout [OPTION] DURATION COMMAND [ARG]...\nRun COMMAND and terminate it if DURATION elapses.\n  -f, --foreground        signal only the command on Unix\n  -k, --kill-after=TIME   send KILL after TIME\n  -p, --preserve-status   return the command's status after timeout\n  -s, --signal=SIGNAL     select the initial signal\n  -v, --verbose           report signals sent\n      --help              display this help\n      --version           output version information\nExit status: 124 timeout, 125 internal failure, 126 cannot execute, 127 not found.\n";

#[derive(Default)]
struct Options {
    foreground: bool,
    preserve: bool,
    verbose: bool,
    signal: i32,
    kill_after: Option<Duration>,
}

enum SpawnError {
    Command(std::io::Error),
    #[cfg(windows)]
    Monitor(std::io::Error),
}

fn duration(text: &str) -> Option<Duration> {
    let (num, scale) = match text.as_bytes().last() {
        Some(b's') => (&text[..text.len() - 1], 1.0),
        Some(b'm') => (&text[..text.len() - 1], 60.0),
        Some(b'h') => (&text[..text.len() - 1], 3600.0),
        Some(b'd') => (&text[..text.len() - 1], 86400.0),
        _ => (text, 1.0),
    };
    let n = num.parse::<f64>().ok()? * scale;
    if n < 0.0 || n.is_nan() {
        return None;
    }
    if n.is_infinite() {
        return Some(Duration::MAX);
    }
    Duration::try_from_secs_f64(n).ok()
}

fn specs() -> [OptSpec; 5] {
    [
        OptSpec::flag("foreground", 'f'),
        OptSpec::value("kill-after", 'k'),
        OptSpec::flag("preserve-status", 'p'),
        OptSpec::value("signal", 's'),
        OptSpec::flag("verbose", 'v'),
    ]
}
fn options(argv: &[String]) -> Result<(Options, Duration, Vec<String>), i32> {
    let items = args::parse_command("timeout", argv, HELP, &specs(), false).map_err(|code| {
        if code == 0 {
            0
        } else {
            125
        }
    })?;
    let mut opts = Options {
        signal: 15,
        ..Options::default()
    };
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt("foreground", _) => opts.foreground = true,
            Item::Opt("preserve-status", _) => opts.preserve = true,
            Item::Opt("verbose", _) => opts.verbose = true,
            Item::Opt("kill-after", Some(v)) => {
                opts.kill_after = Some(duration(&v).ok_or_else(|| {
                    error::usage("timeout", format!("invalid time interval '{v}'"), 125)
                })?)
                .filter(|d| *d != Duration::ZERO)
            }
            Item::Opt("signal", Some(v)) => {
                opts.signal = super::kill::signal_number(&v)
                    .ok_or_else(|| error::usage("timeout", format!("invalid signal '{v}'"), 125))?
            }
            Item::Operand(v) => operands.push(v),
            _ => {}
        }
    }
    if operands.len() < 2 {
        return Err(error::usage("timeout", "missing operand", 125));
    }
    let interval = duration(&operands[0]).ok_or_else(|| {
        error::usage(
            "timeout",
            format!("invalid time interval '{}'", operands[0]),
            125,
        )
    })?;
    Ok((opts, interval, operands[1..].to_vec()))
}

fn signal_notice(command: &str, signal: i32) {
    let name = super::kill::signal_name(signal).unwrap_or_else(|| signal.to_string());
    eprintln!("timeout: sending signal {name} to command '{command}'");
}

fn wait(
    child: &mut Child,
    group: &platform::Group,
    command: &str,
    duration: Duration,
    opts: &Options,
) -> std::io::Result<i32> {
    let started = Instant::now();
    let mut timed_out = false;
    let mut sent_at = None;
    let mut killed = false;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(if timed_out && !opts.preserve {
                if killed {
                    137
                } else {
                    124
                }
            } else {
                process::status_code(status)
            });
        }
        if !timed_out && duration != Duration::ZERO && started.elapsed() >= duration {
            timed_out = true;
            sent_at = Some(Instant::now());
            if opts.verbose {
                signal_notice(command, opts.signal);
            }
            group.signal(child, opts.signal)?;
            killed = opts.signal == 9;
        }
        if timed_out
            && opts
                .kill_after
                .is_some_and(|after| sent_at.is_some_and(|sent| sent.elapsed() >= after))
        {
            if opts.verbose {
                signal_notice(command, 9);
            }
            group.signal(child, 9)?;
            killed = true;
            sent_at = None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn run(argv: &[String]) -> i32 {
    let (opts, interval, operands) = match options(argv) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let environment: Vec<_> = std::env::vars_os().collect();
    let mut command: Command = match process::command(OsStr::new(&operands[0]), &environment) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "timeout: failed to run command '{}': {}",
                operands[0],
                error::io_text(&e)
            );
            return process::error_code(&e);
        }
    };
    command.args(&operands[1..]);
    let (mut child, group) = match platform::spawn(&mut command, opts.foreground) {
        Ok(v) => v,
        Err(SpawnError::Command(e)) => {
            eprintln!(
                "timeout: failed to run command '{}': {}",
                operands[0],
                error::io_text(&e)
            );
            return process::error_code(&e);
        }
        #[cfg(windows)]
        Err(SpawnError::Monitor(e)) => {
            eprintln!("timeout: {}", error::io_text(&e));
            return 125;
        }
    };
    match wait(&mut child, &group, &operands[0], interval, &opts) {
        Ok(status) => status,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("timeout: {}", error::io_text(&e));
            125
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_durations() {
        for (text, seconds) in [
            ("0", 0),
            ("1", 1),
            ("1s", 1),
            ("1m", 60),
            ("2h", 7200),
            ("1d", 86400),
        ] {
            assert_eq!(duration(text), Some(Duration::from_secs(seconds)));
        }
        assert_eq!(duration(".5s"), Some(Duration::from_millis(500)));
    }
    #[test]
    fn invalid_durations() {
        for text in ["", "-1", "nan", "1x", "s"] {
            assert_eq!(duration(text), None);
        }
        assert_eq!(duration("inf"), Some(Duration::MAX));
    }
    #[test]
    fn nonpermuting() {
        let args = ["1", "cmd", "--verbose"].map(str::to_owned);
        let (_, _, a) = options(&args).unwrap();
        assert_eq!(a, ["cmd", "--verbose"]);
    }
    #[test]
    fn option_values() {
        let args = ["-fvp", "-s", "HUP", "-k", "2s", "1m", "cmd"].map(str::to_owned);
        let (o, d, _) = options(&args).unwrap();
        assert!(o.foreground && o.verbose && o.preserve);
        assert_eq!(o.signal, 1);
        assert_eq!(o.kill_after, Some(Duration::from_secs(2)));
        assert_eq!(d, Duration::from_secs(60));
    }
    #[test]
    fn zero_kill_after_is_disabled() {
        let args = ["-k0", "1", "cmd"].map(str::to_owned);
        let (options, _, _) = options(&args).unwrap();
        assert_eq!(options.kill_after, None);
    }
}
