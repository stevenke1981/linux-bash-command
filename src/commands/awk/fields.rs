//! The current record (`$0`) and its fields, with lazy splitting.
//!
//! A record is split on first use of a field or `NF`, using the `FS` that was in effect when
//! the record was set. Assigning a field rebuilds `$0` with `OFS` on the next read.

use regex::bytes::Regex;

use super::ast::{FS, OFS, RS};
use super::interp::{Fault, Interp, Slot};
use super::regex::RegexCache;
use super::value::Value;

/// `$0`, its fields, and the splitting settings captured when the record was set.
#[derive(Debug)]
pub struct Record {
    text: Vec<u8>,
    text_valid: bool,
    fields: Vec<Value>,
    split_done: bool,
    fs: Vec<u8>,
    paragraph: bool,
}

impl Record {
    pub fn new() -> Self {
        Self {
            text: Vec::new(),
            text_valid: true,
            fields: Vec::new(),
            split_done: true,
            fs: b" ".to_vec(),
            paragraph: false,
        }
    }
}

impl Default for Record {
    fn default() -> Self {
        Self::new()
    }
}

impl Interp {
    /// Makes `text` the new `$0`. Fields are split again on demand.
    pub(super) fn set_record(&mut self, text: Vec<u8>) {
        self.rec.fs = self.special_bytes(FS);
        self.rec.paragraph = self.is_paragraph_mode();
        self.rec.text = text;
        self.rec.text_valid = true;
        self.rec.fields.clear();
        self.rec.split_done = false;
    }

    /// The text of `$0`, rebuilt from the fields when a field was assigned.
    pub(super) fn record_text(&mut self) -> &[u8] {
        if !self.rec.text_valid {
            let text = {
                let ofs = self.special_bytes(OFS);
                let fmt = self.convfmt();
                let mut text = Vec::new();
                for (index, field) in self.rec.fields.iter().enumerate() {
                    if index > 0 {
                        text.extend_from_slice(&ofs);
                    }
                    text.extend_from_slice(&field.to_str(fmt));
                }
                text
            };
            self.rec.text = text;
            self.rec.text_valid = true;
        }
        &self.rec.text
    }

    pub(super) fn ensure_split(&mut self) -> Result<(), Fault> {
        if self.rec.split_done {
            return Ok(());
        }
        let parts = split_fields(
            &self.rec.text,
            &self.rec.fs,
            self.rec.paragraph,
            &mut self.regex,
        )
        .map_err(Fault::Fatal)?;
        self.rec.fields = parts.into_iter().map(Value::from_input).collect();
        self.rec.split_done = true;
        Ok(())
    }

    /// The value of `NF`.
    pub(super) fn nf(&mut self) -> Result<usize, Fault> {
        self.ensure_split()?;
        Ok(self.rec.fields.len())
    }

    /// Assigning `NF` truncates or extends the record.
    pub(super) fn set_nf(&mut self, count: usize) -> Result<(), Fault> {
        self.ensure_split()?;
        self.rec.fields.resize(count, Value::Uninit);
        self.rec.text_valid = false;
        Ok(())
    }

    /// `$index`. Field 0 is the whole record as input text (a strnum when numeric).
    pub(super) fn get_field(&mut self, index: usize) -> Result<Value, Fault> {
        if index == 0 {
            let text = self.record_text().to_vec();
            return Ok(Value::from_input(text));
        }
        self.ensure_split()?;
        Ok(self
            .rec
            .fields
            .get(index - 1)
            .cloned()
            .unwrap_or(Value::Uninit))
    }

    /// `$index = value`.
    pub(super) fn set_field(&mut self, index: usize, value: Value) -> Result<(), Fault> {
        if index == 0 {
            let text = self.to_str(&value).into_owned();
            self.set_record(text);
            return Ok(());
        }
        self.ensure_split()?;
        if index > self.rec.fields.len() {
            self.rec.fields.resize(index, Value::Uninit);
        }
        self.rec.fields[index - 1] = value;
        self.rec.text_valid = false;
        Ok(())
    }

    /// True when `RS` is the empty string (paragraph mode).
    fn is_paragraph_mode(&self) -> bool {
        matches!(&self.globals[RS], Slot::Val(value) if value.to_str(b"%.6g").is_empty())
    }
}

/// Splits `text` into fields by `fs`: a single space (blanks, tabs and newlines), one byte,
/// a regex, or the empty string (the whole record is one field, as in gawk --posix).
/// In paragraph mode a newline also separates fields.
pub fn split_fields(
    text: &[u8],
    fs: &[u8],
    paragraph: bool,
    cache: &mut RegexCache,
) -> Result<Vec<Vec<u8>>, String> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    if fs == b" " {
        return Ok(split_blanks(text));
    }
    if fs.is_empty() {
        return Ok(vec![text.to_vec()]);
    }
    if fs.len() == 1 && fs[0] != b'\\' {
        return Ok(split_byte(text, fs[0], paragraph));
    }
    let mut source = fs.to_vec();
    if paragraph {
        source.extend_from_slice(b"|\n");
    }
    let regex = cache.get(&source)?;
    Ok(split_regex(text, &regex))
}

/// Splits into single characters (`split(s, a, "")`).
pub fn split_chars(text: &[u8]) -> Vec<Vec<u8>> {
    char_starts(text)
        .windows(2)
        .map(|w| text[w[0]..w[1]].to_vec())
        .collect()
}

/// Splits on runs of blanks, ignoring leading and trailing blanks.
pub fn split_blanks(text: &[u8]) -> Vec<Vec<u8>> {
    let is_blank = |b: u8| matches!(b, b' ' | b'\t' | b'\n');
    text.split(|&b| is_blank(b))
        .filter(|part| !part.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

fn split_byte(text: &[u8], separator: u8, paragraph: bool) -> Vec<Vec<u8>> {
    text.split(|&b| b == separator || (paragraph && b == b'\n'))
        .map(<[u8]>::to_vec)
        .collect()
}

/// Splits on non-empty matches of `regex`.
pub fn split_regex(text: &[u8], regex: &Regex) -> Vec<Vec<u8>> {
    let mut fields = Vec::new();
    let mut last = 0;
    for found in regex.find_iter(text) {
        if found.is_empty() {
            continue;
        }
        fields.push(text[last..found.start()].to_vec());
        last = found.end();
    }
    fields.push(text[last..].to_vec());
    fields
}

/// Byte offsets where each UTF-8 character of `text` starts, followed by `text.len()`.
/// Invalid bytes count as one character each.
pub fn char_starts(text: &[u8]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(text.len() + 1);
    let mut index = 0;
    while index < text.len() {
        starts.push(index);
        index += utf8_char_len(&text[index..]);
    }
    starts.push(text.len());
    starts
}

/// Length in bytes of the UTF-8 sequence at the start of `bytes`, or 1 when it is invalid.
pub fn utf8_char_len(bytes: &[u8]) -> usize {
    let lead = bytes[0];
    let len = match lead {
        0x00..=0x7f => return 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return 1,
    };
    if bytes.len() < len {
        return 1;
    }
    match std::str::from_utf8(&bytes[..len]) {
        Ok(_) => len,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str, fs: &str, paragraph: bool) -> Vec<String> {
        let mut cache = RegexCache::new();
        split_fields(text.as_bytes(), fs.as_bytes(), paragraph, &mut cache)
            .expect("split")
            .into_iter()
            .map(|f| String::from_utf8(f).unwrap())
            .collect()
    }

    #[test]
    fn default_blank_splitting_ignores_surrounding_blanks() {
        assert_eq!(split("  a \t b  c\n", " ", false), vec!["a", "b", "c"]);
        assert!(split("   ", " ", false).is_empty());
    }

    #[test]
    fn single_character_separators_are_literal() {
        assert_eq!(split("a:b::c", ":", false), vec!["a", "b", "", "c"]);
        assert_eq!(split("a.b", ".", false), vec!["a", "b"]);
        assert_eq!(split("a\tb c", "\t", false), vec!["a", "b c"]);
    }

    #[test]
    fn regex_separators_keep_empty_fields() {
        assert_eq!(split("a1b22c", "[0-9]+", false), vec!["a", "b", "c"]);
        assert_eq!(split("1a2", "[0-9]", false), vec!["", "a", ""]);
    }

    #[test]
    fn empty_separator_keeps_the_record_whole() {
        assert_eq!(split("abc", "", false), vec!["abc"]);
    }

    #[test]
    fn paragraph_mode_adds_newline_as_separator() {
        assert_eq!(split("a,b\nc", ",", true), vec!["a", "b", "c"]);
        assert_eq!(split("a\nb", ":", true), vec!["a", "b"]);
    }

    #[test]
    fn empty_text_has_no_fields() {
        assert!(split("", ",", false).is_empty());
    }

    #[test]
    fn character_starts_handle_utf8_and_invalid_bytes() {
        assert_eq!(char_starts("aé".as_bytes()), vec![0, 1, 3]);
        assert_eq!(char_starts(b"a\xffb"), vec![0, 1, 2, 3]);
        assert_eq!(utf8_char_len(&[0xe2, 0x82]), 1);
        assert_eq!(utf8_char_len("€".as_bytes()), 3);
    }
}
