//! `yes`: repeatedly writes a line made of its operands (default `y`). Options are parsed
//! GNU-style (with permutation), so `--help` and `--version` work after operands.

use std::io::Write;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{is_broken_pipe, stdout_writer};

const HELP: &str = r#"Usage: yes [STRING]...
  or:  yes OPTION
Repeatedly output a line with all specified STRING(s), or 'y'.

      --help     display this help and exit
      --version  output version information and exit
"#;

/// Size of the pre-filled block written per call; a whole number of lines.
const BLOCK_TARGET: usize = 8 * 1024;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Builds the repeated output: `line` (with its newline) copied until the block is large.
fn fill_block(line: &[u8]) -> Vec<u8> {
    let mut block = Vec::with_capacity(BLOCK_TARGET + line.len());
    while block.len() < BLOCK_TARGET {
        block.extend_from_slice(line);
    }
    block
}

/// The line `yes` repeats for `words`: the words joined by spaces, or `y`.
fn line_for(words: &[String]) -> Vec<u8> {
    let text = if words.is_empty() {
        "y".to_owned()
    } else {
        words.join(" ")
    };
    let mut line = text.into_bytes();
    line.push(b'\n');
    line
}

/// Writes `block` forever; returns only on a write error.
fn stream(block: &[u8]) -> i32 {
    let mut out = stdout_writer();
    loop {
        if let Err(error) = out.write_all(block) {
            if is_broken_pipe(&error) {
                return EXIT_OK;
            }
            report("yes", "standard output", &error);
            return EXIT_FAIL;
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("yes", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("yes", &error, EXIT_FAIL),
    };
    let mut words = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                let mut out = stdout_writer();
                let _ = out.write_all(HELP.as_bytes());
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version("yes"),
            Item::Opt { .. } => {}
            Item::Operand(word) => words.push(word),
        }
    }
    stream(&fill_block(&line_for(&words)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_line_is_y() {
        assert_eq!(line_for(&[]), b"y\n");
    }

    #[test]
    fn operands_are_joined_with_single_spaces() {
        assert_eq!(line_for(&words(&["a", "b c"])), b"a b c\n");
    }

    #[test]
    fn block_is_a_whole_number_of_lines_and_large_enough() {
        let line = b"ab\n";
        let block = fill_block(line);
        assert!(block.len() >= BLOCK_TARGET);
        assert_eq!(block.len() % line.len(), 0);
        assert!(block.chunks(line.len()).all(|chunk| chunk == line));
    }

    #[test]
    fn block_for_long_line_still_holds_one_line() {
        let line = vec![b'x'; BLOCK_TARGET + 10];
        let block = fill_block(&line);
        assert_eq!(block.len() % line.len(), 0);
        assert_eq!(block.len(), line.len());
    }

    #[test]
    fn parser_treats_double_dash_as_end_of_options() {
        let items = Parser::new("yes", SPECS)
            .parse(&words(&["--", "-y"]))
            .unwrap();
        assert_eq!(items, vec![Item::Operand("-y".to_owned())]);
    }

    #[test]
    fn unknown_short_option_is_a_usage_error() {
        assert!(Parser::new("yes", SPECS).parse(&words(&["-x"])).is_err());
    }
}
