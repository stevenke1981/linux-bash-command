mod class;
mod nfa;
mod scan;
mod syntax;
use regex::bytes::{Regex, RegexBuilder};
use std::{
    collections::{HashMap, VecDeque},
    rc::Rc,
};
#[derive(Clone)]
pub struct Pattern {
    regex: Regex,
    automaton: nfa::Program,
}
impl Pattern {
    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        let source = class::normalize(&normalize(bytes))?;
        let regex = build(&source)?;
        let automaton = nfa::Program::compile(&syntax::parse(&source)?)?;
        Ok(Self { regex, automaton })
    }

    pub fn is_match(&self, bytes: &[u8]) -> bool {
        self.regex.is_match(bytes)
    }
    pub fn find(&self, bytes: &[u8], at: usize) -> Option<(usize, usize)> {
        self.scanner(at, false).feed(bytes, true).found
    }
    pub fn find_nonempty(&self, bytes: &[u8], at: usize) -> Option<(usize, usize)> {
        self.scanner(at, true).feed(bytes, true).found
    }
    pub fn scanner(&self, at: usize, ignore_empty: bool) -> scan::Scanner<'_> {
        scan::Scanner::new(&self.automaton, at, ignore_empty)
    }
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<Vec<u8>, Rc<Pattern>>,
    order: VecDeque<Vec<u8>>,
}
impl Cache {
    pub fn get(&mut self, bytes: &[u8]) -> Result<Rc<Pattern>, String> {
        if let Some(pattern) = self.entries.get(bytes) {
            return Ok(pattern.clone());
        }
        let pattern = Rc::new(Pattern::new(bytes)?);
        if self.entries.len() >= 128 {
            if let Some(key) = self.order.pop_front() {
                self.entries.remove(&key);
            }
        }
        self.entries.insert(bytes.to_vec(), pattern.clone());
        self.order.push_back(bytes.to_vec());
        Ok(pattern)
    }
}
fn build(source: &str) -> Result<Regex, String> {
    RegexBuilder::new(source)
        .unicode(false)
        .dot_matches_new_line(true)
        .build()
        .map_err(|error| match error {
            regex::Error::CompiledTooBig(_) => {
                "regular expression exceeds 10 MiB compilation budget".into()
            }
            _ => "invalid regular expression".into(),
        })
}
fn normalize(bytes: &[u8]) -> String {
    let (mut out, mut at, mut bracket) = (String::new(), 0, false);
    while at < bytes.len() {
        let byte = bytes[at];
        at += 1;
        if byte == b'\\' && at < bytes.len() {
            let next = bytes[at];
            at += 1;
            let decoded = match next {
                b'a' => Some(7),
                b'b' => Some(8),
                b'f' => Some(12),
                b'n' => Some(10),
                b'r' => Some(13),
                b't' => Some(9),
                b'v' => Some(11),
                _ => None,
            };
            if let Some(byte) = decoded {
                literal(&mut out, byte);
            } else if (b'0'..=b'7').contains(&next) {
                let mut number = u16::from(next - b'0');
                for _ in 0..2 {
                    if let Some(&byte) = bytes.get(at).filter(|b| (b'0'..=b'7').contains(b)) {
                        number = number * 8 + u16::from(byte - b'0');
                        at += 1;
                    } else {
                        break;
                    }
                }
                literal(&mut out, number as u8);
            } else if next.is_ascii_alphabetic() {
                out.push(char::from(next));
            } else {
                out.push('\\');
                if next.is_ascii() {
                    out.push(char::from(next));
                } else {
                    literal(&mut out, next);
                }
            }
        } else {
            if byte == b'[' {
                bracket = true;
            } else if byte == b']' {
                bracket = false;
            }
            if !byte.is_ascii() || byte == 0 {
                literal(&mut out, byte);
            } else if bracket && matches!(byte, b'&' | b'~') {
                out.push('\\');
                out.push(char::from(byte));
            } else {
                out.push(char::from(byte));
            }
        }
    }
    out
}
fn literal(out: &mut String, byte: u8) {
    out.push_str(&format!("\\x{byte:02x}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn longest_alternative() {
        assert_eq!(
            Pattern::new(b"a|ab").unwrap().find(b"zabx", 0),
            Some((1, 3))
        );
    }
    #[test]
    fn real_boundaries() {
        assert_eq!(
            Pattern::new(b"a|ab$").unwrap().find(b"zabx", 0),
            Some((1, 2))
        );
        assert_eq!(
            Pattern::new(b"a|^ab").unwrap().find(b"zab", 0),
            Some((1, 2))
        );
    }
    #[test]
    fn posix_classes() {
        assert!(Pattern::new(b"[[:digit:]]{2,3}").unwrap().is_match(b"12"));
    }
    #[test]
    fn bytes_and_controls() {
        assert!(Pattern::new(br"a\tb").unwrap().is_match(b"a\tb"));
        assert!(Pattern::new(b"\xff").unwrap().is_match(b"\xff"));
    }
    #[test]
    fn incomplete_intervals_are_literal_braces() {
        for bytes in [b"a{b".as_slice(), b"a{", b"a}", b"a{,2}", b"a{,}"] {
            let pattern = Pattern::new(bytes).unwrap();
            assert!(pattern.is_match(bytes));
            assert_eq!(pattern.find(bytes, 0), Some((0, bytes.len())));
        }
        assert_eq!(
            Pattern::new(b"a{2,3}").unwrap().find(b"aaaa", 0),
            Some((0, 3))
        );
        for bytes in [b"a{3,2}".as_slice(), b"a{2,3,4}", b"a{2,", b"a{2", b"a{}"] {
            assert!(Pattern::new(bytes).is_err());
        }
    }
    #[test]
    fn bounded_dynamic_cache() {
        let mut c = Cache::default();
        for n in 0..256 {
            c.get(n.to_string().as_bytes()).unwrap();
        }
        assert_eq!(c.entries.len(), 128);
    }
    #[test]
    fn bracket_literals_and_c_locale_collating_elements() {
        for (pattern, subject) in [
            (b"[]]".as_slice(), b"]".as_slice()),
            (b"[[]", b"["),
            (b"[[:alpha:]/]", b"/"),
            (b"[[=a=]]", b"a"),
            (b"[[.a.]]", b"a"),
        ] {
            assert!(Pattern::new(pattern).unwrap().is_match(subject));
        }
    }
    #[test]
    fn streaming_scan_preserves_extendable_alternative() {
        let pattern = Pattern::new(b"a.*b|a").unwrap();
        let mut scan = pattern.scanner(0, true);
        let first = scan.feed(b"ZaX", false);
        assert_eq!(first.found, Some((1, 2)));
        assert!(first.pending);
        let second = scan.feed(b"ZaXbQ", false);
        assert_eq!(second.found, Some((1, 4)));
        assert!(second.pending);
        let last = scan.feed(b"ZaXbQ", true);
        assert_eq!(last.found, Some((1, 4)));
        assert!(!last.pending);
    }
    #[test]
    fn zero_width_and_nested_repetition_do_not_expand_threads() {
        assert_eq!(
            Pattern::new(b"(a?)*b|a").unwrap().find(b"aaab", 0),
            Some((0, 4))
        );
        assert_eq!(
            Pattern::new(b"()|a.*b").unwrap().find_nonempty(b"aaax", 0),
            None
        );
    }
    #[test]
    fn anchors_use_actual_stream_end() {
        let pattern = Pattern::new(b"a$|a.").unwrap();
        let mut scan = pattern.scanner(0, true);
        let first = scan.feed(b"a", false);
        assert_eq!(first.found, None);
        assert!(first.pending);
        assert_eq!(scan.feed(b"ab", true).found, Some((0, 2)));
        assert_eq!(
            Pattern::new(b"a|^ab").unwrap().find(b"zab", 0),
            Some((1, 2))
        );
    }
    #[test]
    fn empty_and_star_repetition_do_not_allocate_by_repeat_count() {
        for (source, text, expected) in [
            ("(){1000000}", b"abc".as_slice(), Some((0, 0))),
            ("(a*){1000000}", b"aaa".as_slice(), Some((0, 3))),
        ] {
            let program = nfa::Program::compile(&syntax::parse(source).unwrap()).unwrap();
            assert!(program.states.len() < 10);
            assert_eq!(
                scan::Scanner::new(&program, 0, false)
                    .feed(text, true)
                    .found,
                expected
            );
        }
    }
}
