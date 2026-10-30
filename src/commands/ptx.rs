//! `ptx`: permuted index (keyword in context).
//!
//! Every word of the input is a keyword in turn. The text before the keyword is right-aligned
//! to column `W/2 - 1`; the keyword starts at column `W/2 + gap`, followed by the words after
//! it. Context is cut to the widths GNU uses, with `/` marking a cut. Context wraps (GNU's
//! "tail" and "head" fields) are not produced, so wrapped lines can differ from GNU.
//!
//! Default context spans the whole input; `-G` and `-O` keep it within each line. `-O` and
//! `-G` print roff `.xx` lines, `-T` prints TeX `\xx` lines.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::io::Write;

use regex::Regex;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};

const CMD: &str = "ptx";

const SPECS: &[OptSpec] = &[
    OptSpec::flag("auto", Some('A'), Some("auto-reference")),
    OptSpec::flag("traditional", Some('G'), Some("traditional")),
    OptSpec::req("flag", Some('F'), Some("flag-truncation")),
    OptSpec::req("macro", Some('M'), Some("macro-name")),
    OptSpec::flag("roff", Some('O'), None),
    OptSpec::flag("tex", Some('T'), None),
    OptSpec::req("format", None, Some("format")),
    OptSpec::flag("right", Some('R'), Some("right-side-refs")),
    OptSpec::req("sentence", Some('S'), Some("sentence-regexp")),
    OptSpec::req("word", Some('W'), Some("word-regexp")),
    OptSpec::req("break", Some('b'), Some("break-file")),
    OptSpec::flag("fold", Some('f'), Some("ignore-case")),
    OptSpec::req("gap", Some('g'), Some("gap-size")),
    OptSpec::req("ignore", Some('i'), Some("ignore-file")),
    OptSpec::req("only", Some('o'), Some("only-file")),
    OptSpec::flag("refs", Some('r'), Some("references")),
    OptSpec::flag("typeset", Some('t'), Some("typeset-mode")),
    OptSpec::req("width", Some('w'), Some("width")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str =
    "Usage: ptx [OPTION]... [INPUT]...\n  or:  ptx -G [OPTION]... [INPUT [OUTPUT]]\n\
Output a permuted index, including context, of the words in the input files.\n\
With no FILE, or when FILE is -, read standard input.\n\n\
  -A, --auto-reference           output automatically generated references\n\
  -G, --traditional              behave more like System V 'ptx'\n\
  -F, --flag-truncation=STRING   use STRING for flagging line truncations\n\
  -M, --macro-name=STRING        macro name to use instead of 'xx'\n\
  -O, --format=roff              generate output as roff directives\n\
  -R, --right-side-refs          put references at right, not counted in -w\n\
  -S, --sentence-regexp=REGEXP   for end of lines or end of sentences\n\
  -T, --format=tex               generate output as TeX directives\n\
  -W, --word-regexp=REGEXP       use REGEXP to match each keyword\n\
  -b, --break-file=FILE          word break characters in this FILE\n\
  -f, --ignore-case              fold lower case to upper case for sorting\n\
  -g, --gap-size=NUMBER          gap size in output columns\n\
  -i, --ignore-file=FILE         read ignore word list from FILE\n\
  -o, --only-file=FILE           read only word list from this FILE\n\
  -r, --references               first field of each line is a reference\n\
  -t, --typeset-mode             accepted for compatibility\n\
  -w, --width=NUMBER             output width in columns, reference excluded\n\
      --help                     display this help and exit\n\
      --version                  output version information and exit\n";

/// Output dialect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Plain,
    Roff,
    Tex,
}

struct Options {
    dialect: Dialect,
    traditional: bool,
    auto_ref: bool,
    references: bool,
    fold: bool,
    gap: usize,
    width: usize,
    flag: String,
    macro_name: String,
    sentence: Option<Regex>,
    word: Option<Regex>,
    breaks: Vec<u8>,
    ignore: HashSet<String>,
    only: Option<HashSet<String>>,
}

/// One line of input: an optional reference and the text after it.
struct Line {
    reference: Option<String>,
    text: String,
}

/// A word occurrence: byte span in the joined text and the index of its line.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    line: usize,
}

/// The joined text with its word spans and sentence boundaries.
struct Corpus {
    text: String,
    spans: Vec<Span>,
    /// Sentence index of each span.
    sentence: Vec<usize>,
    /// Sorted byte offsets where a sentence ends.
    breaks: Vec<usize>,
    lines: Vec<Line>,
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    let mut names = Vec::new();
    for item in &items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(CMD),
            Item::Operand(name) => names.push(name.clone()),
            Item::Opt { .. } => {}
        }
    }
    let options = match options_from(&items) {
        Ok(options) => options,
        Err(code) => return code,
    };
    if names.is_empty() {
        names.push("-".to_owned());
    }
    let (lines, status) = read_lines(&names, options.auto_ref);
    let corpus = build_corpus(lines, &options);
    let mut out = stdout_writer();
    let written = write_index(&corpus, &options, &mut out);
    if let Err(error) = written {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!(
            "{CMD}: write error: {}",
            crate::common::error::io_error_text(&error)
        );
        return EXIT_FAIL;
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    status
}

fn options_from(items: &[Item]) -> Result<Options, i32> {
    let mut options = Options {
        dialect: Dialect::Plain,
        traditional: false,
        auto_ref: false,
        references: false,
        fold: false,
        gap: 3,
        width: 72,
        flag: "/".to_owned(),
        macro_name: "xx".to_owned(),
        sentence: None,
        word: None,
        breaks: Vec::new(),
        ignore: HashSet::new(),
        only: None,
    };
    for item in items {
        let Item::Opt { id, value } = item else {
            continue;
        };
        let text = value.clone().unwrap_or_default();
        match *id {
            "roff" => options.dialect = Dialect::Roff,
            "tex" => options.dialect = Dialect::Tex,
            "format" => {
                options.dialect = match text.as_str() {
                    "roff" => Dialect::Roff,
                    "tex" => Dialect::Tex,
                    _ => {
                        eprintln!("{CMD}: invalid format '{text}'");
                        return Err(EXIT_FAIL);
                    }
                }
            }
            "traditional" => options.traditional = true,
            "auto" => options.auto_ref = true,
            "refs" => options.references = true,
            "fold" => options.fold = true,
            "flag" => options.flag = text,
            "macro" => options.macro_name = text,
            "gap" => options.gap = number_option(&text, "gap width")?,
            "width" => options.width = number_option(&text, "line width")?,
            "sentence" => options.sentence = Some(compile(&text)?),
            "word" => options.word = Some(compile(&text)?),
            "break" => options.breaks = read_bytes(&text)?,
            "ignore" => options.ignore = read_words(&text, options.fold)?,
            "only" => options.only = Some(read_words(&text, options.fold)?),
            _ => {}
        }
    }
    // GNU prints roff directives for -G (traditional mode) unless -T asks for TeX.
    if options.traditional && options.dialect == Dialect::Plain {
        options.dialect = Dialect::Roff;
    }
    Ok(options)
}

/// A positive decimal option value; zero and non-numbers are rejected as GNU does.
fn number_option(text: &str, what: &str) -> Result<usize, i32> {
    match text.parse::<usize>() {
        Ok(value) if value > 0 => Ok(value),
        _ => {
            eprintln!("{CMD}: invalid {what}: '{text}'");
            Err(EXIT_FAIL)
        }
    }
}

/// `-S` and `-W` patterns use extended syntax (`+` and `?` are operators).
fn compile(pattern: &str) -> Result<Regex, i32> {
    Regex::new(pattern).map_err(|error| {
        eprintln!("{CMD}: invalid regular expression: {error}");
        EXIT_FAIL
    })
}

fn read_bytes(name: &str) -> Result<Vec<u8>, i32> {
    std::fs::read(name).map_err(|error| {
        report(CMD, name, &error);
        EXIT_FAIL
    })
}

/// Words of an ignore or only file, one set of whitespace-separated words.
fn read_words(name: &str, fold: bool) -> Result<HashSet<String>, i32> {
    let bytes = read_bytes(name)?;
    Ok(String::from_utf8_lossy(&bytes)
        .split_whitespace()
        .map(|word| {
            if fold {
                word.to_uppercase()
            } else {
                word.to_owned()
            }
        })
        .collect())
}

/// Reads every input, one line at a time. A failed input is reported and skipped.
fn read_lines(names: &[String], auto_ref: bool) -> (Vec<Line>, i32) {
    let mut lines = Vec::new();
    let mut status = EXIT_OK;
    for name in names {
        let reader = match open_input(name) {
            Ok(reader) => reader,
            Err(error) => {
                report(CMD, name, &error);
                status = EXIT_FAIL;
                continue;
            }
        };
        for (number, raw) in std::io::BufRead::lines(reader).enumerate() {
            let raw = match raw {
                Ok(raw) => raw,
                Err(error) => {
                    report(CMD, name, &error);
                    status = EXIT_FAIL;
                    break;
                }
            };
            let reference = auto_ref.then(|| format!("{name}:{}", number + 1));
            lines.push(Line {
                reference,
                text: raw,
            });
        }
    }
    (lines, status)
}

/// Splits the text into words and sentences. With `-r` the first field of a line is its
/// reference and is not a word.
fn build_corpus(mut lines: Vec<Line>, options: &Options) -> Corpus {
    let traditional = options.traditional;
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut breaks = Vec::new();
    for (index, line) in lines.iter_mut().enumerate() {
        let mut body: &str = &line.text;
        if options.references {
            let trimmed = body.trim_start();
            let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            if line.reference.is_none() {
                line.reference = Some(trimmed[..end].to_owned());
            }
            body = &trimmed[end..];
        }
        let base = text.len();
        text.push_str(body);
        // A sentence-regexp match ends a word as well as a sentence.
        let cuts: Vec<usize> = options
            .sentence
            .as_ref()
            .map(|regex| regex.find_iter(body).map(|found| found.end()).collect())
            .unwrap_or_default();
        for (start, end) in word_ranges(body, options, &cuts) {
            spans.push(Span {
                start: base + start,
                end: base + end,
                line: index,
            });
        }
        breaks.extend(cuts.iter().map(|cut| base + cut));
        if traditional {
            breaks.push(text.len());
        }
        text.push('\n');
    }
    breaks.sort_unstable();
    breaks.dedup();
    let sentence = spans
        .iter()
        .map(|span| breaks.partition_point(|&boundary| boundary <= span.start))
        .collect();
    Corpus {
        text,
        spans,
        sentence,
        breaks,
        lines,
    }
}

/// Byte ranges of the words in `body`.
fn word_ranges(body: &str, options: &Options, cuts: &[usize]) -> Vec<(usize, usize)> {
    if let Some(regex) = &options.word {
        return regex
            .find_iter(body)
            .map(|m| (m.start(), m.end()))
            .collect();
    }
    let is_separator =
        |c: char| c.is_whitespace() || (c.is_ascii() && options.breaks.contains(&(c as u8)));
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, c) in body.char_indices() {
        if cuts.contains(&index) {
            if let Some(begin) = start.take() {
                ranges.push((begin, index));
            }
        }
        if is_separator(c) {
            if let Some(begin) = start.take() {
                ranges.push((begin, index));
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        ranges.push((begin, body.len()));
    }
    ranges
}

/// The key a keyword sorts by, and whether it is excluded by `-i` or `-o`.
fn keyword_key(word: &str, options: &Options) -> Option<String> {
    let key = if options.fold {
        word.to_uppercase()
    } else {
        word.to_owned()
    };
    if options.ignore.contains(&key) {
        return None;
    }
    if let Some(only) = &options.only {
        if !only.contains(&key) {
            return None;
        }
    }
    Some(key)
}

/// One output line before layout: left context, keyword plus right context, reference.
struct Entry {
    key: String,
    index: usize,
    before: String,
    right: String,
    reference: Option<String>,
}

fn write_index(corpus: &Corpus, options: &Options, out: &mut impl Write) -> std::io::Result<()> {
    let mut entries = Vec::new();
    for index in 0..corpus.spans.len() {
        let span = corpus.spans[index];
        let word = &corpus.text[span.start..span.end];
        let Some(key) = keyword_key(word, options) else {
            continue;
        };
        let (before, right) = contexts(corpus, index, options);
        entries.push(Entry {
            key,
            index,
            before,
            right,
            reference: corpus.lines[span.line].reference.clone(),
        });
    }
    entries.sort_by(compare);
    let half = options.width / 2;
    for entry in &entries {
        let line = match options.dialect {
            Dialect::Plain => plain_line(entry, half, options),
            Dialect::Roff => roff_line(entry, options),
            Dialect::Tex => tex_line(entry, options),
        };
        out.write_all(line.as_bytes())?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn compare(a: &Entry, b: &Entry) -> Ordering {
    a.key.cmp(&b.key).then(a.index.cmp(&b.index))
}

/// The left and right context of the keyword at `index`, already cut to size.
fn contexts(corpus: &Corpus, index: usize, options: &Options) -> (String, String) {
    let sentence = corpus.sentence[index];
    let first = corpus.sentence.partition_point(|&s| s < sentence);
    let last = corpus.sentence.partition_point(|&s| s <= sentence);
    let span = corpus.spans[index];
    let half = options.width / 2;
    // The flag marker counts towards this limit; the right limit does not.
    // Plain output reserves one more column than the roff form before the gap.
    let plain_extra = usize::from(options.dialect == Dialect::Plain);
    let before_limit = half.saturating_sub(options.gap + plain_extra);
    let right_limit = (options.width + 1).saturating_sub(half + options.gap);
    // Candidate starts of the before-context, longest first: the sentence start, then each word.
    // The raw text runs to the keyword, so separators that are not words (such as `-` with -W)
    // stay in the context.
    let begin = sentence_begin(corpus, sentence, span.start);
    let starts = std::iter::once(begin).chain(corpus.spans[first..index].iter().map(|s| s.start));
    let mut before = String::new();
    for start in starts {
        let raw = corpus.text[start..span.start].trim_end();
        let marker = usize::from(start > begin);
        if raw.len() + marker <= before_limit {
            before = raw.to_owned();
            if marker == 1 {
                before.insert_str(0, &options.flag);
            }
            break;
        }
    }
    let mut right_end = span.end;
    let mut cut = false;
    for later in index + 1..last {
        let candidate = &corpus.text[span.start..corpus.spans[later].end];
        if candidate.len() > right_limit {
            cut = true;
            break;
        }
        right_end = corpus.spans[later].end;
    }
    let mut right = corpus.text[span.start..right_end].replace('\n', " ");
    if cut {
        right.push_str(&options.flag);
    }
    (before.replace('\n', " "), right)
}

/// Where the sentence containing `keyword_start` begins, after any blanks or line break.
fn sentence_begin(corpus: &Corpus, sentence: usize, keyword_start: usize) -> usize {
    let from = if sentence == 0 {
        0
    } else {
        corpus.breaks[sentence - 1]
    };
    let gap = &corpus.text[from..keyword_start];
    from + (gap.len() - gap.trim_start().len())
}

fn plain_line(entry: &Entry, half: usize, options: &Options) -> String {
    let mut line = String::new();
    let left = match &entry.reference {
        Some(reference) if entry.before.is_empty() => format!("{reference:<half$}"),
        _ => format!("{:>half$}", entry.before),
    };
    line.push_str(&left);
    line.push_str(&" ".repeat(options.gap));
    line.push_str(&entry.right);
    line
}

fn roff_line(entry: &Entry, options: &Options) -> String {
    let mut line = format!(
        ".{} \"\" \"{}\" \"{}\" \"\"",
        options.macro_name, entry.before, entry.right
    );
    if let Some(reference) = &entry.reference {
        line.push_str(&format!(" \"{reference}\""));
    }
    line
}

fn tex_line(entry: &Entry, options: &Options) -> String {
    let (keyword, after) = match entry.right.split_once(' ') {
        Some((keyword, after)) => (keyword.to_owned(), format!(" {after}")),
        None => (entry.right.clone(), String::new()),
    };
    let mut line = format!(
        "\\{} {{}}{{{}}}{{{}}}{{{}}}{{}}",
        options.macro_name, entry.before, keyword, after
    );
    if let Some(reference) = &entry.reference {
        line.push_str(&format!("{{{reference}}}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(width: usize, gap: usize) -> Options {
        Options {
            dialect: Dialect::Plain,
            traditional: false,
            auto_ref: false,
            references: false,
            fold: false,
            gap,
            width,
            flag: "/".to_owned(),
            macro_name: "xx".to_owned(),
            sentence: None,
            word: None,
            breaks: Vec::new(),
            ignore: HashSet::new(),
            only: None,
        }
    }

    fn corpus(input: &str, options: &Options) -> Corpus {
        let lines = input
            .lines()
            .map(|line| Line {
                reference: None,
                text: line.to_owned(),
            })
            .collect();
        build_corpus(lines, options)
    }

    fn render(input: &str, options: &Options) -> Vec<String> {
        let corpus = corpus(input, options);
        let mut out = Vec::new();
        write_index(&corpus, options, &mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn plain_layout_aligns_keywords_at_half_width_plus_gap() {
        let lines = render("one two three four five", &options(72, 3));
        assert_eq!(lines.len(), 5);
        assert_eq!(
            lines[0],
            format!("{}one two three four   five", " ".repeat(18))
        );
        assert_eq!(
            lines[2],
            format!("{}one two three four five", " ".repeat(39))
        );
        assert_eq!(
            lines[4],
            format!("{}one   two three four five", " ".repeat(33))
        );
    }

    #[test]
    fn traditional_mode_keeps_context_on_each_line() {
        let mut opts = options(72, 3);
        opts.traditional = true;
        opts.dialect = Dialect::Roff;
        let lines = render("a b\nc d", &opts);
        assert_eq!(lines[0], ".xx \"\" \"\" \"a b\" \"\"");
        assert_eq!(lines[1], ".xx \"\" \"a\" \"b\" \"\"");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn before_and_after_are_cut_to_gnu_widths() {
        let lines = render("one two three four five", &options(20, 3));
        let expected = [
            "     /four   five",
            "    /three   four/",
            "             one two/",
            "      /two   three/",
            "       one   two/",
        ];
        assert_eq!(lines, expected.map(str::to_owned));
    }

    #[test]
    fn ignore_list_removes_keywords_but_keeps_context() {
        let mut opts = options(72, 3);
        opts.ignore.insert("b".to_owned());
        let lines = render("a b c", &opts);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("a b c"));
    }

    #[test]
    fn tex_splits_keyword_from_context() {
        let mut opts = options(72, 3);
        opts.dialect = Dialect::Tex;
        opts.traditional = true;
        let lines = render("one two", &opts);
        assert_eq!(lines[0], "\\xx {}{}{one}{ two}{}");
    }

    #[test]
    fn word_ranges_split_on_whitespace_and_break_characters() {
        let opts = options(72, 3);
        assert_eq!(word_ranges("a-b c_d", &opts, &[]), vec![(0, 3), (4, 7)]);
        assert_eq!(word_ranges("a;b", &opts, &[2]), vec![(0, 2), (2, 3)]);
    }
}
