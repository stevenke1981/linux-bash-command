//! Tokenizer for awk program text.
//!
//! A `/` starts a regex literal unless the previous token ends an operand (a number, string,
//! name, `)`, `]`, `++`, `--`, a builtin name or a regex), in which case it is division.
//! Newlines are tokens; a backslash before a newline continues the line; `#` starts a comment.

/// One token kind.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Newline,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Not,
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
    Match,
    NoMatch,
    And,
    Or,
    Question,
    Colon,
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    Incr,
    Decr,
    Dollar,
    Append,
    Pipe,
    Regex(Vec<u8>),
    Number(f64),
    Str(Vec<u8>),
    Name(String),
    FuncName(String),
    Builtin(&'static str),
    Kw(&'static str),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
}

/// A lexical error: the line and a message.
#[derive(Debug, PartialEq, Eq)]
pub struct LexError {
    pub line: usize,
    pub message: String,
}

const KEYWORDS: &[&str] = &[
    "BEGIN", "END", "function", "func", "if", "else", "while", "for", "do", "break", "continue",
    "next", "nextfile", "exit", "return", "delete", "getline", "print", "printf", "in",
];

const BUILTINS: &[&str] = &[
    "length", "substr", "index", "split", "sub", "gsub", "match", "sprintf", "sin", "cos", "atan2",
    "exp", "log", "sqrt", "int", "rand", "srand", "tolower", "toupper", "close", "fflush",
    "system",
];

/// Splits program text into tokens. The last token is always [`Tok::Eof`].
pub fn tokenize(src: &[u8]) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer {
        src,
        pos: 0,
        line: 1,
        out: Vec::new(),
    };
    lexer.run()?;
    lexer.push(Tok::Eof);
    Ok(lexer.out)
}

/// Processes the escape sequences of an awk string (also used for `-v` and command-line
/// assignments). Unknown escapes yield the character itself.
pub fn unescape(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    let mut pos = 0;
    while pos < src.len() {
        let c = src[pos];
        pos += 1;
        if c == b'\\' && pos < src.len() {
            decode_escape(src, &mut pos, &mut out);
        } else {
            out.push(c);
        }
    }
    out
}

/// Decodes one escape sequence whose backslash is already consumed; `pos` indexes the
/// character after the backslash and is advanced past the sequence.
fn decode_escape(src: &[u8], pos: &mut usize, out: &mut Vec<u8>) {
    let c = src[*pos];
    *pos += 1;
    let simple = match c {
        b'n' => Some(b'\n'),
        b't' => Some(b'\t'),
        b'r' => Some(b'\r'),
        b'f' => Some(0x0c),
        b'v' => Some(0x0b),
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        _ => None,
    };
    if let Some(byte) = simple {
        out.push(byte);
        return;
    }
    if (b'0'..=b'7').contains(&c) {
        let mut value = u32::from(c - b'0');
        for _ in 0..2 {
            match src.get(*pos) {
                Some(&d) if (b'0'..=b'7').contains(&d) => {
                    value = value * 8 + u32::from(d - b'0');
                    *pos += 1;
                }
                _ => break,
            }
        }
        out.push(value as u8);
        return;
    }
    if c == b'x' {
        let mut value = 0u32;
        let mut digits = 0;
        while digits < 2 {
            match src.get(*pos).and_then(|&d| (d as char).to_digit(16)) {
                Some(digit) => {
                    value = value * 16 + digit;
                    *pos += 1;
                    digits += 1;
                }
                None => break,
            }
        }
        if digits == 0 {
            out.push(b'x');
        } else {
            out.push(value as u8);
        }
        return;
    }
    out.push(c);
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    out: Vec<Token>,
}

impl Lexer<'_> {
    fn run(&mut self) -> Result<(), LexError> {
        while self.pos < self.src.len() {
            let c = self.src[self.pos];
            match c {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => {
                    self.push(Tok::Newline);
                    self.line += 1;
                    self.pos += 1;
                }
                b'\\' if self.peek(1) == b'\n' => {
                    self.line += 1;
                    self.pos += 2;
                }
                b'\\' if self.peek(1) == b'\r' && self.peek(2) == b'\n' => {
                    self.line += 1;
                    self.pos += 3;
                }
                b'#' => {
                    while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                b'"' => {
                    let text = self.string()?;
                    self.push(Tok::Str(text));
                }
                b'/' if !self.operand_before() => {
                    let text = self.regex()?;
                    self.push(Tok::Regex(text));
                }
                b'0'..=b'9' | b'.' if c != b'.' || self.peek(1).is_ascii_digit() => {
                    let value = self.number()?;
                    self.push(Tok::Number(value));
                }
                _ if c.is_ascii_alphabetic() || c == b'_' => self.word(),
                _ => {
                    let (tok, len) = self.operator()?;
                    self.pos += len;
                    self.push(tok);
                }
            }
        }
        Ok(())
    }

    fn peek(&self, offset: usize) -> u8 {
        self.src.get(self.pos + offset).copied().unwrap_or(0)
    }

    fn push(&mut self, tok: Tok) {
        self.out.push(Token {
            tok,
            line: self.line,
        });
    }

    fn error(&self, message: impl Into<String>) -> LexError {
        LexError {
            line: self.line,
            message: message.into(),
        }
    }

    /// True when the previous token ends an operand, so `/` means division.
    fn operand_before(&self) -> bool {
        matches!(
            self.out.last().map(|t| &t.tok),
            Some(
                Tok::Number(_)
                    | Tok::Str(_)
                    | Tok::Name(_)
                    | Tok::Builtin(_)
                    | Tok::RParen
                    | Tok::RBracket
                    | Tok::Incr
                    | Tok::Decr
                    | Tok::Regex(_)
            )
        )
    }

    fn string(&mut self) -> Result<Vec<u8>, LexError> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.src.get(self.pos) else {
                return Err(self.error("unterminated string"));
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\n' => return Err(self.error("newline in string")),
                b'\\' => {
                    self.pos += 1;
                    if self.pos >= self.src.len() {
                        out.push(b'\\');
                    } else if self.src[self.pos] == b'\n' {
                        self.line += 1;
                        self.pos += 1;
                    } else {
                        decode_escape(self.src, &mut self.pos, &mut out);
                    }
                }
                _ => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    /// Reads a regex literal. `\/` becomes `/`; other escapes are kept for the regex
    /// translator. A `/` inside a bracket expression does not end the literal.
    fn regex(&mut self) -> Result<Vec<u8>, LexError> {
        self.pos += 1;
        let mut out = Vec::new();
        let mut in_bracket = false;
        loop {
            let Some(&c) = self.src.get(self.pos) else {
                return Err(self.error("non-terminated regular expression"));
            };
            if c == b'\n' {
                return Err(self.error("newline in regex"));
            }
            if c == b'\\' && self.pos + 1 < self.src.len() {
                let next = self.src[self.pos + 1];
                if next != b'/' {
                    out.push(c);
                }
                out.push(next);
                self.pos += 2;
                continue;
            }
            if in_bracket {
                if c == b']' {
                    in_bracket = false;
                }
            } else if c == b'[' {
                in_bracket = true;
                out.push(c);
                self.pos += 1;
                if self.peek(0) == b'^' {
                    out.push(b'^');
                    self.pos += 1;
                }
                if self.peek(0) == b']' {
                    out.push(b']');
                    self.pos += 1;
                }
                continue;
            } else if c == b'/' {
                self.pos += 1;
                return Ok(out);
            }
            out.push(c);
            self.pos += 1;
        }
    }

    fn number(&mut self) -> Result<f64, LexError> {
        let start = self.pos;
        while self.peek(0).is_ascii_digit() {
            self.pos += 1;
        }
        if self.peek(0) == b'.' {
            self.pos += 1;
            while self.peek(0).is_ascii_digit() {
                self.pos += 1;
            }
        }
        if matches!(self.peek(0), b'e' | b'E') {
            let sign = matches!(self.peek(1), b'+' | b'-');
            let digit_at = if sign { 2 } else { 1 };
            if self.peek(digit_at).is_ascii_digit() {
                self.pos += digit_at;
                while self.peek(0).is_ascii_digit() {
                    self.pos += 1;
                }
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("0");
        text.parse::<f64>()
            .map_err(|_| self.error(format!("invalid number '{text}'")))
    }

    fn word(&mut self) {
        let start = self.pos;
        while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
            self.pos += 1;
        }
        let name = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        if let Some(&keyword) = KEYWORDS.iter().find(|k| **k == name) {
            let keyword = if keyword == "func" {
                "function"
            } else {
                keyword
            };
            self.push(Tok::Kw(keyword));
        } else if let Some(&builtin) = BUILTINS.iter().find(|b| **b == name) {
            self.push(Tok::Builtin(builtin));
        } else if self.peek(0) == b'(' {
            self.push(Tok::FuncName(name));
        } else {
            self.push(Tok::Name(name));
        }
    }

    /// Returns the operator at the current position and its length in bytes.
    fn operator(&self) -> Result<(Tok, usize), LexError> {
        let c = self.peek(0);
        let d = self.peek(1);
        let e = self.peek(2);
        let found = match c {
            b'*' if d == b'*' && e == b'=' => (Tok::PowAssign, 3),
            b'*' if d == b'*' => (Tok::Caret, 2),
            b'*' if d == b'=' => (Tok::MulAssign, 2),
            b'*' => (Tok::Star, 1),
            b'^' if d == b'=' => (Tok::PowAssign, 2),
            b'^' => (Tok::Caret, 1),
            b'+' if d == b'+' => (Tok::Incr, 2),
            b'+' if d == b'=' => (Tok::AddAssign, 2),
            b'+' => (Tok::Plus, 1),
            b'-' if d == b'-' => (Tok::Decr, 2),
            b'-' if d == b'=' => (Tok::SubAssign, 2),
            b'-' => (Tok::Minus, 1),
            b'/' if d == b'=' => (Tok::DivAssign, 2),
            b'/' => (Tok::Slash, 1),
            b'%' if d == b'=' => (Tok::ModAssign, 2),
            b'%' => (Tok::Percent, 1),
            b'=' if d == b'=' => (Tok::Eq, 2),
            b'=' => (Tok::Assign, 1),
            b'!' if d == b'=' => (Tok::Ne, 2),
            b'!' if d == b'~' => (Tok::NoMatch, 2),
            b'!' => (Tok::Not, 1),
            b'~' => (Tok::Match, 1),
            b'<' if d == b'=' => (Tok::Le, 2),
            b'<' => (Tok::Lt, 1),
            b'>' if d == b'=' => (Tok::Ge, 2),
            b'>' if d == b'>' => (Tok::Append, 2),
            b'>' => (Tok::Gt, 1),
            b'&' if d == b'&' => (Tok::And, 2),
            b'|' if d == b'|' => (Tok::Or, 2),
            b'|' => (Tok::Pipe, 1),
            b'?' => (Tok::Question, 1),
            b':' => (Tok::Colon, 1),
            b'{' => (Tok::LBrace, 1),
            b'}' => (Tok::RBrace, 1),
            b'(' => (Tok::LParen, 1),
            b')' => (Tok::RParen, 1),
            b'[' => (Tok::LBracket, 1),
            b']' => (Tok::RBracket, 1),
            b';' => (Tok::Semi, 1),
            b',' => (Tok::Comma, 1),
            b'$' => (Tok::Dollar, 1),
            _ => {
                let shown = char::from(c);
                return Err(self.error(format!("invalid character '{shown}' in program")));
            }
        };
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        tokenize(src.as_bytes())
            .expect("tokenize")
            .into_iter()
            .map(|t| t.tok)
            .collect()
    }

    #[test]
    fn slash_after_operand_is_division() {
        assert_eq!(
            toks("a / 2"),
            vec![
                Tok::Name("a".into()),
                Tok::Slash,
                Tok::Number(2.0),
                Tok::Eof
            ]
        );
        assert_eq!(toks("(1) / 2")[3], Tok::Slash);
        assert_eq!(toks("x++ / 2")[2], Tok::Slash);
    }

    #[test]
    fn slash_elsewhere_starts_regex() {
        assert_eq!(toks("/a+b/")[0], Tok::Regex(b"a+b".to_vec()));
        assert_eq!(toks("x ~ /a/")[2], Tok::Regex(b"a".to_vec()));
        assert_eq!(toks("print /x/")[1], Tok::Regex(b"x".to_vec()));
    }

    #[test]
    fn slash_inside_bracket_does_not_end_regex() {
        assert_eq!(toks("/[/]x/")[0], Tok::Regex(b"[/]x".to_vec()));
        assert_eq!(toks(r"/a\/b/")[0], Tok::Regex(b"a/b".to_vec()));
    }

    #[test]
    fn division_assignment_versus_regex() {
        assert_eq!(toks("a /= 2")[1], Tok::DivAssign);
        assert_eq!(toks("/=/")[0], Tok::Regex(b"=".to_vec()));
    }

    #[test]
    fn newlines_and_continuations() {
        assert_eq!(
            toks("a \\\n b\n"),
            vec![
                Tok::Name("a".into()),
                Tok::Name("b".into()),
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn comments_run_to_end_of_line() {
        assert_eq!(
            toks("x # note\ny"),
            vec![
                Tok::Name("x".into()),
                Tok::Newline,
                Tok::Name("y".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn keywords_builtins_and_function_names() {
        assert_eq!(toks("func")[0], Tok::Kw("function"));
        assert_eq!(toks("length")[0], Tok::Builtin("length"));
        assert_eq!(toks("foo(1)")[0], Tok::FuncName("foo".into()));
        assert_eq!(toks("foo (1)")[0], Tok::Name("foo".into()));
        assert_eq!(toks("getline")[0], Tok::Kw("getline"));
    }

    #[test]
    fn power_operators() {
        assert_eq!(toks("a ** b")[1], Tok::Caret);
        assert_eq!(toks("a **= b")[1], Tok::PowAssign);
        assert_eq!(toks("a ^= b")[1], Tok::PowAssign);
    }

    #[test]
    fn string_escapes() {
        assert_eq!(
            toks(r#""a\tb\"\\\101\x41\q""#)[0],
            Tok::Str(b"a\tb\"\\AAq".to_vec())
        );
        assert_eq!(toks(r#""\/""#)[0], Tok::Str(b"/".to_vec()));
    }

    #[test]
    fn numbers_and_exponents() {
        assert_eq!(toks("1e3")[0], Tok::Number(1000.0));
        assert_eq!(toks(".5")[0], Tok::Number(0.5));
        assert_eq!(toks("2.")[0], Tok::Number(2.0));
        assert_eq!(toks("1e")[0], Tok::Number(1.0));
        assert_eq!(toks("1e")[1], Tok::Name("e".into()));
    }

    #[test]
    fn errors_report_line_numbers() {
        assert_eq!(
            tokenize(b"a\n\"open").unwrap_err(),
            LexError {
                line: 2,
                message: "unterminated string".into()
            }
        );
        assert!(tokenize(b"a & b").is_err());
        assert!(tokenize(b"/abc").is_err());
    }

    #[test]
    fn unescape_handles_all_forms() {
        assert_eq!(unescape(b"x\\ny"), b"x\ny".to_vec());
        assert_eq!(unescape(b"\\\\"), b"\\".to_vec());
        assert_eq!(unescape(b"\\x"), b"x".to_vec());
        assert_eq!(unescape(b"\\x4a"), b"J".to_vec());
    }
}
