use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::fs::OpenOptions;
use std::io::{Read, Write};
const HELP: &str = "Usage: tee [OPTION]... [FILE]...\nCopy standard input to standard output and files.\n  -a, --append             append to files\n  -i, --ignore-interrupts  ignore interrupt signals\n  -p                      diagnose errors writing to non-pipes\n      --output-error[=MODE]  warn, warn-nopipe, exit, exit-nopipe\n      --help              display help\n      --version           display version\n";
const SPECS: &[OptSpec] = &[
    OptSpec::flag("append", 'a'),
    OptSpec::flag("ignore-interrupts", 'i'),
    OptSpec {
        id: "pipe",
        short: 'p',
        long: "",
        has_arg: args::HasArg::No,
    },
    OptSpec::optional("output-error", '\0'),
    OptSpec::flag("help", '\0'),
    OptSpec::flag("version", '\0'),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Default,
    Warn,
    WarnNopipe,
    Exit,
    ExitNopipe,
}
impl Mode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "warn" => Ok(Self::Warn),
            "warn-nopipe" => Ok(Self::WarnNopipe),
            "exit" => Ok(Self::Exit),
            "exit-nopipe" => Ok(Self::ExitNopipe),
            _ => Err(format!(
                "invalid argument '{value}' for 'output error mode'"
            )),
        }
    }
    fn report(self, e: &std::io::Error) -> bool {
        e.kind() != std::io::ErrorKind::BrokenPipe || matches!(self, Self::Warn | Self::Exit)
    }
    fn stop(self, e: &std::io::Error) -> bool {
        matches!(self, Self::Exit | Self::ExitNopipe) && self.report(e)
            || self == Self::Default && e.kind() == std::io::ErrorKind::BrokenPipe
    }
}

fn copy(input: &mut dyn Read, targets: &mut [(String, Option<Box<dyn Write>>)], mode: Mode) -> i32 {
    let mut block = [0; io::BUFFER_SIZE];
    let mut failed = false;
    loop {
        let n = match input.read(&mut block) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                eprintln!("tee: read error: {}", error::io_text(&e));
                return 1;
            }
        };
        for (name, sink) in targets.iter_mut() {
            let Some(writer) = sink else {
                continue;
            };
            if let Err(e) = writer.write_all(&block[..n]).and_then(|()| writer.flush()) {
                if mode.report(&e) {
                    eprintln!("tee: {name}: {}", error::io_text(&e));
                    failed = true;
                }
                *sink = None;
                if mode.stop(&e) {
                    return i32::from(failed);
                }
            }
        }
        if targets.iter().all(|(_, s)| s.is_none()) {
            break;
        }
    }
    i32::from(failed)
}

pub fn run(argv: &[String]) -> i32 {
    let items = match args::parse(argv, SPECS, true) {
        Ok(v) => v,
        Err(e) => return error::usage("tee", &e, 1),
    };
    if let Some(id) = items.iter().find_map(|item| match item {
        Item::Opt(id @ ("help" | "version"), _) => Some(*id),
        _ => None,
    }) {
        return args::help_version("tee", &[format!("--{id}")], HELP, true).unwrap();
    }
    let (mut names, mut append, mut ignore, mut mode) = (Vec::new(), false, false, Mode::Default);
    for item in items {
        match item {
            Item::Operand(f) => names.push(f),
            Item::Opt("append", _) => append = true,
            Item::Opt("ignore-interrupts", _) => ignore = true,
            Item::Opt("pipe", _) => mode = Mode::WarnNopipe,
            Item::Opt("output-error", v) => {
                mode = match Mode::parse(v.as_deref().unwrap_or("warn-nopipe")) {
                    Ok(m) => m,
                    Err(e) => return error::usage("tee", &e, 1),
                }
            }
            _ => {}
        }
    }
    #[cfg(unix)]
    if ignore {
        // Only the current utility process changes its SIGINT disposition.
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_IGN);
        }
    }
    #[cfg(not(unix))]
    let _ = ignore;
    let mut targets: Vec<(String, Option<Box<dyn Write>>)> =
        vec![("standard output".into(), Some(Box::new(std::io::stdout())))];
    let mut failed = false;
    for name in names {
        #[cfg(windows)]
        let path = if name == "/dev/null" {
            "NUL"
        } else {
            name.as_str()
        };
        #[cfg(not(windows))]
        let path = name.as_str();
        let target: std::io::Result<Box<dyn Write>> = match path {
            "/dev/stdout" => Ok(Box::new(std::io::stdout())),
            "/dev/stderr" => Ok(Box::new(std::io::stderr())),
            _ => OpenOptions::new()
                .write(true)
                .create(true)
                .append(append)
                .truncate(!append)
                .open(path)
                .map(|f| Box::new(f) as Box<dyn Write>),
        };
        match target {
            Ok(writer) => targets.push((name, Some(writer))),
            Err(e) => {
                eprintln!("tee: {name}: {}", error::io_text(&e));
                failed = true;
            }
        }
    }
    let code = copy(&mut std::io::stdin().lock(), &mut targets, mode);
    if failed {
        1
    } else {
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes() {
        for s in ["warn", "warn-nopipe", "exit", "exit-nopipe"] {
            assert!(Mode::parse(s).is_ok());
        }
        assert!(Mode::parse("x").is_err());
    }
    #[test]
    fn pipe_reporting() {
        let e = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
        assert!(Mode::Warn.report(&e));
        assert!(!Mode::WarnNopipe.report(&e));
    }
    #[test]
    fn exit_modes() {
        let e = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(Mode::Exit.stop(&e));
        assert!(!Mode::Warn.stop(&e));
    }
    #[test]
    fn copy_to_sink() {
        let mut targets: Vec<(String, Option<Box<dyn Write>>)> =
            vec![("sink".into(), Some(Box::new(std::io::sink())))];
        assert_eq!(
            copy(
                &mut std::io::Cursor::new(b"hello"),
                &mut targets,
                Mode::Default
            ),
            0
        );
    }
    struct Failing;
    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn broken_pipe() {
        let mut targets: Vec<(String, Option<Box<dyn Write>>)> =
            vec![("pipe".into(), Some(Box::new(Failing)))];
        assert_eq!(
            copy(&mut std::io::Cursor::new(b"x"), &mut targets, Mode::Default),
            0
        );
    }
    #[test]
    fn warn_pipe() {
        let mut targets: Vec<(String, Option<Box<dyn Write>>)> =
            vec![("pipe".into(), Some(Box::new(Failing)))];
        assert_eq!(
            copy(&mut std::io::Cursor::new(b"x"), &mut targets, Mode::Warn),
            1
        );
    }
}
