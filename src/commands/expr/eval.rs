use super::{parse::Node, Failure, Result};
use std::cmp::Ordering;

fn numeric(bytes: &[u8]) -> bool {
    let digits = bytes.strip_prefix(b"-").unwrap_or(bytes);
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}
pub(super) fn truth(bytes: &[u8]) -> bool {
    !bytes.is_empty() && (!numeric(bytes) || bytes.iter().any(|b| matches!(b, b'1'..=b'9')))
}
fn integer(bytes: &[u8]) -> Result<i128> {
    if !numeric(bytes) {
        return Err(Failure::runtime("non-integer argument"));
    }
    std::str::from_utf8(bytes)
        .unwrap()
        .parse()
        .map_err(|_| Failure::runtime("integer overflow"))
}
fn number(number: i128) -> Vec<u8> {
    number.to_string().into_bytes()
}

pub(super) fn evaluate(node: &Node) -> Result<Vec<u8>> {
    match node {
        Node::Word(word) => Ok(word.clone()),
        Node::Function(name, arguments) => function(name, arguments),
        Node::Binary(operator, left, right) => {
            let left = evaluate(left)?;
            if *operator == "|" && truth(&left) {
                return Ok(left);
            }
            if *operator == "&" && !truth(&left) {
                return Ok(number(0));
            }
            binary(operator, left, evaluate(right)?)
        }
    }
}
fn binary(operator: &str, left: Vec<u8>, right: Vec<u8>) -> Result<Vec<u8>> {
    match operator {
        "|" => Ok(if truth(&right) { right } else { number(0) }),
        "&" => Ok(if truth(&right) { left } else { number(0) }),
        ":" => pattern(&left, &right),
        "=" | "!=" | "<" | "<=" | ">=" | ">" => {
            let comparison = if numeric(&left) && numeric(&right) {
                integer(&left)?.cmp(&integer(&right)?)
            } else {
                left.cmp(&right)
            };
            let yes = match operator {
                "=" => comparison == Ordering::Equal,
                "!=" => comparison != Ordering::Equal,
                "<" => comparison == Ordering::Less,
                "<=" => comparison != Ordering::Greater,
                ">" => comparison == Ordering::Greater,
                _ => comparison != Ordering::Less,
            };
            Ok(number(i128::from(yes)))
        }
        _ => arithmetic(operator, integer(&left)?, integer(&right)?).map(number),
    }
}
fn arithmetic(operator: &str, left: i128, right: i128) -> Result<i128> {
    if matches!(operator, "/" | "%") && right == 0 {
        return Err(Failure::runtime("division by zero"));
    }
    match operator {
        "+" => left.checked_add(right),
        "-" => left.checked_sub(right),
        "*" => left.checked_mul(right),
        "/" => left.checked_div(right),
        "%" => left.checked_rem(right),
        _ => unreachable!(),
    }
    .ok_or_else(|| Failure::runtime("integer overflow"))
}
fn function(name: &str, arguments: &[Node]) -> Result<Vec<u8>> {
    let values = arguments.iter().map(evaluate).collect::<Result<Vec<_>>>()?;
    match name {
        "length" => Ok(number(values[0].len() as i128)),
        "index" => Ok(number(
            values[0]
                .iter()
                .position(|b| values[1].contains(b))
                .map_or(0, |i| i + 1) as i128,
        )),
        "match" => pattern(&values[0], &values[1]),
        "substr" => {
            if !numeric(&values[1]) || !numeric(&values[2]) {
                return Ok(Vec::new());
            }
            let (start, length) = (integer(&values[1])?, integer(&values[2])?);
            if start < 1 || length < 1 {
                return Ok(Vec::new());
            }
            let start = usize::try_from(start - 1)
                .unwrap_or(usize::MAX)
                .min(values[0].len());
            let end = start
                .saturating_add(usize::try_from(length).unwrap_or(usize::MAX))
                .min(values[0].len());
            Ok(values[0][start..end].to_vec())
        }
        _ => unreachable!(),
    }
}
fn pattern(string: &[u8], expression: &[u8]) -> Result<Vec<u8>> {
    let matcher = crate::common::bre::Matcher::compile(expression).map_err(Failure::syntax)?;
    let matched = matcher.captures_at_start(string);
    Ok(if matcher.group_count() > 0 {
        matched
            .and_then(|c| c.range(1))
            .map_or_else(Vec::new, |range| string[range].to_vec())
    } else {
        number(matched.map_or(0, |c| c.range(0).unwrap().len()) as i128)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(words: &[&str]) -> Result<Vec<u8>> {
        evaluate(&super::super::parse::expression(
            &words.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )?)
    }
    #[test]
    fn precedence_and_parentheses() {
        assert_eq!(value(&["2", "+", "3", "*", "4"]).unwrap(), b"14");
        assert_eq!(value(&["(", "2", "+", "3", ")", "*", "4"]).unwrap(), b"20");
    }
    #[test]
    fn short_circuit_and_truth() {
        assert_eq!(value(&["yes", "|", "1", "/", "0"]).unwrap(), b"yes");
        assert_eq!(value(&["00", "&", "1", "/", "0"]).unwrap(), b"0");
        for v in [b"".as_slice(), b"0", b"-000"] {
            assert!(!truth(v));
        }
        assert!(truth(b"+0"));
    }
    #[test]
    fn byte_functions_and_quoting() {
        assert_eq!(value(&["+", "length"]).unwrap(), b"length");
        assert_eq!(value(&["length", "\u{00e9}"]).unwrap(), b"2");
        assert_eq!(value(&["substr", "abcd", "2", "2"]).unwrap(), b"bc");
        assert_eq!(value(&["index", "abc", "cb"]).unwrap(), b"2");
    }
    #[test]
    fn longest_bre_capture_and_binary() {
        assert_eq!(pattern(b"abc", br"a\|ab").unwrap(), b"2");
        assert_eq!(pattern(b"abc", br"\(a.*\)").unwrap(), b"abc");
        assert_eq!(pattern(b"\xff", b"\xff").unwrap(), b"1");
    }
    #[test]
    fn syntax_and_checked_arithmetic() {
        assert_eq!(value(&["1", "+"]).unwrap_err().code, 2);
        assert_eq!(value(&["1", "/", "0"]).unwrap_err().code, 3);
        assert_eq!(
            value(&[&i128::MAX.to_string(), "+", "1"])
                .unwrap_err()
                .message,
            "integer overflow"
        );
        assert_eq!(value(&["-2", "%", "3"]).unwrap(), b"-2");
    }
}
