mod analyze;
mod ast;
mod fields;
mod format;
mod interp;
mod io;
mod lexer;
mod parser;
mod regexp;
mod value;

use crate::common::{
    args::{self, Item, OptSpec},
    error,
};
use std::io::{self as stdio, BufWriter, Read, Write};
const HELP:&str="Usage: awk [OPTION]... 'program' [VAR=VALUE | FILE]...\n       awk [OPTION]... -f PROGFILE [VAR=VALUE | FILE]...\nRun POSIX awk programs with arrays, functions, and streaming record input.\n  -F FS          set the input field separator\n  -v VAR=VALUE   assign a variable before BEGIN (repeatable)\n  -f PROGFILE    read program source (repeatable)\n  --             stop option parsing\n      --help     display this help\n      --version  display version\nRS supports a byte, paragraph mode, or an extended regular expression.\nlength counts UTF-8 characters; invalid bytes count individually.\nCommand pipes and system() are unsupported and report a runtime error.\nFunction-call nesting is limited to 1024.\nExit status: 0 success, 1 usage, 2 syntax/runtime/I/O error, or explicit exit.\n";
const SPECS: &[OptSpec] = &[
    OptSpec {
        id: "field",
        short: 'F',
        long: "",
        has_arg: args::HasArg::Required,
    },
    OptSpec {
        id: "assign",
        short: 'v',
        long: "",
        has_arg: args::HasArg::Required,
    },
    OptSpec {
        id: "file",
        short: 'f',
        long: "",
        has_arg: args::HasArg::Required,
    },
];
pub fn run(args: &[String]) -> i32 {
    let args = args.to_vec();
    match std::thread::Builder::new()
        .name("awk".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run_inner(&args))
    {
        Ok(worker) => worker.join().unwrap_or_else(|_| {
            eprintln!("awk: execution failed");
            2
        }),
        Err(error) => {
            eprintln!("awk: {}", error::io_text(&error));
            2
        }
    }
}
fn run_inner(argv: &[String]) -> i32 {
    let (source, operands, assignments) = match options(argv) {
        Ok(options) => options,
        Err(status) => return status,
    };
    let program = match parser::parse(source) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("awk: {error}");
            return 2;
        }
    };
    let stdout = stdio::stdout();
    let mut out = BufWriter::with_capacity(65536, stdout.lock());
    let result = (|| {
        let mut runtime = interp::Runtime::new(program, &operands, &mut out);
        for (name, value) in assignments {
            runtime.assign(&name, &value)?;
        }
        runtime.execute()
    })();
    match result.and_then(|status| out.flush().map(|()| status).map_err(interp::Error::Io)) {
        Ok(status) => status,
        Err(interp::Error::Io(error)) if error.kind() == stdio::ErrorKind::BrokenPipe => 0,
        Err(interp::Error::Io(failure)) => {
            eprintln!("awk: {}", error::io_text(&failure));
            2
        }
        Err(interp::Error::Message(message)) => {
            let _ = out.flush();
            eprintln!("awk: {message}");
            2
        }
        Err(_) => {
            eprintln!("awk: invalid control flow");
            2
        }
    }
}
type Options = (Vec<u8>, Vec<String>, Vec<(Vec<u8>, Vec<u8>)>);
fn options(argv: &[String]) -> Result<Options, i32> {
    let items = args::parse_command("awk", argv, HELP, SPECS, true)?;
    let (mut source, mut operands, mut assignments, mut source_files) =
        (Vec::new(), Vec::new(), Vec::new(), false);
    for item in items {
        match item {
            Item::Operand(operand) => operands.push(operand),
            Item::Opt("field", Some(value)) => {
                assignments.push((b"FS".to_vec(), value.into_bytes()))
            }
            Item::Opt("assign", Some(value)) => {
                let (name, value) = io::assignment(value.as_bytes()).ok_or_else(|| {
                    error::usage("awk", "-v argument must have VAR=VALUE form", 1)
                })?;
                assignments.push((name.to_vec(), value.to_vec()));
            }
            Item::Opt("file", Some(value)) => {
                source_files = true;
                let bytes = read_script(&value)?;
                source.extend_from_slice(&bytes);
                source.push(b'\n');
            }
            _ => {}
        }
    }
    if !source_files {
        if operands.is_empty() {
            return Err(error::usage("awk", "no program specified", 1));
        }
        source = operands.remove(0).into_bytes();
    }
    Ok((source, operands, assignments))
}
fn read_script(name: &str) -> Result<Vec<u8>, i32> {
    let result = if matches!(name, "-" | "/dev/stdin") {
        let mut bytes = Vec::new();
        stdio::stdin().read_to_end(&mut bytes).map(|_| bytes)
    } else {
        std::fs::read(name)
    };
    result.map_err(|e| {
        eprintln!(
            "awk: cannot open source file '{name}': {}",
            error::io_text(&e)
        );
        2
    })
}
