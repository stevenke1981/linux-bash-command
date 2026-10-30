struct BreTranslator<'a> {
    chars: &'a [char],
    pos: usize,
    out: String,
    /// True where a `^` is an anchor (start of pattern, after `\(` or `\|`).
    expr_start: bool,
    /// True where a `*` is literal (start of expression, after an anchor).
    star_literal: bool,
}

/// Translates a POSIX basic regular expression (GNU flavor) to regex syntax.
pub fn bre_to_regex(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut translator = BreTranslator {
        chars: &chars,
        pos: 0,
        out: String::with_capacity(pattern.len() * 2),
        expr_start: true,
        star_literal: true,
    };
    while let Some(&c) = chars.get(translator.pos) {
        translator.pos += 1;
        translator.step(c)?;
    }
    Ok(translator.out)
}

impl BreTranslator<'_> {
    fn step(&mut self, c: char) -> Result<(), String> {
        let expr_start = std::mem::replace(&mut self.expr_start, false);
        let star_literal = std::mem::replace(&mut self.star_literal, false);
        match c {
            '\\' => self.escape(star_literal)?,
            '[' => self.bracket()?,
            '^' if expr_start => {
                self.out.push('^');
                self.star_literal = true;
            }
            '$' if self.at_expression_end() => self.out.push('$'),
            '*' if star_literal => self.out.push_str("\\*"),
            '*' | '.' => self.out.push(c),
            _ => self.literal(c),
        }
        Ok(())
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn literal(&mut self, c: char) {
        self.out.push_str(&regex::escape(&c.to_string()));
    }

    fn at_expression_end(&self) -> bool {
        let rest = &self.chars[self.pos..];
        rest.is_empty() || rest.starts_with(&['\\', ')']) || rest.starts_with(&['\\', '|'])
    }

    fn escape(&mut self, star_literal: bool) -> Result<(), String> {
        let Some(next) = self.peek() else {
            return Err("trailing backslash".into());
        };
        self.pos += 1;
        match next {
            '(' | '|' => {
                self.out.push(next);
                self.expr_start = true;
                self.star_literal = true;
            }
            ')' => self.out.push(')'),
            '{' => self.interval()?,
            '+' | '?' if star_literal => self.literal(next),
            '+' | '?' => self.out.push(next),
            '<' | '>' => self.word_edge(next)?,
            'w' | 'W' | 's' | 'S' | 'b' | 'B' => {
                self.out.push('\\');
                self.out.push(next);
            }
            '`' => self.out.push_str("\\A"),
            '\'' => self.out.push_str("\\z"),
            '1'..='9' => return Err("back-references are not supported".into()),
            _ => self.literal(next),
        }
        Ok(())
    }

    /// `\<` and `\>` are only expressible as `\b` next to a word character,
    /// where the two are equivalent. Other placements are rejected.
    fn word_edge(&mut self, edge: char) -> Result<(), String> {
        if edge == '<' {
            if matches!(self.peek(), Some(c) if is_word_char(c)) {
                self.out.push_str("\\b");
                return Ok(());
            }
            return Err("\\< is supported only directly before a word character".into());
        }
        if matches!(self.out.chars().last(), Some(c) if is_word_char(c)) {
            self.out.push_str("\\b");
            return Ok(());
        }
        Err("\\> is supported only directly after a word character".into())
    }

    fn interval(&mut self) -> Result<(), String> {
        let rest = &self.chars[self.pos..];
        let end = rest
            .windows(2)
            .position(|pair| pair[0] == '\\' && pair[1] == '}')
            .ok_or_else(|| "unmatched \\{".to_string())?;
        let body: String = rest[..end].iter().collect();
        if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit() || c == ',') {
            return Err(format!("invalid interval \\{{{body}\\}}"));
        }
        self.out.push('{');
        if body.starts_with(',') {
            self.out.push('0');
        }
        self.out.push_str(&body);
        self.out.push('}');
        self.pos += end + 2;
        Ok(())
    }

    /// Copies a bracket expression. Inside POSIX brackets a backslash is an
    /// ordinary character, so it is escaped for the regex crate.
    fn bracket(&mut self) -> Result<(), String> {
        let chars = self.chars;
        let mut class = String::from("[");
        if self.peek() == Some('^') {
            class.push('^');
            self.pos += 1;
        }
        if self.peek() == Some(']') {
            class.push_str("\\]");
            self.pos += 1;
        }
        loop {
            let Some(c) = self.peek() else {
                return Err("unmatched [".into());
            };
            self.pos += 1;
            match c {
                ']' => {
                    class.push(']');
                    self.out.push_str(&class);
                    return Ok(());
                }
                '[' if self.peek() == Some(':') => {
                    let rest = &chars[self.pos..];
                    let end = rest[1..]
                        .windows(2)
                        .position(|pair| pair[0] == ':' && pair[1] == ']')
                        .map(|index| index + 3)
                        .ok_or_else(|| "unmatched [".to_string())?;
                    let stop = self.pos + end;
                    class.extend(&chars[self.pos - 1..stop]);
                    self.pos = stop;
                }
                '[' if matches!(self.peek(), Some('.' | '=')) => {
                    return Err(
                        "collating symbols and equivalence classes are not supported".into(),
                    );
                }
                '[' => class.push_str("\\["),
                '\\' => class.push_str("\\\\"),
                '&' | '~' => {
                    class.push('\\');
                    class.push(c);
                }
                _ => class.push(c),
            }
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    #[test]
    fn converts_basic_operators() {
        assert_eq!(super::bre_to_regex(r"a\|b").unwrap(), "a|b");
        assert!(super::bre_to_regex("[").is_err());
    }
}
