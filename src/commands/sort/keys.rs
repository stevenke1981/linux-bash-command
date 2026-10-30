//! Sort keys: `-k POS1[,POS2]` parsing, GNU field and character boundaries, and the
//! inheritance of global ordering options by keys that carry none of their own.

/// Ordering modifiers, either global (`-n`) or attached to one key (`-k1,1nr`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    /// `b` written in the start position: skip leading blanks before the start character.
    pub skip_start_blanks: bool,
    /// `b` written in the end position: skip leading blanks before the end character.
    pub skip_end_blanks: bool,
    pub dictionary: bool,
    pub fold: bool,
    pub nonprinting: bool,
    pub general: bool,
    pub human: bool,
    pub month: bool,
    pub numeric: bool,
    pub random: bool,
    pub reverse: bool,
    pub version: bool,
}

impl Modes {
    /// Sets the modifier `letter` (one of `bdfgiMhnRrV`). `start` and `end` say where the
    /// letter was written; `b` affects only the positions it appears in. Returns `false`
    /// for any other letter.
    pub fn set(&mut self, letter: u8, start: bool, end: bool) -> bool {
        match letter {
            b'b' => {
                self.skip_start_blanks |= start;
                self.skip_end_blanks |= end;
            }
            b'd' => self.dictionary = true,
            b'f' => self.fold = true,
            b'i' => self.nonprinting = true,
            b'g' => self.general = true,
            b'h' => self.human = true,
            b'M' => self.month = true,
            b'n' => self.numeric = true,
            b'R' => self.random = true,
            b'r' => self.reverse = true,
            b'V' => self.version = true,
            _ => return false,
        }
        true
    }

    /// True when the key sets any modifier other than `r` (GNU `default_key_compare`).
    pub fn has_own_ordering(&self) -> bool {
        self.skip_start_blanks
            || self.skip_end_blanks
            || self.dictionary
            || self.fold
            || self.nonprinting
            || self.general
            || self.human
            || self.month
            || self.numeric
            || self.random
            || self.version
    }

    /// The letters of the modes set on this key, in GNU's order for diagnostics.
    pub fn conflict_letters(&self) -> String {
        let table = [
            (self.dictionary, 'd'),
            (self.general, 'g'),
            (self.human, 'h'),
            (self.nonprinting, 'i'),
            (self.month, 'M'),
            (self.numeric, 'n'),
            (self.random, 'R'),
            (self.version, 'V'),
        ];
        table
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, c)| *c)
            .collect()
    }

    /// Whether the modes on one key may not be combined. The numeric family (`n g h M`)
    /// excludes each other and also `d`, `i`, `R` and `V`; `d`, `i`, `R`, `V` combine freely.
    pub fn is_incompatible(&self) -> bool {
        let family = [self.numeric, self.general, self.human, self.month]
            .iter()
            .filter(|on| **on)
            .count();
        let others = [self.dictionary, self.nonprinting, self.random, self.version];
        family >= 2 || (family == 1 && others.iter().any(|on| *on))
    }
}

/// A key as written: the zero-based start field and character, and the end position
/// (zero-based field, one-based character where zero means the whole field), with its
/// own modifiers. `end == None` means the key runs to the end of the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyDef {
    pub start_field: usize,
    pub start_char: usize,
    pub end: Option<(usize, usize)>,
    pub modes: Modes,
}

/// A key after global options were inherited, ready for comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub def: KeyDef,
    pub modes: Modes,
}

/// Parses one `-k` argument (`F[.C][OPTS][,F[.C][OPTS]]`) with GNU's messages.
pub fn parse_key(spec: &str) -> Result<KeyDef, String> {
    let bytes = spec.as_bytes();
    let bad = |msg: &str| format!("{msg}: invalid field specification '{spec}'");
    let (field, mut rest) = parse_count(bytes, "invalid number at field start")?;
    if field == 0 {
        return Err(bad("field number is zero"));
    }
    let mut key = KeyDef {
        start_field: field - 1,
        start_char: 0,
        end: None,
        modes: Modes::default(),
    };
    if let [b'.', tail @ ..] = rest {
        let (chars, after) = parse_count(tail, "invalid number after '.'")?;
        if chars == 0 {
            return Err(bad("character offset is zero"));
        }
        key.start_char = chars - 1;
        rest = after;
    }
    rest = parse_modes(rest, &mut key.modes, true, false);
    if let [b',', tail @ ..] = rest {
        let (end_field, after) = parse_count(tail, "invalid number after ','")?;
        if end_field == 0 {
            return Err(bad("field number is zero"));
        }
        rest = after;
        let mut end_char = 0;
        if let [b'.', tail @ ..] = rest {
            let (chars, after) = parse_count(tail, "invalid number after '.'")?;
            end_char = chars;
            rest = after;
        }
        rest = parse_modes(rest, &mut key.modes, false, true);
        key.end = Some((end_field - 1, end_char));
    }
    if !rest.is_empty() {
        return Err(bad("stray character in field spec"));
    }
    Ok(key)
}

fn parse_count<'a>(bytes: &'a [u8], msg: &str) -> Result<(usize, &'a [u8]), String> {
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        let rest = String::from_utf8_lossy(bytes);
        return Err(format!("{msg}: invalid count at start of '{rest}'"));
    }
    let text = std::str::from_utf8(&bytes[..digits]).unwrap_or("0");
    let value = text.parse::<usize>().unwrap_or(usize::MAX);
    Ok((value, &bytes[digits..]))
}

fn parse_modes<'a>(bytes: &'a [u8], modes: &mut Modes, start: bool, end: bool) -> &'a [u8] {
    let mut index = 0;
    while index < bytes.len() && modes.set(bytes[index], start, end) {
        index += 1;
    }
    &bytes[index..]
}

/// Gives every key that has no ordering modifiers of its own (and no `r`) the global
/// modifiers. A key with its own modifiers, or with `r`, keeps only what it wrote.
pub fn resolve_keys(defs: &[KeyDef], global: Modes) -> Vec<Key> {
    defs.iter()
        .map(|def| {
            let inherit = !def.modes.has_own_ordering() && !def.modes.reverse;
            let modes = if inherit { global } else { def.modes };
            Key {
                def: def.clone(),
                modes,
            }
        })
        .collect()
}

fn is_blank(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// Moves forward over `count` fields starting at `ptr`. Without a separator, a field is
/// a run of blanks followed by a run of non-blanks; with one, fields end at the separator.
/// `consume_last` says whether the separator after the final field is also passed over.
fn advance_fields(
    line: &[u8],
    mut ptr: usize,
    count: usize,
    tab: Option<u8>,
    consume_last: bool,
) -> usize {
    let lim = line.len();
    for index in 0..count {
        if ptr >= lim {
            break;
        }
        match tab {
            Some(sep) => {
                while ptr < lim && line[ptr] != sep {
                    ptr += 1;
                }
                if ptr < lim && (index + 1 < count || consume_last) {
                    ptr += 1;
                }
            }
            None => {
                while ptr < lim && is_blank(line[ptr]) {
                    ptr += 1;
                }
                while ptr < lim && !is_blank(line[ptr]) {
                    ptr += 1;
                }
            }
        }
    }
    ptr
}

/// The byte range of `line` that `key` compares (GNU `begfield` and `limfield`).
pub fn key_span(line: &[u8], key: &Key, tab: Option<u8>) -> (usize, usize) {
    let lim = line.len();
    let def = &key.def;
    let mut start = advance_fields(line, 0, def.start_field, tab, true);
    if key.modes.skip_start_blanks {
        while start < lim && is_blank(line[start]) {
            start += 1;
        }
    }
    start = (start + def.start_char).min(lim);
    let end = match def.end {
        None => lim,
        Some((field, chars)) => {
            let count = if chars == 0 { field + 1 } else { field };
            let mut end = advance_fields(line, 0, count, tab, chars != 0);
            if chars != 0 {
                if key.modes.skip_end_blanks {
                    while end < lim && is_blank(line[end]) {
                        end += 1;
                    }
                }
                end = (end + chars).min(lim);
            }
            end
        }
    };
    (start, end.max(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(spec: &str, global: Modes) -> Key {
        let def = parse_key(spec).unwrap();
        resolve_keys(&[def], global).remove(0)
    }

    fn span_text(line: &str, spec: &str) -> String {
        let k = key(spec, Modes::default());
        let (s, e) = key_span(line.as_bytes(), &k, None);
        String::from_utf8_lossy(&line.as_bytes()[s..e]).into_owned()
    }

    #[test]
    fn parses_positions_and_modes() {
        let k = parse_key("2.3,4.5nr").unwrap();
        assert_eq!(k.start_field, 1);
        assert_eq!(k.start_char, 2);
        assert_eq!(k.end, Some((3, 5)));
        assert!(k.modes.numeric && k.modes.reverse);
        let b = parse_key("1b,1b").unwrap();
        assert!(b.modes.skip_start_blanks && b.modes.skip_end_blanks);
    }

    #[test]
    fn reports_gnu_messages() {
        assert_eq!(
            parse_key("0").unwrap_err(),
            "field number is zero: invalid field specification '0'"
        );
        assert_eq!(
            parse_key("1.0").unwrap_err(),
            "character offset is zero: invalid field specification '1.0'"
        );
        assert_eq!(
            parse_key("1,x").unwrap_err(),
            "invalid number after ',': invalid count at start of 'x'"
        );
        assert!(parse_key("1x").unwrap_err().starts_with("stray character"));
    }

    #[test]
    fn whole_line_and_single_field_spans() {
        assert_eq!(span_text("  alpha beta", "1"), "  alpha beta");
        assert_eq!(span_text("  alpha beta", "2,2"), " beta");
        assert_eq!(span_text("  alpha beta", "2.2,2.3"), "be");
    }

    #[test]
    fn tab_separator_limits_fields() {
        let k = key("2,2", Modes::default());
        let (s, e) = key_span(b"a,bb,c", &k, Some(b','));
        assert_eq!(&b"a,bb,c"[s..e], b"bb");
        let whole = key("2", Modes::default());
        let (s, e) = key_span(b"a,bb,c", &whole, Some(b','));
        assert_eq!(&b"a,bb,c"[s..e], b"bb,c");
    }

    #[test]
    fn global_modes_inherit_only_when_key_is_plain() {
        let global = Modes {
            numeric: true,
            ..Modes::default()
        };
        assert!(key("1,1", global).modes.numeric);
        assert!(!key("1,1r", global).modes.numeric);
        assert!(!key("1,1f", global).modes.numeric);
    }

    #[test]
    fn detects_incompatible_modes() {
        let m = Modes {
            numeric: true,
            month: true,
            ..Modes::default()
        };
        assert!(m.is_incompatible());
        assert_eq!(m.conflict_letters(), "Mn");
        let ok = Modes {
            dictionary: true,
            version: true,
            ..Modes::default()
        };
        assert!(!ok.is_incompatible());
    }
}
