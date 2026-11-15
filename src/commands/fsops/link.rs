use crate::common::{
    args::{self, Item},
    error,
};
use std::{fs, path::Path};
const HELP: &str = "Usage: link FILE1 FILE2\nCall the link function to create a hard link to FILE1 named FILE2.\n      --help     display this help and exit\n      --version  output version information and exit\n";

pub fn run(argv: &[String]) -> i32 {
    let items = match args::parse_command("link", argv, HELP, &[], true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let operands: Vec<_> = items
        .into_iter()
        .filter_map(|item| match item {
            Item::Operand(v) => Some(v),
            _ => None,
        })
        .collect();
    match operands.as_slice() {
        [] => error::usage("link", "missing operand", 1),
        [first] => error::usage(
            "link",
            format!("missing operand after {}", super::quote(Path::new(first))),
            1,
        ),
        [first, second] => match fs::hard_link(first, second) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!(
                    "link: cannot create link {} to {}: {}",
                    super::quote(Path::new(second)),
                    super::quote(Path::new(first)),
                    error::io_text(&e)
                );
                1
            }
        },
        [_, _, extra, ..] => error::usage(
            "link",
            format!("extra operand {}", super::quote(Path::new(extra))),
            1,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::Sandbox;
    use super::*;
    #[test]
    fn missing_args() {
        assert_eq!(run(&[]), 1);
    }
    #[test]
    fn too_few_args() {
        let s = Sandbox::new();
        assert_eq!(run(&[s.arg("a")]), 1);
    }
    #[test]
    fn too_many_args() {
        let s = Sandbox::new();
        assert_eq!(run(&[s.arg("a"), s.arg("b"), s.arg("c")]), 1);
    }
    #[test]
    fn creates_hard_link() {
        let s = Sandbox::new();
        s.write("a", b"contents");
        assert_eq!(run(&[s.arg("a"), s.arg("b")]), 0);
        assert_eq!(fs::read(s.path("b")).unwrap(), b"contents");
    }
    #[test]
    fn existing_destination_fails() {
        let s = Sandbox::new();
        s.write("a", b"a");
        s.write("b", b"b");
        assert_eq!(run(&[s.arg("a"), s.arg("b")]), 1);
        assert_eq!(fs::read(s.path("b")).unwrap(), b"b");
    }
}
