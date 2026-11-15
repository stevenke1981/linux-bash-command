use super::{
    addr::{Address, End, Selection},
    options::Source,
    pattern, replacement,
    subst::Substitution,
};
use std::{collections::HashMap, path::PathBuf};

#[derive(Clone, Debug)]
pub enum Action {
    Open(usize),
    Close,
    Number,
    Text(u8, Vec<u8>),
    Branch(u8, usize),
    Label,
    Delete,
    DeleteFirst,
    Filename,
    Get(bool),
    Hold(bool),
    Exchange,
    List(Option<usize>),
    Next(bool),
    Print(bool),
    Quit(bool, i32),
    Read(PathBuf, bool),
    Write(PathBuf, bool),
    Substitute(Substitution),
    Translate(Vec<(u8, u8)>),
    Clear,
}
#[derive(Clone, Debug)]
pub struct Instruction {
    pub selection: Selection,
    pub action: Action,
    pub location: String,
}
#[derive(Debug)]
pub struct Program {
    pub instructions: Vec<Instruction>,
    pub quiet: bool,
}
struct Span {
    start: usize,
    end: usize,
    name: String,
}
struct Parser {
    bytes: Vec<u8>,
    at: usize,
    spans: Vec<Span>,
    extended: bool,
}

pub fn compile(sources: &[Source], extended: bool) -> Result<Program, String> {
    let mut parser = Parser {
        bytes: Vec::new(),
        at: 0,
        spans: Vec::new(),
        extended,
    };
    for source in sources {
        let start = parser.bytes.len();
        parser.bytes.extend_from_slice(&source.bytes);
        parser.spans.push(Span {
            start,
            end: parser.bytes.len(),
            name: source.name.clone(),
        });
        parser.bytes.push(b'\n');
    }
    let quiet = parser.bytes.starts_with(b"#n");
    let instructions = parser.program()?;
    Ok(Program {
        instructions,
        quiet,
    })
}

impl Parser {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    fn spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) {
            self.at += 1;
        }
    }
    fn location(&self) -> String {
        let span = self
            .spans
            .iter()
            .find(|span| self.at <= span.end)
            .or(self.spans.last())
            .unwrap();
        if span.name.starts_with("file ") {
            let line = self.bytes[span.start..self.at.min(span.end)]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1;
            format!("{} line {line}", span.name)
        } else {
            format!("{}, char {}", span.name, self.at.saturating_sub(span.start))
        }
    }
    fn error(&self, message: impl std::fmt::Display) -> String {
        format!("{}: {message}", self.location())
    }
    fn number(&mut self) -> Result<u64, String> {
        let start = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .unwrap()
            .parse()
            .map_err(|_| self.error("number too big"))
    }
    fn program(&mut self) -> Result<Vec<Instruction>, String> {
        let mut instructions: Vec<Instruction> = Vec::new();
        let (mut blocks, mut labels, mut branches) = (Vec::new(), HashMap::new(), Vec::new());
        while self.at < self.bytes.len() {
            self.spaces();
            if matches!(self.peek(), Some(b';' | b'\n')) {
                self.at += 1;
                continue;
            }
            if self.peek().is_none() {
                break;
            }
            let selection = self.selection()?;
            self.spaces();
            let command = self.peek().ok_or_else(|| self.error("missing command"))?;
            self.at += 1;
            let index = instructions.len();
            let action = match command {
                b'{' => {
                    blocks.push(index);
                    Action::Open(usize::MAX)
                }
                b'}' => {
                    if selection.first.is_some() {
                        return Err(self.error("'}' doesn't want any addresses"));
                    }
                    let open = blocks.pop().ok_or_else(|| self.error("unexpected `}'"))?;
                    if let Action::Open(target) = &mut instructions[open].action {
                        *target = index + 1;
                    }
                    Action::Close
                }
                b'#' => {
                    self.line();
                    continue;
                }
                b':' => {
                    if selection.first.is_some() {
                        return Err(self.error(": doesn't want any addresses"));
                    }
                    let name = self.word();
                    labels.insert(name, index);
                    Action::Label
                }
                b'b' | b't' | b'T' => {
                    let name = self.word();
                    branches.push((index, name));
                    Action::Branch(command, usize::MAX)
                }
                b'=' => {
                    self.ending()?;
                    Action::Number
                }
                b'a' | b'i' | b'c' => Action::Text(command, self.text()?),
                b'd' => {
                    self.ending()?;
                    Action::Delete
                }
                b'D' => {
                    self.ending()?;
                    Action::DeleteFirst
                }
                b'F' => {
                    self.ending()?;
                    Action::Filename
                }
                b'g' | b'G' => {
                    self.ending()?;
                    Action::Get(command == b'G')
                }
                b'h' | b'H' => {
                    self.ending()?;
                    Action::Hold(command == b'H')
                }
                b'x' => {
                    self.ending()?;
                    Action::Exchange
                }
                b'l' => Action::List(self.optional_number()?.map(|n| n as usize)),
                b'n' | b'N' => {
                    self.ending()?;
                    Action::Next(command == b'N')
                }
                b'p' | b'P' => {
                    self.ending()?;
                    Action::Print(command == b'P')
                }
                b'q' | b'Q' => Action::Quit(
                    command == b'q',
                    self.optional_number()?.unwrap_or(0) as u8 as i32,
                ),
                b'r' | b'R' => Action::Read(self.filename()?, command == b'R'),
                b'w' | b'W' => Action::Write(self.filename()?, command == b'W'),
                b's' => Action::Substitute(self.substitution()?),
                b'y' => Action::Translate(self.translate()?),
                b'z' => {
                    self.ending()?;
                    Action::Clear
                }
                b'e' => return Err(self.error("command 'e' is not supported by agentcmd")),
                _ => return Err(self.error(format!("unknown command: `{}'", char::from(command)))),
            };
            instructions.push(Instruction {
                selection,
                action,
                location: self.location(),
            });
        }
        if !blocks.is_empty() {
            return Err(self.error("unmatched `{'"));
        }
        for (index, name) in branches {
            let target = if name.is_empty() {
                instructions.len()
            } else {
                *labels
                    .get(&name)
                    .ok_or_else(|| format!("can't find label for jump to `{name}'"))?
            };
            if let Action::Branch(_, jump) = &mut instructions[index].action {
                *jump = target;
            }
        }
        Ok(instructions)
    }
    fn selection(&mut self) -> Result<Selection, String> {
        let first = self.address()?;
        self.spaces();
        let end = if self.peek() == Some(b',') {
            self.at += 1;
            self.spaces();
            match self.peek() {
                Some(b'+' | b'~') => {
                    let kind = self.peek().unwrap();
                    self.at += 1;
                    let n = self.number()?;
                    Some(if kind == b'+' {
                        End::Plus(n)
                    } else {
                        End::Multiple(n)
                    })
                }
                _ => Some(End::Address(
                    self.address()?
                        .ok_or_else(|| self.error("unexpected `,'"))?,
                )),
            }
        } else {
            None
        };
        if matches!(first, Some(Address::Number(0)))
            && !matches!(end, Some(End::Address(Address::Regex(_))))
        {
            return Err(self.error("invalid usage of line address 0"));
        }
        self.spaces();
        let negate = self.peek() == Some(b'!');
        if negate {
            self.at += 1;
        }
        Ok(Selection::new(first, end, negate))
    }
    fn address(&mut self) -> Result<Option<Address>, String> {
        Ok(match self.peek() {
            Some(b'0'..=b'9') => {
                let n = self.number()?;
                if self.peek() == Some(b'~') {
                    self.at += 1;
                    Some(Address::Step(n, self.number()?))
                } else {
                    Some(Address::Number(n))
                }
            }
            Some(b'$') => {
                self.at += 1;
                Some(Address::Last)
            }
            Some(b'/' | b'\\') => {
                let mut delimiter = self.peek().unwrap();
                self.at += 1;
                if delimiter == b'\\' {
                    delimiter = self
                        .peek()
                        .ok_or_else(|| self.error("unterminated address regex"))?;
                    self.at += 1;
                }
                let bytes = self.delimited(delimiter, true, "unterminated address regex")?;
                let (mut insensitive, mut multiline) = (false, false);
                while matches!(self.peek(), Some(b'I' | b'M')) {
                    if self.peek() == Some(b'I') {
                        insensitive = true;
                    } else {
                        multiline = true;
                    }
                    self.at += 1;
                }
                Some(Address::Regex(
                    pattern::compile(&bytes, self.extended, insensitive, multiline)
                        .map_err(|e| self.error(e))?,
                ))
            }
            _ => None,
        })
    }
    fn ending(&mut self) -> Result<(), String> {
        self.spaces();
        if matches!(self.peek(), None | Some(b';' | b'\n' | b'}' | b'#')) {
            Ok(())
        } else {
            Err(self.error("extra characters after command"))
        }
    }
    fn optional_number(&mut self) -> Result<Option<u64>, String> {
        self.spaces();
        let number = if self.peek().is_some_and(|b| b.is_ascii_digit()) {
            Some(self.number()?)
        } else {
            None
        };
        self.ending()?;
        Ok(number)
    }
    fn line(&mut self) -> Vec<u8> {
        let start = self.at;
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.at += 1;
        }
        self.bytes[start..self.at].to_vec()
    }
    fn word(&mut self) -> String {
        self.spaces();
        let start = self.at;
        while !matches!(self.peek(), None | Some(b';' | b'\n' | b'}')) {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..self.at])
            .trim_end()
            .to_owned()
    }
    fn filename(&mut self) -> Result<PathBuf, String> {
        self.spaces();
        let bytes = self.line();
        if bytes.is_empty() {
            return Err(self.error("missing filename in r/R/w/W commands"));
        }
        Ok(filename_path(bytes))
    }
    fn text(&mut self) -> Result<Vec<u8>, String> {
        self.spaces();
        if matches!(self.peek(), None | Some(b'\n')) {
            return Err(self.error("expected \\ after `a', `c' or `i'"));
        }
        if self.peek() == Some(b'\\') {
            self.at += 1;
            if self.peek() == Some(b'\n') {
                self.at += 1;
            }
        }
        let mut text = Vec::new();
        while let Some(byte) = self.peek() {
            self.at += 1;
            if byte == b'\n' {
                break;
            }
            if byte == b'\\' {
                if let Some(next) = self.peek() {
                    self.at += 1;
                    text.push(replacement::escape(next));
                }
            } else {
                text.push(byte);
            }
        }
        Ok(text)
    }
    fn delimited(&mut self, delimiter: u8, regex: bool, message: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut bracket = false;
        while let Some(byte) = self.peek() {
            self.at += 1;
            if byte == b'\\' {
                let next = self.peek().ok_or_else(|| self.error(message))?;
                self.at += 1;
                out.push(byte);
                out.push(next);
                continue;
            }
            if byte == delimiter && !bracket {
                return Ok(out);
            }
            if byte == b'\n' {
                self.at -= 1;
                return Err(self.error(message));
            }
            if regex && byte == b'[' {
                bracket = true;
            }
            if regex && byte == b']' {
                bracket = false;
            }
            out.push(byte);
        }
        Err(self.error(message))
    }
    fn substitution(&mut self) -> Result<Substitution, String> {
        let delimiter = self
            .peek()
            .ok_or_else(|| self.error("unterminated `s' command"))?;
        self.at += 1;
        if delimiter == b'\n' {
            return Err(self.error("unterminated `s' command"));
        }
        let regex = self.delimited(delimiter, true, "unterminated `s' command")?;
        let replacement =
            replacement::parse(&self.delimited(delimiter, false, "unterminated `s' command")?);
        let (mut insensitive, mut multiline) = (false, false);
        let (mut global, mut print, mut number, mut write) = (false, false, None, None);
        loop {
            self.spaces();
            match self.peek() {
                Some(b'g') => {
                    if global {
                        return Err(self.error("multiple `g' options to `s' command"));
                    }
                    global = true;
                    self.at += 1;
                }
                Some(b'p') => {
                    print = true;
                    self.at += 1;
                }
                Some(b'i' | b'I') => {
                    insensitive = true;
                    self.at += 1;
                }
                Some(b'm' | b'M') => {
                    multiline = true;
                    self.at += 1;
                }
                Some(b'0'..=b'9') => {
                    if number.is_some() {
                        return Err(self.error("multiple number options to `s' command"));
                    }
                    number = Some(self.number()?);
                    if number == Some(0) {
                        return Err(self.error("number option to `s' command may not be zero"));
                    }
                }
                Some(b'w') => {
                    self.at += 1;
                    write = Some(self.filename()?);
                    break;
                }
                Some(b'e') => {
                    return Err(self.error("substitution flag 'e' is not supported by agentcmd"))
                }
                None | Some(b';' | b'\n' | b'}' | b'#') => break,
                _ => return Err(self.error("unknown option to `s'")),
            }
        }
        let pattern = pattern::compile(&regex, self.extended, insensitive, multiline)
            .map_err(|e| self.error(e))?;
        if let Some(compiled) = &pattern.compiled {
            if replacement
                .iter()
                .any(|p| matches!(p,replacement::Part::Group(n) if *n>=compiled.groups()))
            {
                return Err(self.error("invalid reference in `s' command's RHS"));
            }
        }
        Ok(Substitution {
            pattern,
            replacement,
            global,
            number: number.unwrap_or(1) as usize,
            print,
            write,
        })
    }
    fn translate(&mut self) -> Result<Vec<(u8, u8)>, String> {
        let delimiter = self
            .peek()
            .ok_or_else(|| self.error("unterminated `y' command"))?;
        self.at += 1;
        let from = self.delimited(delimiter, false, "unterminated `y' command")?;
        let to = self.delimited(delimiter, false, "unterminated `y' command")?;
        let from = decode(&from);
        let to = decode(&to);
        if from.len() != to.len() {
            return Err(self.error("strings for `y' command are different lengths"));
        }
        self.ending()?;
        Ok(from.into_iter().zip(to).collect())
    }
}

fn decode(bytes: &[u8]) -> Vec<u8> {
    let (mut result, mut at) = (Vec::new(), 0);
    while at < bytes.len() {
        let mut byte = bytes[at];
        at += 1;
        if byte == b'\\' && at < bytes.len() {
            byte = replacement::escape(bytes[at]);
            at += 1;
        }
        result.push(byte);
    }
    result
}

fn filename_path(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes).into()
    }
    #[cfg(not(unix))]
    {
        String::from_utf8_lossy(&bytes).into_owned().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(script: &[u8]) -> Result<Program, String> {
        compile(
            &[Source {
                name: "-e expression #1".into(),
                bytes: script.to_vec(),
            }],
            false,
        )
    }
    #[test]
    fn block_jumps() {
        let p = p(b"1{p;d}").unwrap();
        assert!(matches!(p.instructions[0].action, Action::Open(4)));
    }
    #[test]
    fn labels() {
        let p = p(b":a;s/a/b/;ta").unwrap();
        assert!(matches!(p.instructions[2].action, Action::Branch(b't', 0)));
    }
    #[test]
    fn missing_label() {
        assert!(p(b"b nowhere").is_err());
    }
    #[test]
    fn malformed_substitute() {
        assert!(p(b"s/a").unwrap_err().contains("unterminated `s' command"));
    }
    #[test]
    fn text_single_line() {
        assert!(
            matches!(&p(b"a hello").unwrap().instructions[0].action,Action::Text(b'a',t) if t==b"hello")
        );
    }
    #[test]
    fn text_continuation() {
        assert!(
            matches!(&p(b"a\\\nhello\\\nworld").unwrap().instructions[0].action,Action::Text(b'a',t) if t==b"hello\nworld")
        );
    }
    #[test]
    fn transliteration() {
        assert!(
            matches!(&p(b"y/ab/BA/").unwrap().instructions[0].action,Action::Translate(t) if t==&[(b'a',b'B'),(b'b',b'A')])
        );
    }
    #[test]
    fn reject_execute() {
        assert!(p(b"e echo bad").is_err());
        assert!(p(b"s/a/b/e").is_err());
    }
    #[test]
    fn reject_zero() {
        assert!(p(b"0p").is_err());
        assert!(p(b"0,/a/p").is_ok());
    }
    #[test]
    fn first_comment_quiet() {
        assert!(p(b"#n\np").unwrap().quiet);
    }
    #[test]
    fn unmatched_block() {
        assert!(p(b"{").is_err());
        assert!(p(b"}").is_err());
    }
}
