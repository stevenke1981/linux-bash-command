//! `tee`: copies standard input to standard output and to each FILE. Each chunk read is
//! written to every output and flushed at once, so `tee` never holds data back.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const HELP: &str = r#"Usage: tee [OPTION]... [FILE]...
Copy standard input to each FILE, and also to standard output.

  -a, --append              append to the given FILEs, do not overwrite
  -i, --ignore-interrupts   ignore interrupt signals
  -p                        operate in a more appropriate MODE with pipes;
                            same as --output-error=warn-nopipe
      --output-error[=MODE]   set behavior on write error.  See MODE below
      --help                display this help and exit
      --version             output version information and exit

MODE determines behavior with write errors on the outputs:
  'warn'         diagnose errors writing to any output
  'warn-nopipe'  diagnose errors writing to any output not a pipe
  'exit'         exit on error writing to any output
  'exit-nopipe'  exit on error writing to any output not a pipe
The default MODE for the -p option is 'warn-nopipe'.
Without --output-error, tee exits on a write error to a pipe and
diagnoses errors on other outputs, then continues.
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("append", Some('a'), Some("append")),
    OptSpec::flag("ignore", Some('i'), Some("ignore-interrupts")),
    OptSpec::flag("pipe", Some('p'), None),
    OptSpec::opt("output-error", None, Some("output-error")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Read buffer; `read` returns what is available, so a short read is written at once.
const CHUNK: usize = 64 * 1024;

/// Accepted `--output-error` modes, in GNU's listing order.
const MODES: [(&str, OutputError); 4] = [
    ("warn", OutputError::Warn),
    ("warn-nopipe", OutputError::WarnNopipe),
    ("exit", OutputError::Exit),
    ("exit-nopipe", OutputError::ExitNopipe),
];

const VALID_MODES: &str =
    "Valid arguments are:\n  - 'warn'\n  - 'warn-nopipe'\n  - 'exit'\n  - 'exit-nopipe'";

/// How a write error on a file output is handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputError {
    Warn,
    WarnNopipe,
    Exit,
    ExitNopipe,
}

/// What to do after a failed write to one file output.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// Stop writing to this output; optionally report it and mark the run failed.
    Drop { warn: bool, fail: bool },
    /// Report the error and exit with failure.
    Exit,
}

#[derive(Default)]
struct Options {
    append: bool,
    ignore_interrupts: bool,
    mode: Option<OutputError>,
    files: Vec<String>,
}

/// Matches a mode name exactly, else by unique prefix (GNU argmatch).
fn parse_mode(text: &str) -> Result<OutputError, String> {
    if let Some((_, mode)) = MODES.iter().find(|(name, _)| *name == text) {
        return Ok(*mode);
    }
    let candidates: Vec<&(&str, OutputError)> = MODES
        .iter()
        .filter(|(name, _)| name.starts_with(text))
        .collect();
    match candidates.as_slice() {
        [(_, mode)] => Ok(*mode),
        [] => Err(format!(
            "invalid argument '{text}' for '--output-error'\n{VALID_MODES}"
        )),
        _ => Err(format!(
            "ambiguous argument '{text}' for '--output-error'\n{VALID_MODES}"
        )),
    }
}

/// Decides the reaction to a failed write. A pipe is one whose reader has gone.
fn on_write_error(mode: Option<OutputError>, is_pipe: bool) -> Action {
    use OutputError::*;
    match (mode, is_pipe) {
        (None, true) | (Some(Exit), _) | (Some(ExitNopipe), false) => Action::Exit,
        (None, false) | (Some(Warn), _) | (Some(WarnNopipe), false) => Action::Drop {
            warn: true,
            fail: true,
        },
        (Some(WarnNopipe), true) | (Some(ExitNopipe), true) => Action::Drop {
            warn: false,
            fail: false,
        },
    }
}

/// Opens every FILE, reporting those that fail and continuing with the rest.
fn open_outputs(files: &[String], append: bool, status: &mut i32) -> Vec<(String, File)> {
    let mut outputs = Vec::new();
    for name in files {
        let opened = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(!append)
            .append(append)
            .open(name);
        match opened {
            Ok(file) => outputs.push((name.clone(), file)),
            Err(error) => {
                report("tee", name, &error);
                *status = EXIT_FAIL;
            }
        }
    }
    outputs
}

/// Writes `chunk` to every file output. Returns an exit status if `tee` must stop.
fn write_outputs(
    outputs: &mut Vec<(String, File)>,
    chunk: &[u8],
    mode: Option<OutputError>,
    status: &mut i32,
) -> Option<i32> {
    let mut index = 0;
    while index < outputs.len() {
        let (name, file) = &mut outputs[index];
        match file.write_all(chunk) {
            Ok(()) => index += 1,
            Err(error) => {
                let action = on_write_error(mode, is_broken_pipe(&error));
                if action == Action::Exit {
                    report("tee", name.as_str(), &error);
                    return Some(EXIT_FAIL);
                }
                if let Action::Drop { warn: true, fail } = action {
                    report("tee", name.as_str(), &error);
                    if fail {
                        *status = EXIT_FAIL;
                    }
                }
                outputs.remove(index);
            }
        }
    }
    None
}

/// Standard output failed: a closed pipe is silent success, anything else is fatal.
fn stdout_failure(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    report("tee", "standard output", error);
    EXIT_FAIL
}

/// Copies stdin until end of input. Returns the exit status.
fn copy_input(opts: &Options) -> i32 {
    let mut status = EXIT_OK;
    let mut outputs = open_outputs(&opts.files, opts.append, &mut status);
    let mut out = stdout_writer();
    let mut input = io::stdin().lock();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let count = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                report("tee", "standard input", &error);
                status = EXIT_FAIL;
                break;
            }
        };
        let chunk = &buf[..count];
        if let Err(error) = out.write_all(chunk).and_then(|()| out.flush()) {
            return stdout_failure(&error);
        }
        if let Some(code) = write_outputs(&mut outputs, chunk, opts.mode, &mut status) {
            return code;
        }
    }
    match finish_output("tee", &mut out) {
        EXIT_OK => status,
        code => code,
    }
}

#[cfg(unix)]
fn ignore_interrupts() {
    // SAFETY: setting SIGINT to SIG_IGN has no memory-safety preconditions.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
    }
}

#[cfg(not(unix))]
fn ignore_interrupts() {}

/// Writes the help text to stdout. A closed pipe is silent success.
fn print_help() -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(HELP.as_bytes()) {
        return stdout_failure(&error);
    }
    finish_output("tee", &mut out)
}

/// Collects options; `Err` carries the exit status for an immediate return.
fn collect_options(items: Vec<Item>) -> Result<Options, i32> {
    let mut opts = Options::default();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return Err(print_help()),
            Item::Opt { id: "version", .. } => return Err(print_version("tee")),
            Item::Opt { id: "append", .. } => opts.append = true,
            Item::Opt { id: "ignore", .. } => opts.ignore_interrupts = true,
            Item::Opt { id: "pipe", .. } => opts.mode = Some(OutputError::WarnNopipe),
            Item::Opt {
                id: "output-error",
                value,
            } => {
                opts.mode = Some(match value {
                    None => OutputError::Warn,
                    Some(text) => parse_mode(&text).map_err(|message| {
                        report_usage("tee", &UsageError { message }, EXIT_FAIL)
                    })?,
                });
            }
            Item::Opt { .. } => {}
            Item::Operand(file) => opts.files.push(file),
        }
    }
    Ok(opts)
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("tee", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("tee", &error, EXIT_FAIL),
    };
    let opts = match collect_options(items) {
        Ok(opts) => opts,
        Err(code) => return code,
    };
    if opts.ignore_interrupts {
        ignore_interrupts();
    }
    copy_input(&opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_match_exactly_or_by_unique_prefix() {
        assert_eq!(parse_mode("warn"), Ok(OutputError::Warn));
        assert_eq!(parse_mode("warn-n"), Ok(OutputError::WarnNopipe));
        assert_eq!(parse_mode("exit-"), Ok(OutputError::ExitNopipe));
    }

    #[test]
    fn empty_and_ambiguous_modes_are_rejected() {
        assert!(parse_mode("")
            .unwrap_err()
            .starts_with("ambiguous argument ''"));
        assert!(parse_mode("warn").is_ok());
        assert!(parse_mode("bogus")
            .unwrap_err()
            .starts_with("invalid argument 'bogus'"));
    }

    #[test]
    fn default_mode_exits_on_pipes_and_continues_on_files() {
        assert_eq!(on_write_error(None, true), Action::Exit);
        assert_eq!(
            on_write_error(None, false),
            Action::Drop {
                warn: true,
                fail: true
            }
        );
    }

    #[test]
    fn nopipe_modes_ignore_pipe_errors_only() {
        assert_eq!(
            on_write_error(Some(OutputError::WarnNopipe), true),
            Action::Drop {
                warn: false,
                fail: false
            }
        );
        assert_eq!(
            on_write_error(Some(OutputError::ExitNopipe), true),
            Action::Drop {
                warn: false,
                fail: false
            }
        );
        assert_eq!(
            on_write_error(Some(OutputError::ExitNopipe), false),
            Action::Exit
        );
    }

    #[test]
    fn exit_mode_always_exits_and_warn_always_continues() {
        assert_eq!(on_write_error(Some(OutputError::Exit), false), Action::Exit);
        assert_eq!(on_write_error(Some(OutputError::Exit), true), Action::Exit);
        assert_eq!(
            on_write_error(Some(OutputError::Warn), true),
            Action::Drop {
                warn: true,
                fail: true
            }
        );
    }

    #[test]
    fn append_keeps_existing_contents() {
        let dir = std::env::temp_dir().join(format!("agentcmd-tee-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.txt");
        std::fs::write(&path, b"old\n").unwrap();
        let name = path.to_string_lossy().into_owned();
        let mut status = EXIT_OK;
        let mut outputs = open_outputs(std::slice::from_ref(&name), true, &mut status);
        assert_eq!(outputs.len(), 1);
        outputs[0].1.write_all(b"new\n").unwrap();
        drop(outputs);
        assert_eq!(std::fs::read(&path).unwrap(), b"old\nnew\n");
        let mut truncated = open_outputs(std::slice::from_ref(&name), false, &mut status);
        truncated[0].1.write_all(b"x").unwrap();
        drop(truncated);
        assert_eq!(std::fs::read(&path).unwrap(), b"x");
        assert_eq!(status, EXIT_OK);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unopenable_file_sets_failure_and_is_skipped() {
        let dir = std::env::temp_dir().join(format!("agentcmd-tee-missing-{}", std::process::id()));
        let missing = dir.join("no-such-dir").join("f");
        let mut status = EXIT_OK;
        let outputs = open_outputs(
            &[missing.to_string_lossy().into_owned()],
            false,
            &mut status,
        );
        assert!(outputs.is_empty());
        assert_eq!(status, EXIT_FAIL);
    }
}
