use super::compare::Modifiers;

#[derive(Clone, Debug)]
pub(super) struct Key {
    pub field: usize,
    pub column: usize,
    pub end: Option<(usize, usize)>,
    pub modifiers: Modifiers,
    start_blank: bool,
    end_blank: bool,
}

impl Key {
    pub fn parse(text: &str, global: &Modifiers) -> Result<Self, String> {
        fn part(s: &str, end: bool) -> Result<(usize, usize, Modifiers, bool, bool), String> {
            let n = s
                .find(|c: char| !c.is_ascii_digit() && c != '.')
                .unwrap_or(s.len());
            let (field, column) = s[..n]
                .split_once('.')
                .unwrap_or((&s[..n], if end { "0" } else { "1" }));
            let field: usize = field
                .parse()
                .map_err(|_| format!("invalid field specification '{s}'"))?;
            let column: usize = column
                .parse()
                .map_err(|_| format!("invalid field specification '{s}'"))?;
            if field == 0 || (!end && column == 0) {
                return Err(format!(
                    "field number is zero: invalid field specification '{s}'"
                ));
            }
            let mut mods = Modifiers::default();
            let mut any = false;
            for b in s[n..].bytes() {
                if !b"bdfgiMhnRrV".contains(&b) {
                    return Err(format!("invalid field specification '{s}'"));
                }
                mods.set(b);
                any = true;
            }
            let blank = mods.blank;
            Ok((field - 1, column, mods, any, blank))
        }
        let (start, end) = text
            .split_once(',')
            .map_or((text, None), |(a, b)| (a, Some(b)));
        let (field, column, mut modifiers, mut own, start_blank) = part(start, false)?;
        let mut end_blank = false;
        let end = if let Some(end) = end {
            let (f, c, m, a, b) = part(end, true)?;
            if a {
                own = true;
                for flag in b"bdfgiMhnRrV" {
                    let set = match flag {
                        b'b' => m.blank,
                        b'd' => m.dictionary,
                        b'f' => m.fold,
                        b'i' => m.nonprint,
                        b'r' => m.reverse,
                        _ => m.mode == *flag,
                    };
                    if set {
                        modifiers.set(*flag);
                    }
                }
            }
            end_blank = b;
            Some((f, c))
        } else {
            None
        };
        if !own {
            modifiers = global.clone();
        }
        modifiers.blank = false;
        Ok(Self {
            field,
            column,
            end,
            modifiers,
            start_blank: start_blank || (!own && global.blank),
            end_blank: end_blank || (!own && global.blank),
        })
    }
    pub fn slice<'a>(&self, line: &'a [u8], separator: Option<u8>) -> &'a [u8] {
        fn field(line: &[u8], index: usize, sep: Option<u8>) -> (usize, usize) {
            let mut at = 0;
            for n in 0..=index {
                let start = at;
                if let Some(sep) = sep {
                    while at < line.len() && line[at] != sep {
                        at += 1;
                    }
                } else {
                    while at < line.len() && matches!(line[at], b' ' | b'\t') {
                        at += 1;
                    }
                    while at < line.len() && !matches!(line[at], b' ' | b'\t') {
                        at += 1;
                    }
                }
                if n == index {
                    return (start, at);
                }
                if sep.is_some() && at < line.len() {
                    at += 1;
                }
            }
            (line.len(), line.len())
        }
        let (mut start, limit) = field(line, self.field, separator);
        if self.start_blank {
            while start < limit && matches!(line[start], b' ' | b'\t') {
                start += 1;
            }
        }
        start = start.saturating_add(self.column - 1).min(line.len());
        let end = if let Some((f, c)) = self.end {
            let (mut at, limit) = field(line, f, separator);
            if c == 0 {
                limit
            } else {
                if self.end_blank {
                    while at < limit && matches!(line[at], b' ' | b'\t') {
                        at += 1;
                    }
                }
                at.saturating_add(c).min(line.len())
            }
        } else {
            line.len()
        };
        &line[start..end.max(start)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_separator() {
        let k = Key::parse("2,2", &Modifiers::default()).unwrap();
        assert_eq!(k.slice(b"a,b,c", Some(b',')), b"b");
    }
    #[test]
    fn blanks_preserved() {
        let k = Key::parse("2,2", &Modifiers::default()).unwrap();
        assert_eq!(k.slice(b"a   b c", None), b"   b");
    }
    #[test]
    fn blank_modifier() {
        let k = Key::parse("2b,2", &Modifiers::default()).unwrap();
        assert_eq!(k.slice(b"a   b c", None), b"b");
    }
    #[test]
    fn character_positions() {
        let k = Key::parse("1.2,1.3", &Modifiers::default()).unwrap();
        assert_eq!(k.slice(b"abcd", None), b"bc");
    }
    #[test]
    fn invalid_zero() {
        assert!(Key::parse("0", &Modifiers::default()).is_err());
        assert!(Key::parse("1.0", &Modifiers::default()).is_err());
    }
}
