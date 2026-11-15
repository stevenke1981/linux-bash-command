use crate::common::{
    args::{self, Item},
    error,
};
use std::{fs, path::Path};
const HELP: &str = "Usage: unlink FILE\nCall the unlink function to remove FILE.\n      --help     display this help and exit\n      --version  output version information and exit\n";

fn remove(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        return Err(std::io::Error::from(std::io::ErrorKind::IsADirectory));
    }
    #[cfg(windows)]
    if meta.file_type().is_symlink() && path.is_dir() {
        return fs::remove_dir(path);
    }
    fs::remove_file(path)
}

pub fn run(argv: &[String]) -> i32 {
    let items = match args::parse_command("unlink", argv, HELP, &[], true) {
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
        [] => error::usage("unlink", "missing operand", 1),
        [first] => match remove(Path::new(first)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!(
                    "unlink: cannot unlink {}: {}",
                    super::quote(Path::new(first)),
                    error::io_text(&e)
                );
                1
            }
        },
        [_, extra, ..] => error::usage(
            "unlink",
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
    fn too_many_args() {
        let s = Sandbox::new();
        assert_eq!(run(&[s.arg("a"), s.arg("b")]), 1);
    }
    #[test]
    fn removes_file() {
        let s = Sandbox::new();
        s.write("a", b"x");
        assert_eq!(run(&[s.arg("a")]), 0);
        assert!(!s.exists("a"));
    }
    #[test]
    fn refuses_directory() {
        let s = Sandbox::new();
        fs::create_dir(s.path("a")).unwrap();
        assert_eq!(run(&[s.arg("a")]), 1);
        assert!(s.exists("a"));
    }
    #[test]
    fn nonexistent_is_failure() {
        let s = Sandbox::new();
        assert_eq!(run(&[s.arg("a")]), 1);
    }
}
