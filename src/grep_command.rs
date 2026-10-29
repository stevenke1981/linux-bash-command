use regex::{Regex, RegexBuilder};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const USAGE: &str = "Usage: grep [OPTIONS] PATTERN [FILE...]\n\nSearch lines in files or standard input.\n\nOptions:\n  -i, --ignore-case       Ignore case distinctions\n  -n, --line-number       Prefix each result with its line number\n  -H, --with-filename     Always print the file name\n  -h, --no-filename       Never print the file name\n  -F, --fixed-strings     Treat PATTERN as a literal string\n  -v, --invert-match      Select non-matching lines\n  -c, --count             Print a count for each input\n  -e, --regexp PATTERN    Use PATTERN as the expression\n      --                  End option parsing\n      --help              Show this help\n\nExit status: 0 when any line is selected, 1 when none are selected, 2 on error.\n";

struct Options {
    pattern: String,
    paths: Vec<PathBuf>,
    ignore_case: bool,
    line_number: bool,
    with_filename: Option<bool>,
    fixed: bool,
    invert: bool,
    count: bool,
}

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let mut pattern = None;
    let mut paths = Vec::new();
    let mut ignore_case = false;
    let mut line_number = false;
    let mut with_filename = None;
    let mut fixed = false;
    let mut invert = false;
    let mut count = false;
    let mut options = true;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if options && arg == "--help" {
            print!("{USAGE}");
            return Ok(None);
        }
        if options && arg == "--" {
            options = false;
        } else if options && matches!(arg.as_str(), "-i" | "--ignore-case") {
            ignore_case = true;
        } else if options && matches!(arg.as_str(), "-n" | "--line-number") {
            line_number = true;
        } else if options && matches!(arg.as_str(), "-H" | "--with-filename") {
            with_filename = Some(true);
        } else if options && matches!(arg.as_str(), "-h" | "--no-filename") {
            with_filename = Some(false);
        } else if options && matches!(arg.as_str(), "-F" | "--fixed-strings") {
            fixed = true;
        } else if options && matches!(arg.as_str(), "-v" | "--invert-match") {
            invert = true;
        } else if options && matches!(arg.as_str(), "-c" | "--count") {
            count = true;
        } else if options && matches!(arg.as_str(), "-e" | "--regexp") {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(format!("{arg} requires a pattern\n{USAGE}"));
            };
            if pattern.replace(value.clone()).is_some() {
                return Err(format!("pattern was provided more than once\n{USAGE}"));
            }
        } else if options && arg.starts_with('-') && arg.len() > 2 && !arg.starts_with("--") {
            for flag in arg[1..].chars() {
                match flag {
                    'i' => ignore_case = true,
                    'n' => line_number = true,
                    'H' => with_filename = Some(true),
                    'h' => with_filename = Some(false),
                    'F' => fixed = true,
                    'v' => invert = true,
                    'c' => count = true,
                    _ => return Err(format!("unknown option: -{flag}\n{USAGE}")),
                }
            }
        } else if options && arg.starts_with('-') && arg != "-" {
            return Err(format!("unknown option: {arg}\n{USAGE}"));
        } else if pattern.is_none() {
            pattern = Some(arg.clone());
        } else {
            paths.push(PathBuf::from(arg));
        }
        index += 1;
    }
    let pattern = pattern.ok_or_else(|| format!("missing pattern\n{USAGE}"))?;
    Ok(Some(Options {
        pattern,
        paths,
        ignore_case,
        line_number,
        with_filename,
        fixed,
        invert,
        count,
    }))
}

fn search_reader<R: BufRead>(
    reader: R,
    path: Option<&Path>,
    options: &Options,
    regex: &Regex,
    show_filename: bool,
    output: &mut impl Write,
) -> Result<usize, String> {
    let mut selected = 0;
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| {
            path.map_or_else(
                || format!("standard input: {error}"),
                |path| format!("{}: {error}", path.display()),
            )
        })?;
        let is_match = regex.is_match(&line);
        if is_match == options.invert {
            continue;
        }
        selected += 1;
        if options.count {
            continue;
        }
        if show_filename {
            if let Some(path) = path {
                write!(output, "{}:", path.display()).map_err(|error| error.to_string())?;
            }
        }
        if options.line_number {
            write!(output, "{}:", index + 1).map_err(|error| error.to_string())?;
        }
        writeln!(output, "{line}").map_err(|error| error.to_string())?;
    }
    if options.count {
        if show_filename {
            if let Some(path) = path {
                write!(output, "{}:", path.display()).map_err(|error| error.to_string())?;
            }
        }
        writeln!(output, "{selected}").map_err(|error| error.to_string())?;
    }
    Ok(selected)
}

fn search_path(
    path: &Path,
    options: &Options,
    regex: &Regex,
    show_filename: bool,
    output: &mut impl Write,
) -> Result<usize, String> {
    if path == Path::new("-") {
        return search_reader(
            io::stdin().lock(),
            None,
            options,
            regex,
            show_filename,
            output,
        );
    }
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    search_reader(
        BufReader::new(file),
        Some(path),
        options,
        regex,
        show_filename,
        output,
    )
}

pub fn run(args: &[String]) -> i32 {
    let Some(options) = (match parse(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("grep: {error}");
            return 2;
        }
    }) else {
        return 0;
    };
    let expression = if options.fixed {
        regex::escape(&options.pattern)
    } else {
        options.pattern.clone()
    };
    let regex = match RegexBuilder::new(&expression)
        .case_insensitive(options.ignore_case)
        .build()
    {
        Ok(regex) => regex,
        Err(error) => {
            eprintln!("grep: invalid regular expression: {error}");
            return 2;
        }
    };

    let show_filename = options.with_filename.unwrap_or(options.paths.len() > 1);
    let inputs: Vec<PathBuf> = if options.paths.is_empty() {
        vec![PathBuf::from("-")]
    } else {
        options.paths.clone()
    };
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut found = false;
    let mut had_error = false;
    for path in inputs {
        match search_path(&path, &options, &regex, show_filename, &mut output) {
            Ok(count) => found |= count > 0,
            Err(error) => {
                eprintln!("grep: {error}");
                had_error = true;
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
    fn fixed_pattern_escapes_regex_syntax() {
        let expression = regex::escape("a.b");
        let regex = Regex::new(&expression).unwrap();
        assert!(regex.is_match("xa.by"));
        assert!(!regex.is_match("axby"));
    }

    #[test]
    fn invert_selects_nonmatching_lines() {
        let options = Options {
            pattern: "hit".into(),
            paths: Vec::new(),
            ignore_case: false,
            line_number: false,
            with_filename: None,
            fixed: false,
            invert: true,
            count: false,
        };
        let mut output = Vec::new();
        let count = search_reader(
            "hit\nkeep\n".as_bytes(),
            None,
            &options,
            &Regex::new("hit").unwrap(),
            false,
            &mut output,
        )
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(output, b"keep\n");
    }
}
