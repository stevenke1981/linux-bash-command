#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Word(String),
    Number(f64),
    String(Vec<u8>),
    Symbol(&'static str),
    Newline,
    End,
}
#[derive(Clone, Debug)]
pub struct Token {
    pub kind: Kind,
    pub line: usize,
    pub start: usize,
}
pub struct Lexer {
    pub bytes: Vec<u8>,
    pub at: usize,
    pub line: usize,
}
impl Lexer {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            at: 0,
            line: 1,
        }
    }
    pub fn next(&mut self) -> Result<Token, String> {
        self.whitespace();
        let start = self.at;
        let line = self.line;
        let kind = match self.bytes.get(self.at).copied() {
            None => Kind::End,
            Some(b'\n') => {
                self.at += 1;
                self.line += 1;
                Kind::Newline
            }
            Some(b'"') => Kind::String(self.string()?),
            Some(b'0'..=b'9') => Kind::Number(self.number()?),
            Some(b'.') if self.bytes.get(self.at + 1).is_some_and(u8::is_ascii_digit) => {
                Kind::Number(self.number()?)
            }
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => Kind::Word(self.word()),
            Some(_) => Kind::Symbol(self.symbol()?),
        };
        Ok(Token { kind, line, start })
    }
    fn whitespace(&mut self) {
        loop {
            match self.bytes.get(self.at) {
                Some(b' ' | b'\t' | b'\r' | b'\x0c' | b'\x0b') => self.at += 1,
                Some(b'\\') if self.bytes.get(self.at + 1) == Some(&b'\n') => {
                    self.at += 2;
                    self.line += 1;
                }
                Some(b'#') => {
                    while !matches!(self.bytes.get(self.at), None | Some(b'\n')) {
                        self.at += 1;
                    }
                }
                _ => break,
            }
        }
    }
    fn word(&mut self) -> String {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.at += 1;
        }
        String::from_utf8(self.bytes[start..self.at].to_vec()).unwrap()
    }
    fn number(&mut self) -> Result<f64, String> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            let exponent = self.at;
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let digits = self.at;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            if digits == self.at {
                self.at = exponent;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .unwrap()
            .parse()
            .map_err(|_| self.error("invalid number"))
    }
    fn symbol(&mut self) -> Result<&'static str, String> {
        for symbol in [
            "**=", "++", "--", "+=", "-=", "*=", "/=", "%=", "^=", "**", "==", "!=", "<=", ">=",
            "!~", "&&", "||", ">>",
        ] {
            if self.bytes[self.at..].starts_with(symbol.as_bytes()) {
                self.at += symbol.len();
                return Ok(symbol);
            }
        }
        let symbol = match self.bytes[self.at] {
            b'{' => "{",
            b'}' => "}",
            b'(' => "(",
            b')' => ")",
            b'[' => "[",
            b']' => "]",
            b';' => ";",
            b',' => ",",
            b'$' => "$",
            b'?' => "?",
            b':' => ":",
            b'+' => "+",
            b'-' => "-",
            b'*' => "*",
            b'/' => "/",
            b'%' => "%",
            b'^' => "^",
            b'!' => "!",
            b'~' => "~",
            b'=' => "=",
            b'<' => "<",
            b'>' => ">",
            b'|' => "|",
            _ => return Err(self.error(&format!("unexpected byte 0x{:02x}", self.bytes[self.at]))),
        };
        self.at += 1;
        Ok(symbol)
    }
    fn string(&mut self) -> Result<Vec<u8>, String> {
        self.at += 1;
        let mut value = Vec::new();
        while let Some(&byte) = self.bytes.get(self.at) {
            self.at += 1;
            match byte {
                b'"' => return Ok(value),
                b'\n' => return Err(self.error("unterminated string")),
                b'\\' => {
                    if let Some(byte) = self.escape()? {
                        value.push(byte)
                    }
                }
                _ => value.push(byte),
            }
        }
        Err(self.error("unterminated string"))
    }
    fn escape(&mut self) -> Result<Option<u8>, String> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| self.error("trailing backslash"))?;
        self.at += 1;
        if byte == b'\n' {
            self.line += 1;
            return Ok(None);
        }
        if (b'0'..=b'7').contains(&byte) {
            return Ok(Some(self.digits(u32::from(byte - b'0'), 8, 2)));
        }
        Ok(Some(match byte {
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'n' => 10,
            b'r' => 13,
            b't' => 9,
            b'v' => 11,
            _ => byte,
        }))
    }
    fn digits(&mut self, mut value: u32, radix: u32, limit: usize) -> u8 {
        for _ in 0..limit {
            let Some(digit) = self
                .bytes
                .get(self.at)
                .and_then(|b| char::from(*b).to_digit(radix))
            else {
                break;
            };
            value = value.wrapping_mul(radix).wrapping_add(digit);
            self.at += 1;
        }
        value as u8
    }
    pub fn regex(&mut self, start: usize) -> Result<Vec<u8>, String> {
        self.at = start + 1;
        let mut result = Vec::new();
        while let Some(&byte) = self.bytes.get(self.at) {
            self.at += 1;
            match byte {
                b'/' => return Ok(result),
                b'\n' => return Err(self.error("unterminated regexp")),
                b'\\' => {
                    let next = *self
                        .bytes
                        .get(self.at)
                        .ok_or_else(|| self.error("unterminated regexp"))?;
                    self.at += 1;
                    if next == b'\n' {
                        self.line += 1;
                    } else if next == b'/' {
                        result.push(b'/');
                    } else {
                        result.extend_from_slice(&[b'\\', next]);
                    }
                }
                b'[' => {
                    result.push(byte);
                    self.regex_class(&mut result)?;
                }
                _ => result.push(byte),
            }
        }
        Err(self.error("unterminated regexp"))
    }
    fn regex_class(&mut self, result: &mut Vec<u8>) -> Result<(), String> {
        if self.bytes.get(self.at) == Some(&b'^') {
            result.push(b'^');
            self.at += 1;
        }
        if self.bytes.get(self.at) == Some(&b']') {
            result.push(b']');
            self.at += 1;
        }
        while let Some(&byte) = self.bytes.get(self.at) {
            self.at += 1;
            result.push(byte);
            if byte == b']' {
                return Ok(());
            }
            if byte == b'\n' {
                return Err(self.error("unterminated regexp"));
            }
            if byte == b'\\' {
                let next = *self
                    .bytes
                    .get(self.at)
                    .ok_or_else(|| self.error("unterminated regexp"))?;
                if next == b'/' {
                    result.pop();
                }
                result.push(next);
                self.at += 1;
            } else if byte == b'[' && matches!(self.bytes.get(self.at), Some(b':' | b'.' | b'=')) {
                let mark = self.bytes[self.at];
                result.push(mark);
                self.at += 1;
                loop {
                    let byte = *self
                        .bytes
                        .get(self.at)
                        .ok_or_else(|| self.error("unterminated regexp"))?;
                    self.at += 1;
                    result.push(byte);
                    if byte == mark && self.bytes.get(self.at) == Some(&b']') {
                        result.push(b']');
                        self.at += 1;
                        break;
                    }
                    if byte == b'\n' {
                        return Err(self.error("unterminated regexp"));
                    }
                }
            }
        }
        Err(self.error("unterminated regexp"))
    }
    fn error(&self, token: &str) -> String {
        format!("syntax error at source line {} near {token}", self.line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kinds(bytes: &[u8]) -> Vec<Kind> {
        let mut lexer = Lexer::new(bytes.to_vec());
        let mut result = Vec::new();
        loop {
            let token = lexer.next().unwrap();
            if token.kind == Kind::End {
                break;
            }
            result.push(token.kind);
        }
        result
    }
    #[test]
    fn numbers() {
        assert_eq!(
            kinds(b".5 12e2 1. 4e-2"),
            [
                Kind::Number(0.5),
                Kind::Number(1200.0),
                Kind::Number(1.0),
                Kind::Number(0.04)
            ]
        );
    }
    #[test]
    fn strings() {
        assert_eq!(kinds(br#""a\n\141\t""#), [Kind::String(b"a\na\t".to_vec())]);
    }
    #[test]
    fn comments_and_continuations() {
        assert_eq!(
            kinds(b"a # comment\n b\\\n+c"),
            [
                Kind::Word("a".into()),
                Kind::Newline,
                Kind::Word("b".into()),
                Kind::Symbol("+"),
                Kind::Word("c".into())
            ]
        );
    }
    #[test]
    fn operators() {
        assert_eq!(
            kinds(b"**= !~ ++ >>"),
            [
                Kind::Symbol("**="),
                Kind::Symbol("!~"),
                Kind::Symbol("++"),
                Kind::Symbol(">>")
            ]
        );
    }
    #[test]
    fn slash_reinterpreted_as_regexp() {
        let mut lexer = Lexer::new(br"/[/]/".to_vec());
        let token = lexer.next().unwrap();
        assert_eq!(lexer.regex(token.start).unwrap(), b"[/]");
    }
    #[test]
    fn unterminated_strings_are_errors() {
        assert!(Lexer::new(b"\"bad".to_vec()).next().is_err());
    }
}
