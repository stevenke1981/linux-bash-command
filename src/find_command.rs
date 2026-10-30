use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
