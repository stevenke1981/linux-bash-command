use super::{Failure, Result};

#[derive(Debug)]
pub(super) enum Node {
    Word(Vec<u8>),
    Function(&'static str, Vec<Node>),
    Binary(&'static str, Box<Node>, Box<Node>),
}

fn operator(word: &str) -> Option<(&'static str, u8)> {
    Some(match word {
        "|" => ("|", 1),
        "&" => ("&", 2),
        "<" => ("<", 3),
        "<=" => ("<=", 3),
        "=" | "==" => ("=", 3),
        "!=" => ("!=", 3),
        ">=" => (">=", 3),
        ">" => (">", 3),
        "+" => ("+", 4),
        "-" => ("-", 4),
        "*" => ("*", 5),
        "/" => ("/", 5),
        "%" => ("%", 5),
        ":" => (":", 6),
        _ => return None,
    })
}

struct Parser<'a> {
    words: &'a [String],
    at: usize,
}
impl Parser<'_> {
    fn next(&mut self) -> Result<&str> {
        let word = self.words.get(self.at).ok_or_else(|| {
            Failure::syntax(if let Some(last) = self.words.last() {
                format!("syntax error: missing argument after '{last}'")
            } else {
                "missing operand".into()
            })
        })?;
        self.at += 1;
        Ok(word)
    }
    fn primary(&mut self) -> Result<Node> {
        let word = self.next()?.to_owned();
        if word == "+" {
            return Ok(Node::Word(self.next()?.as_bytes().to_vec()));
        }
        if word == ")" {
            return Err(Failure::syntax("syntax error: unexpected ')'"));
        }
        if word == "(" {
            let node = self.binary(1)?;
            if self.next()? != ")" {
                return Err(Failure::syntax("syntax error: expecting ')'"));
            }
            return Ok(node);
        }
        let (name, count) = match word.as_str() {
            "length" => ("length", 1),
            "index" => ("index", 2),
            "substr" => ("substr", 3),
            "match" => ("match", 2),
            _ => return Ok(Node::Word(word.into_bytes())),
        };
        let mut arguments = Vec::new();
        for _ in 0..count {
            arguments.push(self.primary()?);
        }
        Ok(Node::Function(name, arguments))
    }
    fn binary(&mut self, minimum: u8) -> Result<Node> {
        let mut left = self.primary()?;
        while let Some((operator, priority)) = self.words.get(self.at).and_then(|w| operator(w)) {
            if priority < minimum {
                break;
            }
            self.at += 1;
            let right = self.binary(priority + 1)?;
            left = Node::Binary(operator, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
}

pub(super) fn expression(words: &[String]) -> Result<Node> {
    let mut parser = Parser { words, at: 0 };
    let node = parser.binary(1)?;
    if let Some(extra) = words.get(parser.at) {
        Err(Failure::syntax(format!(
            "syntax error: unexpected argument '{extra}'"
        )))
    } else {
        Ok(node)
    }
}
