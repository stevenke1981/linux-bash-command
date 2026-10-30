//! `env -S` splitting: turns the option string into words the way GNU `env` does
//! (quotes, backslash escapes, `${NAME}` expansion, `\c` to stop).

use std::ffi::OsStr;

use super::Environment;

/// Splits the `-S` argument into words, following GNU `env -S`: whitespace separates,
/// quotes group, backslash escapes apply, `${NAME}` expands and `\c` stops.
pub(super) fn split_string(text: &str, env: &Environment) -> Result<Vec<String>, String> {
    let mut scanner = SplitScanner {
        chars: text.chars(),
        env,
        words: Vec::new(),
        current: String::new(),
        in_word: false,
    };
    scanner.run()?;
    Ok(scanner.words)
}

/// What one backslash escape in `-S` produces.
enum Escape {
    Char(char),
    /// `\_` outside quotes ends the current word (GNU).
    Separator,
    /// `\c` ends the whole string.
    Stop,
}

struct SplitScanner<'a, 'e> {
    chars: std::str::Chars<'a>,
    env: &'e Environment,
    words: Vec<String>,
    current: String,
    in_word: bool,
}

impl SplitScanner<'_, '_> {
    /// Runs to the end of the string or a `\c`.
    fn run(&mut self) -> Result<(), String> {
        while let Some(c) = self.chars.next() {
            let keep_going = match c {
                ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c' => {
                    self.end_word();
                    true
                }
                '#' if !self.in_word => false,
                '\'' => {
                    self.in_word = true;
                    self.single_quoted()?
                }
                '"' => {
                    self.in_word = true;
                    self.double_quoted()?
                }
                '\\' => match self.escape()? {
                    Escape::Char(ch) => {
                        self.in_word = true;
                        self.current.push(ch);
                        true
                    }
                    Escape::Separator => {
                        self.end_word();
                        true
                    }
                    Escape::Stop => false,
                },
                '$' => {
                    self.in_word = true;
                    self.variable()?;
                    true
                }
                other => {
                    self.in_word = true;
                    self.current.push(other);
                    true
                }
            };
            if !keep_going {
                break;
            }
        }
        self.end_word();
        Ok(())
    }

    fn end_word(&mut self) {
        if self.in_word {
            self.words.push(std::mem::take(&mut self.current));
            self.in_word = false;
        }
    }

    /// Returns false when processing must stop (after `\c` or a comment).
    fn single_quoted(&mut self) -> Result<bool, String> {
        loop {
            match self.chars.next() {
                Some('\'') => return Ok(true),
                Some(c) => self.current.push(c),
                None => return Err("no terminating quote in -S string".to_owned()),
            }
        }
    }

    fn double_quoted(&mut self) -> Result<bool, String> {
        loop {
            match self.chars.next() {
                Some('"') => return Ok(true),
                Some('\\') => match self.escape()? {
                    Escape::Char(ch) => self.current.push(ch),
                    Escape::Separator => self.current.push(' '),
                    Escape::Stop => return Ok(false),
                },
                Some('$') => self.variable()?,
                Some(c) => self.current.push(c),
                None => return Err("no terminating quote in -S string".to_owned()),
            }
        }
    }

    /// Handles a backslash escape (the backslash is already consumed).
    fn escape(&mut self) -> Result<Escape, String> {
        let Some(c) = self.chars.next() else {
            return Err("invalid backslash at end of string in -S".to_owned());
        };
        Ok(match c {
            '_' => Escape::Separator,
            'c' => Escape::Stop,
            'n' => Escape::Char('\n'),
            't' => Escape::Char('\t'),
            'r' => Escape::Char('\r'),
            'f' => Escape::Char('\x0c'),
            'v' => Escape::Char('\x0b'),
            '\\' | '"' | '\'' | '$' | '#' | ' ' => Escape::Char(c),
            other => return Err(format!("invalid sequence '\\{other}' in -S")),
        })
    }

    /// Expands `${NAME}`; the `$` is already consumed.
    fn variable(&mut self) -> Result<(), String> {
        if self.chars.next() != Some('{') {
            return Err("only ${VARNAME} expansion is supported in -S".to_owned());
        }
        let mut name = String::new();
        loop {
            match self.chars.next() {
                Some('}') => break,
                Some(c) if c.is_ascii_alphanumeric() || c == '_' => name.push(c),
                _ => return Err("only ${VARNAME} expansion is supported in -S".to_owned()),
            }
        }
        if let Some(value) = self.env.get(OsStr::new(&name)) {
            self.current.push_str(&value.to_string_lossy());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with(pairs: &[(&str, &str)]) -> Environment {
        Environment {
            pairs: pairs
                .iter()
                .map(|(k, v)| (std::ffi::OsString::from(k), std::ffi::OsString::from(v)))
                .collect(),
        }
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_string_handles_quotes_and_escapes() {
        let env = env_with(&[("NAME", "value")]);
        let split = |text: &str| split_string(text, &env);
        assert_eq!(split("a  b\tc").unwrap(), words(&["a", "b", "c"]));
        assert_eq!(split("'x y' \"z w\"").unwrap(), words(&["x y", "z w"]));
        assert_eq!(split(r"a\_b").unwrap(), words(&["a", "b"]));
        assert_eq!(split(r#""a\_b""#).unwrap(), words(&["a b"]));
        assert_eq!(split(r"a\nb").unwrap(), words(&["a\nb"]));
        assert_eq!(split("${NAME}!").unwrap(), words(&["value!"]));
        assert_eq!(split("${MISSING}x").unwrap(), words(&["x"]));
    }

    #[test]
    fn split_string_stops_at_backslash_c_and_comments() {
        let env = env_with(&[]);
        assert_eq!(split_string(r"a b\c c", &env).unwrap(), words(&["a", "b"]));
        assert_eq!(split_string("a # b", &env).unwrap(), words(&["a"]));
    }

    #[test]
    fn split_string_reports_bad_input() {
        let env = env_with(&[]);
        assert!(split_string("'open", &env).is_err());
        assert!(split_string(r"bad\q", &env).is_err());
        assert!(split_string("$HOME", &env).is_err());
        assert!(split_string("trailing\\", &env).is_err());
    }
}
