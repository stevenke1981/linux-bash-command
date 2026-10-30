use std::fs;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

pub fn pwd(args: &[String]) -> i32 {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("Usage: pwd\nPrint the current working directory.\n\nExit status: 0 on success, 2 on error.");
        return 0;
    }
    if !args.is_empty() {
        eprintln!("pwd: this command does not accept arguments");
        return 2;
    }
    match std::env::current_dir() {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(error) => {
            eprintln!("pwd: cannot determine current directory: {error}");
            2
        }
    }
}

pub fn ls(args: &[String]) -> i32 {
    let mut all = false;
    let mut long = false;
    let mut classify = false;
    let mut paths = Vec::new();
    let mut options = true;
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if options && (arg == "-h" || arg == "--help") {
            println!("Usage: ls [-a] [-l] [-F] [PATH...]\nList directory entries by name (non-recursive).\n\nOptions:\n  -a, --all       Include dot-prefixed names\n  -l, --long      Show type, read-only status, size and modification time\n  -F, --classify  Append / to directories and @ to symlinks\n  -h, --help      Show this help\n\nAccepted for compatibility and ignored: -1, -A, -C, --color[=WHEN],\n--show-control-chars (Git for Windows aliases ls to use some of these).\n\nExit status: 0 on success, 2 if any path could not be listed.");
            return 0;
        } else if options && (arg == "-a" || arg == "--all") {
            all = true;
        } else if options && (arg == "-l" || arg == "--long") {
            long = true;
        } else if options && arg == "--classify" {
            classify = true;
        } else if options && is_ignored_ls_long_option(arg) {
        } else if options && arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for flag in arg[1..].chars() {
                match flag {
                    'a' | 'A' => all = true,
                    'l' => long = true,
                    'F' => classify = true,
                    '1' | 'C' => {}
                    _ => {
                        eprintln!("ls: unknown option: -{flag}");
                        return 2;
                    }
                }
            }
        } else if options && arg.starts_with('-') && arg != "-" {
            eprintln!("ls: unknown option: {arg}");
            return 2;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let mut failed = false;
    for (position, path) in paths.iter().enumerate() {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => {
                if paths.len() > 1 {
                    if position > 0 {
                        println!();
                    }
                    println!("{}:", path.display());
                }
                let mut entries = match fs::read_dir(path) {
                    Ok(entries) => match entries.collect::<Result<Vec<_>, _>>() {
                        Ok(entries) => entries,
                        Err(error) => {
                            eprintln!("ls: cannot list '{}': {error}", path.display());
                            failed = true;
                            continue;
                        }
                    },
                    Err(error) => {
                        eprintln!("ls: cannot list '{}': {error}", path.display());
                        failed = true;
                        continue;
                    }
                };
                entries.sort_by_key(|entry| entry.file_name());
                for entry in entries {
                    let name = entry.file_name();
                    if !all && name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                    let mut displayed = name.to_string_lossy().into_owned();
                    if classify {
                        if let Ok(kind) = entry.file_type() {
                            displayed.push_str(classify_suffix(kind));
                        }
                    }
                    if long {
                        match entry.metadata() {
                            Ok(metadata) => print_long(&displayed, &metadata),
                            Err(error) => {
                                eprintln!(
                                    "ls: cannot inspect '{}': {error}",
                                    entry.path().display()
                                );
                                failed = true;
                            }
                        }
                    } else {
                        println!("{displayed}");
                    }
                }
            }
            Ok(metadata) => {
                let mut displayed = path.display().to_string();
                if classify {
                    displayed.push_str(classify_suffix(metadata.file_type()));
                }
                if long {
                    print_long(&displayed, &metadata);
                } else {
                    println!("{displayed}");
                }
            }
            Err(error) => {
                eprintln!("ls: cannot access '{}': {error}", path.display());
                failed = true;
            }
        }
    }
    if failed {
        2
    } else {
        0
    }
}

fn is_ignored_ls_long_option(arg: &str) -> bool {
    match arg.strip_prefix("--color") {
        Some("") => true,
        Some(when) => matches!(
            when,
            "=auto"
                | "=always"
                | "=never"
                | "=yes"
                | "=no"
                | "=tty"
                | "=force"
                | "=none"
                | "=if-tty"
        ),
        None => arg == "--show-control-chars",
    }
}

fn classify_suffix(kind: fs::FileType) -> &'static str {
    if kind.is_symlink() {
        "@"
    } else if kind.is_dir() {
        "/"
    } else {
        ""
    }
}

fn print_long(name: &str, metadata: &fs::Metadata) {
    let kind = if metadata.is_dir() {
        'd'
    } else if metadata.is_file() {
        '-'
    } else {
        'l'
    };
    let writable = if metadata.permissions().readonly() {
        "read-only"
    } else {
        "writable"
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| time.as_secs().to_string())
        .unwrap_or_else(|| "-".to_string());
    println!(
        "{kind} {writable:>9} {:>10} {:>12} {name}",
        metadata.len(),
        modified
    );
}
