//! Output file names: prefix, suffix alphabet and width, following GNU `split`.
//!
//! With an explicit `-a LENGTH` the suffix counts up in its alphabet and fails when all
//! names are used. Without `-a`, the suffix starts at two symbols; once the two-symbol
//! names that do not start with the largest symbol are used up, names continue with that
//! symbol followed by three more symbols.

/// The symbols a suffix is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Alphabet {
    Lower,
    Digits,
    Hex,
}

impl Alphabet {
    fn radix(self) -> u64 {
        match self {
            Self::Lower => 26,
            Self::Digits => 10,
            Self::Hex => 16,
        }
    }

    fn symbol(self, digit: u64) -> char {
        let digit = digit as u8;
        match self {
            Self::Lower => char::from(b'a' + digit),
            Self::Digits => char::from(b'0' + digit),
            Self::Hex if digit < 10 => char::from(b'0' + digit),
            Self::Hex => char::from(b'a' + digit - 10),
        }
    }
}

/// Builds the name of the `index`-th output file.
#[derive(Debug)]
pub(super) struct Namer {
    prefix: String,
    suffix: String,
    alphabet: Alphabet,
    /// Explicit suffix length from `-a`; `None` selects automatic extension.
    length: Option<u32>,
    /// First numeric suffix (`--numeric-suffixes=FROM`).
    start: u64,
}

impl Namer {
    pub(super) fn new(
        prefix: &str,
        suffix: &str,
        alphabet: Alphabet,
        length: Option<u32>,
        start: u64,
    ) -> Self {
        Self {
            prefix: prefix.to_owned(),
            suffix: suffix.to_owned(),
            alphabet,
            length,
            start,
        }
    }

    /// The file name for `index`, or `None` when the suffixes are exhausted.
    pub(super) fn name(&self, index: u64) -> Option<String> {
        let radix = self.alphabet.radix();
        let body = match self.length {
            Some(length) => {
                let value = self.start.checked_add(index)?;
                let capacity = radix.checked_pow(length)?;
                if value >= capacity {
                    return None;
                }
                self.digits(value, length as usize)
            }
            None => {
                let value = self.start.checked_add(index)?;
                let two_symbol = (radix - 1) * radix;
                if value < two_symbol {
                    self.digits(value, 2)
                } else {
                    let rest = value - two_symbol;
                    if rest >= radix * radix * radix {
                        return None;
                    }
                    let mut text = String::new();
                    text.push(self.alphabet.symbol(radix - 1));
                    text.push_str(&self.digits(rest, 3));
                    text
                }
            }
        };
        Some(format!("{}{body}{}", self.prefix, self.suffix))
    }

    /// `value` in the alphabet, most significant symbol first, padded to `width`.
    fn digits(&self, mut value: u64, width: usize) -> String {
        let radix = self.alphabet.radix();
        let mut symbols = Vec::with_capacity(width);
        for _ in 0..width {
            symbols.push(self.alphabet.symbol(value % radix));
            value /= radix;
        }
        symbols.iter().rev().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(length: Option<u32>) -> Namer {
        Namer::new("x", "", Alphabet::Lower, length, 0)
    }

    #[test]
    fn explicit_length_counts_up_and_exhausts() {
        let namer = lower(Some(1));
        assert_eq!(namer.name(0).as_deref(), Some("xa"));
        assert_eq!(namer.name(25).as_deref(), Some("xz"));
        assert_eq!(namer.name(26), None);
        assert_eq!(lower(Some(2)).name(675).as_deref(), Some("xzz"));
        assert_eq!(lower(Some(2)).name(676), None);
    }

    #[test]
    fn automatic_length_skips_the_top_symbol_then_widens() {
        let namer = lower(None);
        assert_eq!(namer.name(649).as_deref(), Some("xyz"));
        assert_eq!(namer.name(650).as_deref(), Some("xzaaa"));
        assert_eq!(namer.name(651).as_deref(), Some("xzaab"));
    }

    #[test]
    fn numeric_suffixes_start_at_from_and_keep_width() {
        let namer = Namer::new("p", ".txt", Alphabet::Digits, Some(3), 5);
        assert_eq!(namer.name(0).as_deref(), Some("p005.txt"));
        assert_eq!(namer.name(1).as_deref(), Some("p006.txt"));
        let auto = Namer::new("p", "", Alphabet::Digits, None, 0);
        assert_eq!(auto.name(90).as_deref(), Some("p9000"));
    }

    #[test]
    fn hex_suffixes_use_lowercase_digits() {
        let namer = Namer::new("x", "", Alphabet::Hex, Some(2), 0);
        assert_eq!(namer.name(10).as_deref(), Some("x0a"));
        assert_eq!(namer.name(255).as_deref(), Some("xff"));
        assert_eq!(namer.name(256), None);
    }
}
