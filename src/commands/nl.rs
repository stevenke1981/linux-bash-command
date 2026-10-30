use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
    regex::bre_to_regex,
};
use regex::bytes::Regex;

enum Style {
    All,
    Text,
    None,
    Pattern(Regex),
}
impl Style {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "a" => Ok(Self::All),
            "t" => Ok(Self::Text),
            "n" => Ok(Self::None),
            _ => s
                .strip_prefix('p')
                .ok_or_else(|| format!("invalid line numbering style: '{s}'"))
                .and_then(|r| {
                    regex::bytes::RegexBuilder::new(&bre_to_regex(r)?)
                        .unicode(false)
                        .build()
                        .map(Self::Pattern)
                        .map_err(|e| e.to_string())
                }),
        }
    }
    fn matches(&self, line: &[u8]) -> bool {
        match self {
            Self::All => true,
            Self::Text => !line.is_empty(),
            Self::None => false,
            Self::Pattern(r) => r.is_match(line),
        }
    }
}
struct Numbering {
    styles: [Style; 3],
    section: usize,
    number: i64,
    start: i64,
    increment: i64,
    width: usize,
    format: String,
    separator: String,
    delimiter: Vec<u8>,
    blank_group: usize,
    blanks: usize,
    no_reset: bool,
}
impl Numbering {
    fn new() -> Self {
        Self {
            styles: [Style::None, Style::Text, Style::None],
            section: 1,
            number: 1,
            start: 1,
            increment: 1,
            width: 6,
            format: "rn".into(),
            separator: "\t".into(),
            delimiter: b"\\:".to_vec(),
            blank_group: 1,
            blanks: 0,
            no_reset: false,
        }
    }
    fn line(&mut self, line: &[u8], out: &mut dyn std::io::Write) -> std::io::Result<()> {
        for (section, count) in [(0, 3), (1, 2), (2, 1)] {
            if line == self.delimiter.repeat(count) {
                self.section = section;
                self.blanks = 0;
                if !self.no_reset {
                    self.number = self.start;
                }
                return out.write_all(b"\n");
            }
        }
        let mut numbered = self.styles[self.section].matches(line);
        if matches!(self.styles[self.section], Style::All) && line.is_empty() {
            self.blanks += 1;
            numbered = self.blanks.is_multiple_of(self.blank_group);
        } else {
            self.blanks = 0;
        }
        if numbered {
            match self.format.as_str() {
                "ln" => write!(out, "{:<width$}", self.number, width = self.width)?,
                "rz" => write!(out, "{:0width$}", self.number, width = self.width)?,
                _ => write!(out, "{:>width$}", self.number, width = self.width)?,
            }
            out.write_all(self.separator.as_bytes())?;
            self.number = self.number.wrapping_add(self.increment);
        } else {
            out.write_all(&vec![b' '; self.width + self.separator.len()])?;
        }
        out.write_all(line)?;
        out.write_all(b"\n")
    }
}
pub fn run(args: &[String]) -> i32 {
    let help = "Usage: nl [OPTION]... [FILE]...\n  -b, --body-numbering=STYLE\n  -h, --header-numbering=STYLE\n  -f, --footer-numbering=STYLE\n  -d, --section-delimiter=CC\n  -i, --line-increment=N\n  -l, --join-blank-lines=N\n  -n, --number-format=FORMAT\n  -p, --no-renumber\n  -s, --number-separator=STRING\n  -v, --starting-line-number=N\n  -w, --number-width=N\n  --help --version\n";
    let specs = [
        OptSpec::value("body-numbering", 'b'),
        OptSpec::value("header-numbering", 'h'),
        OptSpec::value("footer-numbering", 'f'),
        OptSpec::value("section-delimiter", 'd'),
        OptSpec::value("line-increment", 'i'),
        OptSpec::value("join-blank-lines", 'l'),
        OptSpec::value("number-format", 'n'),
        OptSpec::flag("no-renumber", 'p'),
        OptSpec::value("number-separator", 's'),
        OptSpec::value("starting-line-number", 'v'),
        OptSpec::value("number-width", 'w'),
    ];
    let parsed = match args::parse_command("nl", args, help, &specs, true) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let mut n = Numbering::new();
    let mut files = Vec::new();
    for item in parsed {
        let result: Result<(), String> = match item {
            Item::Operand(f) => {
                files.push(f);
                Ok(())
            }
            Item::Opt("no-renumber", _) => {
                n.no_reset = true;
                Ok(())
            }
            Item::Opt(id, Some(v)) => match id {
                "body-numbering" | "header-numbering" | "footer-numbering" => {
                    Style::parse(&v).map(|style| {
                        n.styles[match id {
                            "header-numbering" => 0,
                            "body-numbering" => 1,
                            _ => 2,
                        }] = style
                    })
                }
                "section-delimiter" => {
                    if v.len() > 2 {
                        Err("only one or two characters allowed".into())
                    } else {
                        n.delimiter = v.into_bytes();
                        if n.delimiter.len() == 1 {
                            n.delimiter.push(b':');
                        }
                        Ok(())
                    }
                }
                "number-separator" => {
                    n.separator = v;
                    Ok(())
                }
                "number-format" => {
                    if ["ln", "rn", "rz"].contains(&v.as_str()) {
                        n.format = v;
                        Ok(())
                    } else {
                        Err(format!("invalid line numbering format: '{v}'"))
                    }
                }
                "line-increment" | "starting-line-number" => v
                    .parse::<i64>()
                    .map_err(|_| format!("invalid number: '{v}'"))
                    .map(|num| {
                        if id == "line-increment" {
                            n.increment = num
                        } else {
                            n.start = num;
                            n.number = num
                        }
                    }),
                "number-width" | "join-blank-lines" => v
                    .parse::<usize>()
                    .ok()
                    .filter(|v| *v > 0)
                    .ok_or_else(|| format!("invalid number: '{v}'"))
                    .map(|num| {
                        if id == "number-width" {
                            n.width = num
                        } else {
                            n.blank_group = num
                        }
                    }),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        if let Err(e) = result {
            return error::usage("nl", &e, 1);
        }
    }
    let mut failed = false;
    let code = io::output("nl", |out| {
        failed = io::for_inputs("nl", &files, |_, r| {
            io::records(r, b'\n', |line| n.line(line, out))
        })?;
        Ok(())
    });
    code.max(i32::from(failed))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn line(n: &mut Numbering, s: &[u8]) -> Vec<u8> {
        let mut o = Vec::new();
        n.line(s, &mut o).unwrap();
        o
    }
    #[test]
    fn defaults() {
        assert_eq!(line(&mut Numbering::new(), b"x"), b"     1\tx\n");
    }
    #[test]
    fn blank() {
        assert_eq!(line(&mut Numbering::new(), b""), b"       \n");
    }
    #[test]
    fn pages() {
        let mut n = Numbering::new();
        line(&mut n, b"x");
        assert_eq!(line(&mut n, b"\\:\\:"), b"\n");
        assert_eq!(n.number, 1);
    }
    #[test]
    fn styles() {
        assert!(Style::parse("p^x").unwrap().matches(b"xyz"));
        assert!(!Style::parse("n").unwrap().matches(b"x"));
        assert!(Style::parse("bad").is_err());
    }
    #[test]
    fn blank_group() {
        let mut n = Numbering::new();
        n.styles[1] = Style::All;
        n.blank_group = 2;
        line(&mut n, b"");
        assert_eq!(n.number, 1);
        line(&mut n, b"");
        assert_eq!(n.number, 2);
    }
    #[test]
    fn formats() {
        let mut n = Numbering::new();
        n.format = "rz".into();
        assert_eq!(line(&mut n, b"x"), b"000001\tx\n");
        n.format = "ln".into();
        assert_eq!(line(&mut n, b"y"), b"2     \ty\n");
    }
}
