use crate::common::{
    args::{self, Item, OptSpec},
    error::{io_text, usage},
    io::input,
};
use regex::bytes::{Regex, RegexBuilder};
use std::collections::HashSet;
use std::io::Read;
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Format {
    Plain,
    Roff,
    Tex,
}
pub(super) struct Options {
    pub automatic: bool,
    pub traditional: bool,
    pub flag: Vec<u8>,
    pub macro_name: String,
    pub format: Format,
    pub right: bool,
    pub sentence: Option<Regex>,
    pub word: Regex,
    pub folded: bool,
    pub gap: usize,
    pub ignore: HashSet<Vec<u8>>,
    pub only: Option<HashSet<Vec<u8>>>,
    pub references: bool,
    pub width: usize,
    pub custom_words: bool,
    pub breaks: Option<Vec<u8>>,
}
const HELP:&str="Usage: ptx [OPTION]... [INPUT]...\n  or: ptx -G [OPTION]... [INPUT [OUTPUT]]\nOutput a permuted index, including keyword context.\n  -A, --auto-reference       generate filename and line references\n  -G, --traditional          traditional roff output and word boundaries\n  -F, --flag-truncation=STRING truncation marker (default /)\n  -M, --macro-name=STRING    roff or TeX macro name (default xx)\n  -O                        roff output\n      --format=roff|tex      select output format\n  -R, --right-side-refs      put references on the right\n  -S, --sentence-regexp=REGEXP sentence separator\n  -T                        TeX output\n  -W, --word-regexp=REGEXP   keyword regular expression\n  -b, --break-file=FILE      word separator bytes\n  -f, --ignore-case          ASCII case-insensitive sorting\n  -g, --gap-size=N           field gap (default 3)\n  -i, --ignore-file=FILE      words to omit\n  -o, --only-file=FILE        words to include\n  -r, --references           first field of each line is a reference\n  -t, --typeset-mode         accepted compatibility option\n  -w, --width=N              output width (default 72)\n      --help                display this help\n      --version             output version information\n";
fn compile(value: &str) -> Result<Regex, String> {
    let pattern = if value.ends_with('\\')
        && value.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1
    {
        format!("{value}\\")
    } else {
        value.into()
    };
    RegexBuilder::new(&pattern)
        .unicode(false)
        .build()
        .map_err(|e| format!("invalid regular expression '{value}': {e}"))
}
fn bytes(path: &str) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    input(path)
        .and_then(|mut reader| reader.read_to_end(&mut result))
        .map_err(|e| format!("{path}: {}", io_text(&e)))?;
    Ok(result)
}
fn words(path: &str, folded: bool) -> Result<HashSet<Vec<u8>>, String> {
    Ok(bytes(path)?
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            if folded {
                line.to_ascii_lowercase()
            } else {
                line.to_vec()
            }
        })
        .collect())
}
pub(super) fn parse(arguments: &[String]) -> Result<(Options, Vec<String>, Option<String>), i32> {
    let specs = [
        OptSpec::flag("auto-reference", 'A'),
        OptSpec::flag("traditional", 'G'),
        OptSpec::value("flag-truncation", 'F'),
        OptSpec::value("macro-name", 'M'),
        OptSpec {
            id: "roff",
            short: 'O',
            long: "",
            has_arg: args::HasArg::No,
        },
        OptSpec::value("format", '\0'),
        OptSpec::flag("right-side-refs", 'R'),
        OptSpec::value("sentence-regexp", 'S'),
        OptSpec {
            id: "tex",
            short: 'T',
            long: "",
            has_arg: args::HasArg::No,
        },
        OptSpec::value("word-regexp", 'W'),
        OptSpec::value("break-file", 'b'),
        OptSpec::flag("ignore-case", 'f'),
        OptSpec::value("gap-size", 'g'),
        OptSpec::value("ignore-file", 'i'),
        OptSpec::value("only-file", 'o'),
        OptSpec::flag("references", 'r'),
        OptSpec::flag("typeset-mode", 't'),
        OptSpec::value("width", 'w'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = args::parse(arguments, &specs, true).map_err(|e| usage("ptx", e, 1))?;
    let mut o = Options {
        automatic: false,
        traditional: false,
        flag: b"/".to_vec(),
        macro_name: "xx".into(),
        format: Format::Plain,
        right: false,
        sentence: None,
        word: compile("[A-Za-z]+").unwrap(),
        folded: false,
        gap: 3,
        ignore: HashSet::new(),
        only: None,
        references: false,
        width: 72,
        custom_words: false,
        breaks: None,
    };
    let (mut files, mut ignore, mut only, mut breaks) = (Vec::new(), None, None, None);
    let mut explicit_format = false;
    for item in items {
        if let Item::Opt(id @ ("help" | "version"), _) = &item {
            return Err(args::help_version("ptx", &[format!("--{id}")], HELP, true).unwrap());
        }
        let result: Result<(), String> = (|| {
            match item {
                Item::Operand(path) => files.push(path),
                Item::Opt("auto-reference", _) => o.automatic = true,
                Item::Opt("traditional", _) => {
                    o.traditional = true;
                    if !explicit_format {
                        o.format = Format::Roff
                    }
                }
                Item::Opt("flag-truncation", Some(v)) => o.flag = v.into_bytes(),
                Item::Opt("macro-name", Some(v)) => o.macro_name = v,
                Item::Opt("roff", _) => {
                    o.format = Format::Roff;
                    explicit_format = true
                }
                Item::Opt("tex", _) => {
                    o.format = Format::Tex;
                    explicit_format = true
                }
                Item::Opt("format", Some(v)) => {
                    o.format = match v.as_str() {
                        "roff" => Format::Roff,
                        "tex" => Format::Tex,
                        _ => return Err(format!("invalid format '{v}'")),
                    };
                    explicit_format = true
                }
                Item::Opt("right-side-refs", _) => o.right = true,
                Item::Opt("sentence-regexp", Some(v)) => {
                    let regex = compile(&v)?;
                    if regex.is_match(b"") {
                        return Err("regular expression must not match the empty string".into());
                    }
                    o.sentence = Some(regex)
                }
                Item::Opt("word-regexp", Some(v)) => {
                    o.word = compile(&v)?;
                    o.custom_words = true;
                }
                Item::Opt("break-file", v) => breaks = v,
                Item::Opt("ignore-case", _) => o.folded = true,
                Item::Opt("ignore-file", v) => ignore = v,
                Item::Opt("only-file", v) => only = v,
                Item::Opt("references", _) => o.references = true,
                Item::Opt("typeset-mode", _) => {}
                Item::Opt(id @ ("width" | "gap-size"), Some(v)) => {
                    let n = v
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0 && *n <= 1_000_000)
                        .ok_or_else(|| format!("invalid {id} '{v}'"))?;
                    if id == "width" {
                        o.width = n
                    } else {
                        o.gap = n
                    }
                }
                _ => unreachable!(),
            };
            Ok(())
        })();
        if let Err(e) = result {
            return Err(usage("ptx", e, 1));
        }
    }
    let configure: Result<(), String> = (|| {
        if let Some(path) = ignore {
            o.ignore = words(&path, o.folded)?
        }
        if let Some(path) = only {
            o.only = Some(words(&path, o.folded)?)
        }
        if !o.custom_words {
            if let Some(path) = breaks {
                let mut values = bytes(&path)?;
                if o.traditional {
                    values.extend(b" \t\n")
                }
                let mut pattern = String::from("[^");
                for b in &values {
                    pattern.push_str(&format!("\\x{b:02x}"))
                }
                pattern.push_str("]+");
                o.word = compile(if values.is_empty() {
                    "(?s:.+)"
                } else {
                    &pattern
                })?;
                o.breaks = Some(values);
            } else if o.traditional {
                o.word = compile("[^ \\t\\n]+").unwrap();
            }
        }
        Ok(())
    })();
    if let Err(e) = configure {
        return Err(usage("ptx", e, 1));
    }
    if o.traditional && files.len() > 2 {
        return Err(usage("ptx", "extra operand", 1));
    }
    let destination = if o.traditional && files.len() == 2 {
        files.pop()
    } else {
        None
    };
    if files.is_empty() {
        files.push("-".into())
    }
    Ok((o, files, destination))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trailing_escape() {
        assert!(compile("word\\").is_ok());
    }
    #[test]
    fn byte_regex() {
        assert!(compile("[a-z]+").unwrap().is_match(b"\xffword"));
    }
    #[test]
    fn invalid_regex() {
        assert!(compile("[").is_err());
    }
}
