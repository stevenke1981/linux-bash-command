use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
#[derive(Clone, Debug, Default)]
pub enum Scalar {
    #[default]
    Uninit,
    Number(f64),
    String(Rc<Vec<u8>>),
    StrNum(Rc<Vec<u8>>, f64),
}
pub type Array = Rc<RefCell<BTreeMap<Vec<u8>, Scalar>>>;
#[derive(Clone, Debug, Default)]
pub enum Slot {
    #[default]
    Unset,
    Scalar(Scalar),
    Array(Array),
}
impl Scalar {
    pub fn string(bytes: Vec<u8>) -> Self {
        Self::String(Rc::new(bytes))
    }
    pub fn input(bytes: Vec<u8>) -> Self {
        let (number, complete) = numeric(&bytes);
        if complete {
            Self::StrNum(Rc::new(bytes), number)
        } else {
            Self::string(bytes)
        }
    }
    pub fn number(&self) -> f64 {
        match self {
            Self::Uninit => 0.0,
            Self::Number(n) | Self::StrNum(_, n) => *n,
            Self::String(s) => numeric(s).0,
        }
    }
    pub fn numeric(&self) -> bool {
        matches!(self, Self::Uninit | Self::Number(_) | Self::StrNum(..))
    }
    pub fn truth(&self) -> bool {
        match self {
            Self::String(s) => !s.is_empty(),
            _ => self.number() != 0.0,
        }
    }
    pub fn bytes(&self, format: &[u8]) -> Result<Vec<u8>, String> {
        Ok(match self {
            Self::Uninit => Vec::new(),
            Self::String(s) | Self::StrNum(s, _) => s.as_ref().clone(),
            Self::Number(n) => {
                if n.is_finite()
                    && n.fract() == 0.0
                    && *n >= i64::MIN as f64
                    && *n < i64::MAX as f64
                {
                    format!("{}", *n as i64).into_bytes()
                } else {
                    super::format::numeric(format, *n)?
                }
            }
        })
    }
    pub fn shared_bytes(&self, format: &[u8]) -> Result<Rc<Vec<u8>>, String> {
        match self {
            Self::String(bytes) | Self::StrNum(bytes, _) => Ok(bytes.clone()),
            _ => Ok(Rc::new(self.bytes(format)?)),
        }
    }
    pub fn compare(&self, other: &Self, format: &[u8]) -> Result<std::cmp::Ordering, String> {
        if self.numeric() && other.numeric() {
            Ok(self
                .number()
                .partial_cmp(&other.number())
                .unwrap_or(std::cmp::Ordering::Equal))
        } else {
            Ok(self.bytes(format)?.cmp(&other.bytes(format)?))
        }
    }
}
pub fn numeric(bytes: &[u8]) -> (f64, bool) {
    let mut at = 0;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    let start = at;
    if matches!(bytes.get(at), Some(b'+' | b'-')) {
        at += 1;
    }
    let digits = at;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let mut has_digits = at > digits;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let fractional = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        has_digits |= at > fractional;
    }
    if !has_digits {
        return (0.0, false);
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let exponent = at;
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let digits = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == digits {
            at = exponent;
        }
    }
    let number = std::str::from_utf8(&bytes[start..at])
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    (number, bytes[at..].iter().all(u8::is_ascii_whitespace))
}
pub fn character_count(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut at = 0;
    while at < bytes.len() {
        let width = match bytes[at] {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        let width =
            if at + width <= bytes.len() && std::str::from_utf8(&bytes[at..at + width]).is_ok() {
                width
            } else {
                1
            };
        at += width;
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_strnum_preserves_spelling() {
        let v = Scalar::input(b"001".to_vec());
        assert!(v.numeric());
        assert_eq!(v.number(), 1.0);
        assert_eq!(v.bytes(b"%.6g").unwrap(), b"001");
    }
    #[test]
    fn numeric_prefix_does_not_make_string_strnum() {
        let v = Scalar::input(b"12tail".to_vec());
        assert!(!v.numeric());
        assert_eq!(v.number(), 12.0);
    }
    #[test]
    fn numeric_comparison() {
        assert_eq!(
            Scalar::input(b"10".to_vec())
                .compare(&Scalar::Number(2.0), b"%.6g")
                .unwrap(),
            std::cmp::Ordering::Greater
        );
    }
    #[test]
    fn string_comparison() {
        assert_eq!(
            Scalar::string(b"10".to_vec())
                .compare(&Scalar::Number(2.0), b"%.6g")
                .unwrap(),
            std::cmp::Ordering::Less
        );
    }
    #[test]
    fn truth_distinguishes_string_zero() {
        assert!(Scalar::string(b"0".to_vec()).truth());
        assert!(!Scalar::input(b"0".to_vec()).truth());
        assert!(!Scalar::Uninit.truth());
    }
    #[test]
    fn floating_conversion() {
        assert_eq!(Scalar::Number(1.2345).bytes(b"%.2f").unwrap(), b"1.23");
        assert_eq!(Scalar::Number(12.0).bytes(b"%.2f").unwrap(), b"12");
    }
    #[test]
    fn malformed_numeric_exponents() {
        assert_eq!(numeric(b"  -1.2e2  "), (-120.0, true));
        assert_eq!(numeric(b"1e+"), (1.0, false));
    }
    #[test]
    fn unicode_and_invalid_characters() {
        assert_eq!(character_count("a\u{732b}".as_bytes()), 2);
        assert_eq!(character_count(b"\xff\xc3("), 3);
    }
}
