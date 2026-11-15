use super::{
    pattern::{Compiled, Pattern},
    replacement::{self, Part},
};

#[derive(Clone, Debug)]
pub struct Substitution {
    pub pattern: Pattern,
    pub replacement: Vec<Part>,
    pub global: bool,
    pub number: usize,
    pub print: bool,
    pub write: Option<std::path::PathBuf>,
}

impl Substitution {
    pub fn apply(
        &self,
        subject: &mut Vec<u8>,
        previous: &mut Option<Compiled>,
    ) -> Result<bool, String> {
        let regex = self.pattern.resolve(previous)?;
        if self
            .replacement
            .iter()
            .any(|part| matches!(part,Part::Group(n) if *n>=regex.groups()))
        {
            return Err("invalid reference in `s' command's RHS".into());
        }
        let mut result = Vec::new();
        let (mut at, mut copied, mut occurrence, mut changed) = (0, 0, 0, false);
        let mut previous_end = None;
        while let Some(groups) = regex.captures(subject, at)? {
            let (start, end) = groups[0].unwrap();
            let empty_after_match = start == end && previous_end == Some(start);
            if !empty_after_match {
                occurrence += 1;
                if occurrence >= self.number && (self.global || occurrence == self.number) {
                    result.extend_from_slice(&subject[copied..start]);
                    replacement::expand(&self.replacement, subject, &groups, &mut result);
                    copied = end;
                    changed = true;
                    if !self.global {
                        break;
                    }
                }
            }
            previous_end = Some(end);
            if end == start {
                if end == subject.len() {
                    break;
                }
                at = end + 1;
            } else {
                at = end;
            }
        }
        if changed {
            result.extend_from_slice(&subject[copied..]);
            *subject = result;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn replace(pattern: &[u8], text: &[u8], number: usize, global: bool) -> Vec<u8> {
        let s = Substitution {
            pattern: super::super::pattern::compile(pattern, false, false, false).unwrap(),
            replacement: replacement::parse(b"X"),
            number,
            global,
            print: false,
            write: None,
        };
        let mut text = text.to_vec();
        s.apply(&mut text, &mut None).unwrap();
        text
    }
    #[test]
    fn first() {
        assert_eq!(replace(b"a", b"aaa", 1, false), b"Xaa");
    }
    #[test]
    fn global() {
        assert_eq!(replace(b"a", b"aaa", 1, true), b"XXX");
    }
    #[test]
    fn numbered() {
        assert_eq!(replace(b"a", b"aaa", 2, false), b"aXa");
    }
    #[test]
    fn numbered_global() {
        assert_eq!(replace(b"a", b"aaaa", 3, true), b"aaXX");
    }
    #[test]
    fn zero_length() {
        assert_eq!(replace(b"x*", b"ab", 1, true), b"XaXbX");
    }
    #[test]
    fn adjacent_empty() {
        assert_eq!(replace(b"a*", b"ab", 1, true), b"XbX");
    }
    #[test]
    fn absent() {
        assert_eq!(replace(b"z", b"abc", 1, true), b"abc");
    }
}
