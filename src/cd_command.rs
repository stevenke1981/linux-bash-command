use std::path::PathBuf;

pub fn run(args: &[String]) -> i32 {
    if args == ["--help"] || args == ["-h"] {
        println!("Usage: cd [--] [DIRECTORY]\n\nResolve a directory and print its absolute path. An external process cannot change its parent shell's working directory; see shell integration in README. Without DIRECTORY, resolve the user's home. Use - for OLDPWD.\n\nExit status: 0 on success, 1 on error.");
        return 0;
    }
    let args = if args.first().is_some_and(|arg| arg == "--") {
        &args[1..]
    } else {
        args
    };
    if args.len() > 1 {
        eprintln!("cd: expected at most one directory");
        return 1;
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
                eprintln!("cd: cannot get current directory: {}", error);
                return 1;
            }
        }
    };
    match std::fs::metadata(&target) {
        Ok(metadata) if metadata.is_dir() => {
            println!("{}", target.display());
            0
        }
        Ok(_) => {
            eprintln!("cd: {}: not a directory", target.display());
            1
        }
        Err(error) => {
            eprintln!("cd: {}: {}", target.display(), error);
            1
        }
    }
}
