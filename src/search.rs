use ignore::{overrides::OverrideBuilder, WalkBuilder};
use regex::{Regex, RegexBuilder};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

struct Options {
    pattern: String,
    paths: Vec<PathBuf>,
    case_insensitive: bool,
    line_numbers: bool,
    hidden: bool,
    no_ignore: bool,
    json: bool,
    globs: Vec<String>,
    fixed: bool,
}

fn usage(command: &str) {
    println!("Usage: {command} [OPTIONS] PATTERN [PATH...]");
    println!("Search text files recursively with a regular expression; use - as a path to search standard input.");
    println!("\nOptions:");
    println!("  -e, --regexp PATTERN  Set the search pattern");
    println!("  -i, --ignore-case     Match without case sensitivity");
    println!("  -n, --line-number     Print matching line numbers (default)");
    println!("      --no-line-number  Omit line numbers");
    println!("  -F, --fixed-strings   Treat the pattern as a literal string");
    println!("  -g, --glob GLOB       Include paths matching a glob (repeatable)");
    println!("      --hidden          Search hidden files and directories");
    println!("      --no-ignore       Do not honor ignore files");
    println!("      --json            Emit one JSON object per matching line");
    println!("  -h, --help            Show this help");
    println!("\nExit status: 0 when matches are found, 1 when none are found, 2 on error.");
}

fn parse(args: &[String], command: &str, stdin_default: bool) -> Result<Option<Options>, String> {
    let mut pattern = None;
    let mut paths = Vec::new();
    let mut case_insensitive = false;
    let mut line_numbers = true;
    let mut hidden = false;
    let mut no_ignore = false;
    let mut json = false;
    let mut fixed = false;
    let mut globs = Vec::new();
    let mut i = 0;
    let mut options = true;

    while i < args.len() {
        let arg = &args[i];
        if options && (arg == "-h" || arg == "--help") {
            usage(command);
            return Ok(None);
        }
        if options && arg == "--" {
            options = false;
        } else if options && (arg == "-i" || arg == "--ignore-case") {
            case_insensitive = true;
        } else if options && (arg == "-n" || arg == "--line-number") {
            line_numbers = true;
        } else if options && (arg == "--no-line-number") {
            line_numbers = false;
        } else if options && (arg == "-F" || arg == "--fixed-strings") {
            fixed = true;
        } else if options && arg == "--hidden" {
            hidden = true;
        } else if options && arg == "--no-ignore" {
            no_ignore = true;
        } else if options && arg == "--json" {
            json = true;
        } else if options && (arg == "-e" || arg == "--regexp" || arg == "-g" || arg == "--glob") {
            i += 1;
            let value = args
                .get(i)
                .ok_or_else(|| format!("{arg} requires a value"))?;
            if arg == "-g" || arg == "--glob" {
                globs.push(value.clone());
            } else if pattern.replace(value.clone()).is_some() {
                return Err("search pattern was provided more than once".into());
            }
        } else if options && arg.starts_with("--glob=") {
            globs.push(arg[7..].to_string());
        } else if options && arg.starts_with('-') && arg != "-" {
            return Err(format!("unknown option: {arg}"));
        } else if pattern.is_none() {
            pattern = Some(arg.clone());
        } else {
            paths.push(PathBuf::from(arg));
        }
        i += 1;
    }

    let pattern = pattern.ok_or_else(|| "missing search pattern (try --help)".to_string())?;
    if paths.is_empty() {
        paths.push(PathBuf::from(if stdin_default { "-" } else { "." }));
    }
    Ok(Some(Options {
        pattern,
        paths,
        case_insensitive,
        line_numbers,
        hidden,
        no_ignore,
        json,
        globs,
        fixed,
    }))
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn search_file(path: &Path, regex: &Regex, options: &Options) -> Result<usize, String> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| format!("standard input: {error}"))?;
        bytes
    } else {
        fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?
    };
    if bytes.contains(&0) {
        return Ok(0);
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("{}: invalid UTF-8: {error}", path.display()))?;
    let mut matches = 0;
    for (index, line) in text.lines().enumerate() {
        if regex.is_match(line) {
            matches += 1;
            if options.json {
                println!(
                    "{{\"path\":{},\"line_number\":{},\"text\":{}}}",
                    json_string(&path.display().to_string()),
                    index + 1,
                    json_string(line)
                );
            } else if options.line_numbers {
                println!("{}:{}:{}", path.display(), index + 1, line);
            } else {
                println!("{}:{}", path.display(), line);
            }
        }
    }
    Ok(matches)
}

pub fn run(args: &[String]) -> i32 {
    run_impl(args, "search", false)
}

pub fn run_rg(args: &[String]) -> i32 {
    run_impl(args, "rg", grep_cli::is_readable_stdin())
}

fn run_impl(args: &[String], command: &str, stdin_default: bool) -> i32 {
    let options = match parse(args, command, stdin_default) {
        Ok(Some(options)) => options,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("{command}: {error}");
            return 2;
        }
    };
    let expression = if options.fixed {
        regex::escape(&options.pattern)
    } else {
        options.pattern.clone()
    };
    let regex = match RegexBuilder::new(&expression)
        .case_insensitive(options.case_insensitive)
        .build()
    {
        Ok(regex) => regex,
        Err(error) => {
            eprintln!("{command}: invalid regular expression: {error}");
            return 2;
        }
    };

    let mut found = false;
    let mut had_error = false;
    for root in &options.paths {
        if root == Path::new("-") || root.is_file() {
            match search_file(root, &regex, &options) {
                Ok(count) => found |= count > 0,
                Err(error) => {
                    eprintln!("{command}: {error}");
                    had_error = true;
                }
            }
            continue;
        }
        if !root.exists() {
            eprintln!("{command}: {}: path does not exist", root.display());
            had_error = true;
            continue;
        }
        let mut walker = WalkBuilder::new(root);
        walker
            .hidden(!options.hidden)
            .ignore(!options.no_ignore)
            .git_ignore(!options.no_ignore)
            .git_global(!options.no_ignore)
            .parents(!options.no_ignore);
        if !options.globs.is_empty() {
            let mut builder = OverrideBuilder::new(root);
            let mut valid = true;
            for glob in &options.globs {
                if let Err(error) = builder.add(glob) {
                    eprintln!("{command}: invalid glob {glob:?}: {error}");
                    had_error = true;
                    valid = false;
                    break;
                }
            }
            if !valid {
                continue;
            }
            match builder.build() {
                Ok(value) => walker.overrides(value),
                Err(error) => {
                    eprintln!("{command}: invalid glob: {error}");
                    had_error = true;
                    continue;
                }
            };
        }
        for entry in walker.build() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!("{command}: {error}");
                    had_error = true;
                    continue;
                }
            };
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                match search_file(entry.path(), &regex, &options) {
                    Ok(count) => found |= count > 0,
                    Err(error) => {
                        eprintln!("{command}: {error}");
                        had_error = true;
                    }
                }
            }
        }
    }
    if had_error {
        2
    } else if found {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_string_escapes_regex_metacharacters() {
        let args = vec!["-F".into(), "a.b".into()];
        let options = parse(&args, "search", false).unwrap().unwrap();
        let expression = regex::escape(&options.pattern);
        let regex = Regex::new(&expression).unwrap();
        assert!(regex.is_match("xa.by"));
        assert!(!regex.is_match("axby"));
    }

    #[test]
    fn json_escapes_control_characters() {
        assert_eq!(json_string("a\n\"b"), "\"a\\n\\\"b\"");
    }
}
