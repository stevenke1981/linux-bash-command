//! Expression evaluation for `test` and `[`.
//!
//! Argument counts 0 to 4 follow the POSIX special cases. Anything else goes to a
//! recursive-descent parser: `!` binds tighter than `-a`, which binds tighter than `-o`,
//! and parentheses group.

use super::files;

/// Evaluates the operands (without the closing `]` of `[`).
pub fn evaluate(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [one] => Ok(!one.is_empty()),
        [_, _] => two(args),
        [_, _, _] => three(args),
        [_, _, _, _] => four(args),
        _ => general(args),
    }
}

fn two(args: &[String]) -> Result<bool, String> {
    let (first, second) = (args[0].as_str(), args[1].as_str());
    if first == "!" {
        return Ok(second.is_empty());
    }
    if is_unary(first) {
        return unary(first, second);
    }
    Err(format!("'{first}': unary operator expected"))
}

fn three(args: &[String]) -> Result<bool, String> {
    let (first, middle, last) = (args[0].as_str(), args[1].as_str(), args[2].as_str());
    if is_binary(middle) {
        return binary(first, middle, last);
    }
    if first == "!" {
        return Ok(!two(&args[1..])?);
    }
    if first == "(" && last == ")" {
        return Ok(!middle.is_empty());
    }
    general(args)
}

fn four(args: &[String]) -> Result<bool, String> {
    if args[0] == "!" {
        return Ok(!three(&args[1..])?);
    }
    if args[0] == "(" && args[3] == ")" {
        return two(&args[1..3]);
    }
    general(args)
}

/// Parses the whole operand list with the precedence grammar.
fn general(args: &[String]) -> Result<bool, String> {
    let mut parser = Parser {
        tokens: args,
        pos: 0,
    };
    let value = parser.or()?;
    match parser.peek() {
        Some(extra) => Err(format!("extra argument '{extra}'")),
        None => Ok(value),
    }
}

struct Parser<'a> {
    tokens: &'a [String],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    fn remaining(&self) -> usize {
        self.tokens.len() - self.pos
    }

    fn or(&mut self) -> Result<bool, String> {
        let mut value = self.and()?;
        while self.peek() == Some("-o") {
            self.pos += 1;
            let right = self.and()?;
            value = value || right;
        }
        Ok(value)
    }

    fn and(&mut self) -> Result<bool, String> {
        let mut value = self.not()?;
        while self.peek() == Some("-a") {
            self.pos += 1;
            let right = self.not()?;
            value = value && right;
        }
        Ok(value)
    }

    fn not(&mut self) -> Result<bool, String> {
        if self.peek() == Some("!") && self.remaining() > 1 {
            self.pos += 1;
            return Ok(!self.not()?);
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<bool, String> {
        let Some(token) = self.peek() else {
            return Err("argument expected".to_owned());
        };
        if token == "(" && self.remaining() > 1 {
            self.pos += 1;
            let value = self.or()?;
            if self.peek() != Some(")") {
                return Err("')' expected".to_owned());
            }
            self.pos += 1;
            return Ok(value);
        }
        let next = self.tokens.get(self.pos + 1).map(String::as_str);
        if let Some(op) = next.filter(|op| is_binary(op)) {
            if self.remaining() < 3 {
                return Err(format!("missing argument after '{op}'"));
            }
            let right = self.tokens[self.pos + 2].as_str();
            let value = binary(token, op, right)?;
            self.pos += 3;
            return Ok(value);
        }
        if let (true, Some(operand)) = (is_unary(token), next) {
            let value = unary(token, operand)?;
            self.pos += 2;
            return Ok(value);
        }
        self.pos += 1;
        Ok(!token.is_empty())
    }
}

fn is_unary(token: &str) -> bool {
    matches!(
        token,
        "-b" | "-c"
            | "-d"
            | "-e"
            | "-f"
            | "-g"
            | "-G"
            | "-h"
            | "-L"
            | "-k"
            | "-N"
            | "-O"
            | "-p"
            | "-r"
            | "-s"
            | "-S"
            | "-t"
            | "-u"
            | "-w"
            | "-x"
            | "-z"
            | "-n"
    )
}

fn is_binary(token: &str) -> bool {
    matches!(
        token,
        "=" | "=="
            | "!="
            | "<"
            | ">"
            | "-eq"
            | "-ne"
            | "-lt"
            | "-le"
            | "-gt"
            | "-ge"
            | "-nt"
            | "-ot"
            | "-ef"
    )
}

fn unary(op: &str, operand: &str) -> Result<bool, String> {
    match op {
        "-n" => Ok(!operand.is_empty()),
        "-z" => Ok(operand.is_empty()),
        "-t" => {
            let fd = parse_integer(operand)?;
            Ok(i32::try_from(fd).is_ok_and(files::is_terminal))
        }
        _ => Ok(files::file_test(op, operand)),
    }
}

fn binary(left: &str, op: &str, right: &str) -> Result<bool, String> {
    match op {
        "=" | "==" => Ok(left == right),
        "!=" => Ok(left != right),
        "<" => Ok(left < right),
        ">" => Ok(left > right),
        "-nt" => Ok(files::newer(left, right)),
        "-ot" => Ok(files::older(left, right)),
        "-ef" => Ok(files::same(left, right)),
        _ => {
            let (a, b) = (parse_integer(left)?, parse_integer(right)?);
            Ok(match op {
                "-eq" => a == b,
                "-ne" => a != b,
                "-lt" => a < b,
                "-le" => a <= b,
                "-gt" => a > b,
                _ => a >= b,
            })
        }
    }
}

/// An integer operand: optional surrounding white space and sign, then decimal digits.
/// Values beyond the `i64` range are clamped.
fn parse_integer(text: &str) -> Result<i64, String> {
    let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("invalid integer '{text}'"));
    }
    let magnitude = digits.parse::<u128>().unwrap_or(u128::MAX);
    Ok(if negative {
        if magnitude >= 1 << 63 {
            i64::MIN
        } else {
            -(magnitude as i64)
        }
    } else {
        i64::try_from(magnitude).unwrap_or(i64::MAX)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(items: &[&str]) -> Result<bool, String> {
        let args: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
        evaluate(&args)
    }

    #[test]
    fn posix_argument_count_rules() {
        assert_eq!(eval(&[]), Ok(false));
        assert_eq!(eval(&["-n"]), Ok(true));
        assert_eq!(eval(&["!", ""]), Ok(true));
        assert_eq!(eval(&["a", "=", "a"]), Ok(true));
        assert_eq!(eval(&["(", "x", ")"]), Ok(true));
        assert_eq!(eval(&["!", "a", "=", "b"]), Ok(true));
        assert!(eval(&["a", "b"]).is_err());
    }

    #[test]
    fn precedence_is_not_then_and_then_or() {
        assert_eq!(
            eval(&["1", "-eq", "2", "-o", "3", "-eq", "3", "-a", "4", "-eq", "5"]),
            Ok(false)
        );
        assert_eq!(
            eval(&["-z", "", "-a", "(", "-n", "x", "-o", "-z", "y", ")"]),
            Ok(true)
        );
        assert_eq!(eval(&["!", "a", "-a", "b"]), Ok(false));
    }

    #[test]
    fn integers_allow_space_and_sign() {
        assert_eq!(parse_integer(" 12 "), Ok(12));
        assert_eq!(parse_integer("-7"), Ok(-7));
        assert_eq!(parse_integer("+3"), Ok(3));
        assert_eq!(parse_integer("99999999999999999999"), Ok(i64::MAX));
        assert_eq!(parse_integer("x"), Err("invalid integer 'x'".to_owned()));
    }

    #[test]
    fn string_comparisons_are_bytewise() {
        assert_eq!(eval(&["abc", "<", "abd"]), Ok(true));
        assert_eq!(eval(&["b", ">", "a"]), Ok(true));
        assert_eq!(eval(&["x", "!=", "x"]), Ok(false));
    }

    #[test]
    fn syntax_errors_are_reported() {
        assert!(eval(&["(", "a"]).is_err());
        assert!(eval(&["a", "-eq"]).is_err());
        assert!(eval(&["a", "-a", "b", "c", "d", "e"]).is_err());
    }
}
