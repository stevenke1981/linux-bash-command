use regex::bytes::{Captures, Regex, RegexBuilder};
type Groups = Vec<Option<(usize, usize)>>;

#[derive(Clone, Debug)]
pub struct Pattern {
    pub compiled: Option<Compiled>,
}
#[derive(Clone, Debug)]
pub struct Compiled {
    regex: Regex,
    contexts: Option<[Regex; 4]>,
}

pub fn compile(
    bytes: &[u8],
    extended: bool,
    insensitive: bool,
    multiline: bool,
) -> Result<Pattern, String> {
    if bytes.is_empty() {
        if insensitive || multiline {
            return Err("cannot specify modifiers on empty regexp".into());
        }
        return Ok(Pattern { compiled: None });
    }
    let normalized = normalize(bytes, extended)?;
    let source = if extended {
        normalized
    } else {
        crate::common::regex::bre_to_regex(&normalized)?
    };
    let source = source
        .chars()
        .map(|c| match c as u32 {
            0xe100 => r"\b{start}".to_owned(),
            0xe101 => r"\b{end}".to_owned(),
            0xe102 => r"\A".to_owned(),
            0xe103 => r"\z".to_owned(),
            0xe000..=0xe0ff => format!("\\x{:02x}", c as u32 - 0xe000),
            _ => c.to_string(),
        })
        .collect::<String>();
    let regex = build(&source, insensitive, multiline)?;
    let alternate = has_alternate(source.as_bytes());
    let contexts = if alternate {
        Some(contexts(&source, insensitive, multiline)?)
    } else {
        None
    };
    Ok(Pattern {
        compiled: Some(Compiled { regex, contexts }),
    })
}
fn contexts(source: &str, insensitive: bool, multiline: bool) -> Result<[Regex; 4], String> {
    let expression = |left, right| {
        format!(
            "\\A{}(?:{source}){}\\z",
            if left { "(?s:.)" } else { "" },
            if right { "(?s:.)" } else { "" }
        )
    };
    Ok([
        build(&expression(false, false), insensitive, multiline)?,
        build(&expression(false, true), insensitive, multiline)?,
        build(&expression(true, false), insensitive, multiline)?,
        build(&expression(true, true), insensitive, multiline)?,
    ])
}
fn has_alternate(source: &[u8]) -> bool {
    let (mut escaped, mut bracket) = (false, false);
    for &byte in source {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'[' => bracket = true,
            b']' => bracket = false,
            b'|' if !bracket => return true,
            _ => {}
        }
    }
    false
}

fn build(source: &str, insensitive: bool, multiline: bool) -> Result<Regex, String> {
    RegexBuilder::new(source)
        .unicode(false)
        .case_insensitive(insensitive)
        .multi_line(multiline)
        .dot_matches_new_line(!multiline)
        .build()
        .map_err(|_| "Invalid regular expression".into())
}

fn normalize(bytes: &[u8], extended: bool) -> Result<String, String> {
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        if byte == b'\\' && i < bytes.len() {
            let next = bytes[i];
            i += 1;
            if (b'1'..=b'9').contains(&next) {
                return Err("unsupported backreference in regex".into());
            }
            match next {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'a' => out.push('\x07'),
                b'f' => out.push('\x0c'),
                b'v' => out.push('\x0b'),
                b'<' => out.push('\u{e100}'),
                b'>' => out.push('\u{e101}'),
                b'`' => out.push('\u{e102}'),
                b'\'' => out.push('\u{e103}'),
                b'x' | b'o' | b'd' => {
                    let radix = match next {
                        b'x' => 16,
                        b'o' => 8,
                        _ => 10,
                    };
                    let limit = if next == b'x' { 2 } else { 3 };
                    let (mut value, mut count) = (0, 0);
                    while i < bytes.len() && count < limit {
                        let Some(digit) = char::from(bytes[i]).to_digit(radix) else {
                            break;
                        };
                        value = value * radix + digit;
                        i += 1;
                        count += 1;
                    }
                    if count == 0 {
                        out.push(char::from(next));
                    } else {
                        push_byte(&mut out, value as u8);
                    }
                }
                _ if !next.is_ascii() => push_byte(&mut out, next),
                _ if extended && next.is_ascii_alphabetic() && !b"wWsSbB".contains(&next) => {
                    out.push(char::from(next))
                }
                _ => {
                    out.push('\\');
                    out.push(char::from(next));
                }
            }
        } else {
            push_byte(&mut out, byte);
        }
    }
    Ok(out)
}
fn push_byte(out: &mut String, byte: u8) {
    out.push(if byte.is_ascii() {
        char::from(byte)
    } else {
        char::from_u32(0xe000 + u32::from(byte)).unwrap()
    });
}

impl Pattern {
    pub fn resolve<'a>(
        &'a self,
        previous: &'a mut Option<Compiled>,
    ) -> Result<&'a Compiled, String> {
        if let Some(compiled) = &self.compiled {
            *previous = Some(compiled.clone());
        }
        previous
            .as_ref()
            .ok_or_else(|| "no previous regular expression".into())
    }
}

impl Compiled {
    pub fn groups(&self) -> usize {
        self.regex.captures_len()
    }
    pub fn is_match(&self, text: &[u8]) -> bool {
        self.regex.is_match(text)
    }
    pub fn captures(&self, text: &[u8], at: usize) -> Result<Option<Groups>, String> {
        let Some(capture) = self.regex.captures_at(text, at) else {
            return Ok(None);
        };
        let found = capture.get(0).unwrap();
        if self.contexts.is_none() || found.end() == text.len() {
            return Ok(Some(ranges(&capture)));
        }
        let start = found.start();
        // Constant-size, anchored matchers retain one real byte at each edge.
        // In C locale that preserves all supported assertions, including absolute
        // anchors, without compiling a regex proportional to the subject length.
        for end in (found.end() + 1..=text.len()).rev() {
            if let Some(groups) = self.ending(text, start, end) {
                return Ok(Some(groups));
            }
        }
        Ok(Some(ranges(&capture)))
    }
    fn ending(&self, text: &[u8], start: usize, end: usize) -> Option<Groups> {
        let left = usize::from(start > 0);
        let right = usize::from(end < text.len());
        let offset = start - left;
        let regex = &self.contexts.as_ref().unwrap()[left * 2 + right];
        regex.captures(&text[offset..end + right]).map(|capture| {
            let mut result = ranges(&capture);
            for range in result.iter_mut().flatten() {
                range.0 += offset;
                range.1 += offset;
            }
            result[0] = Some((start, end));
            result
        })
    }
}

fn ranges(capture: &Captures<'_>) -> Vec<Option<(usize, usize)>> {
    capture
        .iter()
        .map(|group| group.map(|m| (m.start(), m.end())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn matcher(s: &[u8], ere: bool) -> Compiled {
        compile(s, ere, false, false).unwrap().compiled.unwrap()
    }
    #[test]
    fn basic_groups() {
        assert_eq!(matcher(br"\(ab\)", false).groups(), 2);
    }
    #[test]
    fn extended_groups() {
        assert_eq!(matcher(b"(ab)", true).groups(), 2);
    }
    #[test]
    fn reject_pattern_reference() {
        assert_eq!(
            compile(br"\(a\)\1", false, false, false).unwrap_err(),
            "unsupported backreference in regex"
        );
    }
    #[test]
    fn newline_and_tab() {
        assert!(matcher(br"a\nb\t", false).is_match(b"a\nb\t"));
    }
    #[test]
    fn byte_subject() {
        assert!(matcher(b".", false).is_match(b"\xff"));
    }
    #[test]
    fn alternate_longest() {
        assert_eq!(
            matcher(b"a|ab", true).captures(b"ab", 0).unwrap().unwrap()[0],
            Some((0, 2))
        );
    }
    #[test]
    fn alternate_real_end() {
        assert_eq!(
            matcher(b"a|ab$", true)
                .captures(b"abx", 0)
                .unwrap()
                .unwrap()[0],
            Some((0, 1))
        );
    }
    #[test]
    fn empty_reuse() {
        let mut previous = None;
        assert!(compile(b"", false, false, false)
            .unwrap()
            .resolve(&mut previous)
            .is_err());
    }
    #[test]
    fn multiline_dot() {
        assert!(!compile(b"a.b", true, false, true)
            .unwrap()
            .compiled
            .unwrap()
            .is_match(b"a\nb"));
    }
    #[test]
    fn insensitive() {
        assert!(compile(b"a", false, true, false)
            .unwrap()
            .compiled
            .unwrap()
            .is_match(b"A"));
    }
    #[test]
    fn word_edge_before_class() {
        assert!(matcher(br"\<[a-z]", false).is_match(b"ab"));
    }
    #[test]
    fn raw_pattern_bytes() {
        assert!(matcher(b"\xff", false).is_match(b"\xff"));
    }
    #[test]
    fn hexadecimal_pattern() {
        assert!(matcher(br"\x41", false).is_match(b"A"));
    }
    #[test]
    fn alternatives_preserve_both_edge_contexts() {
        for (pattern, text, range) in [
            (b"a|\\bab".as_slice(), b"zab".as_slice(), (1, 2)),
            (b"a|\\`ab", b"zab", (1, 2)),
            (b"a|ab\\'", b"zabx", (1, 2)),
            (b"a|ab\\B", b"zabx", (1, 3)),
        ] {
            assert_eq!(
                matcher(pattern, true).captures(text, 0).unwrap().unwrap()[0],
                Some(range),
                "{pattern:?}"
            );
        }
    }
    #[test]
    fn multiline_alternatives_retain_real_newline_context() {
        let p = compile(b"a|^ab$", true, false, true)
            .unwrap()
            .compiled
            .unwrap();
        assert_eq!(
            p.captures(b"z\nab\nx", 0).unwrap().unwrap()[0],
            Some((2, 4))
        );
        assert_eq!(p.captures(b"zab\nx", 0).unwrap().unwrap()[0], Some((1, 2)));
    }
}
