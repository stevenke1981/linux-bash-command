use similar::TextDiff;
use std::fs;
use std::io::{self, Write};

const USAGE: &str = "Usage: diff [OPTIONS] LEFT RIGHT\n\nCompare two UTF-8 text files and print a unified diff.\n\nOptions:\n  -U, --context N  Number of unchanged lines shown around each change (default: 3)\n  -h, --help       Show this help\n\nExit status: 0 when equal, 1 when different, 2 on error.\n";

/// Compare two UTF-8 text files. Returns 0 when equal, 1 when different, and 2 on error.
pub fn run(args: &[String]) -> i32 {
    let mut context = 3usize;
    let mut paths = Vec::with_capacity(2);
    let mut options = true;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if options && (arg == "-h" || arg == "--help") {
            print!("{USAGE}");
            return 0;
        }
        if options && arg == "--" {
            options = false;
        } else if options && (arg == "-U" || arg == "--context") {
            index += 1;
            let Some(value) = args.get(index) else {
                eprintln!("diff: {arg} requires a number of context lines\n{USAGE}");
                return 2;
            };
            let Ok(parsed) = value.parse::<usize>() else {
                eprintln!("diff: invalid context line count: {value}");
                return 2;
            };
            context = parsed;
        } else if options && arg.starts_with('-') {
            eprintln!("diff: unknown option: {arg}\n{USAGE}");
            return 2;
        } else {
            paths.push(arg.as_str());
        }
        index += 1;
    }

    if paths.len() != 2 {
        eprintln!("diff: expected LEFT and RIGHT file paths\n{USAGE}");
        return 2;
    }

    let left = match fs::read_to_string(paths[0]) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!("diff: cannot read {}: {error}", paths[0]);
            return 2;
        }
    };
    let right = match fs::read_to_string(paths[1]) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!("diff: cannot read {}: {error}", paths[1]);
            return 2;
        }
    };

    if left == right {
        return 0;
    }

    let output = TextDiff::from_lines(&left, &right)
        .unified_diff()
        .context_radius(context)
        .header(paths[0], paths[1])
        .to_string();
    if let Err(error) = io::stdout().lock().write_all(output.as_bytes()) {
        eprintln!("diff: cannot write output: {error}");
        return 2;
    }
    1
}
