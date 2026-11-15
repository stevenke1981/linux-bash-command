use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::{fs, path::Path};

const HELP: &str = "Usage: mkdir [OPTION]... DIRECTORY...\nCreate directories.\n  -m, --mode=MODE    set file mode (validated but ignored on Windows)\n  -p, --parents      create parent directories as needed\n  -v, --verbose      print a message for each created directory\n      --help         display this help\n      --version      output version information\n";

#[cfg(unix)]
fn apply_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}
#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

fn create(
    path: &Path,
    parents: bool,
    mode: Option<u32>,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut made = Vec::new();
    if parents {
        let mut ancestors: Vec<_> = path
            .ancestors()
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        ancestors.reverse();
        for p in ancestors {
            if p.is_dir() {
                continue;
            }
            match fs::create_dir(p) {
                Ok(()) => {
                    if p == path {
                        if let Some(mode) = mode {
                            apply_mode(p, mode)?;
                        }
                    }
                    made.push(p.to_path_buf());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && p.is_dir() => {}
                Err(e) => return Err(e),
            }
        }
    } else {
        fs::create_dir(path)?;
        if let Some(mode) = mode {
            apply_mode(path, mode)?;
        }
        made.push(path.to_path_buf());
    }
    Ok(made)
}

pub fn run(argv: &[String]) -> i32 {
    let specs = [
        OptSpec::value("mode", 'm'),
        OptSpec::flag("parents", 'p'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::optional("context", 'Z'),
    ];
    let items = match args::parse_command("mkdir", argv, HELP, &specs, true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let mut parents = false;
    let mut verbose = false;
    let mut mode = None;
    let mut paths = Vec::new();
    for item in items {
        match item {
            Item::Operand(path) => paths.push(path),
            Item::Opt("parents", _) => parents = true,
            Item::Opt("verbose", _) => verbose = true,
            Item::Opt("mode", Some(text)) => match super::mode::parse(&text, 0o777, true) {
                Ok(bits) => mode = Some(bits),
                Err(e) => return error::usage("mkdir", e, 1),
            },
            Item::Opt("context", _) => {
                return error::usage(
                    "mkdir",
                    "option '--context' is not supported by agentcmd",
                    1,
                )
            }
            _ => unreachable!(),
        }
    }
    if paths.is_empty() {
        return error::usage("mkdir", "missing operand", 1);
    }
    let mut failed = false;
    let status = io::output("mkdir", |out| {
        for name in paths {
            let path = Path::new(&name);
            match create(path, parents, mode) {
                Ok(created) if verbose => {
                    for made in created {
                        writeln!(out, "mkdir: created directory '{}'", super::display(&made))?;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!(
                        "mkdir: cannot create directory '{}': {}",
                        super::display(path),
                        error::io_text(&e)
                    );
                    failed = true;
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
    fn creates_one() {
        let s = Sandbox::new();
        let p = s.path("a");
        assert_eq!(create(&p, false, None).unwrap(), vec![p.clone()]);
        assert!(p.is_dir());
    }
    #[test]
    fn creates_parents() {
        let s = Sandbox::new();
        let p = s.path("a/b");
        create(&p, true, None).unwrap();
        assert!(p.is_dir());
    }
    #[test]
    fn parents_accept_existing() {
        let s = Sandbox::new();
        let p = s.path("a");
        create(&p, true, None).unwrap();
        assert!(create(&p, true, None).unwrap().is_empty());
    }
    #[test]
    fn normal_rejects_existing() {
        let s = Sandbox::new();
        let p = s.path("a");
        create(&p, false, None).unwrap();
        assert!(create(&p, false, None).is_err());
    }
    #[test]
    fn parent_file_is_error() {
        let s = Sandbox::new();
        s.write("a", b"x");
        assert!(create(&s.path("a/b"), true, None).is_err());
    }
}
