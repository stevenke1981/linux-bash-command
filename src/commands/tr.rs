//! `tr`: translate, delete, or squeeze bytes from standard input.
//!
//! Everything is byte oriented. Sets support escapes, ranges, the POSIX character classes
//! (C locale), equivalence classes `[=c=]`, and repeats `[c*n]` / `[c*]`.

use std::io::{self, Read, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const CMD: &str = "tr";
const CHUNK: usize = 64 * 1024;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("complement", Some('c'), Some("complement")),
    OptSpec::flag("complement_upper", Some('C'), None),
    OptSpec::flag("delete", Some('d'), Some("delete")),
    OptSpec::flag("squeeze", Some('s'), Some("squeeze-repeats")),
    OptSpec::flag("truncate", Some('t'), Some("truncate-set1")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: tr [OPTION]... SET1 [SET2]\n\
Translate, squeeze, and/or delete characters from standard input,\n\
writing to standard output.\n\n\
  -c, -C, --complement    use the complement of SET1\n\
  -d, --delete            delete characters in SET1, do not translate\n\
  -s, --squeeze-repeats   replace each sequence of a repeated character that is\n\
                          listed in the last specified SET with a single occurrence\n\
  -t, --truncate-set1     first truncate SET1 to length of SET2\n\
      --help              display this help and exit\n\
      --version           output version information and exit\n\n\
SETs are strings of characters. Escapes \\\\, \\a, \\b, \\f, \\n, \\r, \\t, \\v and\n\
\\NNN (octal) are supported. Ranges such as a-z are expanded. Character classes\n\
[:alnum:] [:alpha:] [:blank:] [:cntrl:] [:digit:] [:graph:] [:lower:] [:print:]\n\
[:punct:] [:space:] [:upper:] [:xdigit:] may be used. [=c=] is the character c,\n\
[c*n] repeats c n times (n is octal when it starts with 0), and [c*] in SET2\n\
fills SET2 to the length of SET1.\n\
Exit status: 0 on success, 1 on error.\n";

/// One element of a parsed set, before expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Byte(u8),
    Range(u8, u8),
    Class(Class),
    Equiv(u8),
    /// `[c*n]` (count given) or `[c*]` (count None: fills SET2).
    Repeat(u8, Option<usize>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Alnum,
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Xdigit,
}

impl Class {
    fn parse(name: &str) -> Option<Class> {
        Some(match name {
            "alnum" => Class::Alnum,
            "alpha" => Class::Alpha,
            "blank" => Class::Blank,
            "cntrl" => Class::Cntrl,
            "digit" => Class::Digit,
            "graph" => Class::Graph,
            "lower" => Class::Lower,
            "print" => Class::Print,
            "punct" => Class::Punct,
            "space" => Class::Space,
            "upper" => Class::Upper,
            "xdigit" => Class::Xdigit,
            _ => return None,
        })
    }

    fn contains(self, byte: u8) -> bool {
        match self {
            Class::Alnum => byte.is_ascii_alphanumeric(),
            Class::Alpha => byte.is_ascii_alphabetic(),
            Class::Blank => byte == b' ' || byte == b'\t',
            Class::Cntrl => byte.is_ascii_control(),
            Class::Digit => byte.is_ascii_digit(),
            Class::Graph => byte.is_ascii_graphic(),
            Class::Lower => byte.is_ascii_lowercase(),
            Class::Print => byte == b' ' || byte.is_ascii_graphic(),
            Class::Punct => byte.is_ascii_punctuation(),
            Class::Space => matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
            Class::Upper => byte.is_ascii_uppercase(),
            Class::Xdigit => byte.is_ascii_hexdigit(),
        }
    }

    fn is_case(self) -> bool {
        matches!(self, Class::Upper | Class::Lower)
    }
}

/// An expanded set: its bytes in order, and the offsets where `[:upper:]`/`[:lower:]` start.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Expanded {
    bytes: Vec<u8>,
    case_starts: Vec<usize>,
}

/// Options that matter to the translation.
#[derive(Debug, Default)]
struct Flags {
    complement: bool,
    delete: bool,
    squeeze: bool,
    truncate: bool,
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    for item in &items {
        if let Item::Opt { id, .. } = item {
            match *id {
                "help" => {
                    print!("{HELP}");
                    return EXIT_OK;
                }
                "version" => return print_version(CMD),
                _ => {}
            }
        }
    }
    let (flags, operands) = collect(&items);
    let plan = match Plan::build(&flags, &operands) {
        Ok(plan) => plan,
        Err(message) => {
            eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
            return EXIT_FAIL;
        }
    };
    filter_stdin(&plan)
}

fn collect(items: &[Item]) -> (Flags, Vec<String>) {
    let mut flags = Flags::default();
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id, .. } => match *id {
                "complement" | "complement_upper" => flags.complement = true,
                "delete" => flags.delete = true,
                "squeeze" => flags.squeeze = true,
                "truncate" => flags.truncate = true,
                _ => {}
            },
            Item::Operand(text) => operands.push(text.clone()),
        }
    }
    (flags, operands)
}

/// The compiled translation: 256-entry tables and the squeeze set.
#[derive(Debug)]
struct Plan {
    map: [u8; 256],
    delete: [bool; 256],
    squeeze: Option<[bool; 256]>,
}

impl Plan {
    fn build(flags: &Flags, operands: &[String]) -> Result<Plan, String> {
        check_operand_count(flags, operands)?;
        let mut map: [u8; 256] = std::array::from_fn(|i| i as u8);
        let mut delete = [false; 256];
        let squeeze_source: Vec<u8>;
        if flags.delete {
            let set1 = set1_bytes(&operands[0], flags)?;
            delete = mask_of(&set1);
            squeeze_source = match operands.get(1) {
                Some(text) if flags.squeeze => expand_set(text.as_bytes(), Some(set1.len()))?.bytes,
                _ => Vec::new(),
            };
        } else if operands.len() == 2 {
            let set1 = set1_bytes(&operands[0], flags)?;
            let first = expand_set(operands[0].as_bytes(), None)?;
            let second = expand_set(operands[1].as_bytes(), Some(set1.len()))?;
            for (from, to) in translation_pairs(&set1, &first, &second, flags)? {
                map[from as usize] = to;
            }
            squeeze_source = second.bytes;
        } else {
            squeeze_source = set1_bytes(&operands[0], flags)?;
        }
        let squeeze = flags.squeeze.then(|| mask_of(&squeeze_source));
        Ok(Plan {
            map,
            delete,
            squeeze,
        })
    }

    /// Converts one chunk. `last` is the last byte written, carried across chunks for squeezing.
    fn process(&self, input: &[u8], last: &mut Option<u8>, out: &mut Vec<u8>) {
        for &byte in input {
            if self.delete[byte as usize] {
                continue;
            }
            let value = self.map[byte as usize];
            if let Some(squeeze) = &self.squeeze {
                if squeeze[value as usize] && *last == Some(value) {
                    continue;
                }
            }
            out.push(value);
            *last = Some(value);
        }
    }
}

fn check_operand_count(flags: &Flags, operands: &[String]) -> Result<(), String> {
    match (operands.len(), flags.delete, flags.squeeze) {
        (0, _, _) => Err("missing operand".to_owned()),
        (1, false, false) => Err(format!(
            "missing operand after '{}'\nTwo strings must be given when translating.",
            operands[0]
        )),
        (1, true, true) => Err(format!(
            "missing operand after '{}'\nTwo strings must be given when both deleting and squeezing repeats.",
            operands[0]
        )),
        (2, true, false) => Err(format!(
            "extra operand '{}'\nOnly one string may be given when deleting without squeezing repeats.",
            operands[1]
        )),
        (n, _, _) if n > 2 => Err(format!("extra operand '{}'", operands[2])),
        _ => Ok(()),
    }
}

/// SET1 after expansion and, with `-c`, complementation.
fn set1_bytes(text: &str, flags: &Flags) -> Result<Vec<u8>, String> {
    let set = expand_set(text.as_bytes(), None)?;
    if flags.complement {
        Ok(invert_list(&set.bytes))
    } else {
        Ok(set.bytes)
    }
}

/// Pairs each SET1 byte with its SET2 counterpart. Later pairs win for a repeated SET1 byte.
fn translation_pairs(
    set1: &[u8],
    first: &Expanded,
    second: &Expanded,
    flags: &Flags,
) -> Result<Vec<(u8, u8)>, String> {
    if second.bytes.is_empty() {
        return Err("when translating, the second string must not be empty".to_owned());
    }
    check_case_alignment(first, second, flags)?;
    let len = if flags.truncate {
        set1.len().min(second.bytes.len())
    } else {
        set1.len()
    };
    let last = second.bytes[second.bytes.len() - 1];
    Ok(set1
        .iter()
        .take(len)
        .enumerate()
        .map(|(index, &from)| (from, second.bytes.get(index).copied().unwrap_or(last)))
        .collect())
}

/// A `[:upper:]` or `[:lower:]` in SET2 must line up with one in SET1 (`-c` exempt).
fn check_case_alignment(first: &Expanded, second: &Expanded, flags: &Flags) -> Result<(), String> {
    if flags.complement {
        return Ok(());
    }
    if second
        .case_starts
        .iter()
        .all(|start| first.case_starts.contains(start))
    {
        Ok(())
    } else {
        Err("misaligned [:upper:] and/or [:lower:] construct".to_owned())
    }
}

fn mask_of(bytes: &[u8]) -> [bool; 256] {
    let mut mask = [false; 256];
    for &byte in bytes {
        mask[byte as usize] = true;
    }
    mask
}

fn invert_list(bytes: &[u8]) -> Vec<u8> {
    let mask = mask_of(bytes);
    (0..=255u8).filter(|&byte| !mask[byte as usize]).collect()
}

/// Expands SET text. `fill_len` is the length of SET1 when expanding SET2; a `[c*]` repeat
/// takes up the difference. `None` (SET1) rejects `[c*]`.
fn expand_set(text: &[u8], fill_len: Option<usize>) -> Result<Expanded, String> {
    let tokens = parse_tokens(text)?;
    let stars = tokens
        .iter()
        .filter(|token| matches!(token, Token::Repeat(_, None)))
        .count();
    if stars > 0 && fill_len.is_none() {
        return Err("the [c*] repeat construct may not appear in string1".to_owned());
    }
    if stars > 1 {
        return Err("only one [c*] repeat construct may appear in string2".to_owned());
    }
    let fixed: usize = tokens.iter().map(fixed_len).sum();
    let fill = fill_len.unwrap_or(0).saturating_sub(fixed);
    let mut out = Expanded::default();
    for token in &tokens {
        expand_token(token, fill, &mut out)?;
    }
    Ok(out)
}

/// Number of bytes a token contributes, ignoring a `[c*]` fill.
fn fixed_len(token: &Token) -> usize {
    match token {
        Token::Byte(_) | Token::Equiv(_) => 1,
        Token::Range(lo, hi) => (*hi as usize + 1).saturating_sub(*lo as usize),
        Token::Class(class) => (0..=255u8).filter(|&b| class.contains(b)).count(),
        Token::Repeat(_, Some(count)) => *count,
        Token::Repeat(_, None) => 0,
    }
}

fn expand_token(token: &Token, fill: usize, out: &mut Expanded) -> Result<(), String> {
    match token {
        Token::Byte(byte) | Token::Equiv(byte) => out.bytes.push(*byte),
        Token::Range(lo, hi) => {
            if lo > hi {
                return Err(format!(
                    "range-endpoints of '{}-{}' are in reverse collating sequence order",
                    escape_char(*lo),
                    escape_char(*hi)
                ));
            }
            out.bytes.extend(*lo..=*hi);
        }
        Token::Class(class) => {
            if class.is_case() {
                out.case_starts.push(out.bytes.len());
            }
            out.bytes.extend((0..=255u8).filter(|&b| class.contains(b)));
        }
        Token::Repeat(byte, Some(count)) => out.bytes.extend(std::iter::repeat_n(*byte, *count)),
        Token::Repeat(byte, None) => out.bytes.extend(std::iter::repeat_n(*byte, fill)),
    }
    Ok(())
}

fn escape_char(byte: u8) -> String {
    if byte.is_ascii_graphic() {
        (byte as char).to_string()
    } else {
        format!("\\{byte:03o}")
    }
}

/// Splits SET text into tokens.
fn parse_tokens(text: &[u8]) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < text.len() {
        if text[index] == b'[' {
            if let Some((token, next)) = parse_bracket(text, index)? {
                tokens.push(token);
                index = next;
                continue;
            }
        }
        let (lo, next) = read_char(text, index);
        if next + 1 < text.len() && text[next] == b'-' {
            let (hi, after) = read_char(text, next + 1);
            tokens.push(Token::Range(lo, hi));
            index = after;
        } else {
            tokens.push(Token::Byte(lo));
            index = next;
        }
    }
    Ok(tokens)
}

/// Reads one (possibly escaped) byte at `index`; returns it and the next index.
fn read_char(text: &[u8], index: usize) -> (u8, usize) {
    if text[index] != b'\\' || index + 1 >= text.len() {
        return (text[index], index + 1);
    }
    let code = text[index + 1];
    let simple = match code {
        b'a' => Some(7),
        b'b' => Some(8),
        b'f' => Some(12),
        b'n' => Some(10),
        b'r' => Some(13),
        b't' => Some(9),
        b'v' => Some(11),
        b'\\' => Some(b'\\'),
        _ => None,
    };
    if let Some(byte) = simple {
        return (byte, index + 2);
    }
    if (b'0'..=b'7').contains(&code) {
        let mut value: u32 = 0;
        let mut end = index + 1;
        while end < text.len() && end < index + 4 && (b'0'..=b'7').contains(&text[end]) {
            value = value * 8 + u32::from(text[end] - b'0');
            end += 1;
        }
        return ((value & 0xff) as u8, end);
    }
    (code, index + 2)
}

/// Parses a bracket construct at `index` (`[:class:]`, `[=c=]`, `[c*n]`). Returns `None`
/// when the `[` is an ordinary character.
fn parse_bracket(text: &[u8], index: usize) -> Result<Option<(Token, usize)>, String> {
    let Some(&kind) = text.get(index + 1) else {
        return Ok(None);
    };
    match kind {
        b':' => parse_class(text, index),
        b'=' => parse_equiv(text, index),
        _ => Ok(parse_repeat(text, index)),
    }
}

fn parse_class(text: &[u8], index: usize) -> Result<Option<(Token, usize)>, String> {
    let Some(close) = find_pair(text, index + 2, b':') else {
        return Ok(None);
    };
    let name = String::from_utf8_lossy(&text[index + 2..close]).into_owned();
    match Class::parse(&name) {
        Some(class) => Ok(Some((Token::Class(class), close + 2))),
        None => Err(format!("invalid character class '{name}'")),
    }
}

fn parse_equiv(text: &[u8], index: usize) -> Result<Option<(Token, usize)>, String> {
    let Some(close) = find_pair(text, index + 2, b'=') else {
        return Ok(None);
    };
    let (byte, next) = read_char(text, index + 2);
    if next != close {
        return Err("equivalence class operand must be a single character".to_owned());
    }
    Ok(Some((Token::Equiv(byte), close + 2)))
}

/// Position of `marker` followed by `]`, searching from `start`.
fn find_pair(text: &[u8], start: usize, marker: u8) -> Option<usize> {
    (start..text.len().saturating_sub(1)).find(|&pos| text[pos] == marker && text[pos + 1] == b']')
}

/// `[c*n]` or `[c*]`. Returns `None` when the text is not a repeat construct.
fn parse_repeat(text: &[u8], index: usize) -> Option<(Token, usize)> {
    let (byte, star) = read_char(text, index + 1);
    if text.get(star) != Some(&b'*') {
        return None;
    }
    let close = (star + 1..text.len()).find(|&pos| text[pos] == b']')?;
    let digits = &text[star + 1..close];
    if !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let count = if digits.is_empty() {
        None
    } else {
        Some(parse_count(digits))
    };
    Some((Token::Repeat(byte, count), close + 1))
}

/// Repeat counts are octal when they start with 0, decimal otherwise.
fn parse_count(digits: &[u8]) -> usize {
    let radix = if digits.len() > 1 && digits[0] == b'0' {
        8
    } else {
        10
    };
    digits.iter().fold(0usize, |acc, &digit| {
        acc.saturating_mul(radix)
            .saturating_add(usize::from(digit - b'0'))
    })
}

fn filter_stdin(plan: &Plan) -> i32 {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut out = stdout_writer();
    let mut buf = vec![0u8; CHUNK];
    let mut converted = Vec::with_capacity(CHUNK);
    let mut last = None;
    loop {
        let read = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                eprintln!("{CMD}: read error: {}", io_error_text(&error));
                return EXIT_FAIL;
            }
        };
        converted.clear();
        plan.process(&buf[..read], &mut last, &mut converted);
        if let Err(error) = out.write_all(&converted) {
            if is_broken_pipe(&error) {
                return EXIT_OK;
            }
            eprintln!("{CMD}: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
    }
    finish_output(CMD, &mut out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_plan(args: &[&str], input: &[u8]) -> Result<Vec<u8>, String> {
        let has = |letter: char| {
            args.iter()
                .any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains(letter))
        };
        let flags = Flags {
            complement: has('c'),
            delete: has('d'),
            squeeze: has('s'),
            truncate: has('t'),
        };
        let operands: Vec<String> = args
            .iter()
            .filter(|a| !a.starts_with('-'))
            .map(|a| a.to_string())
            .collect();
        let plan = Plan::build(&flags, &operands)?;
        let mut out = Vec::new();
        let mut last = None;
        plan.process(input, &mut last, &mut out);
        Ok(out)
    }

    #[test]
    fn expands_ranges_and_escapes() {
        let set = expand_set(b"a-cx\\101\\n", None).unwrap();
        assert_eq!(set.bytes, b"abcxA\n");
        assert!(expand_set(b"z-a", None).is_err());
    }

    #[test]
    fn expands_classes_and_records_case_starts() {
        let set = expand_set(b"[:upper:]0", None).unwrap();
        assert_eq!(set.bytes.len(), 27);
        assert_eq!(set.case_starts, vec![0]);
        assert!(expand_set(b"[:nope:]", None).is_err());
    }

    #[test]
    fn repeat_counts_are_octal_with_leading_zero() {
        assert_eq!(expand_set(b"[x*3]", None).unwrap().bytes, b"xxx");
        assert_eq!(expand_set(b"[x*010]", None).unwrap().bytes.len(), 8);
        let fill = expand_set(b"[y*]a", Some(4)).unwrap();
        assert_eq!(fill.bytes, b"yyya");
        assert!(expand_set(b"[y*]", None).is_err());
    }

    #[test]
    fn translates_with_last_byte_padding() {
        assert_eq!(run_plan(&["abc", "xy"], b"abcd").unwrap(), b"xyyd");
        assert_eq!(run_plan(&["-t", "abc", "xy"], b"abc").unwrap(), b"xyc");
    }

    #[test]
    fn translates_classes_when_aligned() {
        assert_eq!(run_plan(&["[:lower:]", "[:upper:]"], b"aB").unwrap(), b"AB");
        assert!(run_plan(&["a", "[:upper:]"], b"a").is_err());
    }

    #[test]
    fn deletes_and_complements() {
        assert_eq!(run_plan(&["-d", "[:digit:]"], b"a1b2").unwrap(), b"ab");
        assert_eq!(run_plan(&["-cd", "a-z"], b"a1B\n").unwrap(), b"a");
        assert_eq!(run_plan(&["-c", "a-z", "_"], b"a!").unwrap(), b"a_");
    }

    #[test]
    fn squeezes_runs_of_the_set() {
        assert_eq!(run_plan(&["-s", " "], b"a   b  c").unwrap(), b"a b c");
        assert_eq!(run_plan(&["-s", "a-z", "A-Z"], b"aaBBcc").unwrap(), b"ABC");
        assert_eq!(run_plan(&["-ds", "x", "y"], b"axxyyb").unwrap(), b"ayb");
    }

    #[test]
    fn reports_operand_errors() {
        assert!(run_plan(&["-d", "a", "b"], b"").is_err());
        assert!(run_plan(&["a"], b"").is_err());
        assert!(run_plan(&["a", "b", "c"], b"").is_err());
        assert!(run_plan(&["[a*]", "x"], b"").is_err());
    }

    #[test]
    fn squeeze_carries_across_chunks() {
        let flags = Flags {
            squeeze: true,
            ..Flags::default()
        };
        let plan = Plan::build(&flags, &["x".to_owned()]).unwrap();
        let mut out = Vec::new();
        let mut last = None;
        plan.process(b"axx", &mut last, &mut out);
        plan.process(b"xb", &mut last, &mut out);
        assert_eq!(out, b"axb");
    }
}
