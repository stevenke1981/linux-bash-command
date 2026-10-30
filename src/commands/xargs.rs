//! `xargs`: reads items from standard input and runs a command with them as arguments,
//! in as few invocations as the limits allow. Items are split on blanks with GNU quoting,
//! or on NUL (`-0`) or a chosen delimiter (`-d`). Runs are serial (`-P` is accepted).
//! Specification: docs/spec/coreutils-alignment/03-commands-core.md.

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::escape::{unescape, EscapeMode};
use crate::common::io::{open_input, read_record};
use crate::common::process::{exit_status_code, resolve_command, spawn_error_status};

const CMD: &str = "xargs";
const EXIT_SOME_FAILED: i32 = 123;
const EXIT_COMMAND_255: i32 = 124;
const EXIT_SIGNALLED: i32 = 125;

const OPTS: &[OptSpec] = &[
    OptSpec::flag("0", Some('0'), Some("null")),
    OptSpec::req("a", Some('a'), Some("arg-file")),
    OptSpec::req("d", Some('d'), Some("delimiter")),
    OptSpec::req("E", Some('E'), None),
    OptSpec::opt("e", Some('e'), Some("eof")),
    OptSpec::req("I", Some('I'), None),
    OptSpec::opt("i", Some('i'), Some("replace")),
    OptSpec::req("L", Some('L'), None),
    OptSpec::opt("l", Some('l'), Some("max-lines")),
    OptSpec::req("n", Some('n'), Some("max-args")),
    OptSpec::req("P", Some('P'), Some("max-procs")),
    OptSpec::req("s", Some('s'), Some("max-chars")),
    OptSpec::flag("r", Some('r'), Some("no-run-if-empty")),
    OptSpec::flag("t", Some('t'), Some("verbose")),
    OptSpec::flag("x", Some('x'), Some("exit")),
    OptSpec::req("process-slot-var", None, Some("process-slot-var")),
    OptSpec::flag("show-limits", None, Some("show-limits")),
    OptSpec::flag("p", Some('p'), Some("interactive")),
    OptSpec::flag("o", Some('o'), Some("open-tty")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// How input is split into items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Split {
    /// Blanks and newlines, with single quotes, double quotes and backslashes.
    Quoted,
    /// Exactly this byte; no quoting (NUL for `-0`).
    Delim(u8),
}

#[derive(Debug)]
struct Options {
    split: Split,
    eof: Option<Vec<u8>>,
    replace: Option<Vec<u8>>,
    max_lines: Option<usize>,
    max_args: Option<usize>,
    max_chars: Option<u64>,
    no_run_if_empty: bool,
    verbose: bool,
    exit_on_overflow: bool,
    arg_file: Option<String>,
    slot_var: Option<String>,
    show_limits: bool,
    command: Vec<String>,
}

/// Entry point for `xargs` and the `agentcmd xargs` multi-call form.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, OPTS).no_permute().parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage(CMD, &err, EXIT_FAIL),
    };
    let mut options = Options {
        split: Split::Quoted,
        eof: None,
        replace: None,
        max_lines: None,
        max_args: None,
        max_chars: None,
        no_run_if_empty: false,
        verbose: false,
        exit_on_overflow: false,
        arg_file: None,
        slot_var: None,
        show_limits: false,
        command: Vec::new(),
    };
    for item in items {
        match item {
            Item::Operand(text) => options.command.push(text),
            Item::Opt { id, value } => match apply(&mut options, id, value) {
                Ok(None) => {}
                Ok(Some(code)) => return code,
                Err(code) => return code,
            },
        }
    }
    if options.show_limits {
        show_limits(&options);
    }
    let mut runner = Runner::new(&options);
    let result = read_input(&options, &mut runner).and_then(|()| runner.finish());
    match result {
        Ok(code) | Err(code) => code,
    }
}

/// Applies one option. `Ok(Some(code))` ends the run with that status (help, version).
fn apply(options: &mut Options, id: &str, value: Option<String>) -> Result<Option<i32>, i32> {
    match id {
        "0" => options.split = Split::Delim(0),
        "a" => options.arg_file = value,
        "d" => options.split = Split::Delim(single_delimiter(&value.unwrap_or_default())?),
        "E" | "e" => options.eof = Some(value.unwrap_or_default().into_bytes()),
        "I" => options.replace = Some(value.unwrap_or_default().into_bytes()),
        "i" => options.replace = Some(value.unwrap_or_else(|| "{}".to_owned()).into_bytes()),
        "L" | "l" => options.max_lines = Some(positive(value.as_deref().unwrap_or("1"), "-L")?),
        "n" => options.max_args = Some(positive(&value.unwrap_or_default(), "-n")?),
        "P" => {
            let text = value.unwrap_or_default();
            if text.parse::<usize>().is_err() {
                return Err(fail(&format!("invalid number for -P: '{text}'")));
            }
        }
        "s" => options.max_chars = Some(number(&value.unwrap_or_default(), "-s")? as u64),
        "r" => options.no_run_if_empty = true,
        "t" => options.verbose = true,
        "x" => options.exit_on_overflow = true,
        "process-slot-var" => options.slot_var = value,
        "show-limits" => options.show_limits = true,
        "p" => return Err(unsupported(CMD, "-p")),
        "o" => return Err(unsupported(CMD, "-o")),
        "help" => {
            let mut out = io::stdout();
            let _ = out.write_all(HELP.as_bytes());
            return Ok(Some(EXIT_OK));
        }
        "version" => return Ok(Some(print_version(CMD))),
        _ => {}
    }
    Ok(None)
}

fn fail(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

fn number(text: &str, opt: &str) -> Result<usize, i32> {
    text.parse::<usize>()
        .map_err(|_| fail(&format!("invalid number for {opt}: '{text}'")))
}

fn positive(text: &str, opt: &str) -> Result<usize, i32> {
    match number(text, opt)? {
        0 => Err(fail(&format!("invalid number for {opt}: '{text}'"))),
        n => Ok(n),
    }
}

/// `-d` takes one character, with the usual backslash escapes (`\n`, `\t`, `\0`, ...).
fn single_delimiter(text: &str) -> Result<u8, i32> {
    let (bytes, _) = unescape(text, EscapeMode::Echo);
    match bytes.as_slice() {
        [byte] => Ok(*byte),
        _ => Err(fail(&format!(
            "the delimiter must be a single character: '{text}'"
        ))),
    }
}

/// What one invocation carries and how the limits are measured.
struct Runner<'a> {
    options: &'a Options,
    /// Program and its fixed arguments, as bytes.
    template: Vec<Vec<u8>>,
    /// Use the built-in `echo` when no command was named and `echo` is not installed.
    builtin_echo: bool,
    /// Bytes taken by the environment and the template: the fixed part of a command line.
    base: u64,
    limit: Option<u64>,
    pending: Vec<Vec<u8>>,
    pending_size: u64,
    /// Complete input lines that contributed arguments to `pending` (for `-L`).
    pending_lines: usize,
    /// True while the current input line has no argument yet.
    line_fresh: bool,
    invoked: bool,
    status: i32,
}

impl<'a> Runner<'a> {
    fn new(options: &'a Options) -> Self {
        let mut template: Vec<Vec<u8>> = options
            .command
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect();
        // Windows has no `echo` program, so `echo` (the default, or named explicitly) falls
        // back to the built-in one.
        let names_echo = template.first().is_none_or(|word| word == b"echo");
        let builtin_echo = names_echo && resolve_command("echo").is_none();
        if template.is_empty() {
            template.push(b"echo".to_vec());
        }
        let env_bytes: u64 = std::env::vars_os()
            .map(|(k, v)| (k.len() + v.len() + 2) as u64)
            .sum();
        let fixed: u64 = template.iter().map(|t| t.len() as u64 + 1).sum::<u64>() + env_bytes;
        let limit = options.max_chars.or(Some(default_limit()));
        Self {
            options,
            template,
            builtin_echo,
            base: fixed,
            limit,
            pending: Vec::new(),
            pending_size: 0,
            pending_lines: 0,
            line_fresh: true,
            invoked: false,
            status: EXIT_OK,
        }
    }

    /// Adds one argument, starting a new invocation first when a limit would be exceeded.
    fn add_arg(&mut self, arg: Vec<u8>) -> Result<(), i32> {
        if let Some(max) = self.options.max_lines {
            if self.line_fresh && self.pending_lines >= max {
                self.flush()?;
            }
        }
        if let Some(max) = self.options.max_args {
            if self.pending.len() >= max {
                self.flush()?;
            }
        }
        let size = arg.len() as u64 + 1;
        if let Some(limit) = self.limit {
            if self.base + self.pending_size + size > limit {
                if self.options.exit_on_overflow {
                    return Err(too_long());
                }
                self.flush()?;
                if self.base + size > limit {
                    return Err(too_long());
                }
            }
        }
        self.pending_size += size;
        self.pending.push(arg);
        self.line_fresh = false;
        Ok(())
    }

    /// Notes the end of an input line. A line ending in a blank continues into the next.
    fn end_line(&mut self, continued: bool) {
        if !self.line_fresh && !continued {
            self.pending_lines += 1;
            self.line_fresh = true;
        }
    }

    /// Runs the command with the pending arguments, if any.
    fn flush(&mut self) -> Result<(), i32> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let args = std::mem::take(&mut self.pending);
        self.pending_size = 0;
        self.pending_lines = 0;
        self.line_fresh = true;
        self.invoke(&args)
    }

    /// Runs the last partial batch, or the command once for empty input (unless `-r`).
    fn finish(mut self) -> Result<i32, i32> {
        if !self.pending.is_empty() {
            self.flush()?;
        } else if !self.invoked && !self.options.no_run_if_empty && self.options.replace.is_none() {
            self.invoke(&[])?;
        }
        Ok(self.status)
    }

    /// Builds the argument vector: the template with arguments appended, or with `-I`
    /// replacement in every template word.
    fn argv(&self, args: &[Vec<u8>]) -> Vec<Vec<u8>> {
        match &self.options.replace {
            Some(marker) => {
                let item: &[u8] = args.first().map_or(&[], Vec::as_slice);
                self.template
                    .iter()
                    .map(|word| replace_all(word, marker, item))
                    .collect()
            }
            None => self
                .template
                .iter()
                .cloned()
                .chain(args.iter().cloned())
                .collect(),
        }
    }

    /// Runs one invocation. An `Err` stops reading and gives the final status.
    fn invoke(&mut self, args: &[Vec<u8>]) -> Result<(), i32> {
        let argv = self.argv(args);
        if self.options.verbose {
            let mut line = Vec::new();
            for (index, word) in argv.iter().enumerate() {
                if index > 0 {
                    line.push(b' ');
                }
                line.extend_from_slice(word);
            }
            line.push(b'\n');
            let _ = io::stderr().write_all(&line);
        }
        self.invoked = true;
        if self.builtin_echo {
            return echo_builtin(&argv[1..]);
        }
        let program = String::from_utf8_lossy(&argv[0]).into_owned();
        let Some(path) = resolve_command(&program) else {
            eprintln!("{CMD}: {program}: No such file or directory");
            return Err(127);
        };
        let mut command = Command::new(path);
        command
            .args(argv[1..].iter().map(|word| os_string(word)))
            .stdin(Stdio::null());
        if let Some(var) = &self.options.slot_var {
            command.env(var, "0");
        }
        match command.status() {
            Ok(status) => self.judge(exit_status_code(status), status.code().is_some()),
            Err(error) => {
                eprintln!("{CMD}: {program}: {}", io_error_text(&error));
                Err(spawn_error_status(&error))
            }
        }
    }

    /// Records a child's outcome: 255 stops with 124, a signal stops with 125, any other
    /// failure is remembered as 123 and the run goes on.
    fn judge(&mut self, code: i32, exited: bool) -> Result<(), i32> {
        if !exited {
            return Err(EXIT_SIGNALLED);
        }
        match code {
            0 => Ok(()),
            255 => Err(EXIT_COMMAND_255),
            _ => {
                self.status = EXIT_SOME_FAILED;
                Ok(())
            }
        }
    }
}

fn too_long() -> i32 {
    eprintln!("{CMD}: argument line too long");
    EXIT_FAIL
}

/// The most command-line bytes one invocation may use when `-s` is absent.
fn default_limit() -> u64 {
    if cfg!(windows) {
        32767
    } else {
        128 * 1024
    }
}

/// Replaces every occurrence of `marker` in `word` with `item`.
fn replace_all(word: &[u8], marker: &[u8], item: &[u8]) -> Vec<u8> {
    if marker.is_empty() {
        return word.to_vec();
    }
    let mut out = Vec::with_capacity(word.len());
    let mut index = 0;
    while index < word.len() {
        if word[index..].starts_with(marker) {
            out.extend_from_slice(item);
            index += marker.len();
        } else {
            out.push(word[index]);
            index += 1;
        }
    }
    out
}

fn os_string(bytes: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes.to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The fallback `echo`: arguments joined by spaces, then a newline.
fn echo_builtin(args: &[Vec<u8>]) -> Result<(), i32> {
    let mut line = Vec::new();
    for (index, word) in args.iter().enumerate() {
        if index > 0 {
            line.push(b' ');
        }
        line.extend_from_slice(word);
    }
    line.push(b'\n');
    let mut out = io::stdout().lock();
    let _ = out.write_all(&line);
    let _ = out.flush();
    Ok(())
}

/// Feeds the whole input to the runner according to the split and replace modes.
fn read_input(options: &Options, runner: &mut Runner) -> Result<(), i32> {
    let source = options.arg_file.as_deref().unwrap_or("-");
    let mut reader = open_input(source).map_err(|error| {
        eprintln!("{CMD}: {source}: {}", io_error_text(&error));
        EXIT_FAIL
    })?;
    if options.replace.is_some() {
        return read_replace_lines(options, runner, reader.as_mut());
    }
    match options.split {
        Split::Quoted => read_quoted(options, runner, reader.as_mut()),
        Split::Delim(delim) => read_records(options, runner, reader.as_mut(), delim),
    }
}

/// Whether `item` is the end-of-input marker (`-E`/`-e`).
fn is_eof(options: &Options, item: &[u8]) -> bool {
    options.eof.as_deref() == Some(item)
}

/// `-I`: each input line (leading blanks removed) is one argument of its own invocation.
fn read_replace_lines(
    options: &Options,
    runner: &mut Runner,
    reader: &mut dyn BufRead,
) -> Result<(), i32> {
    let delim = match options.split {
        Split::Quoted => b'\n',
        Split::Delim(delim) => delim,
    };
    let mut line = Vec::new();
    loop {
        if !read_record(reader, delim, &mut line).map_err(read_failed)? {
            return Ok(());
        }
        if line.last() == Some(&delim) {
            line.pop();
        }
        if delim == b'\n' {
            let start = line
                .iter()
                .position(|b| *b != b' ' && *b != b'\t')
                .unwrap_or(line.len());
            line.drain(..start);
        }
        if line.is_empty() {
            continue;
        }
        if is_eof(options, &line) {
            return Ok(());
        }
        runner.invoke(std::slice::from_ref(&line))?;
    }
}

/// `-0` and `-d`: each delimited record is one argument.
fn read_records(
    options: &Options,
    runner: &mut Runner,
    reader: &mut dyn BufRead,
    delim: u8,
) -> Result<(), i32> {
    let mut record = Vec::new();
    loop {
        if !read_record(reader, delim, &mut record).map_err(read_failed)? {
            break;
        }
        if record.last() == Some(&delim) {
            record.pop();
        }
        if is_eof(options, &record) {
            break;
        }
        runner.add_arg(std::mem::take(&mut record))?;
        runner.end_line(false);
    }
    runner.end_line(false);
    Ok(())
}

fn read_failed(error: io::Error) -> i32 {
    eprintln!("{CMD}: read error: {}", io_error_text(&error));
    EXIT_FAIL
}

/// Default splitting: blanks and newlines separate items; single quotes keep everything
/// literally, double quotes allow backslash escapes, and a backslash quotes the next byte.
fn read_quoted(
    options: &Options,
    runner: &mut Runner,
    reader: &mut dyn BufRead,
) -> Result<(), i32> {
    let mut scanner = Scanner::default();
    let mut chunk = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(read_failed)?;
        if available.is_empty() {
            break;
        }
        let consumed = available.len();
        chunk.clear();
        chunk.extend_from_slice(available);
        reader.consume(consumed);
        for &byte in &chunk {
            if !scanner.step(byte, options, runner)? {
                return Ok(());
            }
        }
    }
    scanner.finish(options, runner)
}

/// Quote and word state for [`read_quoted`].
#[derive(Default)]
struct Scanner {
    word: Vec<u8>,
    in_word: bool,
    quote: Option<u8>,
    escaped: bool,
    last_blank: bool,
}

impl Scanner {
    /// Consumes one byte. `Ok(false)` means the end-of-input marker was reached.
    fn step(&mut self, byte: u8, options: &Options, runner: &mut Runner) -> Result<bool, i32> {
        let blank = byte == b' ' || byte == b'\t';
        if self.escaped {
            self.escaped = false;
            self.word.push(byte);
            self.in_word = true;
        } else if let Some(quote) = self.quote {
            if byte == quote {
                self.quote = None;
            } else if byte == b'\\' && quote == b'"' {
                self.escaped = true;
            } else {
                self.word.push(byte);
            }
        } else {
            match byte {
                b'\'' | b'"' => {
                    self.quote = Some(byte);
                    self.in_word = true;
                }
                b'\\' => self.escaped = true,
                b'\n' => {
                    if self.in_word && !self.emit(options, runner)? {
                        return Ok(false);
                    }
                    runner.end_line(self.last_blank);
                }
                _ if blank => {
                    if self.in_word && !self.emit(options, runner)? {
                        return Ok(false);
                    }
                }
                _ => {
                    self.word.push(byte);
                    self.in_word = true;
                }
            }
        }
        self.last_blank = blank;
        Ok(true)
    }

    /// Sends the current word as an argument. `Ok(false)` at the end-of-input marker.
    fn emit(&mut self, options: &Options, runner: &mut Runner) -> Result<bool, i32> {
        let word = std::mem::take(&mut self.word);
        self.in_word = false;
        if is_eof(options, &word) {
            return Ok(false);
        }
        runner.add_arg(word)?;
        Ok(true)
    }

    fn finish(mut self, options: &Options, runner: &mut Runner) -> Result<(), i32> {
        if self.quote.is_some() {
            let kind = if self.quote == Some(b'\'') {
                "single"
            } else {
                "double"
            };
            eprintln!(
                "{CMD}: unmatched {kind} quote; by default quotes are special to xargs unless you use the -0 option"
            );
            return Err(EXIT_FAIL);
        }
        if self.in_word && !self.emit(options, runner)? {
            return Ok(());
        }
        runner.end_line(false);
        Ok(())
    }
}

/// `--show-limits`: describes the limits in use, on standard error.
fn show_limits(options: &Options) {
    let env: u64 = std::env::vars_os()
        .map(|(k, v)| (k.len() + v.len() + 2) as u64)
        .sum();
    let posix: u64 = if cfg!(windows) { 32767 } else { 2_097_152 };
    let usable = posix.saturating_sub(env + 2048);
    let buffer = options.max_chars.unwrap_or(default_limit()).min(usable);
    let mut err = io::stderr();
    let _ = writeln!(err, "Your environment variables take up {env} bytes");
    let _ = writeln!(
        err,
        "POSIX upper limit on argument length (this system): {posix}"
    );
    let _ = writeln!(
        err,
        "POSIX smallest allowed upper limit on argument length (all systems): 4096"
    );
    let _ = writeln!(
        err,
        "Maximum length of command we could actually use: {usable}"
    );
    let _ = writeln!(
        err,
        "Size of command buffer we are actually using: {buffer}"
    );
    let _ = writeln!(
        err,
        "Maximum parallelism (--max-procs must be no greater): 2147483647"
    );
}

const HELP: &str = "\
Usage: xargs [OPTION]... COMMAND [INITIAL-ARGS]...
Run COMMAND with arguments INITIAL-ARGS and more arguments read from input.

Mandatory arguments to long options are mandatory for short options too.
  -0, --null                   items are separated by a null, not whitespace;
                                 disables quote and backslash processing and
                                 logical EOF processing
  -a, --arg-file=FILE          read arguments from FILE, not standard input
  -d, --delimiter=CHARACTER    items in input are separated by CHARACTER,
                                 not by whitespace; disables quote and
                                 backslash processing and logical EOF processing
  -E END                       set logical EOF string; if END occurs as a line
                                 of input, the rest of the input is ignored
  -e, --eof[=END]              equivalent to -E END if END is specified;
                                 otherwise, there is no end-of-file string
  -I R, --replace[=R]          replace R in INITIAL-ARGS with names read
                                 from input; implies -x and -L 1
  -L, --max-lines=MAX-LINES    use at most MAX-LINES non-blank input lines per
                                 command line; a line ending in a blank continues
  -l[MAX-LINES]                similar to -L but defaults to at most one non-blank
                                 input line if MAX-LINES is not specified
  -n, --max-args=MAX-ARGS      use at most MAX-ARGS arguments per command line
  -P, --max-procs=MAX-PROCS    run up to MAX-PROCS processes at a time
                                 (accepted; runs are serial)
  -r, --no-run-if-empty        if there are no arguments, do not run COMMAND;
                                 if this option is not given, COMMAND will be
                                 run at least once
  -s, --max-chars=MAX-CHARS    limit length of command line to MAX-CHARS
  -t, --verbose                print commands before executing them
  -x, --exit                   exit if the size (see -s) is exceeded
      --process-slot-var=VAR   set environment variable VAR in child processes
      --show-limits            show limits on command-line length
      --help     display this help and exit
      --version  output version information and exit

Exit status is 123 if any invocation of the command exited with status 1-125,
124 if the command exited with status 255, 125 if the command is killed by a
signal, 126 if the command cannot be run, 127 if the command is not found, and
1 if some other error occurred.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_substitutes_every_marker() {
        assert_eq!(replace_all(b"x{}y{}", b"{}", b"A"), b"xAyA");
        assert_eq!(replace_all(b"plain", b"{}", b"A"), b"plain");
    }

    #[test]
    fn delimiter_accepts_one_byte_with_escapes() {
        assert_eq!(single_delimiter("\\n").unwrap(), b'\n');
        assert_eq!(single_delimiter(",").unwrap(), b',');
        assert!(single_delimiter("ab").is_err());
    }

    #[test]
    fn counts_must_be_positive() {
        assert_eq!(positive("3", "-n").unwrap(), 3);
        assert!(positive("0", "-n").is_err());
        assert!(number("x", "-s").is_err());
    }

    #[test]
    fn exit_status_mapping() {
        let options = Options {
            split: Split::Quoted,
            eof: None,
            replace: None,
            max_lines: None,
            max_args: None,
            max_chars: None,
            no_run_if_empty: false,
            verbose: false,
            exit_on_overflow: false,
            arg_file: None,
            slot_var: None,
            show_limits: false,
            command: vec!["true".to_owned()],
        };
        let mut runner = Runner::new(&options);
        assert_eq!(runner.judge(0, true), Ok(()));
        assert_eq!(runner.judge(1, true), Ok(()));
        assert_eq!(runner.judge(255, true), Err(124));
        assert_eq!(runner.judge(0, false), Err(125));
        assert_eq!(runner.status, 123);
    }

    #[test]
    fn scanner_splits_on_blanks_and_honours_quotes() {
        let options = Options {
            split: Split::Quoted,
            eof: None,
            replace: None,
            max_lines: None,
            max_args: None,
            max_chars: None,
            no_run_if_empty: false,
            verbose: false,
            exit_on_overflow: false,
            arg_file: None,
            slot_var: None,
            show_limits: false,
            command: vec!["cat".to_owned()],
        };
        let mut runner = Runner::new(&options);
        let mut reader: &[u8] = b"one 'two three' \"four \\\"five\\\"\" six\\ seven\n";
        read_quoted(&options, &mut runner, &mut reader).unwrap();
        assert_eq!(
            runner.pending,
            vec![
                b"one".to_vec(),
                b"two three".to_vec(),
                b"four \"five\"".to_vec(),
                b"six seven".to_vec(),
            ]
        );
    }
}
