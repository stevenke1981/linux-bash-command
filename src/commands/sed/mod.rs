//! A byte-oriented, streaming implementation of the specified GNU sed subset.
mod addr;
mod exec;
mod inplace;
mod input;
mod options;
mod output;
mod parse;
mod pattern;
mod replacement;
mod subst;

use crate::common::{error, io as common_io};
use std::io::{self, BufWriter, Write};

#[derive(Debug)]
pub(super) struct ScriptError(pub String);
impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for ScriptError {}

pub fn run(args: &[String]) -> i32 {
    let options = match options::parse(args) {
        Ok(options) => options,
        Err(status) => return status,
    };
    let mut program = match parse::compile(&options.sources, options.extended) {
        Ok(program) => program,
        Err(message) => {
            eprintln!("sed: {message}");
            return 1;
        }
    };
    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(common_io::BUFFER_SIZE, stdout.lock());
    let result = if options.in_place.is_some() {
        inplace::run(&options, &mut program, &mut out)
    } else {
        exec::run(&options, &mut program, &options.files, &mut out)
    };
    match result.and_then(|code| out.flush().map(|()| code)) {
        Ok(code) => code,
        Err(failure) if failure.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(failure) => {
            eprintln!("sed: {}", error::io_text(&failure));
            if failure
                .get_ref()
                .is_some_and(|error| error.is::<ScriptError>())
            {
                1
            } else {
                4
            }
        }
    }
}
