use regex::bytes::RegexBuilder;
use std::rc::Rc;
#[derive(Clone)]
pub(super) enum Test {
    Byte(u8),
    Set(Rc<[u64; 4]>),
    Any,
}
impl Test {
    pub fn matches(&self, byte: u8) -> bool {
        match self {
            Self::Byte(b) => *b == byte,
            Self::Any => true,
            Self::Set(bits) => bits[usize::from(byte) / 64] & (1u64 << (byte % 64)) != 0,
        }
    }
    pub fn possible(&self) -> bool {
        match self {
            Self::Set(bits) => bits.iter().any(|n| *n != 0),
            _ => true,
        }
    }
}
pub(super) enum Node {
    Empty,
    Consume(Test),
    Start,
    End,
    Sequence(Vec<Node>),
    Alternative(Vec<Node>),
    Repeat(Box<Node>, usize, Option<usize>),
}
impl Node {
    pub fn consumes(&self) -> bool {
        match self {
            Self::Consume(test) => test.possible(),
            Self::Sequence(nodes) | Self::Alternative(nodes) => nodes.iter().any(Self::consumes),
            Self::Repeat(node, _, max) => *max != Some(0) && node.consumes(),
            _ => false,
        }
    }
    pub fn possible(&self) -> bool {
        match self {
            Self::Consume(test) => test.possible(),
            Self::Sequence(nodes) => nodes.iter().all(Self::possible),
            Self::Alternative(nodes) => nodes.iter().any(Self::possible),
            Self::Repeat(node, min, _) => *min == 0 || node.possible(),
            _ => true,
        }
    }
}
struct Parser<'a> {
    source: &'a [u8],
    at: usize,
}
pub(super) fn parse(source: &str) -> Result<Node, String> {
    let mut p = Parser {
        source: source.as_bytes(),
        at: 0,
    };
    let node = p.alternative()?;
    if p.at != p.source.len() {
        return Err("invalid regular expression".into());
    }
    Ok(node)
}
impl Parser<'_> {
    fn alternative(&mut self) -> Result<Node, String> {
        let mut nodes = vec![self.sequence()?];
        while self.source.get(self.at) == Some(&b'|') {
            self.at += 1;
            nodes.push(self.sequence()?);
        }
        Ok(if nodes.len() == 1 {
            nodes.pop().unwrap()
        } else {
            Node::Alternative(nodes)
        })
    }
    fn sequence(&mut self) -> Result<Node, String> {
        let mut nodes = Vec::new();
        while self.at < self.source.len() && !matches!(self.source[self.at], b'|' | b')') {
            nodes.push(self.repeated()?);
        }
        Ok(match nodes.len() {
            0 => Node::Empty,
            1 => nodes.pop().unwrap(),
            _ => Node::Sequence(nodes),
        })
    }
    fn repeated(&mut self) -> Result<Node, String> {
        let mut node = self.atom()?;
        loop {
            let bounds = match self.source.get(self.at) {
                Some(b'*') => Some((0, None)),
                Some(b'+') => Some((1, None)),
                Some(b'?') => Some((0, Some(1))),
                Some(b'{') => {
                    self.at += 1;
                    let min = self.number()?;
                    let max = if self.source.get(self.at) == Some(&b',') {
                        self.at += 1;
                        if self.source.get(self.at) == Some(&b'}') {
                            None
                        } else {
                            Some(self.number()?)
                        }
                    } else {
                        Some(min)
                    };
                    if self.source.get(self.at) != Some(&b'}') {
                        return Err("invalid repetition".into());
                    }
                    Some((min, max))
                }
                _ => None,
            };
            let Some((min, max)) = bounds else {
                break;
            };
            self.at += 1;
            if max.is_some_and(|max| max < min) {
                return Err("invalid repetition".into());
            }
            node = Node::Repeat(Box::new(node), min, max);
        }
        Ok(node)
    }
    fn number(&mut self) -> Result<usize, String> {
        let start = self.at;
        while self.source.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        std::str::from_utf8(&self.source[start..self.at])
            .unwrap()
            .parse()
            .map_err(|_| "invalid repetition".into())
    }
    fn atom(&mut self) -> Result<Node, String> {
        let byte = *self
            .source
            .get(self.at)
            .ok_or("invalid regular expression")?;
        self.at += 1;
        Ok(match byte {
            b'(' => {
                let node = self.alternative()?;
                if self.source.get(self.at) != Some(&b')') {
                    return Err("invalid group".into());
                }
                self.at += 1;
                node
            }
            b'.' => Node::Consume(Test::Any),
            b'^' => Node::Start,
            b'$' => Node::End,
            b'[' => Node::Consume(self.class()?),
            b'\\' => {
                let next = *self.source.get(self.at).ok_or("invalid escape")?;
                self.at += 1;
                let byte = if next == b'x' {
                    let bytes = self
                        .source
                        .get(self.at..self.at + 2)
                        .ok_or("invalid escape")?;
                    self.at += 2;
                    u8::from_str_radix(std::str::from_utf8(bytes).unwrap(), 16)
                        .map_err(|_| "invalid escape")?
                } else {
                    next
                };
                Node::Consume(Test::Byte(byte))
            }
            b'*' | b'+' | b'?' | b'{' => return Err("invalid repetition".into()),
            _ => Node::Consume(Test::Byte(byte)),
        })
    }
    fn class(&mut self) -> Result<Test, String> {
        let start = self.at - 1;
        while let Some(&byte) = self.source.get(self.at) {
            self.at += 1;
            if byte == b'\\' {
                self.at += 1;
            } else if byte == b'[' && self.source.get(self.at) == Some(&b':') {
                self.at += 1;
                while self.at + 1 < self.source.len() && self.source[self.at..self.at + 2] != *b":]"
                {
                    self.at += 1;
                }
                self.at += 2;
            } else if byte == b']' {
                let source = std::str::from_utf8(&self.source[start..self.at]).unwrap();
                let regex = RegexBuilder::new(&format!("\\A{source}\\z"))
                    .unicode(false)
                    .build()
                    .map_err(|_| "invalid character class")?;
                let mut bits = [0u64; 4];
                for byte in 0..=u8::MAX {
                    if regex.is_match(&[byte]) {
                        bits[usize::from(byte) / 64] |= 1u64 << (byte % 64);
                    }
                }
                return Ok(Test::Set(Rc::new(bits)));
            }
        }
        Err("invalid character class".into())
    }
}
