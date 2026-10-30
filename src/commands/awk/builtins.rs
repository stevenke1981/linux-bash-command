//! Builtin functions: string, array, math and I/O helpers.

use std::time::{SystemTime, UNIX_EPOCH};

use regex::bytes::Regex;

use super::ast::{Builtin, Expr, FS, RLENGTH, RSTART};
use super::fields::{char_starts, split_chars, split_fields, split_regex};
use super::format::sprintf;
use super::interp::{Fault, Interp, Place};
use super::value::Value;

/// The pseudo-random generator behind `rand()` and `srand()`. Deterministic for a given seed.
#[derive(Debug)]
pub struct Rng {
    seed: f64,
    state: u64,
}

impl Rng {
    pub fn new() -> Self {
        Self {
            seed: 0.0,
            state: Self::state_for(0.0),
        }
    }

    fn state_for(seed: f64) -> u64 {
        let state = (seed as i64 as u64) ^ 0x9e37_79b9_7f4a_7c15;
        if state == 0 {
            1
        } else {
            state
        }
    }

    /// The seed in effect (the value `srand()` returns next time).
    pub fn seed(&self) -> f64 {
        self.seed
    }

    pub fn reseed(&mut self, seed: f64) {
        self.seed = seed;
        self.state = Self::state_for(seed);
    }

    /// A number in `[0, 1)` (xorshift64*).
    pub fn next_unit(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        let scrambled = x.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (scrambled >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

/// Expands `&` to the matched text and `\&` to a literal `&`; `\\` becomes one backslash.
pub fn append_replacement(out: &mut Vec<u8>, replacement: &[u8], matched: &[u8]) {
    let mut i = 0;
    while i < replacement.len() {
        let c = replacement[i];
        match c {
            b'\\' if replacement.get(i + 1) == Some(&b'&') => {
                out.push(b'&');
                i += 2;
            }
            b'\\' if replacement.get(i + 1) == Some(&b'\\') => {
                out.push(b'\\');
                i += 2;
            }
            b'&' => {
                out.extend_from_slice(matched);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
}

/// Replaces the first (or every, when `global`) match of `regex` in `text`. Returns the new
/// text and the number of replacements. An empty match right after a previous match is
/// not replaced.
pub fn substitute(
    regex: &Regex,
    text: &[u8],
    replacement: &[u8],
    global: bool,
) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(text.len());
    let mut count = 0;
    let mut pos = 0;
    let mut last_end: Option<usize> = None;
    while pos <= text.len() {
        let Some(found) = regex.find_at(text, pos) else {
            break;
        };
        let (start, end) = (found.start(), found.end());
        if start == end && last_end == Some(start) {
            if start >= text.len() {
                break;
            }
            out.extend_from_slice(&text[pos..=start]);
            pos = start + 1;
            continue;
        }
        out.extend_from_slice(&text[pos..start]);
        append_replacement(&mut out, replacement, &text[start..end]);
        count += 1;
        last_end = Some(end);
        if start == end {
            if start < text.len() {
                out.push(text[start]);
            }
            pos = start + 1;
        } else {
            pos = end;
        }
        if !global {
            break;
        }
    }
    if pos < text.len() {
        out.extend_from_slice(&text[pos..]);
    }
    (out, count)
}

/// Converts a byte offset to a 1-based character position.
fn char_position(starts: &[usize], byte: usize) -> usize {
    starts.partition_point(|&start| start < byte) + 1
}

/// Number of characters in `text[start..end]` given the character starts of the whole text.
fn chars_between(starts: &[usize], start: usize, end: usize) -> usize {
    starts.partition_point(|&s| s < end) - starts.partition_point(|&s| s < start)
}

fn is_lvalue(expr: &Expr) -> bool {
    matches!(expr, Expr::Var(_) | Expr::Field(_) | Expr::Index(..))
}

impl Interp {
    pub(super) fn call_builtin(&mut self, builtin: Builtin, args: &[Expr]) -> Result<Value, Fault> {
        match builtin {
            Builtin::Length => self.builtin_length(args),
            Builtin::Substr => self.builtin_substr(args),
            Builtin::Index => self.builtin_index(args),
            Builtin::Split => self.builtin_split(args),
            Builtin::Sub => self.builtin_sub(args, false),
            Builtin::Gsub => self.builtin_sub(args, true),
            Builtin::Match => self.builtin_match(args),
            Builtin::Sprintf => self.builtin_sprintf(args),
            Builtin::Sin => self.math(args, f64::sin),
            Builtin::Cos => self.math(args, f64::cos),
            Builtin::Exp => self.math(args, f64::exp),
            Builtin::Log => self.math(args, f64::ln),
            Builtin::Sqrt => self.math(args, f64::sqrt),
            Builtin::Int => self.math(args, f64::trunc),
            Builtin::Atan2 => {
                let y = self.eval(&args[0])?.to_num();
                let x = self.eval(&args[1])?.to_num();
                Ok(Value::Num(y.atan2(x)))
            }
            Builtin::Rand => Ok(Value::Num(self.rng.next_unit())),
            Builtin::Srand => self.builtin_srand(args),
            Builtin::Tolower => self.case_map(args, false),
            Builtin::Toupper => self.case_map(args, true),
            Builtin::Close => {
                let name = self.eval_string(&args[0])?;
                Ok(Value::Num(self.close_stream(&name)))
            }
            Builtin::Fflush => {
                let result = match args.first() {
                    None => self.flush_stream(None),
                    Some(expr) => {
                        let name = self.eval_string(expr)?;
                        self.flush_stream(Some(&name))
                    }
                };
                Ok(Value::Num(result))
            }
            Builtin::System => Err(Fault::Fatal("system() is not supported by agentcmd".into())),
        }
    }

    /// Evaluates `expr` to an owned string value.
    pub(super) fn eval_string(&mut self, expr: &Expr) -> Result<Vec<u8>, Fault> {
        let value = self.eval(expr)?;
        Ok(self.to_str(&value).into_owned())
    }

    /// The regex for a pattern argument: a literal, or the string value compiled at run time.
    pub(super) fn regex_of(&mut self, expr: &Expr) -> Result<std::rc::Rc<Regex>, Fault> {
        match expr {
            Expr::Regex(source) => self.regex.get(source).map_err(Fault::Fatal),
            other => {
                let source = self.eval_string(other)?;
                self.regex.get(&source).map_err(Fault::Fatal)
            }
        }
    }

    fn builtin_length(&mut self, args: &[Expr]) -> Result<Value, Fault> {
        let count = match args.first() {
            None => {
                let text = self.record_text().to_vec();
                char_starts(&text).len() - 1
            }
            Some(Expr::Var(var)) if self.var_is_array(*var) => {
                let array = self.array_of(*var)?;
                let len = array.borrow().len();
                len
            }
            Some(expr) => {
                let text = self.eval_string(expr)?;
                char_starts(&text).len() - 1
            }
        };
        Ok(Value::Num(count as f64))
    }

    fn builtin_substr(&mut self, args: &[Expr]) -> Result<Value, Fault> {
        let text = self.eval_string(&args[0])?;
        let starts = char_starts(&text);
        let len = (starts.len() - 1) as f64;
        let start = self.eval(&args[1])?.to_num().round().max(1.0);
        let end = match args.get(2) {
            Some(expr) => start + self.eval(expr)?.to_num().round(),
            None => len + 1.0,
        };
        let end = end.min(len + 1.0);
        if end <= start || start > len {
            return Ok(Value::Str(Vec::new()));
        }
        let from = starts[start as usize - 1];
        let to = starts[end as usize - 1];
        Ok(Value::Str(text[from..to].to_vec()))
    }

    fn builtin_index(&mut self, args: &[Expr]) -> Result<Value, Fault> {
        let haystack = self.eval_string(&args[0])?;
        let needle = self.eval_string(&args[1])?;
        if needle.is_empty() {
            return Ok(Value::Num(1.0));
        }
        let position = haystack
            .windows(needle.len())
            .position(|window| window == needle.as_slice());
        Ok(Value::Num(match position {
            Some(byte) => char_position(&char_starts(&haystack), byte) as f64,
            None => 0.0,
        }))
    }

    fn builtin_split(&mut self, args: &[Expr]) -> Result<Value, Fault> {
        let text = self.eval_string(&args[0])?;
        let parts: Vec<Vec<u8>> = if text.is_empty() {
            Vec::new()
        } else {
            match args.get(2) {
                Some(Expr::Regex(source)) => {
                    let regex = self.regex.get(source).map_err(Fault::Fatal)?;
                    split_regex(&text, &regex)
                }
                Some(expr) => {
                    let separator = self.eval_string(expr)?;
                    if separator.is_empty() {
                        split_chars(&text)
                    } else {
                        split_fields(&text, &separator, false, &mut self.regex)
                            .map_err(Fault::Fatal)?
                    }
                }
                None => {
                    let separator = self.special_bytes(FS);
                    split_fields(&text, &separator, false, &mut self.regex).map_err(Fault::Fatal)?
                }
            }
        };
        let Expr::Var(var) = &args[1] else {
            return Err(Fault::Fatal(
                "split: second argument must be an array".into(),
            ));
        };
        let array = self.array_of(*var)?;
        let count = parts.len();
        let mut map = array.borrow_mut();
        map.clear();
        for (index, part) in parts.into_iter().enumerate() {
            map.insert(
                (index + 1).to_string().into_bytes(),
                Value::from_input(part),
            );
        }
        Ok(Value::Num(count as f64))
    }

    fn builtin_sub(&mut self, args: &[Expr], global: bool) -> Result<Value, Fault> {
        let regex = self.regex_of(&args[0])?;
        let replacement = self.eval_string(&args[1])?;
        let place = match args.get(2) {
            None => Some(Place::Field(0)),
            Some(expr) if is_lvalue(expr) => Some(self.place(expr)?),
            Some(expr) => {
                self.eval(expr)?;
                None
            }
        };
        let current = match &place {
            Some(place) => {
                let value = self.get_place(place)?;
                self.to_str(&value).into_owned()
            }
            None => Vec::new(),
        };
        let (updated, count) = substitute(&regex, &current, &replacement, global);
        if count > 0 {
            if let Some(place) = place {
                self.set_place(place, Value::Str(updated))?;
            }
        }
        Ok(Value::Num(count as f64))
    }

    fn builtin_match(&mut self, args: &[Expr]) -> Result<Value, Fault> {
        let text = self.eval_string(&args[0])?;
        let regex = self.regex_of(&args[1])?;
        let (start, length) = match regex.find(&text) {
            Some(found) => {
                let starts = char_starts(&text);
                let first = char_position(&starts, found.start()) as f64;
                let count = chars_between(&starts, found.start(), found.end()) as f64;
                (first, count)
            }
            None => (0.0, -1.0),
        };
        self.set_global_value(RSTART, Value::Num(start));
        self.set_global_value(RLENGTH, Value::Num(length));
        Ok(Value::Num(start))
    }

    fn builtin_sprintf(&mut self, args: &[Expr]) -> Result<Value, Fault> {
        let format = self.eval_string(&args[0])?;
        let mut values = Vec::with_capacity(args.len() - 1);
        for expr in &args[1..] {
            values.push(self.eval(expr)?);
        }
        Ok(Value::Str(sprintf(&format, &values, self.convfmt())))
    }

    fn math(&mut self, args: &[Expr], function: fn(f64) -> f64) -> Result<Value, Fault> {
        let x = self.eval(&args[0])?.to_num();
        Ok(Value::Num(function(x)))
    }

    fn builtin_srand(&mut self, args: &[Expr]) -> Result<Value, Fault> {
        let previous = self.rng.seed();
        let seed = match args.first() {
            Some(expr) => self.eval(expr)?.to_num(),
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs() as f64),
        };
        self.rng.reseed(seed);
        Ok(Value::Num(previous))
    }

    fn case_map(&mut self, args: &[Expr], upper: bool) -> Result<Value, Fault> {
        let mut text = self.eval_string(&args[0])?;
        if upper {
            text.make_ascii_uppercase();
        } else {
            text.make_ascii_lowercase();
        }
        Ok(Value::Str(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gsub(pattern: &str, text: &str, replacement: &str, global: bool) -> (String, usize) {
        let regex =
            Regex::new(&crate::commands::awk::regex::translate(pattern.as_bytes()).unwrap())
                .expect("regex");
        let (out, count) = substitute(&regex, text.as_bytes(), replacement.as_bytes(), global);
        (String::from_utf8(out).unwrap(), count)
    }

    #[test]
    fn sub_replaces_only_the_first_match() {
        assert_eq!(gsub("o", "foo", "0", false), ("f0o".to_owned(), 1));
        assert_eq!(gsub("z", "foo", "0", false), ("foo".to_owned(), 0));
    }

    #[test]
    fn gsub_replaces_every_match() {
        assert_eq!(gsub("o", "foo boo", "0", true), ("f00 b00".to_owned(), 4));
    }

    #[test]
    fn ampersand_and_escaped_ampersand_in_replacements() {
        assert_eq!(gsub("b+", "abbc", "[&]", false), ("a[bb]c".to_owned(), 1));
        assert_eq!(gsub("b+", "abbc", "\\&", false), ("a&c".to_owned(), 1));
        assert_eq!(gsub("b", "abc", "\\\\", false), ("a\\c".to_owned(), 1));
    }

    #[test]
    fn empty_matches_follow_gawk_rules() {
        assert_eq!(gsub("x*", "abc", "-", true), ("-a-b-c-".to_owned(), 4));
        assert_eq!(gsub("b*", "abc", "-", true), ("-a-c-".to_owned(), 3));
    }

    #[test]
    fn anchors_only_match_the_whole_string_start() {
        assert_eq!(gsub("^a", "aaa", "b", true), ("baa".to_owned(), 1));
        assert_eq!(gsub("a$", "aaa", "b", true), ("aab".to_owned(), 1));
    }

    #[test]
    fn rng_is_deterministic_for_a_seed() {
        let mut first = Rng::new();
        let mut second = Rng::new();
        first.reseed(42.0);
        second.reseed(42.0);
        let a = first.next_unit();
        assert_eq!(a, second.next_unit());
        assert!((0.0..1.0).contains(&a));
        assert_eq!(first.seed(), 42.0);
    }

    #[test]
    fn character_positions_are_utf8_aware() {
        let starts = char_starts("aéb".as_bytes());
        assert_eq!(char_position(&starts, 3), 3);
        assert_eq!(chars_between(&starts, 1, 3), 1);
    }
}
