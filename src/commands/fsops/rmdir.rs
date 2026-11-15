use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::{fs, path::Path};
const HELP: &str = "Usage: rmdir [OPTION]... DIRECTORY...\nRemove empty directories.\n  -p, --parents     remove DIRECTORY and its ancestors\n  -v, --verbose     print each directory processed\n      --ignore-fail-on-non-empty  ignore failures for nonempty directories\n      --help        display this help\n      --version     output version information\n";

pub fn run(argv: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("parents", 'p'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::flag("ignore-fail-on-non-empty", '\0'),
    ];
    let items = match args::parse_command("rmdir", argv, HELP, &specs, true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let (mut parents, mut verbose, mut ignore, mut paths) = (false, false, false, Vec::new());
    for item in items {
        match item {
            Item::Opt("parents", _) => parents = true,
            Item::Opt("verbose", _) => verbose = true,
            Item::Opt("ignore-fail-on-non-empty", _) => ignore = true,
            Item::Operand(path) => paths.push(path),
            _ => unreachable!(),
        }
    }
    if paths.is_empty() {
        return error::usage("rmdir", "missing operand", 1);
    }
    let mut failed = false;
    let status = io::output("rmdir", |out| {
        for path in paths {
            let mut current = Path::new(&path);
            loop {
                if verbose {
                    writeln!(
                        out,
                        "rmdir: removing directory, '{}'",
                        super::display(current)
                    )?;
                }
                match fs::remove_dir(current) {
                    Ok(()) => {}
                    Err(e) if ignore && e.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
                    Err(e) => {
                        eprintln!(
                            "rmdir: failed to remove '{}': {}",
                            super::display(current),
                            error::io_text(&e)
                        );
                        failed = true;
                        break;
                    }
                }
                if !parents {
                    break;
                }
                match current.parent().filter(|p| !p.as_os_str().is_empty()) {
                    Some(parent) => current = parent,
                    None => break,
                }
            }
        }
        Ok(())
    });
    status.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::Sandbox;
    use super::*;
    #[test]
    fn missing() {
        assert_eq!(run(&[]), 1);
    }
    #[test]
    fn removes_empty() {
        let s = Sandbox::new();
        fs::create_dir(s.path("a")).unwrap();
        assert_eq!(run(&[s.arg("a")]), 0);
        assert!(!s.exists("a"));
    }
    #[test]
    fn refuses_nonempty() {
        let s = Sandbox::new();
        s.write("a/f", b"x");
        assert_eq!(run(&[s.arg("a")]), 1);
        assert!(s.exists("a/f"));
    }
    #[test]
    fn ignores_nonempty() {
        let s = Sandbox::new();
        s.write("a/f", b"x");
        assert_eq!(run(&["--ignore-fail-on-non-empty".into(), s.arg("a")]), 0);
    }
    #[test]
    fn missing_is_failure() {
        let s = Sandbox::new();
        assert_eq!(run(&[s.arg("a")]), 1);
    }
}
