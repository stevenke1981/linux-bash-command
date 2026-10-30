use std::cmp::Ordering;

#[derive(Clone, Default, Debug)]
pub(super) struct Modifiers {
    pub blank: bool,
    pub dictionary: bool,
    pub fold: bool,
    pub nonprint: bool,
    pub mode: u8,
    pub reverse: bool,
}
impl Modifiers {
    pub fn set(&mut self, flag: u8) {
        match flag {
            b'b' => self.blank = true,
            b'd' => self.dictionary = true,
            b'f' => self.fold = true,
            b'i' => self.nonprint = true,
            b'r' => self.reverse = true,
            _ => self.mode = flag,
        }
    }
    pub fn compare(&self, a: &[u8], b: &[u8], seed: u64) -> Ordering {
        let a = self.transform(a);
        let b = self.transform(b);
        let value = match self.mode {
            b'n' => numeric(&a, &b),
            b'g' => {
                let (a_class, a_value) = general(&a);
                let (b_class, b_value) = general(&b);
                a_class
                    .cmp(&b_class)
                    .then_with(|| a_value.partial_cmp(&b_value).unwrap_or(Ordering::Equal))
            }
            b'h' => human(&a, &b),
            b'M' => month(&a).cmp(&month(&b)),
            b'V' => version(&a, &b),
            b'R' => hash(&a, seed).cmp(&hash(&b, seed)),
            _ => a.cmp(&b),
        };
        if self.reverse {
            value.reverse()
        } else {
            value
        }
    }
    fn transform(&self, bytes: &[u8]) -> Vec<u8> {
        let bytes = if self.blank {
            bytes.trim_ascii_start()
        } else {
            bytes
        };
        bytes
            .iter()
            .copied()
            .filter(|b| !self.dictionary || b.is_ascii_alphanumeric() || matches!(b, b' ' | b'\t'))
            .filter(|b| !self.nonprint || (32..=126).contains(b))
            .map(|b| if self.fold { b.to_ascii_uppercase() } else { b })
            .collect()
    }
}

fn numeric_parts(bytes: &[u8]) -> (bool, &[u8], &[u8]) {
    let bytes = bytes.trim_ascii_start();
    let negative = bytes.first() == Some(&b'-');
    let bytes = if negative { &bytes[1..] } else { bytes };
    let end = bytes
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(bytes.len());
    let integer = bytes[..end].trim_ascii_start();
    let integer = &integer[integer
        .iter()
        .position(|b| *b != b'0')
        .unwrap_or(integer.len())..];
    let fraction = if bytes.get(end) == Some(&b'.') {
        let f = &bytes[end + 1..];
        let end = f
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(f.len());
        &f[..end]
    } else {
        b""
    };
    let fraction = &fraction[..fraction
        .iter()
        .rposition(|b| *b != b'0')
        .map_or(0, |i| i + 1)];
    (
        negative && (!integer.is_empty() || !fraction.is_empty()),
        integer,
        fraction,
    )
}
pub(super) fn numeric(a: &[u8], b: &[u8]) -> Ordering {
    let (an, ai, af) = numeric_parts(a);
    let (bn, bi, bf) = numeric_parts(b);
    if an != bn {
        return if an {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let cmp = ai
        .len()
        .cmp(&bi.len())
        .then_with(|| ai.cmp(bi))
        .then_with(|| {
            (0..af.len().max(bf.len()))
                .map(|i| {
                    af.get(i)
                        .copied()
                        .unwrap_or(b'0')
                        .cmp(&bf.get(i).copied().unwrap_or(b'0'))
                })
                .find(|o| *o != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        });
    if an {
        cmp.reverse()
    } else {
        cmp
    }
}

fn general(bytes: &[u8]) -> (u8, f64) {
    // C strtod skips ASCII whitespace, including vertical tab. Non-numeric
    // strings form a separate group before NaNs; ordinary numbers follow.
    // GNU 9.4 on the target Ubuntu image orders negative NaNs before positive.
    let bytes = &bytes[bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace() && *b != 11)
        .unwrap_or(bytes.len())..];
    let text = String::from_utf8_lossy(bytes);
    for end in (0..=text.len()).rev().filter(|i| text.is_char_boundary(*i)) {
        if let Ok(n) = text[..end].parse::<f64>() {
            return if n.is_nan() {
                (if n.is_sign_negative() { 1 } else { 2 }, 0.0)
            } else {
                (3, n)
            };
        }
    }
    (0, 0.0)
}

fn human(a: &[u8], b: &[u8]) -> Ordering {
    fn rank(bytes: &[u8]) -> (i32, usize) {
        let s = bytes.trim_ascii_start();
        let negative = s.first() == Some(&b'-');
        let end = s
            .iter()
            .enumerate()
            .skip(usize::from(negative))
            .find(|(_, b)| !b.is_ascii_digit() && **b != b'.')
            .map_or(s.len(), |(i, _)| i);
        let suffix = s.get(end).copied().unwrap_or(0).to_ascii_uppercase();
        let rank = b"KMGTPEZYRQ"
            .iter()
            .position(|b| *b == suffix)
            .map_or(0, |n| n + 1) as i32;
        let zero = numeric(s, b"0") == Ordering::Equal;
        (
            if zero {
                0
            } else if negative {
                -(rank + 1)
            } else {
                rank + 1
            },
            end,
        )
    }
    let (ar, ae) = rank(a);
    let (br, be) = rank(b);
    ar.cmp(&br)
        .then_with(|| numeric(&a.trim_ascii_start()[..ae], &b.trim_ascii_start()[..be]))
}

fn month(bytes: &[u8]) -> usize {
    let b = bytes.trim_ascii_start();
    [
        b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV",
        b"DEC",
    ]
    .iter()
    .position(|m| b.get(..3).is_some_and(|b| b.eq_ignore_ascii_case(*m)))
    .map_or(0, |n| n + 1)
}

pub(super) fn hash(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed ^ 0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

fn version_part(a: &[u8], b: &[u8]) -> Ordering {
    fn order(b: Option<u8>) -> i32 {
        match b {
            Some(b'~') => -1,
            None => 0,
            Some(b) if b.is_ascii_digit() => 0,
            Some(b) if b.is_ascii_alphabetic() => i32::from(b),
            Some(b) => i32::from(b) + 256,
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        while a.get(i).is_some_and(|b| !b.is_ascii_digit())
            || b.get(j).is_some_and(|b| !b.is_ascii_digit())
        {
            let o = order(a.get(i).copied()).cmp(&order(b.get(j).copied()));
            if o != Ordering::Equal {
                return o;
            }
            if i < a.len() {
                i += 1;
            }
            if j < b.len() {
                j += 1;
            }
        }
        while a.get(i) == Some(&b'0') {
            i += 1;
        }
        while b.get(j) == Some(&b'0') {
            j += 1;
        }
        let ai = i;
        let bj = j;
        while a.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        let o = (i - ai)
            .cmp(&(j - bj))
            .then_with(|| a[ai..i].cmp(&b[bj..j]));
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

pub(super) fn version(a: &[u8], b: &[u8]) -> Ordering {
    fn special(s: &[u8]) -> u8 {
        if s.is_empty() {
            0
        } else if s == b"." {
            1
        } else if s == b".." {
            2
        } else if s.starts_with(b".") {
            3
        } else {
            4
        }
    }
    let rank = special(a).cmp(&special(b));
    if rank != Ordering::Equal {
        return rank;
    }
    fn stem(s: &[u8]) -> &[u8] {
        let mut end = s.len();
        while let Some(dot) = s[..end].iter().rposition(|b| *b == b'.') {
            let suffix = &s[dot + 1..end];
            if suffix
                .first()
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'~')
                && suffix
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'~')
            {
                end = dot;
            } else {
                break;
            }
        }
        &s[..end]
    }
    version_part(stem(a), stem(b)).then_with(|| version_part(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_numeric() {
        assert_eq!(
            numeric(b"999999999999999999999", b"1000000000000000000000"),
            Ordering::Less
        );
    }
    #[test]
    fn negative_fractions() {
        assert_eq!(numeric(b"-0.10", b"-.2"), Ordering::Greater);
        assert_eq!(numeric(b"-0", b"junk"), Ordering::Equal);
    }
    #[test]
    fn plus_is_zero() {
        assert_eq!(numeric(b"+9", b"0"), Ordering::Equal);
    }
    #[test]
    fn natural_version() {
        assert_eq!(version(b"x9", b"x10"), Ordering::Less);
        assert_eq!(version(b"x1~rc", b"x1"), Ordering::Less);
    }
    #[test]
    fn version_suffix() {
        assert_eq!(version(b"x1.tar.gz", b"x1.1"), Ordering::Less);
    }
    #[test]
    fn human_suffix() {
        assert_eq!(human(b"1000K", b"1M"), Ordering::Less);
        assert_eq!(human(b"-1M", b"-1000K"), Ordering::Less);
    }
    #[test]
    fn calendar() {
        assert_eq!(month(b" jan"), 1);
        assert_eq!(month(b"other"), 0);
    }
    #[test]
    fn filters() {
        let m = Modifiers {
            dictionary: true,
            fold: true,
            ..Modifiers::default()
        };
        assert_eq!(m.compare(b"a-1", b"A1", 0), Ordering::Equal);
    }
    #[test]
    fn general_signed_zero_is_equal() {
        let m = Modifiers {
            mode: b'g',
            ..Modifiers::default()
        };
        assert_eq!(m.compare(b"0", b"-0", 0), Ordering::Equal);
        assert_eq!(m.compare(b"-0", b"+0", 0), Ordering::Equal);
    }
    #[test]
    fn general_nonnumeric_before_nan_before_numbers() {
        let m = Modifiers {
            mode: b'g',
            ..Modifiers::default()
        };
        assert_eq!(m.compare(b"zebra", b"nan", 0), Ordering::Less);
        assert_eq!(m.compare(b"nan", b"-inf", 0), Ordering::Less);
        assert_eq!(m.compare(b"zebra", b"apple", 0), Ordering::Equal);
    }
    #[test]
    fn general_nan_spellings_are_equal() {
        let m = Modifiers {
            mode: b'g',
            ..Modifiers::default()
        };
        assert_eq!(m.compare(b"nan", b"+nan", 0), Ordering::Equal);
    }
    #[test]
    fn general_nan_negative_before_positive() {
        let m = Modifiers {
            mode: b'g',
            ..Modifiers::default()
        };
        assert_eq!(m.compare(b"-nan", b"nan", 0), Ordering::Less);
        assert_eq!(m.compare(b"zebra", b"-nan", 0), Ordering::Less);
    }
    #[test]
    fn general_ascii_space_and_numeric_prefix() {
        let m = Modifiers {
            mode: b'g',
            ..Modifiers::default()
        };
        assert_eq!(m.compare(b"\x0b\t 2.5e1suffix", b"25", 0), Ordering::Equal);
        assert_eq!(m.compare(b"\xc2\xa01", b"nan", 0), Ordering::Less);
    }
}
