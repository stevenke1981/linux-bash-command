mod eval;
mod parse;

use crate::common::{args, io};

const HELP: &str = "Usage: expr EXPRESSION\nEvaluate an expression using byte strings in the C locale.\nOperators, from low to high precedence:\n  |  &  < <= = == != >= >  + -  * / %  :\nFunctions: match STRING REGEX, substr STRING POS LENGTH,\n           index STRING CHARS, length STRING, + TOKEN, ( EXPR )\nBRE matching is anchored at the beginning; captures return the first group.\nInteger arithmetic uses i128; overflow reports 'integer overflow' (GNU uses bignum).\n      --help     display help (only as the sole argument)\n      --version  display version (only as the sole argument)\nExit status: 0 nonempty/nonzero, 1 empty/zero, 2 syntax error, 3 other errors.\n";

#[derive(Debug)]
struct Failure {
    code: i32,
    message: String,
}
impl Failure {
    fn syntax(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }
    fn runtime(message: impl Into<String>) -> Self {
        Self {
            code: 3,
            message: message.into(),
        }
    }
}
type Result<T> = std::result::Result<T, Failure>;

pub fn run(arguments: &[String]) -> i32 {
    if let Some(code) = args::help_version("expr", arguments, HELP, true) {
        return code;
    }
    let arguments = arguments
        .strip_prefix(&["--".to_owned()])
        .unwrap_or(arguments);
    let value = parse::expression(arguments).and_then(|node| eval::evaluate(&node));
    match value {
        Ok(value) => {
            let result = i32::from(!eval::truth(&value));
            io::output_status("expr", result, 3, |out| {
                out.write_all(&value)?;
                out.write_all(b"\n")
            })
        }
        Err(error) => {
            eprintln!("expr: {}", error.message);
            error.code
        }
    }
}
