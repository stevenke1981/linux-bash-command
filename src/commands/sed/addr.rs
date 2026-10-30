//! Addresses and ranges: which input lines a command applies to.

use std::rc::Rc;

use ::regex::bytes::Regex;

use super::pattern::{resolve, Pattern};

/// The first address of a command.
#[derive(Clone, Debug)]
pub enum Addr {
    /// `N`. Line 0 is only valid as `0,/re/`.
    Line(u64),
    /// `$`, the last line of input (of each file with `-s` or `-i`).
    Last,
    /// `/re/`.
    Re(Pattern),
    /// `first~step`.
    Step(u64, u64),
}

/// The second address of a range.
#[derive(Clone, Debug)]
pub enum Addr2 {
    Line(u64),
    Last,
    Re(Pattern),
    /// `addr1,+N`: the next N lines.
    Plus(u64),
    /// `addr1,~N`: up to the next line whose number is a multiple of N.
    Tilde(u64),
}

/// A command's selector: optional addresses and the `!` negation.
#[derive(Clone, Debug, Default)]
pub struct Selector {
    pub start: Option<Addr>,
    pub end: Option<Addr2>,
    pub negate: bool,
}

/// Runtime state of a range (`addr1,addr2`) for one command.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub active: bool,
    /// Last line of the range when the end is a line number (or `+N`/`~N`).
    pub end: u64,
}

/// What the matcher needs to know about the current line.
pub struct Env<'a> {
    pub line: u64,
    pub space: &'a [u8],
    pub last_re: &'a mut Option<Rc<Regex>>,
    pub is_last: &'a mut dyn FnMut() -> bool,
}

impl Range {
    /// The state a command starts in: `0,/re/` is active before line 1.
    pub fn initial(selector: &Selector) -> Self {
        Range {
            active: matches!(selector.start, Some(Addr::Line(0))),
            end: 0,
        }
    }
}

/// Decides whether the selector picks the current line, updating `range`.
pub fn selects(selector: &Selector, range: &mut Range, env: &mut Env) -> Result<bool, String> {
    let hit = match (&selector.start, &selector.end) {
        (None, _) => true,
        (Some(start), None) => single(start, env)?,
        (Some(start), Some(end)) => in_range(start, end, range, env)?,
    };
    Ok(hit != selector.negate)
}

fn single(addr: &Addr, env: &mut Env) -> Result<bool, String> {
    Ok(match addr {
        Addr::Line(n) => env.line == *n,
        Addr::Last => (env.is_last)(),
        Addr::Re(pattern) => matches_regex(pattern, env)?,
        Addr::Step(first, step) => {
            if *step == 0 {
                env.line == *first
            } else {
                env.line >= *first && (env.line - first).is_multiple_of(*step)
            }
        }
    })
}

fn matches_regex(pattern: &Pattern, env: &mut Env) -> Result<bool, String> {
    let regex = resolve(pattern, env.last_re)?;
    Ok(regex.is_match(env.space))
}

/// Range matching in the GNU style: an end line already passed closes the range
/// without matching; a start address whose end is at or before it matches one line.
fn in_range(start: &Addr, end: &Addr2, range: &mut Range, env: &mut Env) -> Result<bool, String> {
    let line = env.line;
    if range.active {
        return match end {
            Addr2::Line(_) | Addr2::Plus(_) | Addr2::Tilde(_) => {
                if line >= range.end {
                    range.active = false;
                }
                Ok(line <= range.end)
            }
            Addr2::Last => {
                if (env.is_last)() {
                    range.active = false;
                }
                Ok(true)
            }
            Addr2::Re(pattern) => {
                if matches_regex(pattern, env)? {
                    range.active = false;
                }
                Ok(true)
            }
        };
    }
    if !single(start, env)? {
        return Ok(false);
    }
    match end {
        Addr2::Line(n) => {
            if *n > line {
                *range = Range {
                    active: true,
                    end: *n,
                };
            }
        }
        Addr2::Plus(n) => {
            if *n > 0 {
                *range = Range {
                    active: true,
                    end: line + n,
                };
            }
        }
        Addr2::Tilde(n) => {
            // The range runs to the next line that is a multiple of n, which is
            // after the start line even when the start line is itself a multiple.
            if *n > 0 {
                *range = Range {
                    active: true,
                    end: (line / n + 1) * n,
                };
            }
        }
        Addr2::Last | Addr2::Re(_) => range.active = true,
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_at<'a>(
        line: u64,
        space: &'a [u8],
        last: &'a mut Option<Rc<Regex>>,
        is_last: &'a mut dyn FnMut() -> bool,
    ) -> Env<'a> {
        Env {
            line,
            space,
            last_re: last,
            is_last,
        }
    }

    fn regex(source: &str) -> Pattern {
        Pattern(Some(Rc::new(
            super::super::pattern::compile(source.as_bytes(), false, false, false).unwrap(),
        )))
    }

    /// Runs `selector` over lines 1..=`count` (each as `text`) and returns the hits.
    fn hits(selector: &Selector, texts: &[&str]) -> Vec<u64> {
        let mut range = Range::initial(selector);
        let mut last = None;
        let mut out = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            let line = index as u64 + 1;
            let total = texts.len() as u64;
            let mut is_last = move || line == total;
            let mut env = env_at(line, text.as_bytes(), &mut last, &mut is_last);
            if selects(selector, &mut range, &mut env).unwrap() {
                out.push(line);
            }
        }
        out
    }

    fn sel(start: Option<Addr>, end: Option<Addr2>) -> Selector {
        Selector {
            start,
            end,
            negate: false,
        }
    }

    #[test]
    fn line_number_and_last() {
        let lines = ["a", "b", "c"];
        assert_eq!(hits(&sel(Some(Addr::Line(2)), None), &lines), vec![2]);
        assert_eq!(hits(&sel(Some(Addr::Last), None), &lines), vec![3]);
    }

    #[test]
    fn step_addresses() {
        let lines = ["1", "2", "3", "4", "5", "6", "7"];
        assert_eq!(hits(&sel(Some(Addr::Step(0, 3)), None), &lines), vec![3, 6]);
        assert_eq!(
            hits(&sel(Some(Addr::Step(2, 2)), None), &lines),
            vec![2, 4, 6]
        );
        assert_eq!(hits(&sel(Some(Addr::Step(4, 0)), None), &lines), vec![4]);
    }

    #[test]
    fn regex_single_address() {
        let lines = ["foo", "bar", "food"];
        assert_eq!(
            hits(&sel(Some(Addr::Re(regex("foo"))), None), &lines),
            vec![1, 3]
        );
    }

    #[test]
    fn negated_selector() {
        let lines = ["a", "b"];
        let selector = Selector {
            start: Some(Addr::Line(1)),
            end: None,
            negate: true,
        };
        assert_eq!(hits(&selector, &lines), vec![2]);
    }

    #[test]
    fn line_range_to_line() {
        let lines = ["1", "2", "3", "4", "5"];
        let selector = sel(Some(Addr::Line(2)), Some(Addr2::Line(4)));
        assert_eq!(hits(&selector, &lines), vec![2, 3, 4]);
    }

    #[test]
    fn range_with_end_before_start_matches_one_line() {
        let lines = ["1", "2", "3"];
        let selector = sel(Some(Addr::Line(2)), Some(Addr2::Line(1)));
        assert_eq!(hits(&selector, &lines), vec![2]);
    }

    #[test]
    fn plus_and_tilde_ranges() {
        let lines = ["1", "2", "3", "4", "5", "6"];
        assert_eq!(
            hits(&sel(Some(Addr::Line(2)), Some(Addr2::Plus(1))), &lines),
            vec![2, 3]
        );
        assert_eq!(
            hits(&sel(Some(Addr::Line(2)), Some(Addr2::Tilde(4))), &lines),
            vec![2, 3, 4]
        );
        assert_eq!(
            hits(&sel(Some(Addr::Line(4)), Some(Addr2::Tilde(4))), &lines),
            vec![4, 5, 6]
        );
        assert_eq!(
            hits(&sel(Some(Addr::Line(2)), Some(Addr2::Plus(0))), &lines),
            vec![2]
        );
    }

    #[test]
    fn regex_range_ends_on_a_later_match() {
        let lines = ["a", "start", "x", "end", "b", "start", "y"];
        let selector = sel(
            Some(Addr::Re(regex("start"))),
            Some(Addr2::Re(regex("end"))),
        );
        assert_eq!(hits(&selector, &lines), vec![2, 3, 4, 6, 7]);
    }

    #[test]
    fn zero_address_range_starts_before_line_one() {
        let lines = ["x", "x", "x"];
        let selector = sel(Some(Addr::Line(0)), Some(Addr2::Re(regex("x"))));
        assert_eq!(hits(&selector, &lines), vec![1]);
    }

    #[test]
    fn last_as_range_end() {
        let lines = ["1", "2", "3"];
        let selector = sel(Some(Addr::Line(2)), Some(Addr2::Last));
        assert_eq!(hits(&selector, &lines), vec![2, 3]);
    }

    #[test]
    fn empty_regex_in_address_reuses_last() {
        let lines = ["foo", "bar"];
        let mut range = Range::default();
        let mut last = Some(Rc::new(
            super::super::pattern::compile(b"bar", false, false, false).unwrap(),
        ));
        let selector = sel(Some(Addr::Re(Pattern::default())), None);
        let mut is_last = || false;
        let mut env = env_at(1, lines[0].as_bytes(), &mut last, &mut is_last);
        assert!(!selects(&selector, &mut range, &mut env).unwrap());
        let mut env = env_at(2, lines[1].as_bytes(), &mut last, &mut is_last);
        assert!(selects(&selector, &mut range, &mut env).unwrap());
    }

    #[test]
    fn empty_regex_without_previous_is_an_error() {
        let mut range = Range::default();
        let mut last = None;
        let selector = sel(Some(Addr::Re(Pattern::default())), None);
        let mut is_last = || false;
        let mut env = env_at(1, b"x", &mut last, &mut is_last);
        assert!(selects(&selector, &mut range, &mut env).is_err());
    }

    #[test]
    fn range_state_is_per_command() {
        let selector = sel(Some(Addr::Line(0)), Some(Addr2::Re(regex("z"))));
        assert!(Range::initial(&selector).active);
        let other = sel(Some(Addr::Line(1)), Some(Addr2::Last));
        assert!(!Range::initial(&other).active);
    }
}
