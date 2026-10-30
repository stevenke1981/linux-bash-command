use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};

#[derive(Debug)]
struct Tabs {
    stops: Vec<usize>,
    repeat: Option<(usize, bool)>,
}
impl Tabs {
    fn parse(s: &str) -> Result<Self, String> {
        let mut stops = Vec::new();
        let mut repeat = None;
        for word in s.split([',', ' ']).filter(|w| !w.is_empty()) {
            let (prefix, n) = if word.starts_with(['+', '/']) {
                (word.as_bytes()[0], &word[1..])
            } else {
                (0, word)
            };
            let n = n
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("tab size contains invalid character(s): '{s}'"))?;
            if repeat.is_some() || (prefix == 0 && stops.last().is_some_and(|last| *last >= n)) {
                return Err("tab sizes must be ascending".into());
            }
            if prefix == 0 {
                stops.push(n)
            } else {
                repeat = Some((n, prefix == b'+'));
            }
        }
        if stops.is_empty() && repeat.is_none() {
            return Err("tab size cannot be zero".into());
        }
        if stops.len() == 1 && repeat.is_none() {
            repeat = Some((stops[0], false));
            stops.clear();
        }
        Ok(Self { stops, repeat })
    }
    fn next(&self, col: usize) -> Option<usize> {
        self.stops.iter().copied().find(|n| *n > col).or_else(|| {
            self.repeat.map(|(n, relative)| {
                let base = if relative {
                    self.stops.last().copied().unwrap_or(0)
                } else {
                    0
                };
                base + (col - base) / n * n + n
            })
        })
    }
}

fn expand(line: &[u8], tabs: &Tabs, initial: bool) -> Vec<u8> {
    let (mut out, mut col, mut leading) = (Vec::new(), 0, true);
    for &b in line {
        if b == b'\t' && (!initial || leading) {
            let end = tabs.next(col).unwrap_or(col + 1);
            out.resize(out.len() + end - col, b' ');
            col = end;
        } else {
            out.push(b);
            col = match b {
                8 => col.saturating_sub(1),
                b'\t' => tabs.next(col).unwrap_or(col + 1),
                _ => col + 1,
            };
        }
        leading &= matches!(b, b' ' | b'\t');
    }
    out
}

fn unexpand(line: &[u8], tabs: &Tabs, all: bool) -> Vec<u8> {
    let (mut out, mut pos, mut col, mut leading) = (Vec::new(), 0, 0, true);
    while pos < line.len() {
        if matches!(line[pos], b' ' | b'\t') && (all || leading) {
            let start = pos;
            let initial_col = col;
            let mut end = col;
            while pos < line.len() && matches!(line[pos], b' ' | b'\t') {
                end = if line[pos] == b'\t' {
                    tabs.next(end).unwrap_or(end + 1)
                } else {
                    end + 1
                };
                pos += 1;
            }
            while col < end {
                if let Some(next) = tabs.next(col).filter(|n| {
                    *n <= end && (end - initial_col > 1 || line[start..pos].contains(&b'\t'))
                }) {
                    out.push(b'\t');
                    col = next;
                } else {
                    out.push(b' ');
                    col += 1;
                }
            }
        } else {
            let b = line[pos];
            out.push(b);
            col = if b == 8 {
                col.saturating_sub(1)
            } else if b == b'\t' {
                tabs.next(col).unwrap_or(col + 1)
            } else {
                col + 1
            };
            leading = false;
            pos += 1;
        }
    }
    out
}

pub fn run_expand(a: &[String]) -> i32 {
    run(a, false)
}
pub fn run_unexpand(a: &[String]) -> i32 {
    run(a, true)
}
fn run(args: &[String], reverse: bool) -> i32 {
    let cmd = if reverse { "unexpand" } else { "expand" };
    let help = if reverse {
        "Usage: unexpand [OPTION]... [FILE]...\n  -a, --all\n  --first-only\n  -t, --tabs=LIST\n  --help --version\n"
    } else {
        "Usage: expand [OPTION]... [FILE]...\n  -i, --initial\n  -t, --tabs=LIST\n  --help --version\n"
    };
    let mut options = true;
    let args: Vec<_> = args
        .iter()
        .map(|s| {
            if s == "--" {
                options = false;
                return s.clone();
            }
            if !options {
                return s.clone();
            }
            if std::env::var_os("POSIXLY_CORRECT").is_some() && !s.starts_with('-') {
                options = false;
            }

            if !reverse
                && s.len() > 1
                && s.starts_with('-')
                && s[1..].bytes().all(|b| b.is_ascii_digit() || b == b',')
            {
                format!("--tabs={}", &s[1..])
            } else {
                s.clone()
            }
        })
        .collect();
    let mut specs = vec![OptSpec::value("tabs", 't')];
    if reverse {
        specs.extend([OptSpec::flag("all", 'a'), OptSpec::flag("first-only", '\0')]);
    } else {
        specs.push(OptSpec::flag("initial", 'i'));
    }
    let parsed = match args::parse_command(cmd, &args, help, &specs, true) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let (mut tabs, mut all, mut initial, mut files) =
        (Tabs::parse("8").unwrap(), false, false, Vec::new());
    for item in parsed {
        match item {
            Item::Opt("tabs", Some(v)) => {
                tabs = match Tabs::parse(&v) {
                    Ok(t) => t,
                    Err(e) => return error::usage(cmd, e, 1),
                };
                all = true;
            }
            Item::Opt("all", _) => all = true,
            Item::Opt("first-only", _) => all = false,
            Item::Opt("initial", _) => initial = true,
            Item::Operand(v) => files.push(v),
            _ => unreachable!(),
        }
    }
    let mut failed = false;
    let code = io::output(cmd, |out| {
        failed = io::for_inputs(cmd, &files, |_, r| {
            let mut line = Vec::new();
            while r.read_until(b'\n', &mut line)? != 0 {
                let nl = line.last() == Some(&b'\n');
                if nl {
                    line.pop();
                }
                out.write_all(&if reverse {
                    unexpand(&line, &tabs, all)
                } else {
                    expand(&line, &tabs, initial)
                })?;
                if nl {
                    out.write_all(b"\n")?;
                }
                line.clear();
            }
            Ok(())
        })?;
        Ok(())
    });
    code.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expand_default() {
        assert_eq!(
            expand(b"a\tb", &Tabs::parse("8").unwrap(), false),
            b"a       b"
        );
    }
    #[test]
    fn expand_initial() {
        assert_eq!(
            expand(b"\tx\ty", &Tabs::parse("4").unwrap(), true),
            b"    x\ty"
        );
    }
    #[test]
    fn expand_stops() {
        assert_eq!(
            expand(b"\t\t\t", &Tabs::parse("2,5").unwrap(), false),
            b"      "
        );
    }
    #[test]
    fn expand_repeat() {
        assert_eq!(Tabs::parse("3,+4").unwrap().next(8), Some(11));
        assert_eq!(Tabs::parse("3,/4").unwrap().next(8), Some(12));
    }
    #[test]
    fn expand_backspace() {
        assert_eq!(
            expand(b"ab\x08\t", &Tabs::parse("4").unwrap(), false),
            b"ab\x08   "
        );
    }
    #[test]
    fn unexpand_default() {
        assert_eq!(
            unexpand(b"        x        y", &Tabs::parse("8").unwrap(), false),
            b"\tx        y"
        );
    }
    #[test]
    fn unexpand_all() {
        assert_eq!(
            unexpand(b"a       b", &Tabs::parse("8").unwrap(), true),
            b"a\tb"
        );
    }
    #[test]
    fn unexpand_single() {
        assert_eq!(
            unexpand(b"       x", &Tabs::parse("8").unwrap(), true),
            b"       x"
        );
    }
    #[test]
    fn unexpand_existing() {
        assert_eq!(
            unexpand(b"\t   x", &Tabs::parse("8").unwrap(), true),
            b"\t   x"
        );
    }
    #[test]
    fn unexpand_invalid() {
        assert!(Tabs::parse("0").is_err());
        assert!(Tabs::parse("4,2").is_err());
    }
}
