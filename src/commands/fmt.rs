//! `fmt`: reformat paragraphs of text to a given width.
//!
//! Paragraphs are re-broken with a minimum-cost layout: every line but the last costs the
//! square of its distance from the goal width, and the last line of a paragraph is free.
//! Lines are kept shorter than the width. `-s` only splits long lines (greedily), `-u`
//! normalises spacing, `-p` reformats only lines that start with a prefix.

use std::io::{self, BufRead, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};

const CMD: &str = "fmt";
const DEFAULT_WIDTH: usize = 75;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("crown", Some('c'), Some("crown-margin")),
    OptSpec::flag("tagged", Some('t'), Some("tagged-paragraph")),
    OptSpec::flag("split", Some('s'), Some("split-only")),
    OptSpec::flag("uniform", Some('u'), Some("uniform-spacing")),
    OptSpec::req("prefix", Some('p'), Some("prefix")),
    OptSpec::req("width", Some('w'), Some("width")),
    OptSpec::req("goal", Some('g'), Some("goal")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = r"Usage: fmt [-WIDTH] [OPTION]... [FILE]...
Reformat each paragraph in the FILE(s), writing to standard output.
The option -WIDTH is an abbreviated form of --width=DIGITS.

With no FILE, or when FILE is -, read standard input.

  -c, --crown-margin        preserve indentation of first two lines
  -p, --prefix=STRING       reformat only lines beginning with STRING,
                              reattaching the prefix to reformatted lines
  -s, --split-only          split long lines, but do not refill
  -t, --tagged-paragraph    indentation of first line different from second
  -u, --uniform-spacing     one space between words, two after sentences
  -w, --width=WIDTH         maximum line width (default of 75 columns)
  -g, --goal=WIDTH          goal width (default of 93% of width)
      --help                display this help and exit
      --version             output version information and exit

Exit status: 0 on success, 1 on failure.
";

#[derive(Clone, Copy, Debug)]
struct Options {
    width: usize,
    goal: usize,
    crown: bool,
    tagged: bool,
    split: bool,
    uniform: bool,
    /// Columns taken by the `-p` prefix, which count towards the width.
    prefix_len: usize,
}

/// A word and the spacing that precedes it.
#[derive(Clone, Debug)]
struct Word {
    text: Vec<u8>,
    /// Spaces before the word when it follows another word on the same input line; at a line
    /// break, the gap that the join produces.
    gap: usize,
    sentence_end: bool,
}

/// One input line of a paragraph: its indentation and words.
struct Para {
    indent: usize,
    words: Vec<Word>,
}

pub fn run(args: &[String]) -> i32 {
    let args = legacy_width(args);
    let items = match Parser::new(CMD, SPECS).parse(&args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    for item in &items {
        if let Item::Opt { id, .. } = item {
            match *id {
                "help" => {
                    print!("{HELP}");
                    return EXIT_OK;
                }
                "version" => return print_version(CMD),
                _ => {}
            }
        }
    }
    let mut options = Options {
        width: DEFAULT_WIDTH,
        goal: 0,
        crown: false,
        tagged: false,
        split: false,
        uniform: false,
        prefix_len: 0,
    };
    let mut prefix: Option<Vec<u8>> = None;
    let mut goal_given: Option<usize> = None;
    for item in &items {
        let Item::Opt { id, value } = item else {
            continue;
        };
        let value = value.as_deref().unwrap_or("");
        match *id {
            "crown" => options.crown = true,
            "tagged" => options.tagged = true,
            "split" => options.split = true,
            "uniform" => options.uniform = true,
            "prefix" => prefix = Some(value.as_bytes().to_vec()),
            "width" => match parse_width(value) {
                Ok(width) => options.width = width,
                Err(message) => return fail(&message),
            },
            "goal" => match parse_width(value) {
                Ok(goal) => goal_given = Some(goal),
                Err(message) => return fail(&message),
            },
            _ => {}
        }
    }
    options.prefix_len = prefix.as_ref().map_or(0, |text| text.len());
    options.goal = match goal_given {
        Some(goal) if goal > options.width => {
            return fail(&format!(
                "invalid width: '{goal}': Numerical result out of range"
            ))
        }
        Some(goal) => goal,
        None => (options.width * 93 + 50) / 100,
    };
    let files: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Operand(text) => Some(text.clone()),
            Item::Opt { .. } => None,
        })
        .collect();
    let files = if files.is_empty() {
        vec!["-".to_owned()]
    } else {
        files
    };
    let mut status = EXIT_OK;
    let mut out = stdout_writer();
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for file in &files {
        match open_input(file) {
            Ok(mut reader) => {
                if let Err(error) = read_lines(reader.as_mut(), &mut lines) {
                    eprintln!("{CMD}: {file}: {}", io_error_text(&error));
                    status = EXIT_FAIL;
                }
            }
            Err(error) => {
                eprintln!("{CMD}: {file}: {}", io_error_text(&error));
                status = EXIT_FAIL;
            }
        }
    }
    let result = format_lines(&lines, options, prefix.as_deref(), &mut out);
    if let Err(error) = result {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{CMD}: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    match finish_output(CMD, &mut out) {
        EXIT_OK => status,
        code => code,
    }
}

fn fail(message: &str) -> i32 {
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    EXIT_FAIL
}

/// `-NUM` is the same as `-w NUM`.
fn legacy_width(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        if arg.len() > 1 && arg.starts_with('-') && arg[1..].bytes().all(|b| b.is_ascii_digit()) {
            out.push("-w".to_owned());
            out.push(arg[1..].to_owned());
        } else {
            out.push(arg.clone());
        }
    }
    out
}

fn parse_width(value: &str) -> Result<usize, String> {
    match value.parse::<usize>() {
        Ok(width) if width > 0 => Ok(width),
        _ => Err(format!("invalid width: '{value}'")),
    }
}

fn read_lines(reader: &mut dyn BufRead, lines: &mut Vec<Vec<u8>>) -> io::Result<()> {
    let mut buffer = Vec::new();
    while reader.read_until(b'\n', &mut buffer)? > 0 {
        if buffer.last() == Some(&b'\n') {
            buffer.pop();
        }
        lines.push(std::mem::take(&mut buffer));
    }
    Ok(())
}

fn is_blank(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn sentence_end(text: &[u8]) -> bool {
    matches!(text.last(), Some(b'.' | b'?' | b'!'))
}

/// Splits one input line into words. A word's `gap` is the number of spaces before it on the
/// same input line; the first word gets 0.
fn words_of(body: &[u8]) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut index = 0;
    let mut pending_gap = 0usize;
    while index < body.len() {
        if is_blank(body[index]) {
            pending_gap += 1;
            index += 1;
            continue;
        }
        let start = index;
        while index < body.len() && !is_blank(body[index]) {
            index += 1;
        }
        let text = body[start..index].to_vec();
        let gap = if words.is_empty() { 0 } else { pending_gap };
        words.push(Word {
            sentence_end: sentence_end(&text),
            text,
            gap,
        });
        pending_gap = 0;
    }
    words
}

/// `-u`: one space between words, two after a sentence end.
fn normalise(words: &mut [Word], uniform: bool) {
    if !uniform {
        return;
    }
    for index in 1..words.len() {
        words[index].gap = if words[index - 1].sentence_end { 2 } else { 1 };
    }
}

fn format_lines(
    lines: &[Vec<u8>],
    options: Options,
    prefix: Option<&[u8]>,
    out: &mut impl Write,
) -> io::Result<()> {
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        let body = match prefix {
            Some(prefix) => match line.strip_prefix(prefix) {
                Some(rest) => rest,
                None => {
                    out.write_all(line)?;
                    out.write_all(b"\n")?;
                    index += 1;
                    continue;
                }
            },
            None => line.as_slice(),
        };
        if body.iter().all(|b| is_blank(*b)) {
            out.write_all(prefix.unwrap_or(b""))?;
            out.write_all(b"\n")?;
            index += 1;
            continue;
        }
        let mut paragraph: Vec<&[u8]> = Vec::new();
        while index < lines.len() {
            let candidate = &lines[index];
            let rest = match prefix {
                Some(prefix) => match candidate.strip_prefix(prefix) {
                    Some(rest) => rest,
                    None => break,
                },
                None => candidate.as_slice(),
            };
            if rest.iter().all(|b| is_blank(*b)) {
                break;
            }
            paragraph.push(rest);
            index += 1;
        }
        format_paragraph(&paragraph, options, prefix, out)?;
    }
    Ok(())
}

fn format_paragraph(
    lines: &[&[u8]],
    options: Options,
    prefix: Option<&[u8]>,
    out: &mut impl Write,
) -> io::Result<()> {
    let paras: Vec<Para> = lines
        .iter()
        .map(|line| Para {
            indent: line.iter().take_while(|b| is_blank(**b)).count(),
            words: words_of(line),
        })
        .collect();
    if options.split {
        // Each input line is laid out on its own; only long lines change.
        for para in &paras {
            let mut words = para.words.clone();
            normalise(&mut words, options.uniform);
            if words.is_empty() {
                continue;
            }
            let starts = layout(&words, para.indent, para.indent, options);
            emit(&words, &starts, para.indent, para.indent, prefix, out)?;
        }
        return Ok(());
    }
    let first_indent = paras[0].indent;
    let second_indent = paras.get(1).map_or(first_indent, |p| p.indent);
    let mut all_words: Vec<Word> = Vec::new();
    for (position, para) in paras.iter().enumerate() {
        for (k, word) in para.words.iter().enumerate() {
            let mut word = word.clone();
            if position > 0 && k == 0 {
                let prev_end = all_words.last().is_some_and(|w: &Word| w.sentence_end);
                word.gap = if prev_end { 2 } else { 1 };
            }
            all_words.push(word);
        }
    }
    normalise(&mut all_words, options.uniform);
    // Which first lines stand alone: by default when the first two indents differ, with -t
    // when they agree (the tag line). Crown margins always join.
    let split_head = paras.len() > 1
        && !options.crown
        && if options.tagged {
            first_indent == second_indent
        } else {
            first_indent != second_indent
        };
    let mut groups: Vec<(Vec<Word>, usize, usize)> = Vec::new();
    if split_head {
        let first_count = paras[0].words.len();
        let (head, rest) = all_words.split_at(first_count);
        groups.push((head.to_vec(), first_indent, first_indent));
        groups.push((rest.to_vec(), second_indent, second_indent));
    } else {
        groups.push((all_words, first_indent, second_indent));
    }
    for (words, first, other) in groups {
        if words.is_empty() {
            continue;
        }
        let starts = layout(&words, first, other, options);
        emit(&words, &starts, first, other, prefix, out)?;
    }
    Ok(())
}

/// Minimum-cost breaking of `words`. Returns the index of the first word of each line.
fn layout(words: &[Word], first: usize, other: usize, options: Options) -> Vec<usize> {
    let count = words.len();
    let width = options.width as i64;
    let goal = options.goal as i64;
    let margin = options.prefix_len as i64;
    let mut best = vec![i64::MAX / 4; count + 1];
    let mut back = vec![0usize; count + 1];
    best[0] = 0;
    for end in 1..=count {
        for start in (0..end).rev() {
            let indent = margin + if start == 0 { first } else { other } as i64;
            let length = indent + line_text_len(words, start, end);
            if length >= width && end - start > 1 {
                continue;
            }
            // The last line of a paragraph is free unless it holds a single word.
            let cost = if end == count && end - start > 1 {
                0
            } else {
                (goal - length) * (goal - length)
            };
            let total = best[start] + cost;
            if total <= best[end] {
                best[end] = total;
                back[end] = start;
            }
        }
    }
    let mut starts = Vec::new();
    let mut end = count;
    while end > 0 {
        let start = back[end];
        starts.push(start);
        end = start;
    }
    starts.reverse();
    starts
}

fn line_text_len(words: &[Word], start: usize, end: usize) -> i64 {
    let mut total = 0i64;
    for (offset, word) in words[start..end].iter().enumerate() {
        if offset > 0 {
            total += word.gap as i64;
        }
        total += word.text.len() as i64;
    }
    total
}

fn emit(
    words: &[Word],
    starts: &[usize],
    first: usize,
    other: usize,
    prefix: Option<&[u8]>,
    out: &mut impl Write,
) -> io::Result<()> {
    for (line_number, &start) in starts.iter().enumerate() {
        let end = starts.get(line_number + 1).copied().unwrap_or(words.len());
        let indent = if line_number == 0 { first } else { other };
        write_line(&words[start..end], indent, prefix, out)?;
    }
    Ok(())
}

fn write_line(
    words: &[Word],
    indent: usize,
    prefix: Option<&[u8]>,
    out: &mut impl Write,
) -> io::Result<()> {
    out.write_all(prefix.unwrap_or(b""))?;
    out.write_all(&vec![b' '; indent])?;
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            out.write_all(&vec![b' '; word.gap])?;
        }
        out.write_all(&word.text)?;
    }
    out.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(width: usize) -> Options {
        Options {
            width,
            goal: (width * 93 + 50) / 100,
            crown: false,
            tagged: false,
            split: false,
            uniform: false,
            prefix_len: 0,
        }
    }

    #[test]
    fn words_keep_their_gaps() {
        let words = words_of(b"a  b c");
        let gaps: Vec<usize> = words.iter().map(|w| w.gap).collect();
        assert_eq!(gaps, vec![0, 2, 1]);
    }

    #[test]
    fn layout_keeps_lines_under_width() {
        let words = words_of(b"aaa bbb ccc ddd eee");
        let starts = layout(&words, 0, 0, options(8));
        assert_eq!(starts.first(), Some(&0));
        for (line, &start) in starts.iter().enumerate() {
            let end = starts.get(line + 1).copied().unwrap_or(words.len());
            assert!(
                line_text_len(&words, start, end) < 8,
                "line {line} too long"
            );
        }
        let covered: usize = starts
            .iter()
            .enumerate()
            .map(|(line, &start)| {
                let end = starts.get(line + 1).copied().unwrap_or(words.len());
                end - start
            })
            .sum();
        assert_eq!(covered, words.len());
    }

    #[test]
    fn legacy_numeric_option_becomes_width() {
        let args = vec!["-20".to_owned(), "file".to_owned()];
        assert_eq!(legacy_width(&args), vec!["-w", "20", "file"]);
    }

    #[test]
    fn default_goal_rounds_ninety_three_percent() {
        assert_eq!((75 * 93 + 50) / 100, 70);
        assert_eq!((50 * 93 + 50) / 100, 47);
    }

    #[test]
    fn uniform_spacing_puts_two_spaces_after_sentences() {
        let mut uniform = words_of(b"Hi.   There you");
        normalise(&mut uniform, true);
        let gaps: Vec<usize> = uniform.iter().map(|w| w.gap).collect();
        assert_eq!(gaps, vec![0, 2, 1]);
        let mut kept = words_of(b"Hi.   There");
        normalise(&mut kept, false);
        assert_eq!(kept[1].gap, 3);
    }
}
