use super::pattern::{Compiled, Pattern};

#[derive(Clone, Debug)]
pub enum Address {
    Number(u64),
    Last,
    Regex(Pattern),
    Step(u64, u64),
}
#[derive(Clone, Debug)]
pub enum End {
    Address(Address),
    Plus(u64),
    Multiple(u64),
}
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub first: Option<Address>,
    pub end: Option<End>,
    pub negate: bool,
    active: bool,
    start: u64,
    finished: bool,
}

impl Address {
    fn matches(
        &self,
        line: u64,
        last: bool,
        text: &[u8],
        previous: &mut Option<Compiled>,
    ) -> Result<bool, String> {
        Ok(match self {
            Self::Number(n) => line == *n,
            Self::Last => last,
            Self::Regex(pattern) => pattern.resolve(previous)?.is_match(text),
            Self::Step(first, step) => {
                if *step == 0 {
                    line == *first
                } else {
                    line >= *first && (line - *first).is_multiple_of(*step)
                }
            }
        })
    }
}

impl Selection {
    pub fn needs_last(&self) -> bool {
        matches!(self.first, Some(Address::Last))
            || matches!(self.end, Some(End::Address(Address::Last)))
    }
    pub fn new(first: Option<Address>, end: Option<End>, negate: bool) -> Self {
        Self {
            first,
            end,
            negate,
            ..Self::default()
        }
    }
    pub fn reset(&mut self) {
        self.active = false;
        self.start = 0;
        self.finished = false;
    }
    pub fn select(
        &mut self,
        line: u64,
        last: bool,
        text: &[u8],
        previous: &mut Option<Compiled>,
    ) -> Result<(bool, bool), String> {
        let Some(first) = &self.first else {
            return Ok((!self.negate, true));
        };
        if self.end.is_none() {
            return Ok((
                first.matches(line, last, text, previous)? != self.negate,
                true,
            ));
        }
        let mut started = false;
        if !self.active
            && !self.finished
            && (matches!(first, Address::Number(0)) || first.matches(line, last, text, previous)?)
        {
            self.active = true;
            self.start = line;
            started = true;
        }
        let selected = self.active;
        let ending = if self.active {
            match self.end.as_ref().unwrap() {
                End::Plus(n) => line.saturating_sub(self.start) >= *n,
                End::Multiple(n) => *n == 0 || (line > self.start && line.is_multiple_of(*n)),
                End::Address(Address::Number(n)) => line >= *n,
                End::Address(Address::Regex(_))
                    if started && !matches!(first, Address::Number(0)) =>
                {
                    false
                }
                End::Address(address) => address.matches(line, last, text, previous)?,
            }
        } else {
            false
        };
        if ending {
            self.active = false;
            self.finished = matches!(first, Address::Number(_));
        }
        Ok((selected != self.negate, ending || self.negate || last))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chosen(mut selection: Selection) -> Vec<u64> {
        (1..=8)
            .filter(|&n| selection.select(n, n == 8, b"a", &mut None).unwrap().0)
            .collect()
    }
    #[test]
    fn exact() {
        assert_eq!(
            chosen(Selection::new(Some(Address::Number(3)), None, false)),
            [3]
        );
    }
    #[test]
    fn last() {
        assert_eq!(
            chosen(Selection::new(Some(Address::Last), None, false)),
            [8]
        );
    }
    #[test]
    fn step() {
        assert_eq!(
            chosen(Selection::new(Some(Address::Step(2, 3)), None, false)),
            [2, 5, 8]
        );
    }
    #[test]
    fn numeric_range() {
        assert_eq!(
            chosen(Selection::new(
                Some(Address::Number(2)),
                Some(End::Address(Address::Number(4))),
                false
            )),
            [2, 3, 4]
        );
    }
    #[test]
    fn plus_range() {
        assert_eq!(
            chosen(Selection::new(
                Some(Address::Number(3)),
                Some(End::Plus(2)),
                false
            )),
            [3, 4, 5]
        );
    }
    #[test]
    fn multiple_range() {
        assert_eq!(
            chosen(Selection::new(
                Some(Address::Number(3)),
                Some(End::Multiple(3)),
                false
            )),
            [3, 4, 5, 6]
        );
    }
    #[test]
    fn inverse() {
        assert_eq!(
            chosen(Selection::new(
                Some(Address::Number(3)),
                Some(End::Plus(2)),
                true
            )),
            [1, 2, 6, 7, 8]
        );
    }
}
