//! `env` and `printenv`.
//!
//! `env` parses its own options the way GNU getopt does with a leading `+`: the first
//! operand ends them. A lone `-` means `-i`. `-i`, `-u`, `-C` and `-S` act in the order
//! given. After the options come `NAME=VALUE` assignments, then an optional COMMAND, which
//! runs with the resulting environment; with no COMMAND the environment is printed.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{io_error_text, print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::process::{exit_status_code, resolve_in, spawn_error_status};

mod split;

use split::split_string;

/// `env` itself failed (usage error, bad `-u`/`-C`/`-S`).
const EXIT_CANCELED: i32 = 125;

const ENV_HELP: &str = r#"Usage: env [OPTION]... [-] [NAME=VALUE]... [COMMAND [ARG]...]
Set each NAME to VALUE in the environment and run COMMAND.

Options:
  -i, --ignore-environment  start with an empty environment
  -0, --null                end each output line with NUL, not newline
  -u, --unset=NAME          remove variable from the environment
  -C, --chdir=DIR           change working directory to DIR
  -S, --split-string=S      process and split S into separate arguments;
                            used to pass multiple arguments on shebang lines
  -v, --debug               print verbose information for each processing step
      --help                display this help and exit
      --version             output version information and exit

A mere - implies -i.  If no COMMAND, print the resulting environment.

Exit status:
  125  if the option processing fails
  126  if COMMAND is found but cannot be invoked
  127  if COMMAND cannot be found
  -    the exit status of COMMAND otherwise
"#;

const PRINTENV_HELP: &str = r#"Usage: printenv [OPTION]... [VARIABLE]...
Print the values of the specified environment VARIABLE(s).
If no VARIABLE is specified, print name and value pairs for them all.

  -0, --null     end each output line with NUL, not newline
      --help     display this help and exit
      --version  output version information and exit
"#;

const PRINTENV_SPECS: &[OptSpec] = &[
    OptSpec::flag("null", Some('0'), Some("null")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Long options of `env`. The signal ones are GNU options this project does not implement.
const LONG_OPTIONS: &[(&str, LongKind)] = &[
    ("block-signal", LongKind::Unsupported),
    ("chdir", LongKind::Value),
    ("debug", LongKind::Flag),
    ("default-signal", LongKind::Unsupported),
    ("help", LongKind::Flag),
    ("ignore-environment", LongKind::Flag),
    ("ignore-signal", LongKind::Unsupported),
    ("list-signal-handling", LongKind::Unsupported),
    ("null", LongKind::Flag),
    ("split-string", LongKind::Value),
    ("unset", LongKind::Value),
    ("version", LongKind::Flag),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum LongKind {
    Flag,
    Value,
    Unsupported,
}

/// Name and value pairs in environment order, as `setenv` keeps them.
type Pairs = Vec<(OsString, OsString)>;

/// The environment being built: names compare case-insensitively on Windows.
struct Environment {
    pairs: Pairs,
}

impl Environment {
    fn from_process() -> Self {
        let pairs = std::env::vars_os()
            .filter(|(name, _)| !name.to_string_lossy().starts_with('='))
            .collect();
        Self { pairs }
    }

    fn same_name(a: &OsStr, b: &OsStr) -> bool {
        if cfg!(windows) {
            a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy())
        } else {
            a == b
        }
    }

    fn get(&self, name: &OsStr) -> Option<&OsString> {
        self.pairs
            .iter()
            .find(|(key, _)| Self::same_name(key, name))
            .map(|(_, value)| value)
    }

    /// Replaces the value of an existing name in place, or appends a new pair.
    fn set(&mut self, name: &OsStr, value: &OsStr) {
        match self
            .pairs
            .iter_mut()
            .find(|(key, _)| Self::same_name(key, name))
        {
            Some(pair) => pair.1 = value.to_owned(),
            None => self.pairs.push((name.to_owned(), value.to_owned())),
        }
    }

    fn unset(&mut self, name: &OsStr) {
        self.pairs.retain(|(key, _)| !Self::same_name(key, name));
    }

    fn clear(&mut self) {
        self.pairs.clear();
    }
}

/// Settings collected while scanning options.
struct State {
    env: Environment,
    null: bool,
    debug: bool,
}

/// An option that ends processing and prints a fixed answer.
enum Special {
    Help,
    Version,
}

/// Failure while handling options or variables.
#[derive(Debug)]
enum EnvError {
    /// A usage error: message, then the `Try` line, status 125.
    Usage(String),
    /// A runtime failure such as a bad `-C` directory: status 125.
    Failed(String),
    /// The failure was already reported; exit with this status.
    Exit(i32),
}

pub fn run_env(args: &[String]) -> i32 {
    match env_main(args) {
        Ok(code) => code,
        Err(EnvError::Usage(message)) => {
            report_usage("env", &UsageError { message }, EXIT_CANCELED)
        }
        Err(EnvError::Failed(message)) => {
            eprintln!("env: {message}");
            EXIT_CANCELED
        }
        Err(EnvError::Exit(code)) => code,
    }
}

fn env_main(args: &[String]) -> Result<i32, EnvError> {
    let mut queue: VecDeque<String> = args.iter().cloned().collect();
    let mut state = State {
        env: Environment::from_process(),
        null: false,
        debug: false,
    };
    if let Some(special) = scan_options(&mut queue, &mut state)? {
        return Ok(match special {
            Special::Help => emit("env", ENV_HELP.as_bytes()),
            Special::Version => print_version("env"),
        });
    }
    let mut command = Vec::new();
    while let Some(word) = queue.pop_front() {
        let assignment = word
            .split_once('=')
            .map(|(name, value)| (name.to_owned(), value.to_owned()));
        match assignment {
            Some((name, value)) => {
                if state.debug {
                    eprintln!("setenv:   {word}");
                }
                state.env.set(OsStr::new(&name), OsStr::new(&value));
            }
            None => {
                command.push(word);
                command.extend(queue.drain(..));
                break;
            }
        }
    }
    if command.is_empty() {
        return Ok(print_environment(&state));
    }
    if state.null {
        return Err(EnvError::Usage(
            "cannot specify --null (-0) with command".to_owned(),
        ));
    }
    Ok(run_command(&state, &command))
}

/// Consumes options from the front of `queue`, stopping at the first operand.
fn scan_options(
    queue: &mut VecDeque<String>,
    state: &mut State,
) -> Result<Option<Special>, EnvError> {
    while let Some(front) = queue.front().cloned() {
        if front == "--" {
            queue.pop_front();
            break;
        }
        if front == "-" {
            queue.pop_front();
            clear_environment(state);
            break;
        }
        if let Some(body) = front.strip_prefix("--") {
            queue.pop_front();
            if let Some(special) = long_option(body, queue, state)? {
                return Ok(Some(special));
            }
        } else if front.len() > 1 && front.starts_with('-') {
            queue.pop_front();
            if let Some(special) = short_cluster(&front[1..], queue, state)? {
                return Ok(Some(special));
            }
        } else {
            break;
        }
    }
    Ok(None)
}

fn clear_environment(state: &mut State) {
    state.env.clear();
    if state.debug {
        eprintln!("cleaning environ");
    }
}

/// Handles one long option (`body` without the leading `--`).
fn long_option(
    body: &str,
    queue: &mut VecDeque<String>,
    state: &mut State,
) -> Result<Option<Special>, EnvError> {
    let (name, inline) = match body.split_once('=') {
        Some((name, value)) => (name, Some(value.to_owned())),
        None => (body, None),
    };
    let (full, kind) = resolve_long(name, body)?;
    match kind {
        LongKind::Unsupported => {
            unsupported("env", &format!("--{full}"));
            Err(EnvError::Exit(EXIT_CANCELED))
        }
        LongKind::Flag => {
            if inline.is_some() {
                return Err(EnvError::Usage(format!(
                    "option '--{full}' doesn't allow an argument"
                )));
            }
            apply_flag(full, state)
        }
        LongKind::Value => {
            let value = match inline {
                Some(value) => value,
                None => queue.pop_front().ok_or_else(|| {
                    EnvError::Usage(format!("option '--{full}' requires an argument"))
                })?,
            };
            apply_value(full, &value, queue, state)
        }
    }
}

/// Finds a long option by exact name or unique prefix.
fn resolve_long(name: &str, shown: &str) -> Result<(&'static str, LongKind), EnvError> {
    if let Some((exact, kind)) = LONG_OPTIONS.iter().find(|(long, _)| *long == name) {
        return Ok((*exact, *kind));
    }
    let matches: Vec<&(&'static str, LongKind)> = LONG_OPTIONS
        .iter()
        .filter(|(long, _)| !name.is_empty() && long.starts_with(name))
        .collect();
    match matches.as_slice() {
        [] => Err(EnvError::Usage(format!("unrecognized option '--{shown}'"))),
        [only] => Ok((only.0, only.1)),
        many => {
            let listed: Vec<String> = many.iter().map(|(long, _)| format!("'--{long}'")).collect();
            Err(EnvError::Usage(format!(
                "option '--{name}' is ambiguous; possibilities: {}",
                listed.join(" ")
            )))
        }
    }
}

/// Handles one cluster of short options (`flags` without the leading `-`).
fn short_cluster(
    flags: &str,
    queue: &mut VecDeque<String>,
    state: &mut State,
) -> Result<Option<Special>, EnvError> {
    for (position, letter) in flags.char_indices() {
        match letter {
            'i' | '0' | 'v' => {
                let name = match letter {
                    'i' => "ignore-environment",
                    '0' => "null",
                    _ => "debug",
                };
                if let Some(special) = apply_flag(name, state)? {
                    return Ok(Some(special));
                }
            }
            'u' | 'C' | 'S' => {
                let rest = &flags[position + 1..];
                let value = if rest.is_empty() {
                    queue.pop_front().ok_or_else(|| {
                        EnvError::Usage(format!("option requires an argument -- '{letter}'"))
                    })?
                } else {
                    rest.to_owned()
                };
                let name = match letter {
                    'u' => "unset",
                    'C' => "chdir",
                    _ => "split-string",
                };
                return apply_value(name, &value, queue, state);
            }
            other => {
                return Err(EnvError::Usage(format!("invalid option -- '{other}'")));
            }
        }
    }
    Ok(None)
}

fn apply_flag(name: &str, state: &mut State) -> Result<Option<Special>, EnvError> {
    match name {
        "ignore-environment" => clear_environment(state),
        "null" => state.null = true,
        "debug" => state.debug = true,
        "help" => return Ok(Some(Special::Help)),
        "version" => return Ok(Some(Special::Version)),
        _ => {}
    }
    Ok(None)
}

fn apply_value(
    name: &str,
    value: &str,
    queue: &mut VecDeque<String>,
    state: &mut State,
) -> Result<Option<Special>, EnvError> {
    match name {
        "unset" => unset_variable(value, state)?,
        "chdir" => change_directory(value)?,
        _ => {
            let words = split_string(value, &state.env).map_err(EnvError::Failed)?;
            for word in words.into_iter().rev() {
                queue.push_front(word);
            }
        }
    }
    Ok(None)
}

fn unset_variable(name: &str, state: &mut State) -> Result<(), EnvError> {
    if name.is_empty() || name.contains('=') {
        return Err(EnvError::Failed(format!(
            "cannot unset '{name}': Invalid argument"
        )));
    }
    state.env.unset(OsStr::new(name));
    if state.debug {
        eprintln!("unset:    {name}");
    }
    Ok(())
}

fn change_directory(dir: &str) -> Result<(), EnvError> {
    std::env::set_current_dir(dir).map_err(|error| {
        EnvError::Failed(format!(
            "cannot change directory to '{dir}': {}",
            io_error_text(&error)
        ))
    })
}

/// The environment in output order, each entry followed by `\n` or `\0`.
fn print_environment(state: &State) -> i32 {
    let delimiter = if state.null { b'\0' } else { b'\n' };
    let mut bytes = Vec::new();
    for (name, value) in &state.env.pairs {
        bytes.extend_from_slice(&os_bytes(name));
        bytes.push(b'=');
        bytes.extend_from_slice(&os_bytes(value));
        bytes.push(delimiter);
    }
    emit("env", &bytes)
}

/// The directories searched for COMMAND: the PATH of the new environment.
fn search_path(env: &Environment) -> Option<OsString> {
    match env.get(OsStr::new("PATH")) {
        Some(value) => Some(value.clone()),
        None if cfg!(windows) => None,
        None => Some(OsString::from("/bin:/usr/bin")),
    }
}

fn resolve_program(program: &str, env: &Environment) -> Option<PathBuf> {
    let has_separator = program.contains('/') || (cfg!(windows) && program.contains('\\'));
    if has_separator {
        return Some(PathBuf::from(program));
    }
    let path = search_path(env);
    let pathext = env
        .get(OsStr::new("PATHEXT"))
        .and_then(|value| value.to_str().map(str::to_owned));
    resolve_in(program, path.as_deref(), pathext.as_deref())
}

/// Runs `command` with the new environment and returns its status (or env's own failure).
fn run_command(state: &State, command: &[String]) -> i32 {
    let program = &command[0];
    let Some(resolved) = resolve_program(program, &state.env) else {
        eprintln!("env: '{program}': No such file or directory");
        return 127;
    };
    if state.debug {
        eprintln!("executing: {}", resolved.display());
        for (index, arg) in command.iter().enumerate() {
            eprintln!("   arg[{index}]= '{arg}'");
        }
    }
    let mut child = Command::new(&resolved);
    spawn_setup(&mut child, program);
    child.args(&command[1..]).env_clear().envs(
        state
            .env
            .pairs
            .iter()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str())),
    );
    match child.spawn() {
        Ok(mut running) => match running.wait() {
            Ok(status) => exit_status_code(status),
            Err(error) => {
                eprintln!("env: '{program}': {}", io_error_text(&error));
                EXIT_FAIL
            }
        },
        Err(error) => {
            eprintln!("env: '{program}': {}", io_error_text(&error));
            spawn_error_status(&error)
        }
    }
}

/// Sets argv[0] to the name the user typed (Unix); Windows has no such field.
#[cfg(unix)]
fn spawn_setup(command: &mut Command, program: &str) {
    use std::os::unix::process::CommandExt;
    command.arg0(program);
}

#[cfg(not(unix))]
fn spawn_setup(_command: &mut Command, _program: &str) {}

#[cfg(unix)]
fn os_bytes(text: &OsStr) -> Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    Cow::Borrowed(text.as_bytes())
}

#[cfg(not(unix))]
fn os_bytes(text: &OsStr) -> Cow<'_, [u8]> {
    match text.to_string_lossy() {
        Cow::Borrowed(text) => Cow::Borrowed(text.as_bytes()),
        Cow::Owned(text) => Cow::Owned(text.into_bytes()),
    }
}

/// Writes `bytes` to stdout. A closed pipe is silent success.
fn emit(cmd: &str, bytes: &[u8]) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(bytes) {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{cmd}: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    finish_output(cmd, &mut out)
}

/// `printenv [-0] [NAME]...`: prints the values of NAMEs, or every variable.
pub fn run_printenv(args: &[String]) -> i32 {
    let items = match Parser::new("printenv", PRINTENV_SPECS)
        .no_permute()
        .parse(args)
    {
        Ok(items) => items,
        // GNU printenv exits 2 here; 02-conventions §3 requires 1 (known difference).
        Err(error) => return report_usage("printenv", &error, 2),
    };
    let mut null = false;
    let mut names = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return emit("printenv", PRINTENV_HELP.as_bytes()),
            Item::Opt { id: "version", .. } => return print_version("printenv"),
            Item::Opt { id: "null", .. } => null = true,
            Item::Opt { .. } => {}
            Item::Operand(name) => names.push(name),
        }
    }
    let env = Environment::from_process();
    let delimiter = if null { b'\0' } else { b'\n' };
    if names.is_empty() {
        let state = State {
            env,
            null,
            debug: false,
        };
        return print_environment(&state);
    }
    let mut bytes = Vec::new();
    let mut status = EXIT_OK;
    for name in &names {
        match env.get(OsStr::new(name)) {
            Some(value) => {
                bytes.extend_from_slice(&os_bytes(value));
                bytes.push(delimiter);
            }
            None => status = EXIT_FAIL,
        }
    }
    let written = emit("printenv", &bytes);
    if written != EXIT_OK {
        return written;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with(pairs: &[(&str, &str)]) -> Environment {
        Environment {
            pairs: pairs
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v)))
                .collect(),
        }
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn set_replaces_in_place_and_appends_new_names() {
        let mut env = env_with(&[("A", "1"), ("B", "2")]);
        env.set(OsStr::new("A"), OsStr::new("x"));
        env.set(OsStr::new("C"), OsStr::new("3"));
        let names: Vec<String> = env
            .pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k.to_string_lossy(), v.to_string_lossy()))
            .collect();
        assert_eq!(names, ["A=x", "B=2", "C=3"]);
    }

    #[test]
    fn unset_removes_every_match() {
        let mut env = env_with(&[("A", "1"), ("B", "2")]);
        env.unset(OsStr::new("A"));
        assert!(env.get(OsStr::new("A")).is_none());
        assert!(env.get(OsStr::new("B")).is_some());
    }

    #[test]
    fn unset_rejects_names_with_equals_or_empty() {
        let mut state = State {
            env: env_with(&[("A", "1")]),
            null: false,
            debug: false,
        };
        assert!(unset_variable("A=B", &mut state).is_err());
        assert!(unset_variable("", &mut state).is_err());
        assert!(unset_variable("A", &mut state).is_ok());
    }

    #[test]
    fn long_names_resolve_by_unique_prefix() {
        assert_eq!(
            resolve_long("ignore-e", "x").unwrap().0,
            "ignore-environment"
        );
        assert_eq!(resolve_long("unset", "unset").unwrap().0, "unset");
        assert_eq!(resolve_long("s", "s").unwrap().0, "split-string");
        assert!(resolve_long("ignore", "ignore").is_err());
        assert!(resolve_long("nope", "nope").is_err());
    }

    #[test]
    fn short_cluster_rejects_unknown_letters() {
        let mut state = State {
            env: env_with(&[]),
            null: false,
            debug: false,
        };
        let mut queue = VecDeque::new();
        assert!(short_cluster("ix", &mut queue, &mut state).is_err());
        assert!(short_cluster("0", &mut queue, &mut state)
            .unwrap()
            .is_none());
        assert!(state.null);
    }

    #[test]
    fn printenv_options_are_getopt_style_without_permutation() {
        let items = Parser::new("printenv", PRINTENV_SPECS)
            .no_permute()
            .parse(&words(&["NAME", "-0"]))
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1], Item::Operand(text) if text == "-0"));
    }
}
