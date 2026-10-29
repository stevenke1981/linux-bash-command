use std::path::PathBuf;

fn main() {
    let args = agentcmd::arguments("cd");
    if args == ["--help"] || args == ["-h"] {
        println!("Usage: cd [--] [DIRECTORY]\n\nResolve a directory and print its absolute path. An external process cannot change its parent shell's working directory; see shell integration in README. Without DIRECTORY, resolve the user's home. Use - for OLDPWD.\n\nExit status: 0 on success, 2 on error.");
        return;
    }
    let args = if args.first().is_some_and(|arg| arg == "--") {
        &args[1..]
    } else {
        &args[..]
    };
    if args.len() > 1 {
        eprintln!("cd: expected at most one directory");
        std::process::exit(2);
    }
    let target = match args.first().map(String::as_str) {
        None => std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
        Some("-") => std::env::var_os("OLDPWD"),
        Some(path) => Some(path.into()),
    };
    let Some(target) = target else {
        eprintln!("cd: home or previous directory is unavailable");
        std::process::exit(2);
    };
    let target = PathBuf::from(target);
    let target = if target.is_absolute() {
        target
    } else {
        match std::env::current_dir() {
            Ok(current) => current.join(target),
            Err(error) => {
                eprintln!("cd: cannot get current directory: {error}");
                std::process::exit(2);
            }
        }
    };
    match std::fs::metadata(&target) {
        Ok(metadata) if metadata.is_dir() => println!("{}", target.display()),
        Ok(_) => {
            eprintln!("cd: {}: not a directory", target.display());
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("cd: {}: {error}", target.display());
            std::process::exit(2);
        }
    }
}
