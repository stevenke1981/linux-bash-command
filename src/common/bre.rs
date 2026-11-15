//! Anchored C-locale BRE matching with capture disambiguation and backreferences.
type Result<T> = std::result::Result<T, String>;
use std::collections::HashSet;

#[derive(Debug)]
enum Pattern {
    Sequence(Vec<Pattern>),
    Alternative(Vec<Pattern>),
    Repeat(Box<Pattern>, usize, Option<usize>),
    Group(usize, Box<Pattern>),
    Byte(u8),
    Class(regex::bytes::Regex),
    Any,
    Start,
    End,
    Boundary(bool),
    WordStart,
    WordEnd,
    Reference(usize),
}
struct Parser<'a> {
    source: &'a [u8],
    at: usize,
    groups: usize,
    closed: [bool; 10],
}
fn invalid() -> String {
    "Invalid regular expression".to_owned()
}

impl Parser<'_> {
    fn ahead(&self, token: &[u8]) -> bool {
        self.source[self.at..].starts_with(token)
    }
    fn alternatives(&mut self) -> Result<Pattern> {
        let mut alternatives = vec![self.sequence()?];
        while self.ahead(br"\|") {
            self.at += 2;
            alternatives.push(self.sequence()?);
        }
        Ok(Pattern::Alternative(alternatives))
    }
    fn sequence(&mut self) -> Result<Pattern> {
        let mut parts = Vec::new();
        while self.at < self.source.len() && !self.ahead(br"\)") && !self.ahead(br"\|") {
            let atom = self.atom(parts.is_empty())?;
            let atom = if matches!(
                atom,
                Pattern::Start
                    | Pattern::End
                    | Pattern::Boundary(_)
                    | Pattern::WordStart
                    | Pattern::WordEnd
            ) {
                atom
            } else {
                self.quantified(atom)?
            };
            parts.push(atom);
        }
        Ok(Pattern::Sequence(parts))
    }
    fn atom(&mut self, first: bool) -> Result<Pattern> {
        let byte = self.source[self.at];
        self.at += 1;
        Ok(match byte {
            b'.' => Pattern::Any,
            b'^' if first => Pattern::Start,
            b'$' if self.at == self.source.len() || self.ahead(br"\)") || self.ahead(br"\|") => {
                Pattern::End
            }
            b'[' => self.class()?,
            b'\\' => self.escape()?,
            _ => Pattern::Byte(byte),
        })
    }
    fn escape(&mut self) -> Result<Pattern> {
        let byte = *self.source.get(self.at).ok_or_else(invalid)?;
        self.at += 1;
        Ok(match byte {
            b'(' => {
                self.groups += 1;
                let index = self.groups;
                let contents = self.alternatives()?;
                if !self.ahead(br"\)") {
                    return Err(invalid());
                }
                self.at += 2;
                if index < self.closed.len() {
                    self.closed[index] = true;
                }
                Pattern::Group(index, Box::new(contents))
            }
            b'1'..=b'9' => {
                let group = usize::from(byte - b'0');
                if !self.closed[group] {
                    return Err("Invalid back reference".to_owned());
                }
                Pattern::Reference(group)
            }
            b'b' => Pattern::Boundary(true),
            b'B' => Pattern::Boundary(false),
            b'<' => Pattern::WordStart,
            b'>' => Pattern::WordEnd,
            b'`' => Pattern::Start,
            b'\'' => Pattern::End,
            b'n' => Pattern::Byte(b'\n'),
            b't' => Pattern::Byte(b'\t'),
            b'r' => Pattern::Byte(b'\r'),
            b'w' | b'W' | b's' | b'S' => Pattern::Class(class_regex(&[b'\\', byte])?),
            _ => Pattern::Byte(byte),
        })
    }
    fn class(&mut self) -> Result<Pattern> {
        let start = self.at - 1;
        if self.source.get(self.at) == Some(&b'^') {
            self.at += 1;
        }
        if self.source.get(self.at) == Some(&b']') {
            self.at += 1;
        }
        while let Some(&byte) = self.source.get(self.at) {
            if byte == b']' {
                self.at += 1;
                return Ok(Pattern::Class(class_regex(&self.source[start..self.at])?));
            }
            if byte == b'['
                && self
                    .source
                    .get(self.at + 1)
                    .is_some_and(|b| matches!(b, b':' | b'.' | b'='))
            {
                let closing = [self.source[self.at + 1], b']'];
                let offset = self.source[self.at + 2..]
                    .windows(2)
                    .position(|w| w == closing)
                    .ok_or_else(invalid)?;
                self.at += offset + 4;
            } else {
                self.at += 1;
            }
        }
        Err(invalid())
    }
    fn quantified(&mut self, atom: Pattern) -> Result<Pattern> {
        let limits = if self.ahead(b"*") {
            self.at += 1;
            Some((0, None))
        } else if self.ahead(br"\+") {
            self.at += 2;
            Some((1, None))
        } else if self.ahead(br"\?") {
            self.at += 2;
            Some((0, Some(1)))
        } else if self.ahead(br"\{") {
            Some(self.interval()?)
        } else {
            None
        };
        Ok(if let Some((minimum, maximum)) = limits {
            Pattern::Repeat(Box::new(atom), minimum, maximum)
        } else {
            atom
        })
    }
    fn interval(&mut self) -> Result<(usize, Option<usize>)> {
        self.at += 2;
        let size = self.source[self.at..]
            .windows(2)
            .position(|w| w == br"\}")
            .ok_or_else(invalid)?;
        let bounds =
            std::str::from_utf8(&self.source[self.at..self.at + size]).map_err(|_| invalid())?;
        self.at += size + 2;
        let (minimum, maximum) = if let Some((a, b)) = bounds.split_once(',') {
            (
                if a.is_empty() {
                    0
                } else {
                    a.parse().map_err(|_| invalid())?
                },
                if b.is_empty() {
                    None
                } else {
                    Some(b.parse().map_err(|_| invalid())?)
                },
            )
        } else {
            let value = bounds.parse().map_err(|_| invalid())?;
            (value, Some(value))
        };
        if maximum.is_some_and(|max| max < minimum) {
            return Err(invalid());
        }
        Ok((minimum, maximum))
    }
}

fn class_regex(source: &[u8]) -> Result<regex::bytes::Regex> {
    let source: String = source.iter().map(|b| char::from(*b)).collect();
    let translated = crate::common::regex::bre_to_regex(&source)?;
    let mut bytes = String::new();
    for ch in translated.chars() {
        if ch.is_ascii() {
            bytes.push(ch);
        } else {
            bytes.push_str(&format!("\\x{:02x}", u32::from(ch)));
        }
    }
    regex::bytes::RegexBuilder::new(&format!("\\A(?:{bytes})\\z"))
        .unicode(false)
        .dot_matches_new_line(true)
        .build()
        .map_err(|_| invalid())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct State {
    at: usize,
    captures: [Option<(usize, usize)>; 10],
}
fn distinct(states: impl IntoIterator<Item = State>) -> Vec<State> {
    let mut seen = HashSet::new();
    states
        .into_iter()
        .filter(|state| seen.insert(*state))
        .collect()
}
fn word(byte: Option<&u8>) -> bool {
    byte.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn advance(pattern: &Pattern, state: State, input: &[u8]) -> Vec<State> {
    let matched = match pattern {
        Pattern::Sequence(parts) => {
            let mut states = vec![state];
            for part in parts {
                states = distinct(states.into_iter().flat_map(|s| advance(part, s, input)));
            }
            return states;
        }
        Pattern::Alternative(choices) => {
            return distinct(choices.iter().flat_map(|p| advance(p, state, input)))
        }
        Pattern::Group(index, body) => {
            return advance(body, state, input)
                .into_iter()
                .map(|mut result| {
                    if *index < result.captures.len() {
                        result.captures[*index] = Some((state.at, result.at));
                    }
                    result
                })
                .collect();
        }
        Pattern::Repeat(body, minimum, maximum) => {
            return repeated(body, state, input, *minimum, *maximum)
        }
        Pattern::Reference(index) => {
            let Some((start, end)) = state.captures[*index] else {
                return Vec::new();
            };
            return if input[state.at..].starts_with(&input[start..end]) {
                vec![State {
                    at: state.at + end - start,
                    ..state
                }]
            } else {
                Vec::new()
            };
        }
        Pattern::Byte(byte) => input.get(state.at) == Some(byte),
        Pattern::Any => state.at < input.len(),
        Pattern::Class(class) => {
            state.at < input.len() && class.is_match(&input[state.at..state.at + 1])
        }
        Pattern::Start => state.at == 0,
        Pattern::End => state.at == input.len(),
        Pattern::Boundary(yes) => {
            (word(state.at.checked_sub(1).and_then(|i| input.get(i))) != word(input.get(state.at)))
                == *yes
        }
        Pattern::WordStart => {
            !word(state.at.checked_sub(1).and_then(|i| input.get(i))) && word(input.get(state.at))
        }
        Pattern::WordEnd => {
            word(state.at.checked_sub(1).and_then(|i| input.get(i))) && !word(input.get(state.at))
        }
    };
    if !matched {
        return Vec::new();
    }
    let consumes = matches!(pattern, Pattern::Byte(_) | Pattern::Any | Pattern::Class(_));
    vec![State {
        at: state.at + usize::from(consumes),
        ..state
    }]
}
fn repeated(
    body: &Pattern,
    state: State,
    input: &[u8],
    minimum: usize,
    maximum: Option<usize>,
) -> Vec<State> {
    let mut times = 0;
    let mut frontier = vec![state];
    let mut results = Vec::new();
    let bound = maximum.unwrap_or_else(|| input.len().saturating_add(minimum).saturating_add(1));
    loop {
        if times >= minimum {
            results.extend_from_slice(&frontier);
        }
        if times >= bound {
            break;
        }
        let next = distinct(frontier.iter().flat_map(|s| advance(body, *s, input)));
        if next.is_empty() {
            break;
        }
        // A zero-length repeating body reaches a fixed capture state. Additional
        // iterations cannot create another match and must not loop indefinitely.
        let fixed = next.len() == frontier.len() && next.iter().all(|s| frontier.contains(s));
        times += 1;
        frontier = next;
        if fixed {
            if bound >= minimum {
                results.extend_from_slice(&frontier);
            }
            break;
        }
    }
    distinct(results)
}
fn preference(left: &State, right: &State) -> std::cmp::Ordering {
    left.at.cmp(&right.at).then_with(|| {
        for (left, right) in left.captures.iter().zip(right.captures.iter()).skip(1) {
            let rank = |capture: &Option<(usize, usize)>| {
                capture.map(|(start, end)| (end - start, std::cmp::Reverse(start)))
            };
            let order = rank(left).cmp(&rank(right));
            if !order.is_eq() {
                return order;
            }
        }
        std::cmp::Ordering::Equal
    })
}

pub struct Matcher {
    pattern: Pattern,
    groups: usize,
    fast: Option<Regular>,
}
struct Regular {
    anchored: regex::bytes::Regex,
    search: regex::bytes::Regex,
    source: String,
    context: bool,
}
// A regular pattern can use the linear-space regex engine. Force the longest
// whole prefix, retaining ordered alternatives for GNU's subgroup selection.
// Backreferences require the capture-state interpreter above.
fn fast_matchers(pattern: &Pattern) -> Option<Regular> {
    fn render(pattern: &Pattern) -> Option<String> {
        Some(match pattern {
            Pattern::Sequence(parts) => parts
                .iter()
                .map(render)
                .collect::<Option<Vec<_>>>()?
                .join(""),
            Pattern::Alternative(parts) => format!(
                "(?:{})",
                parts
                    .iter()
                    .map(render)
                    .collect::<Option<Vec<_>>>()?
                    .join("|")
            ),
            Pattern::Group(_, body) => format!("({})", render(body)?),
            Pattern::Repeat(body, min, max) => {
                let max = max.map_or_else(String::new, |n| n.to_string());
                format!("(?:{}){{{min},{max}}}", render(body)?)
            }
            Pattern::Byte(byte) => format!("\\x{byte:02x}"),
            Pattern::Class(class) => class
                .as_str()
                .strip_prefix("\\A(?:")?
                .strip_suffix(")\\z")?
                .to_owned(),
            Pattern::Any => ".".to_owned(),
            Pattern::Start => "\\A".to_owned(),
            Pattern::End => "\\z".to_owned(),
            Pattern::Boundary(true) => "\\b".to_owned(),
            Pattern::Boundary(false) => "\\B".to_owned(),
            Pattern::WordStart => "\\b{start}".to_owned(),
            Pattern::WordEnd => "\\b{end}".to_owned(),
            Pattern::Reference(_) => return None,
        })
    }
    fn contextual(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Sequence(parts) | Pattern::Alternative(parts) => parts.iter().any(contextual),
            Pattern::Repeat(body, _, _) | Pattern::Group(_, body) => contextual(body),
            Pattern::End | Pattern::Boundary(_) | Pattern::WordStart | Pattern::WordEnd => true,
            _ => false,
        }
    }
    let context = contextual(pattern);
    let pattern = render(pattern)?;
    let build = |text: &str| {
        regex::bytes::RegexBuilder::new(text)
            .unicode(false)
            .dot_matches_new_line(true)
            .build()
            .ok()
    };
    Some(Regular {
        anchored: build(&format!("\\A(?:{pattern})\\z"))?,
        search: build(&pattern)?,
        source: pattern,
        context,
    })
}
#[derive(Debug)]
pub struct Captures {
    start: usize,
    state: State,
}
impl Captures {
    pub fn range(&self, group: usize) -> Option<std::ops::Range<usize>> {
        if group == 0 {
            return Some(self.start..self.state.at);
        }
        self.state
            .captures
            .get(group)
            .copied()
            .flatten()
            .map(|(start, end)| start..end)
    }
}
impl Matcher {
    pub fn compile(expression: &[u8]) -> Result<Self> {
        let mut parser = Parser {
            source: expression,
            at: 0,
            groups: 0,
            closed: [false; 10],
        };
        let pattern = parser.alternatives()?;
        if parser.at != expression.len() {
            return Err(invalid());
        }
        Ok(Self {
            fast: fast_matchers(&pattern),
            pattern,
            groups: parser.groups,
        })
    }
    pub fn group_count(&self) -> usize {
        self.groups
    }
    pub fn captures_at_start(&self, input: &[u8]) -> Option<Captures> {
        if let Some(regular) = &self.fast {
            for end in (0..=input.len()).rev() {
                let contextual;
                let (matcher, haystack) = if regular.context && end < input.len() {
                    contextual = regex::bytes::RegexBuilder::new(&format!(
                        "\\A(?:{})(?s:.){{{}}}\\z",
                        regular.source,
                        input.len() - end
                    ))
                    .unicode(false)
                    .dot_matches_new_line(true)
                    .build()
                    .ok()?;
                    (&contextual, input)
                } else {
                    (&regular.anchored, &input[..end])
                };
                if let Some(found) = matcher.captures(haystack) {
                    let mut state = State {
                        at: end,
                        captures: [None; 10],
                    };
                    for (index, capture) in state.captures.iter_mut().enumerate().skip(1) {
                        *capture = found.get(index).map(|m| (m.start(), m.end()));
                    }
                    return Some(Captures { start: 0, state });
                }
            }
            return None;
        }
        let initial = State {
            at: 0,
            captures: [None; 10],
        };
        advance(&self.pattern, initial, input)
            .into_iter()
            .max_by(preference)
            .map(|state| Captures { start: 0, state })
    }
    pub fn is_match(&self, input: &[u8]) -> bool {
        if let Some(regular) = &self.fast {
            return regular.search.is_match(input);
        }
        (0..=input.len()).any(|start| {
            let initial = State {
                at: start,
                captures: [None; 10],
            };
            !advance(&self.pattern, initial, input).is_empty()
        })
    }
}
#[cfg(test)]
fn matching(input: &[u8], expression: &[u8]) -> Result<Vec<u8>> {
    let matcher = Matcher::compile(expression)?;
    let found = matcher.captures_at_start(input);
    Ok(if matcher.group_count() != 0 {
        found
            .and_then(|c| c.range(1))
            .map_or_else(Vec::new, |range| input[range].to_vec())
    } else {
        found
            .map_or(0, |c| c.range(0).unwrap().len())
            .to_string()
            .into_bytes()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backreferences_and_missing_matches() {
        assert_eq!(matching(b"abab", br"\(ab\)\1").unwrap(), b"ab");
        assert_eq!(matching(b"aabaa", br"\(a*\)b\1").unwrap(), b"aa");
        assert_eq!(matching(b"abac", br"\(ab\)\1").unwrap(), b"");
    }
    #[test]
    fn longest_whole_match_keeps_first_subexpression_alternative() {
        assert_eq!(matching(b"abc", br"\(a\|ab\).*").unwrap(), b"a");
        assert_eq!(matching(b"aaaa", br"\(a\|aa\)*").unwrap(), b"a");
        assert_eq!(matching(b"ab", br"\(a*\)\(.*\)").unwrap(), b"a");
    }
    #[test]
    fn ambiguous_regular_captures_do_not_enumerate_partitions() {
        let text = vec![b'a'; 4096];
        let matcher = Matcher::compile(br"\(a*\)\(a*\)").unwrap();
        assert!(matcher.fast.is_some());
        assert_eq!(
            matcher.captures_at_start(&text).unwrap().range(1),
            Some(0..4096)
        );
    }
    #[test]
    fn empty_repetitions_terminate() {
        assert_eq!(matching(b"xxx", br"\(\)*").unwrap(), b"");
        assert_eq!(matching(b"", br"\(\)\{1000000\}").unwrap(), b"");
    }
    #[test]
    fn classes_intervals_and_boundaries() {
        assert_eq!(matching(b"123x", br"[[:digit:]]\{1,3\}").unwrap(), b"3");
        assert_eq!(matching(b"ab!", br"\<\(ab\)\>").unwrap(), b"ab");
        assert_eq!(matching(b"\xff", b"\\(\xff\\)").unwrap(), b"\xff");
    }
    #[test]
    fn invalid_syntax_is_reported() {
        for expression in [br"\1".as_slice(), br"\(a\1\)", br"[", br"\(a", br"a\{3,2\}"] {
            assert!(!matching(b"a", expression).unwrap_err().is_empty());
        }
    }
    #[test]
    fn unanchored_search_and_explicit_anchors() {
        let matcher = Matcher::compile(br"\(ab\)\1").unwrap();
        assert!(matcher.is_match(b"prefix abab suffix"));
        assert!(!matcher.is_match(b"prefix abac suffix"));
        assert!(Matcher::compile(br"^\(ab\)\1$").unwrap().is_match(b"abab"));
        assert!(!Matcher::compile(br"^\(ab\)\1$").unwrap().is_match(b"xabab"));
        let captures = matcher.captures_at_start(b"abab").unwrap();
        assert_eq!(captures.range(0), Some(0..4));
        assert_eq!(captures.range(1), Some(0..2));
        assert_eq!(captures.range(99), None);
    }
    #[test]
    fn optional_groups_intervals_and_character_classes() {
        for (expression, input, expected) in [
            (br"\(a\)\?b".as_slice(), b"b".as_slice(), b"".as_slice()),
            (br"\([a-z]\+\)", b"abc9", b"abc"),
            (br"\([^x]*\)", b"abx", b"ab"),
            (br"\([]a]\+\)", b"]aa", b"]aa"),
            (br"\(a\{,2\}\)", b"aaaa", b"aa"),
            (br"\(a\{2,\}\)", b"aaaa", b"aaaa"),
            (br"\(a\{2\}\)", b"aaaa", b"aa"),
            (br"\(\w\+\)\b", b"a_3!", b"a_3"),
            (br"\(\W\+\)", b"!?x", b"!?"),
            (br"\(\s\+\)", b" \tX", b" \t"),
            (br"\(\S\+\)", b"ab c", b"ab"),
            (br"\(a\)\B", b"ab", b"a"),
            (br"\`\(a\)\'", b"a", b"a"),
            (br"\(\n\t\r\)", b"\n\t\r", b"\n\t\r"),
        ] {
            assert_eq!(
                matching(input, expression).unwrap(),
                expected,
                "{expression:?}"
            );
        }
        for expression in [br"a\{x\}".as_slice(), br"a\{2", br"[[.bad", b"\\"] {
            assert!(Matcher::compile(expression).is_err());
        }
    }
}
