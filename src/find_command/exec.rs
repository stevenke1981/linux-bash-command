//! Spawning the commands given to `-exec`, `-execdir` and batched `{} +`.

use std::path::Path;

/// Replaces every `{}` in each argument (including the program name) with `text`.
pub fn substitute(argv: &[String], text: &str) -> Vec<String> {
    argv.iter().map(|arg| arg.replace("{}", text)).collect()
}

/// Runs `argv` with inherited stdio, optionally in `dir`. Returns whether it exited
/// successfully, or an error when the program could not be started.
pub fn run(argv: &[String], dir: Option<&Path>) -> Result<bool, String> {
    let Some((program, args)) = argv.split_first() else {
        return Err("missing command for -exec".to_string());
    };
    let environment: Vec<_> = std::env::vars_os().collect();
    let mut command =
        crate::common::process::command_in(std::ffi::OsStr::new(program), &environment, dir)
            .map_err(|error| format!("'{program}': {}", super::walk::message(&error)))?;
    command.args(args);
    match command.status() {
        Ok(status) => Ok(status.success()),
        Err(error) => Err(format!("'{program}': {}", super::walk::message(&error))),
    }
}
