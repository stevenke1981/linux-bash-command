use crate::common::error::io_text;
use crate::display_path;
use std::path::PathBuf;

pub fn run(args: &[String]) -> i32 {
    let expanded;
    let first = if let Some(arg) = args
        .first()
        .filter(|arg| arg.starts_with("--") && *arg != "--")
    {
        expanded = match crate::common::args::expand_long(arg, &["help", "version"]) {
            Ok(arg) => arg,
            Err(error) => return crate::common::error::usage("cd", error, 1),
        };
        Some(expanded.as_str())
    } else {
        args.first().map(String::as_str)
    };
    if first == Some("--version") {
        return crate::common::args::help_version("cd", &["--version".into()], "", true).unwrap();
    }
    if first == Some("--help") || args == ["-h"] {
        return crate::common::io::output("cd", |out| {
            writeln!(out, "Usage: cd [--] [DIRECTORY]\n\nResolve a directory and print its absolute path. An external process cannot change its parent shell's working directory; see shell integration in README. Without DIRECTORY, resolve the user's home. Use - for OLDPWD.\n\nOptions:\n  -h, --help  Show this help\n      --version  Show version\n\nExit status: 0 on success, 1 on error.")
        });
    }
    if let Some(arg) = first.filter(|arg| arg.starts_with("--") && arg.contains('=')) {
        return crate::common::error::usage(
            "cd",
            format!(
                "option '{}' doesn't allow an argument",
                arg.split('=').next().unwrap()
            ),
            1,
        );
    }
    let args = if args.first().is_some_and(|arg| arg == "--") {
        &args[1..]
    } else {
        args
    };
    if args.len() > 1 {
        return crate::common::error::usage("cd", "expected at most one directory", 1);
    }
    let target = match args.first().map(String::as_str) {
        None => std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
        Some("-") => std::env::var_os("OLDPWD"),
        Some(path) => Some(path.into()),
    };
    let Some(target) = target else {
        eprintln!("cd: home or previous directory is unavailable");
        return 1;
    };
    let target = PathBuf::from(target);
    let target = if target.is_absolute() {
        target
    } else {
        match std::env::current_dir() {
            Ok(current) => current.join(target),
            Err(error) => {
                eprintln!("cd: cannot get current directory: {}", io_text(&error));
                return 1;
            }
        }
    };
    match std::fs::metadata(&target) {
        Ok(metadata) if metadata.is_dir() => {
            println!("{}", display_path(&target));
            0
        }
        Ok(_) => {
            eprintln!("cd: {}: Not a directory", display_path(&target));
            1
        }
        Err(error) => {
            eprintln!("cd: {}: {}", display_path(&target), io_text(&error));
            1
        }
    }
}
