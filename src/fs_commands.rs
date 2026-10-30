use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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

#[derive(Default)]
struct FindOptions {
    name: Option<String>,
    kind: Option<char>,
    max_depth: usize,
}

pub fn find(args: &[String]) -> i32 {
    let mut roots = Vec::new();
    let mut options = FindOptions {
        max_depth: usize::MAX,
        ..FindOptions::default()
    };
    let mut i = 0;
    let mut positional_only = false;
    while i < args.len() {
        let arg = &args[i];
        if !positional_only && arg == "--" {
            positional_only = true;
        } else if !positional_only && (arg == "-h" || arg == "--help") {
            println!("Usage: find [PATH...] [-name GLOB] [-type f|d|l] [-maxdepth N]\nFind paths recursively. Patterns match the basename; '*' matches any sequence and '?' one character.\nSymlinks are listed but never traversed.\n\nExit status: 0 on success, 2 on traversal or argument error.");
            return 0;
        } else if !positional_only && (arg == "-name" || arg == "-type" || arg == "-maxdepth") {
            i += 1;
            let Some(value) = args.get(i) else {
                eprintln!("find: {arg} requires a value");
                return 2;
            };
            match arg.as_str() {
                "-name" => options.name = Some(value.clone()),
                "-type" => {
                    let mut chars = value.chars();
                    let Some(kind @ ('f' | 'd' | 'l')) = chars.next() else {
                        eprintln!("find: -type accepts f, d, or l");
                        return 2;
                    };
                    if chars.next().is_some() {
                        eprintln!("find: -type accepts one of f, d, or l");
                        return 2;
                    }
                    options.kind = Some(kind);
                }
                "-maxdepth" => match value.parse::<usize>() {
                    Ok(depth) => options.max_depth = depth,
                    Err(_) => {
                        eprintln!("find: invalid -maxdepth value: {value}");
                        return 2;
                    }
                },
                _ => unreachable!(),
            }
        } else if !positional_only && arg.starts_with('-') && arg != "-" {
            eprintln!("find: unknown expression or option: {arg}\nTry 'find --help' for usage.");
            return 2;
        } else {
            roots.push(PathBuf::from(arg));
        }
        i += 1;
    }
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    let mut matches = Vec::new();
    for root in roots {
        if let Err(error) = visit(&root, 0, &options, &mut matches) {
            eprintln!("find: {error}");
            return 2;
        }
    }
    matches.sort();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    for path in matches {
        if let Err(error) = writeln!(output, "{}", path.display()) {
            eprintln!("find: cannot write output: {error}");
            return 2;
        }
    }
    0
}

fn visit(
    path: &Path,
    depth: usize,
    options: &FindOptions,
    matches: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot access '{}': {error}", path.display()))?;
    if matches_options(path, &metadata, options) {
        matches.push(path.to_path_buf());
    }
    if !metadata.is_dir() || depth >= options.max_depth {
        return Ok(());
    }
    let entries =
        fs::read_dir(path).map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        visit(&entry.path(), depth + 1, options, matches)?;
    }
    Ok(())
}

fn matches_options(path: &Path, metadata: &fs::Metadata, options: &FindOptions) -> bool {
    if let Some(pattern) = &options.name {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        if !wildcard_match(&name, pattern) {
            return false;
        }
    }
    if let Some(kind) = options.kind {
        let file_type = metadata.file_type();
        return match kind {
            'f' => file_type.is_file(),
            'd' => file_type.is_dir(),
            'l' => file_type.is_symlink(),
            _ => false,
        };
    }
    true
}

fn wildcard_match(text: &str, pattern: &str) -> bool {
    let text = text.chars().collect::<Vec<_>>();
    let pattern = pattern.chars().collect::<Vec<_>>();
    let mut previous = vec![false; pattern.len() + 1];
    previous[0] = true;
    for (j, token) in pattern.iter().enumerate() {
        if *token == '*' {
            previous[j + 1] = previous[j];
        }
    }
    for character in &text {
        let mut current = vec![false; pattern.len() + 1];
        for (j, token) in pattern.iter().enumerate() {
            current[j + 1] = if *token == '*' {
                current[j] || previous[j + 1]
            } else {
                previous[j] && (*token == '?' || *token == *character)
            };
        }
        previous = current;
    }
    previous[pattern.len()]
}

#[cfg(test)]
mod tests {
    use super::wildcard_match;

    #[test]
    fn find_name_globs_match_star_and_question() {
        assert!(wildcard_match("main.rs", "*.rs"));
        assert!(wildcard_match("a.txt", "?.txt"));
        assert!(!wildcard_match("long.txt", "?.txt"));
        assert!(wildcard_match("abc", "a*c"));
    }
}
