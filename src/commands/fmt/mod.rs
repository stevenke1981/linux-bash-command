//! Paragraph formatting with optimal Knuth–Plass line breaking.
use crate::common::{
    args::{self, Item, OptSpec},
    error::{io_text, usage},
    io::{input, output},
};
use std::io::{self, BufRead, Write};
mod linebreak;
mod parasplit;

const HELP: &str = "Usage: fmt [-WIDTH] [OPTION]... [FILE]...\nReformat paragraphs, reading standard input when no FILE is given.\n  -c, --crown-margin       preserve indentation of first two lines\n  -p, --prefix=STRING      format only lines beginning with STRING\n  -s, --split-only         split lines without joining them\n  -t, --tagged-paragraph   require distinct first-line indentation\n  -u, --uniform-spacing    one space between words, two after sentences\n  -w, --width=N            maximum width (default 75)\n  -g, --goal=N             target width (default 93% of width)\n      --help              display this help\n      --version           output version information\n";

struct FmtOptions {
    crown: bool,
    tagged: bool,
    mail: bool,
    split_only: bool,
    prefix: Option<String>,
    xprefix: bool,
    anti_prefix: Option<String>,
    xanti_prefix: bool,
    uniform: bool,
    quick: bool,
    width: usize,
    goal: usize,
    tabwidth: usize,
}
impl Default for FmtOptions {
    fn default() -> Self {
        Self {
            crown: false,
            tagged: false,
            mail: false,
            split_only: false,
            prefix: None,
            xprefix: false,
            anti_prefix: None,
            xanti_prefix: false,
            uniform: false,
            quick: false,
            width: 75,
            goal: 70,
            tabwidth: 8,
        }
    }
}
struct FileOrStdReader {
    inner: Box<dyn BufRead>,
    error: Option<io::Error>,
}

fn parse(arguments: &[String]) -> Result<(FmtOptions, Vec<String>), i32> {
    let mut rewritten = arguments.to_vec();
    // A required argument (including a negative one) must reach getopt unchanged.
    if rewritten.first().is_some_and(|arg| {
        arg.starts_with('-') && arg.len() > 1 && arg[1..].bytes().all(|b| b.is_ascii_digit())
    }) {
        rewritten[0] = format!("--width={}", &rewritten[0][1..]);
    }
    let specs = [
        OptSpec::flag("crown-margin", 'c'),
        OptSpec::value("prefix", 'p'),
        OptSpec::flag("split-only", 's'),
        OptSpec::flag("tagged-paragraph", 't'),
        OptSpec::flag("uniform-spacing", 'u'),
        OptSpec::value("width", 'w'),
        OptSpec::value("goal", 'g'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = args::parse(&rewritten, &specs, true).map_err(|error| usage("fmt", error, 1))?;
    let mut options = FmtOptions::default();
    let mut files = Vec::new();
    let (mut width, mut goal) = (None, None);
    for item in items {
        match item {
            Item::Opt(id @ ("help" | "version"), _) => {
                return Err(args::help_version("fmt", &[format!("--{id}")], HELP, true).unwrap())
            }
            Item::Opt("crown-margin", _) => options.crown = true,
            Item::Opt("tagged-paragraph", _) => options.tagged = true,
            Item::Opt("split-only", _) => options.split_only = true,
            Item::Opt("uniform-spacing", _) => options.uniform = true,
            Item::Opt("prefix", value) => options.prefix = value,
            Item::Opt(id @ ("width" | "goal"), Some(value)) => {
                let n = value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n < 2500)
                    .ok_or_else(|| usage("fmt", format!("invalid {id}: '{value}'"), 1))?;
                if id == "width" {
                    width = Some(n)
                } else {
                    goal = Some(n)
                }
            }
            Item::Operand(file) => files.push(file),
            _ => unreachable!(),
        }
    }
    options.width = width.unwrap_or_else(|| goal.map_or(75, |g| (g * 100 / 93).max(g + 3)));
    options.goal = goal.unwrap_or(if options.width == 0 {
        0
    } else {
        (options.width * 93 / 100).max(1)
    });
    if options.goal > options.width {
        return Err(usage("fmt", "GOAL cannot be greater than WIDTH", 1));
    }
    if options.crown {
        options.tagged = false;
    }
    if options.split_only {
        options.crown = false;
        options.tagged = false;
    }
    if files.is_empty() {
        files.push("-".into())
    }
    Ok((options, files))
}

fn format(reader: Box<dyn BufRead>, options: &FmtOptions, out: &mut dyn Write) -> io::Result<()> {
    let mut reader = FileOrStdReader {
        inner: reader,
        error: None,
    };
    for paragraph in parasplit::ParagraphStream::new(options, &mut reader) {
        match paragraph {
            Ok(paragraph) => linebreak::break_lines(&paragraph, options, out)?,
            Err(line) => {
                out.write_all(&line.line)?;
                if line.newline {
                    out.write_all(b"\n")?;
                }
            }
        }
    }
    if let Some(error) = reader.error {
        return Err(error);
    }
    Ok(())
}
pub fn run(arguments: &[String]) -> i32 {
    let (options, files) = match parse(arguments) {
        Ok(result) => result,
        Err(code) => return code,
    };
    let mut failed = false;
    let code = output("fmt", |out| {
        for file in &files {
            match input(file) {
                Ok(reader) => {
                    if let Err(error) = format(reader, &options, out) {
                        if error.kind() == io::ErrorKind::BrokenPipe {
                            return Err(error);
                        }
                        eprintln!("fmt: {file}: {}", io_text(&error));
                        failed = true;
                    }
                }
                Err(error) => {
                    eprintln!("fmt: {file}: {}", io_text(&error));
                    failed = true;
                }
            }
        }
        Ok(())
    });
    code.max(i32::from(failed))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wrap(bytes: &[u8], options: FmtOptions) -> Vec<u8> {
        let mut out = Vec::new();
        format(
            Box::new(io::Cursor::new(bytes.to_vec())),
            &options,
            &mut out,
        )
        .unwrap();
        out
    }
    #[test]
    fn joins_lines() {
        assert_eq!(wrap(b"one\ntwo\n", FmtOptions::default()), b"one two\n");
    }
    #[test]
    fn split_preserves_lines() {
        assert_eq!(
            wrap(
                b"one\ntwo\n",
                FmtOptions {
                    split_only: true,
                    ..Default::default()
                }
            ),
            b"one\ntwo\n"
        );
    }
    #[test]
    fn retains_bytes() {
        assert_eq!(
            wrap(b"one \xff two\n", FmtOptions::default()),
            b"one \xff two\n"
        );
    }
}
