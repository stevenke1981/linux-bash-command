//! Format string parsing and execution for `printf`.
//!
//! The format is parsed once into pieces. Parse errors are kept as a trailing error so the
//! text before them is still printed, as GNU does. Arguments are consumed across repeated
//! passes of the format while any remain.

use std::io::{self, Write};

use super::float::{self, pad, push_repeat, Spec};
use super::num::{self, NumError};
use super::quote::quote;

/// One element of a parsed format.
#[derive(Debug)]
pub enum Piece {
    Text(Vec<u8>),
    Directive(Directive),
    /// `\c`: stop all output.
    Stop,
}

/// Flags of a conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub left: bool,
    pub plus: bool,
    pub space: bool,
    pub alt: bool,
    pub zero: bool,
}

/// A width or precision: absent, a number, or `*` (taken from the arguments).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Unset,
    Value(usize),
    Star,
}

/// One `%` conversion.
#[derive(Clone, Copy, Debug)]
pub struct Directive {
    pub flags: Flags,
    pub width: Count,
    pub precision: Count,
    pub conv: u8,
}

/// A format split into pieces, plus the error that stopped parsing, if any.
#[derive(Debug)]
pub struct Parsed {
    pub pieces: Vec<Piece>,
    pub error: Option<String>,
}

/// What a single directive asks the runner to do next.
enum Outcome {
    Go,
    Stop,
    Fail(String),
}

/// Result of decoding one backslash escape.
enum Escape {
    Bytes(Vec<u8>),
    Stop,
}

const CONVERSIONS: &[u8] = b"diouxXfFeEgGaAcsbq";

/// Splits `format` into pieces. Escapes are decoded here; `%%` becomes text.
pub fn parse(format: &[u8]) -> Parsed {
    let mut pieces = Vec::new();
    let mut text = Vec::new();
    let mut index = 0;
    while index < format.len() {
        let byte = format[index];
        if byte == b'\\' {
            match decode_escape(format, &mut index, false) {
                Ok(Escape::Bytes(bytes)) => text.extend_from_slice(&bytes),
                Ok(Escape::Stop) => {
                    flush_text(&mut pieces, &mut text);
                    pieces.push(Piece::Stop);
                    return Parsed {
                        pieces,
                        error: None,
                    };
                }
                Err(error) => return failed(pieces, text, error),
            }
        } else if byte == b'%' && format.get(index + 1) == Some(&b'%') {
            text.push(b'%');
            index += 2;
        } else if byte == b'%' {
            match parse_directive(format, index + 1) {
                Ok((directive, next)) => {
                    flush_text(&mut pieces, &mut text);
                    pieces.push(Piece::Directive(directive));
                    index = next;
                }
                Err(error) => return failed(pieces, text, error),
            }
        } else {
            text.push(byte);
            index += 1;
        }
    }
    flush_text(&mut pieces, &mut text);
    Parsed {
        pieces,
        error: None,
    }
}

fn failed(mut pieces: Vec<Piece>, mut text: Vec<u8>, error: String) -> Parsed {
    flush_text(&mut pieces, &mut text);
    Parsed {
        pieces,
        error: Some(error),
    }
}

fn flush_text(pieces: &mut Vec<Piece>, text: &mut Vec<u8>) {
    if !text.is_empty() {
        pieces.push(Piece::Text(std::mem::take(text)));
    }
}

/// Parses the conversion whose text starts at `start` (just after `%`). Returns the
/// directive and the index after it.
fn parse_directive(bytes: &[u8], start: usize) -> Result<(Directive, usize), String> {
    let mut index = start;
    let mut flags = Flags::default();
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'-' => flags.left = true,
            b'+' => flags.plus = true,
            b' ' => flags.space = true,
            b'#' => flags.alt = true,
            b'0' => flags.zero = true,
            b'\'' => {}
            _ => break,
        }
        index += 1;
    }
    let (width, next) = parse_count(bytes, index);
    index = next;
    let mut precision = Count::Unset;
    if bytes.get(index) == Some(&b'.') {
        let (value, next) = parse_count(bytes, index + 1);
        precision = match value {
            Count::Unset => Count::Value(0),
            other => other,
        };
        index = next;
    }
    while bytes.get(index).is_some_and(|b| b"hlLjzt".contains(b)) {
        index += 1;
    }
    let conv = bytes.get(index).copied();
    let spec_text = String::from_utf8_lossy(&bytes[start - 1..(index + 1).min(bytes.len())]);
    let Some(conv) = conv.filter(|c| CONVERSIONS.contains(c)) else {
        return Err(format!("{spec_text}: invalid conversion specification"));
    };
    let valid = match conv {
        b'q' => flags == Flags::default() && width == Count::Unset && precision == Count::Unset,
        b'b' => !(flags.plus || flags.space || flags.alt || flags.zero),
        b'c' => !(flags.alt || flags.zero) && precision == Count::Unset,
        b's' => !(flags.alt || flags.zero),
        _ => true,
    };
    if !valid {
        return Err(format!("{spec_text}: invalid conversion specification"));
    }
    Ok((
        Directive {
            flags,
            width,
            precision,
            conv,
        },
        index + 1,
    ))
}

/// Parses `*` or a decimal number at `index`.
fn parse_count(bytes: &[u8], index: usize) -> (Count, usize) {
    if bytes.get(index) == Some(&b'*') {
        return (Count::Star, index + 1);
    }
    let (value, used) = digits(&bytes[index..], 10, usize::MAX);
    if used == 0 {
        (Count::Unset, index)
    } else {
        (Count::Value(value as usize), index + used)
    }
}

/// Decodes the escape whose backslash is at `bytes[*index]`, advancing `index` past it.
/// `echo_style` selects `%b` rules (`\0NNN` for octal); otherwise format rules apply
/// (`\NNN`).
fn decode_escape(bytes: &[u8], index: &mut usize, echo_style: bool) -> Result<Escape, String> {
    *index += 1;
    let Some(&code) = bytes.get(*index) else {
        return Ok(Escape::Bytes(vec![b'\\']));
    };
    *index += 1;
    let simple = match code {
        b'\\' => Some(b'\\'),
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'e' => Some(0x1b),
        b'f' => Some(0x0c),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0b),
        b'"' => Some(b'"'),
        _ => None,
    };
    if let Some(byte) = simple {
        return Ok(Escape::Bytes(vec![byte]));
    }
    match code {
        b'c' => Ok(Escape::Stop),
        b'0' if echo_style => {
            let (value, used) = digits(&bytes[*index..], 8, 3);
            *index += used;
            Ok(Escape::Bytes(vec![value as u8]))
        }
        b'0'..=b'7' if !echo_style => {
            let (value, used) = digits(&bytes[*index - 1..], 8, 3);
            *index += used - 1;
            Ok(Escape::Bytes(vec![value as u8]))
        }
        b'x' => {
            let (value, used) = digits(&bytes[*index..], 16, 2);
            if used == 0 {
                return Err(MISSING_HEX.to_owned());
            }
            *index += used;
            Ok(Escape::Bytes(vec![value as u8]))
        }
        b'u' | b'U' => {
            let max = if code == b'u' { 4 } else { 8 };
            let (value, used) = digits(&bytes[*index..], 16, max);
            if used == 0 {
                return Err(MISSING_HEX.to_owned());
            }
            let Some(character) = char::from_u32(value) else {
                return Err("invalid universal character name".to_owned());
            };
            *index += used;
            let mut buffer = [0u8; 4];
            Ok(Escape::Bytes(
                character.encode_utf8(&mut buffer).as_bytes().to_vec(),
            ))
        }
        _ => Ok(Escape::Bytes(vec![b'\\', code])),
    }
}

const MISSING_HEX: &str = "missing hexadecimal number in escape";

/// Reads up to `max` digits in `radix`. Returns the value and the number of bytes used.
fn digits(bytes: &[u8], radix: u32, max: usize) -> (u32, usize) {
    let mut value = 0u32;
    let mut used = 0;
    for &byte in bytes.iter().take(max) {
        let Some(digit) = (byte as char).to_digit(radix) else {
            break;
        };
        value = value.wrapping_mul(radix).wrapping_add(digit);
        used += 1;
    }
    (value, used)
}

/// The arguments not yet consumed, plus the exit status accumulated by numeric errors.
struct Arguments<'a> {
    items: &'a [String],
    next: usize,
    status: i32,
}

impl<'a> Arguments<'a> {
    /// The next argument, or the empty string once they are used up.
    fn take(&mut self) -> &'a str {
        let value = self.items.get(self.next).map_or("", String::as_str);
        self.next += 1;
        value
    }

    fn exhausted(&self) -> bool {
        self.next >= self.items.len()
    }

    fn report(&mut self, arg: &str, error: Option<NumError>) {
        if let Some(error) = error {
            eprintln!("printf: '{arg}': {}", error.text());
            self.status = 1;
        }
    }

    fn signed(&mut self) -> i64 {
        let arg = self.take();
        let (value, error) = num::parse_signed(arg);
        self.report(arg, error);
        value
    }

    fn unsigned(&mut self) -> u64 {
        let arg = self.take();
        let (value, error) = num::parse_unsigned(arg);
        self.report(arg, error);
        value
    }

    fn float(&mut self) -> f64 {
        let arg = self.take();
        let (value, error) = num::parse_float(arg);
        self.report(arg, error);
        value
    }
}

/// Runs `format` over `args`, writing to `out`. Returns the exit status (1 after a numeric
/// or format error, 0 otherwise). `\c` ends output early with status 0.
pub fn execute(format: &str, args: &[String], out: &mut impl Write) -> io::Result<i32> {
    let parsed = parse(format.as_bytes());
    let consumes = parsed
        .pieces
        .iter()
        .any(|piece| matches!(piece, Piece::Directive(_)));
    if !consumes {
        if let Some(first) = args.first() {
            eprintln!("printf: warning: ignoring excess arguments, starting with '{first}'");
        }
    }
    let mut state = Arguments {
        items: args,
        next: 0,
        status: 0,
    };
    loop {
        for piece in &parsed.pieces {
            match piece {
                Piece::Text(bytes) => out.write_all(bytes)?,
                Piece::Stop => return Ok(state.status),
                Piece::Directive(directive) => match emit(directive, &mut state, out)? {
                    Outcome::Go => {}
                    Outcome::Stop => return Ok(state.status),
                    Outcome::Fail(message) => {
                        eprintln!("printf: {message}");
                        return Ok(1);
                    }
                },
            }
        }
        if let Some(error) = &parsed.error {
            eprintln!("printf: {error}");
            return Ok(1);
        }
        if !consumes || state.exhausted() {
            break;
        }
    }
    Ok(state.status)
}

/// Formats one conversion, taking its width, precision and value from `state`.
fn emit(directive: &Directive, state: &mut Arguments, out: &mut impl Write) -> io::Result<Outcome> {
    let mut flags = directive.flags;
    let width = match directive.width {
        Count::Unset => 0,
        Count::Value(value) => value,
        Count::Star => {
            let value = state.signed();
            if value < 0 {
                flags.left = true;
            }
            value.unsigned_abs() as usize
        }
    };
    let precision = match directive.precision {
        Count::Unset => None,
        Count::Value(value) => Some(value),
        Count::Star => usize::try_from(state.signed()).ok(),
    };
    let spec = Spec {
        left: flags.left,
        plus: flags.plus,
        space: flags.space,
        alt: flags.alt,
        zero: flags.zero,
        width,
        precision,
    };
    let mut buffer = Vec::new();
    let mut outcome = Outcome::Go;
    match directive.conv {
        b'd' | b'i' => {
            let value = state.signed();
            format_signed(&mut buffer, value, &spec);
        }
        b'o' | b'u' | b'x' | b'X' => {
            let value = state.unsigned();
            format_unsigned(&mut buffer, value, directive.conv, &spec);
        }
        b'c' => {
            let byte = state.take().as_bytes().first().copied().unwrap_or(0);
            pad_bytes(&mut buffer, &[byte], &spec);
        }
        b's' => {
            let arg = state.take().as_bytes();
            pad_bytes(&mut buffer, truncate(arg, precision), &spec);
        }
        b'b' => {
            let arg = state.take().to_owned();
            match expand_b(&arg) {
                Ok((bytes, stop)) => {
                    pad_bytes(&mut buffer, truncate(&bytes, precision), &spec);
                    if stop {
                        outcome = Outcome::Stop;
                    }
                }
                Err(message) => return Ok(Outcome::Fail(message)),
            }
        }
        b'q' => {
            let arg = state.take().to_owned();
            buffer = quote(&arg);
        }
        conv => {
            let value = state.float();
            float::format_float(&mut buffer, value, conv, &spec);
        }
    }
    out.write_all(&buffer)?;
    Ok(outcome)
}

/// `%d %i`: sign, precision, `#`-free digits, then padding.
fn format_signed(buffer: &mut Vec<u8>, value: i64, spec: &Spec) {
    let magnitude = value.unsigned_abs();
    let sign = if value < 0 {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    };
    let digits = integer_digits(magnitude.to_string(), magnitude, spec.precision);
    pad(buffer, sign, &digits, spec, spec.precision.is_none());
}

/// `%o %u %x %X`: digits in the requested radix, with `#` prefixes.
fn format_unsigned(buffer: &mut Vec<u8>, value: u64, conv: u8, spec: &Spec) {
    let text = match conv {
        b'o' => format!("{value:o}"),
        b'x' => format!("{value:x}"),
        b'X' => format!("{value:X}"),
        _ => value.to_string(),
    };
    let mut digits = integer_digits(text, value, spec.precision);
    let mut prefix = "";
    if spec.alt {
        match conv {
            b'o' if !digits.starts_with('0') => digits.insert(0, '0'),
            b'x' if value != 0 => prefix = "0x",
            b'X' if value != 0 => prefix = "0X",
            _ => {}
        }
    }
    pad(buffer, prefix, &digits, spec, spec.precision.is_none());
}

/// Applies an integer precision: zero digits are added in front, and a zero value with
/// precision 0 prints nothing (C rules).
fn integer_digits(mut digits: String, magnitude: u64, precision: Option<usize>) -> String {
    if precision == Some(0) && magnitude == 0 {
        return String::new();
    }
    if let Some(min) = precision {
        if digits.len() < min {
            digits.insert_str(0, &"0".repeat(min - digits.len()));
        }
    }
    digits
}

/// Pads a byte string with spaces to the field width.
fn pad_bytes(buffer: &mut Vec<u8>, bytes: &[u8], spec: &Spec) {
    let fill = spec.width.saturating_sub(bytes.len());
    if spec.left {
        buffer.extend_from_slice(bytes);
        push_repeat(buffer, b' ', fill);
    } else {
        push_repeat(buffer, b' ', fill);
        buffer.extend_from_slice(bytes);
    }
}

fn truncate(bytes: &[u8], precision: Option<usize>) -> &[u8] {
    match precision {
        Some(limit) if limit < bytes.len() => &bytes[..limit],
        _ => bytes,
    }
}

/// Decodes the escapes of a `%b` argument. Returns the bytes and whether `\c` stopped output.
fn expand_b(arg: &str) -> Result<(Vec<u8>, bool), String> {
    let bytes = arg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            match decode_escape(bytes, &mut index, true)? {
                Escape::Bytes(decoded) => out.extend_from_slice(&decoded),
                Escape::Stop => return Ok((out, true)),
            }
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    Ok((out, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_format(format: &str, args: &[&str]) -> (String, i32) {
        let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let mut out = Vec::new();
        let status = execute(format, &args, &mut out).unwrap();
        (String::from_utf8(out).unwrap(), status)
    }

    #[test]
    fn format_is_reused_while_arguments_remain() {
        assert_eq!(
            run_format("%s|%s\n", &["1", "2", "3"]),
            ("1|2\n3|\n".into(), 0)
        );
        assert_eq!(run_format("%s\n", &[]), ("\n".into(), 0));
    }

    #[test]
    fn escapes_and_stop_follow_gnu() {
        assert_eq!(run_format("a\\tb\\n", &[]), ("a\tb\n".into(), 0));
        assert_eq!(run_format("a\\cb\\n", &[]), ("a".into(), 0));
        assert_eq!(run_format("\\101\\x41", &[]), ("AA".into(), 0));
        assert_eq!(run_format("%b|", &["x\\ty\\c", "z"]), ("x\ty".into(), 0));
    }

    #[test]
    fn numeric_errors_continue_and_set_status() {
        assert_eq!(run_format("%d %d\n", &["abc", "5"]), ("0 5\n".into(), 1));
        assert_eq!(run_format("%d\n", &["1.5"]), ("1\n".into(), 1));
    }

    #[test]
    fn width_and_precision_from_arguments() {
        assert_eq!(run_format("%*d|", &["4", "7"]), ("   7|".into(), 0));
        assert_eq!(run_format("%*d|", &["-5", "3"]), ("3    |".into(), 0));
        assert_eq!(run_format("%.*f|", &["2", "3.14159"]), ("3.14|".into(), 0));
    }

    #[test]
    fn invalid_directives_stop_after_prefix() {
        assert_eq!(run_format("x%z", &[]), ("x".into(), 1));
        assert_eq!(run_format("%5%", &[]), ("".into(), 1));
    }

    #[test]
    fn integer_conversions_follow_c() {
        assert_eq!(
            run_format("%5.2d|%-4x|%#o|%+d", &["3", "255", "8", "4"]),
            ("   03|ff  |010|+4".into(), 0)
        );
        assert_eq!(
            run_format("%u %x", &["-1", "-1"]),
            ("18446744073709551615 ffffffffffffffff".into(), 0)
        );
    }

    #[test]
    fn character_and_string_conversions() {
        assert_eq!(
            run_format("%c|%5c|%.2s|", &["hello", "65", "abc"]),
            ("h|    6|ab|".into(), 0)
        );
        assert_eq!(run_format("%c", &[""]), ("\0".into(), 0));
    }
}
