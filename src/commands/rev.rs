use crate::common::{args, io};
use std::io::Write;

const HELP: &str = "Usage: rev [OPTION]... [FILE]...\nReverse the characters of each line.\n      --help     display this help and exit\n      --version  output version information and exit\n";

fn reverse_line(line: &[u8]) -> Vec<u8> {
    let (body, ending) = if line.ends_with(b"\n") {
        (&line[..line.len() - 1], &line[line.len() - 1..])
    } else {
        (line, &[][..])
    };
    let mut reversed = if let Ok(text) = std::str::from_utf8(body) {
        text.chars().rev().collect::<String>().into_bytes()
    } else {
        body.iter().rev().copied().collect()
    };
    reversed.extend_from_slice(ending);
    reversed
}

pub fn run(arguments: &[String]) -> i32 {
    let items = match args::parse_command("rev", arguments, HELP, &[], true) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let files = items
        .into_iter()
        .filter_map(|item| match item {
            args::Item::Operand(v) => Some(v),
            _ => None,
        })
        .collect::<Vec<_>>();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let failed = match io::for_inputs("rev", &files, |_, reader| {
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            output.write_all(&reverse_line(&line))?;
        }
        Ok(())
    }) {
        Ok(v) => v,
        Err(e) if io::is_closed_pipe(&e) => return 0,
        Err(e) => {
            eprintln!("rev: {e}");
            return 1;
        }
    };
    if failed {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ascii() {
        assert_eq!(reverse_line(b"abc\n"), b"cba\n");
    }
    #[test]
    fn unicode() {
        assert_eq!(reverse_line("aλ😊\n".as_bytes()), "😊λa\n".as_bytes());
    }
    #[test]
    fn no_final_newline() {
        assert_eq!(reverse_line(b"abc"), b"cba");
    }
    #[test]
    fn invalid_utf8_reverses_bytes() {
        assert_eq!(reverse_line(b"a\xffb\n"), b"b\xffa\n");
    }
    #[test]
    fn empty_line() {
        assert_eq!(reverse_line(b"\n"), b"\n");
    }
    #[test]
    fn cr_is_reversed_as_character() {
        assert_eq!(reverse_line(b"ab\r\n"), b"\rba\n");
    }
    #[test]
    fn empty_input() {
        assert!(reverse_line(b"").is_empty());
    }
    #[test]
    fn option_help() {
        assert_eq!(
            args::help_version("rev", &["--help".into()], HELP, true),
            Some(0)
        );
    }
}
