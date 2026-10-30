//! `expr`: evaluate an expression given as separate arguments.
//!
//! Options are not parsed: every argument is an operand, except that a lone `--help` or
//! `--version` is honoured. Precedence, lowest first: `|`, `&`, the comparisons, `+ -`,
//! `* / %`, `:` (match), then parenthesised groups and literals.
//!
//! Integers are `i128`; results that overflow it fail with exit 3 instead of growing a bignum.
//! Strings are handled as bytes (the C locale), so `length` counts bytes.

use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::regex::bre_to_regex;
use std::io::Write;

const CMD: &str = "expr";

/// Exit status for syntax errors (GNU `expr` uses 2 for every error).
const EXIT_SYNTAX: i32 = 2;
/// Exit status for evaluation errors such as division by zero or overflow.
const EXIT_EVAL: i32 = 3;

const HELP: &str = "Usage: expr EXPRESSION\n  or:  expr OPTION\n\
Print the value of EXPRESSION to standard output. A blank line below\n\
separates increasing precedence groups.\n\n\
  ARG1 | ARG2     ARG1 if it is neither null nor 0, otherwise ARG2\n\
  ARG1 & ARG2     ARG1 if neither argument is null or 0, otherwise 0\n\
  ARG1 < ARG2     ARG1 is less than ARG2 (also <=, =, ==, !=, >=, >)\n\
  ARG1 + ARG2     arithmetic sum (also -, *, /, %)\n\
  STRING : REGEXP  anchored match of a basic regular expression; the number of\n\
                   matched bytes, or the first \\(...\\) group if there is one\n\
  match STRING REGEXP   same as STRING : REGEXP\n\
  substr STRING POS LENGTH   substring of LENGTH bytes starting at POS (1-based)\n\
  index STRING CHARS   position of the first byte of STRING found in CHARS, or 0\n\
  length STRING   number of bytes in STRING\n\
  + TOKEN         TOKEN itself, even when it looks like an operator\n\
  ( EXPRESSION )  grouping\n\n\
      --help     display this help and exit\n\
      --version  output version information and exit\n\n\
Exit status: 0 if EXPRESSION is neither null nor 0, 1 if it is null or 0,\n\
2 if it is a syntax error, 3 if an evaluation error occurred (for example\n\
division by zero, integer overflow or a non-integer argument).\n";

/// A failed evaluation, with the exit status to report.
#[derive(Debug, PartialEq, Eq)]
struct Failure {
    code: i32,
    message: String,
}

type Value = Vec<u8>;
type Eval = Result<Value, Failure>;

fn syntax(message: impl Into<String>) -> Failure {
    Failure {
        code: EXIT_SYNTAX,
        message: message.into(),
    }
}

fn eval_error(message: impl Into<String>) -> Failure {
    Failure {
        code: EXIT_EVAL,
        message: message.into(),
    }
}

pub fn run(args: &[String]) -> i32 {
    if let [only] = args {
        match only.as_str() {
            "--help" => {
                print!("{HELP}");
                return EXIT_OK;
            }
            "--version" => return print_version(CMD),
            _ => {}
        }
    }
    // Like getopt, a leading `--` ends option parsing and is not an operand.
    let args = match args.first().map(String::as_str) {
        Some("--") => &args[1..],
        _ => args,
    };
    if args.is_empty() {
        eprintln!("{CMD}: missing operand\nTry '{CMD} --help' for more information.");
        return EXIT_SYNTAX;
    }
    let mut evaluator = Evaluator {
        tokens: args,
        pos: 0,
    };
    let value = match evaluator.evaluate() {
        Ok(value) => value,
        Err(failure) => {
            eprintln!("{CMD}: {}", failure.message);
            return failure.code;
        }
    };
    let mut out = stdout_writer();
    let written = out.write_all(&value).and_then(|()| out.write_all(b"\n"));
    if let Err(error) = written {
        if is_broken_pipe(&error) {
            return if truthy(&value) { 0 } else { 1 };
        }
        eprintln!("{CMD}: write error: {error}");
        return EXIT_FAIL;
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    if truthy(&value) {
        0
    } else {
        1
    }
}

/// Null means the empty string or the integer zero.
fn truthy(value: &[u8]) -> bool {
    !(value.is_empty() || as_integer(value) == Some(Ok(0)))
}

/// `Some(Ok(n))` for an integer, `Some(Err(..))` for an integer that overflows `i128`, `None`
/// for anything else.
fn as_integer(value: &[u8]) -> Option<Result<i128, ()>> {
    let digits = value.strip_prefix(b"-").unwrap_or(value);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let text = std::str::from_utf8(value).ok()?;
    Some(text.parse::<i128>().map_err(|_| ()))
}

fn integer_operand(value: &[u8]) -> Result<i128, Failure> {
    match as_integer(value) {
        Some(Ok(number)) => Ok(number),
        Some(Err(())) => Err(eval_error("integer overflow")),
        None => Err(eval_error("non-integer argument")),
    }
}

fn number(value: i128) -> Value {
    value.to_string().into_bytes()
}

fn flag(value: bool) -> Value {
    if value {
        b"1".to_vec()
    } else {
        b"0".to_vec()
    }
}

struct Evaluator<'a> {
    tokens: &'a [String],
    pos: usize,
}

impl Evaluator<'_> {
    fn evaluate(&mut self) -> Eval {
        let value = self.or_expr()?;
        match self.peek() {
            Some(token) => Err(syntax(format!(
                "syntax error: unexpected argument '{token}'"
            ))),
            None => Ok(value),
        }
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    fn peek_is(&self, options: &[&str]) -> Option<&str> {
        self.peek().filter(|token| options.contains(token))
    }

    fn bump(&mut self) -> Option<&str> {
        let token = self.tokens.get(self.pos).map(String::as_str);
        self.pos += 1;
        token
    }

    /// The token before the current position, for "missing argument" messages.
    fn previous(&self) -> &str {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map_or("", String::as_str)
    }

    fn missing_argument(&self) -> Failure {
        syntax(format!(
            "syntax error: missing argument after '{}'",
            self.previous()
        ))
    }

    fn or_expr(&mut self) -> Eval {
        let mut left = self.and_expr()?;
        while self.peek_is(&["|"]).is_some() {
            self.bump();
            let right = self.and_expr()?;
            if !truthy(&left) {
                left = right;
            }
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Eval {
        let mut left = self.comparison()?;
        while self.peek_is(&["&"]).is_some() {
            self.bump();
            let right = self.comparison()?;
            if !(truthy(&left) && truthy(&right)) {
                left = b"0".to_vec();
            }
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Eval {
        let mut left = self.additive()?;
        while let Some(op) = self.peek_is(&["<", "<=", "=", "==", "!=", ">=", ">"]) {
            let op = op.to_owned();
            self.bump();
            let right = self.additive()?;
            let ordering = compare(&left, &right);
            let result = match op.as_str() {
                "<" => ordering.is_lt(),
                "<=" => ordering.is_le(),
                "=" | "==" => ordering.is_eq(),
                "!=" => ordering.is_ne(),
                ">=" => ordering.is_ge(),
                _ => ordering.is_gt(),
            };
            left = flag(result);
        }
        Ok(left)
    }

    fn additive(&mut self) -> Eval {
        let mut left = self.multiplicative()?;
        while let Some(op) = self.peek_is(&["+", "-"]) {
            let op = op.to_owned();
            self.bump();
            let right = self.multiplicative()?;
            let (a, b) = (integer_operand(&left)?, integer_operand(&right)?);
            let result = if op == "+" {
                a.checked_add(b)
            } else {
                a.checked_sub(b)
            };
            left = number(result.ok_or_else(|| eval_error("integer overflow"))?);
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> Eval {
        let mut left = self.match_expr()?;
        while let Some(op) = self.peek_is(&["*", "/", "%"]) {
            let op = op.to_owned();
            self.bump();
            let right = self.match_expr()?;
            let (a, b) = (integer_operand(&left)?, integer_operand(&right)?);
            left = number(arithmetic(&op, a, b)?);
        }
        Ok(left)
    }

    fn match_expr(&mut self) -> Eval {
        let mut left = self.primary()?;
        while self.peek_is(&[":"]).is_some() {
            self.bump();
            let pattern = self.primary()?;
            left = match_pattern(&left, &pattern)?;
        }
        Ok(left)
    }

    fn primary(&mut self) -> Eval {
        let Some(token) = self.peek() else {
            return Err(self.missing_argument());
        };
        let token = token.to_owned();
        match token.as_str() {
            "(" => {
                self.bump();
                let inner = self.or_expr()?;
                if self.peek_is(&[")"]).is_none() {
                    return Err(syntax(format!(
                        "syntax error: expecting ')' after '{}'",
                        self.previous()
                    )));
                }
                self.bump();
                Ok(inner)
            }
            "+" => {
                self.bump();
                self.take_literal()
            }
            "length" | "index" | "substr" | "match" if self.function_has_arguments(&token) => {
                self.bump();
                self.function(&token)
            }
            _ => {
                self.bump();
                Ok(token.into_bytes())
            }
        }
    }

    fn function_has_arguments(&self, name: &str) -> bool {
        let needed = match name {
            "length" => 1,
            "substr" => 3,
            _ => 2,
        };
        self.tokens.len() - self.pos > needed
    }

    fn take_literal(&mut self) -> Eval {
        match self.bump() {
            Some(token) => Ok(token.as_bytes().to_vec()),
            None => Err(self.missing_argument()),
        }
    }

    fn function(&mut self, name: &str) -> Eval {
        let mut args = Vec::new();
        let count = match name {
            "length" => 1,
            "substr" => 3,
            _ => 2,
        };
        for _ in 0..count {
            args.push(self.take_literal()?);
        }
        match name {
            "length" => Ok(number(args[0].len() as i128)),
            "index" => Ok(number(index_of(&args[0], &args[1]))),
            "substr" => substring(&args[0], &args[1], &args[2]),
            _ => match_pattern(&args[0], &args[1]),
        }
    }
}

fn compare(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    match (as_integer(left), as_integer(right)) {
        (Some(Ok(a)), Some(Ok(b))) => a.cmp(&b),
        _ => left.cmp(right),
    }
}

fn arithmetic(op: &str, a: i128, b: i128) -> Result<i128, Failure> {
    if matches!(op, "/" | "%") && b == 0 {
        return Err(eval_error("division by zero"));
    }
    let result = match op {
        "*" => a.checked_mul(b),
        "/" => a.checked_div(b),
        _ => a.checked_rem(b),
    };
    result.ok_or_else(|| eval_error("integer overflow"))
}

/// 1-based byte position of the first byte of `subject` that occurs in `chars`, or 0.
fn index_of(subject: &[u8], chars: &[u8]) -> i128 {
    subject
        .iter()
        .position(|byte| chars.contains(byte))
        .map_or(0, |position| position as i128 + 1)
}

/// `substr STRING POS LENGTH`: empty unless both POS and LENGTH are at least 1.
fn substring(subject: &[u8], pos: &[u8], length: &[u8]) -> Eval {
    let start = integer_operand(pos)?;
    let len = integer_operand(length)?;
    if start < 1 || len < 1 || start as u128 > subject.len() as u128 {
        return Ok(Vec::new());
    }
    let begin = (start - 1) as usize;
    let end = begin
        .saturating_add(len.min(i128::from(u32::MAX)) as usize)
        .min(subject.len());
    Ok(subject[begin..end].to_vec())
}

/// `STRING : REGEX`: anchored at the start. Returns the first group when the pattern has one,
/// otherwise the number of matched bytes (0 on no match, empty on no match with a group).
fn match_pattern(subject: &[u8], pattern: &[u8]) -> Eval {
    let pattern_text = String::from_utf8_lossy(pattern);
    let translated = bre_to_regex(&pattern_text).map_err(syntax)?;
    let anchored = format!("^(?:{translated})");
    let regex =
        regex::bytes::Regex::new(&anchored).map_err(|error| syntax(regex_error_text(&error)))?;
    let has_group = regex.captures_len() > 1;
    match regex.captures(subject) {
        Some(captures) if has_group => Ok(captures
            .get(1)
            .map_or_else(Vec::new, |group| group.as_bytes().to_vec())),
        Some(captures) => Ok(number(
            captures.get(0).map_or(0, |whole| whole.len()) as i128
        )),
        None if has_group => Ok(Vec::new()),
        None => Ok(number(0)),
    }
}

/// GNU wording for the group errors the translated pattern can produce.
fn regex_error_text(error: &regex::Error) -> String {
    let text = error.to_string();
    if text.contains("unclosed group") {
        "Unmatched ( or \\(".to_owned()
    } else if text.contains("unopened group") {
        "Unmatched ) or \\)".to_owned()
    } else {
        "Invalid regular expression".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(tokens: &[&str]) -> Result<String, i32> {
        let owned: Vec<String> = tokens.iter().map(|t| (*t).to_owned()).collect();
        let mut evaluator = Evaluator {
            tokens: &owned,
            pos: 0,
        };
        match evaluator.evaluate() {
            Ok(value) => Ok(String::from_utf8(value).unwrap()),
            Err(failure) => Err(failure.code),
        }
    }

    #[test]
    fn arithmetic_follows_precedence() {
        assert_eq!(eval(&["1", "+", "2", "*", "3"]), Ok("7".into()));
        assert_eq!(eval(&["(", "1", "+", "2", ")", "*", "3"]), Ok("9".into()));
        assert_eq!(eval(&["-7", "/", "2"]), Ok("-3".into()));
        assert_eq!(eval(&["-7", "%", "2"]), Ok("-1".into()));
        assert_eq!(eval(&["+", "007", "+", "1"]), Ok("8".into()));
    }

    #[test]
    fn comparisons_are_numeric_for_integers_and_bytewise_otherwise() {
        assert_eq!(eval(&["10", ">", "9"]), Ok("1".into()));
        assert_eq!(eval(&["10", "<", "9x"]), Ok("1".into()));
        assert_eq!(eval(&["a", "=", "a"]), Ok("1".into()));
        assert_eq!(eval(&["a", "!=", "a"]), Ok("0".into()));
    }

    #[test]
    fn logic_operators_return_operands_or_zero() {
        assert_eq!(eval(&["0", "|", "x"]), Ok("x".into()));
        assert_eq!(eval(&["5", "&", "7"]), Ok("5".into()));
        assert_eq!(eval(&["5", "&", "0"]), Ok("0".into()));
    }

    #[test]
    fn match_returns_length_or_group() {
        assert_eq!(eval(&["abc", ":", "a\\(b\\)c"]), Ok("b".into()));
        assert_eq!(eval(&["abc", ":", "b"]), Ok("0".into()));
        assert_eq!(eval(&["abc", ":", ".*"]), Ok("3".into()));
        assert_eq!(eval(&["abc", ":", "a\\(x\\)"]), Ok(String::new()));
    }

    #[test]
    fn string_functions_work_on_bytes() {
        assert_eq!(eval(&["length", "abc"]), Ok("3".into()));
        assert_eq!(eval(&["substr", "abcdef", "2", "3"]), Ok("bcd".into()));
        assert_eq!(eval(&["substr", "abc", "0", "2"]), Ok(String::new()));
        assert_eq!(eval(&["index", "abc", "c"]), Ok("3".into()));
        assert_eq!(eval(&["index", "abc", ""]), Ok("0".into()));
    }

    #[test]
    fn errors_map_to_gnu_style_exit_codes() {
        assert_eq!(eval(&["1", "+"]), Err(EXIT_SYNTAX));
        assert_eq!(eval(&["1", "2"]), Err(EXIT_SYNTAX));
        assert_eq!(eval(&["(", "1"]), Err(EXIT_SYNTAX));
        assert_eq!(eval(&["1", "/", "0"]), Err(EXIT_EVAL));
        assert_eq!(eval(&["x", "+", "1"]), Err(EXIT_EVAL));
        assert_eq!(
            eval(&["170141183460469231731687303715884105727", "+", "1"]),
            Err(EXIT_EVAL)
        );
    }

    #[test]
    fn truthiness_treats_empty_and_zero_as_null() {
        assert!(!truthy(b""));
        assert!(!truthy(b"00"));
        assert!(!truthy(b"-0"));
        assert!(truthy(b"0x"));
        assert!(truthy(b"-3"));
    }
}
