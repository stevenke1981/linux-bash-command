//! `awk`: pattern scanning and text processing (POSIX awk with common gawk extensions).
//!
//! `lexer` and `parser` (with `ast`) build the program. `interp` runs it, using `builtins`
//! for the builtin functions, `fields` for `$0` and its fields, and `io` for records,
//! `getline` and output streams. `value`, `format` and `regex` are the supporting pieces.
//! Pipes and `system()` are not supported and fail at run time.

mod ast;
mod builtins;
mod fields;
mod format;
mod interp;
mod io;
mod lexer;
mod parser;
mod regex;
mod value;

use std::fs;
use std::io::Read;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, EXIT_OK};
use ast::Symbols;
use interp::{Fault, Interp};

const CMD: &str = "awk";

const SPECS: &[OptSpec] = &[
    OptSpec::req("field", Some('F'), None),
    OptSpec::req("assign", Some('v'), None),
    OptSpec::req("file", Some('f'), None),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "\
Usage: awk [OPTION]... [-f PROGFILE | PROGRAM] [--] [FILE]...

Scan and process each FILE (standard input by default) with the awk PROGRAM.
Pattern-action rules run for each input record. A '-' FILE means standard input.

  -F FS              use FS as the input field separator
  -v VAR=VAL         assign VAL to the variable VAR before the program runs
  -f PROGFILE        read the program from PROGFILE (may be repeated)
      --help         display this help and exit
      --version      output version information and exit

Pipes (cmd | getline, print | cmd) and system() are not supported.

Exit status: 0 on success, 2 on a syntax error, a fatal run-time error, or a
file operand that cannot be read.
";

/// Stack size for the interpreter thread: deep awk recursion uses the native stack.
const STACK_BYTES: usize = 256 * 1024 * 1024;

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).no_permute().parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, 1),
    };
    let mut field_sep = None;
    let mut assigns = Vec::new();
    let mut progfiles = Vec::new();
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(CMD),
            Item::Opt { id: "field", value } => field_sep = value,
            Item::Opt {
                id: "assign",
                value,
            } => assigns.push(value.unwrap_or_default()),
            Item::Opt { id: "file", value } => progfiles.push(value.unwrap_or_default()),
            Item::Opt { .. } => {}
            Item::Operand(text) => operands.push(text),
        }
    }
    for assign in &assigns {
        if io::split_assignment(assign.as_bytes()).is_none() {
            eprintln!("awk: invalid -v argument: '{assign}'");
            return 2;
        }
    }
    let (source, mut operands) = match load_program(progfiles, operands) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let mut symbols = Symbols::new();
    let program = match parser::parse_program(&source, &mut symbols) {
        Ok(program) => program,
        Err(error) => {
            eprintln!(
                "awk: syntax error at source line {}: {}",
                error.line, error.message
            );
            return 2;
        }
    };
    let mut argv = vec![b"awk".to_vec()];
    argv.extend(operands.drain(..).map(String::into_bytes));
    let environ: Vec<(Vec<u8>, Vec<u8>)> = std::env::vars_os()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned().into_bytes(),
                value.to_string_lossy().into_owned().into_bytes(),
            )
        })
        .collect();
    let spawned = std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let mut interp = Interp::new(program, symbols, argv, environ);
            if let Some(separator) = field_sep {
                interp.assign_plain("FS", separator_text(&separator));
            }
            for assign in assigns {
                if let Some((name, value)) = io::split_assignment(assign.as_bytes()) {
                    if let Err(Fault::Fatal(message)) = interp.assign_from_text(name, value) {
                        eprintln!("awk: {message}");
                        return 2;
                    }
                }
            }
            interp.run()
        });
    match spawned {
        Ok(handle) => handle.join().unwrap_or(2),
        Err(error) => {
            eprintln!("awk: cannot start the interpreter: {error}");
            2
        }
    }
}

/// The program text and the remaining operands. Without `-f`, the first operand is the program.
fn load_program(
    progfiles: Vec<String>,
    mut operands: Vec<String>,
) -> Result<(Vec<u8>, Vec<String>), i32> {
    if progfiles.is_empty() {
        if operands.is_empty() {
            eprintln!("Usage: awk [OPTION]... [-f PROGFILE | PROGRAM] [--] [FILE]...");
            eprintln!("Try 'awk --help' for more information.");
            return Err(1);
        }
        let program = operands.remove(0);
        return Ok((program.into_bytes(), operands));
    }
    let mut text = Vec::new();
    for path in progfiles {
        let contents = if path == "-" {
            let mut buffer = Vec::new();
            std::io::stdin().read_to_end(&mut buffer).map(|_| buffer)
        } else {
            fs::read(&path)
        };
        match contents {
            Ok(bytes) => {
                text.extend_from_slice(&bytes);
                text.push(b'\n');
            }
            Err(error) => {
                eprintln!(
                    "awk: fatal: cannot open source file '{path}': {}",
                    crate::common::error::io_error_text(&error)
                );
                return Err(2);
            }
        }
    }
    Ok((text, operands))
}

/// The value of a `-F` argument, with escape sequences processed.
fn separator_text(argument: &str) -> Vec<u8> {
    lexer::unescape(argument.as_bytes())
}
