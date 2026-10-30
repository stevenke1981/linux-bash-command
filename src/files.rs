use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};

pub fn list(args: &[String]) -> i32 {
    let mut include_hidden = false;
    let mut include_common_ignored = false;
    let mut include_directories = false;
    let mut max_depth = usize::MAX;
    let mut root = None;
    let mut positional_only = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if !positional_only && arg == "--" {
            positional_only = true;
        } else if !positional_only && (arg == "--help" || arg == "-h") {
            print!("{}", list_help());
            return 0;
        } else if !positional_only && (arg == "--all" || arg == "-a") {
            include_hidden = true;
        } else if !positional_only && arg == "--no-ignore" {
            include_common_ignored = true;
        } else if !positional_only && (arg == "--dirs" || arg == "-d") {
            include_directories = true;
        } else if !positional_only && arg == "--max-depth" {
            index += 1;
            let Some(value) = args.get(index) else {
                eprintln!("list: --max-depth requires a non-negative integer");
                return 2;
            };
            let Ok(value) = value.parse::<usize>() else {
                eprintln!("list: invalid --max-depth value: {value}");
                return 2;
            };
            max_depth = value;
        } else if !positional_only && arg.starts_with('-') {
            eprintln!("list: unknown option: {arg}\nTry 'list --help' for more information.");
            return 2;
        } else if root.is_none() {
            root = Some(PathBuf::from(arg));
        } else {
            eprintln!("list: expected at most one path\nTry 'list --help' for more information.");
            return 2;
        }
        index += 1;
    }

    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let root = match fs::canonicalize(&root) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("list: cannot access '{}': {error}", root.display());
            return 2;
        }
    };
    if !root.is_dir() {
        eprintln!("list: '{}' is not a directory", root.display());
        return 2;
    }

    let mut entries = Vec::new();
    let mut errors = Vec::new();
    collect_entries(
        &root,
        &root,
        0,
        max_depth,
        include_hidden,
        include_common_ignored,
        include_directories,
        &mut entries,
        &mut errors,
    );
    entries.sort();
    for entry in entries {
        println!("{}", display_path(&entry));
    }
    for (path, error) in &errors {
        eprintln!("list: cannot read '{}': {error}", path.display());
    }
    if errors.is_empty() {
        0
    } else {
        2
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_entries(
    root: &Path,
    directory: &Path,
    depth: usize,
    max_depth: usize,
    include_hidden: bool,
    include_common_ignored: bool,
    include_directories: bool,
    entries: &mut Vec<String>,
    errors: &mut Vec<(PathBuf, io::Error)>,
) {
    if depth >= max_depth {
        return;
    }
    let children = match fs::read_dir(directory) {
        Ok(children) => children,
        Err(error) => {
            errors.push((directory.to_path_buf(), error));
            return;
        }
    };
    let mut children = children
        .filter_map(|result| match result {
            Ok(entry) => Some(entry),
            Err(error) => {
                errors.push((directory.to_path_buf(), error));
                None
            }
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|entry| entry.file_name());

    for child in children {
        let name = child.file_name();
        let name_text = name.to_string_lossy();
        if !include_hidden && name_text.starts_with('.') {
            continue;
        }
        let file_type = match child.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                errors.push((child.path(), error));
                continue;
            }
        };
        let is_directory = file_type.is_dir();
        if is_directory && !include_common_ignored && is_common_ignored_dir(&name_text) {
            continue;
        }
        if is_directory {
            if include_directories {
                entries.push(relative_path(root, &child.path()));
            }
            if depth < max_depth {
                collect_entries(
                    root,
                    &child.path(),
                    depth + 1,
                    max_depth,
                    include_hidden,
                    include_common_ignored,
                    include_directories,
                    entries,
                    errors,
                );
            }
        } else if file_type.is_file() || file_type.is_symlink() {
            entries.push(relative_path(root, &child.path()));
        }
    }
}

fn is_common_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        "target" | "node_modules" | ".git" | ".hg" | ".svn" | ".venv" | "venv" | "__pycache__"
    )
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| match part {
            Component::Normal(value) => value.to_string_lossy().into_owned(),
            other => other.as_os_str().to_string_lossy().into_owned(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn display_path(path: &str) -> String {
    path.to_owned()
}

fn list_help() -> &'static str {
    "Usage: list [OPTIONS] [PATH]\n\nRecursively list files below PATH (default: current directory), sorted by path.\n\nOptions:\n  -a, --all          Include hidden entries\n      --dirs         Include directory entries\n      --max-depth N  Limit traversal depth (direct children are depth 1)\n      --no-ignore    Include common generated and VCS directories\n  -h, --help         Show this help\n\nExit status: 0 on success, 2 on error.\n"
}

pub fn read(args: &[String]) -> i32 {
    let mut start = 1usize;
    let mut end = None;
    let mut limit = None;
    let mut path = None;
    let mut positional_only = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if !positional_only && arg == "--" {
            positional_only = true;
        } else if !positional_only && (arg == "--help" || arg == "-h") {
            print!("{}", read_help());
            return 0;
        } else if !positional_only && (arg == "--start" || arg == "--end" || arg == "--limit") {
            let option = arg.as_str();
            index += 1;
            let Some(value) = args.get(index) else {
                eprintln!("read: {option} requires a positive integer");
                return 2;
            };
            let Ok(value) = value.parse::<usize>() else {
                eprintln!("read: invalid {option} value: {value}");
                return 2;
            };
            if value == 0 {
                eprintln!("read: {option} must be greater than zero");
                return 2;
            }
            match option {
                "--start" => start = value,
                "--end" => end = Some(value),
                "--limit" => limit = Some(value),
                _ => unreachable!(),
            }
        } else if !positional_only && arg.starts_with('-') {
            eprintln!("read: unknown option: {arg}\nTry 'read --help' for more information.");
            return 2;
        } else if path.is_none() {
            path = Some(PathBuf::from(arg));
        } else {
            eprintln!(
                "read: expected exactly one file path\nTry 'read --help' for more information."
            );
            return 2;
        }
        index += 1;
    }

    let Some(path) = path else {
        eprintln!("read: missing file path\nTry 'read --help' for more information.");
        return 2;
    };
    if let Some(end) = end {
        if end < start {
            eprintln!("read: --end must be greater than or equal to --start");
            return 2;
        }
    }

    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("read: cannot read '{}': {error}", path.display());
            return 2;
        }
    };
    let count = end.map_or(usize::MAX, |last| {
        last.saturating_sub(start).saturating_add(1)
    });
    let count = limit.map_or(count, |max| count.min(max));
    let stdout = io::stdout();
    let mut output = stdout.lock();
    for (index, line) in BufReader::new(file)
        .lines()
        .enumerate()
        .take(start.saturating_sub(1).saturating_add(count))
    {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("read: cannot read '{}': {error}", path.display());
                return 2;
            }
        };
        if index + 1 < start {
            continue;
        }
        if let Err(error) = writeln!(output, "{:>6}\t{line}", index + 1) {
            eprintln!("read: cannot write output: {error}");
            return 2;
        }
    }
    0
}

fn read_help() -> &'static str {
    "Usage: read [OPTIONS] FILE\n\nPrint UTF-8 text with one-based line numbers.\n\nOptions:\n      --start N  First line to print (default: 1)\n      --end N    Last line to print, inclusive\n      --limit N  Print at most N lines from --start\n  -h, --help     Show this help\n\nExit status: 0 on success, 2 on error.\n"
}

#[cfg(test)]
mod tests {
    use super::{is_common_ignored_dir, relative_path};
    use std::path::Path;

    #[test]
    fn common_generated_directories_are_skipped_by_default() {
        assert!(is_common_ignored_dir("target"));
        assert!(is_common_ignored_dir("node_modules"));
        assert!(!is_common_ignored_dir("src"));
    }

    #[test]
    fn listed_paths_use_forward_slashes() {
        assert_eq!(
            relative_path(Path::new("C:/repo"), Path::new("C:/repo/src/main.rs")),
            "src/main.rs"
        );
    }
}
