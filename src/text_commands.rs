use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

pub fn cat(args: &[String]) -> i32 {
    let mut paths = Vec::new();
    let mut positional_only = false;
    for arg in args {
        if !positional_only && arg == "--" {
            positional_only = true;
        } else if !positional_only && matches!(arg.as_str(), "-h" | "--help") {
            print!("{CAT_HELP}");
            return 0;
        } else if !positional_only && arg.starts_with('-') && arg != "-" {
            eprintln!("cat: unknown option: {arg}\nTry 'cat --help' for usage.");
            return 2;
        } else {
            paths.push(arg.as_str());
        }
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let inputs = if paths.is_empty() { vec!["-"] } else { paths };
    let mut had_error = false;
    for path in inputs {
        let result = if path == "-" {
            io::copy(&mut stdin.lock(), &mut output)
        } else {
            match File::open(path) {
                Ok(mut file) => io::copy(&mut file, &mut output),
                Err(error) => {
                    eprintln!("cat: cannot open '{}': {error}", path);
                    had_error = true;
                    continue;
                }
            }
        };
        if let Err(error) = result {
            eprintln!("cat: cannot copy '{}': {error}", path);
            had_error = true;
            if error.kind() == io::ErrorKind::BrokenPipe {
                return 2;
            }
        }
    }
    if had_error {
        2
    } else {
        0
    }
}

pub fn head(args: &[String]) -> i32 {
    let (count, paths) = match parse_lines(args, "head", HEAD_HELP) {
        Ok(Some(options)) => options,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("head: {error}\nTry 'head --help' for usage.");
            return 2;
        }
    };
    let mut output = io::stdout().lock();
    let mut had_error = false;
    for (index, path) in paths.iter().enumerate() {
        if paths.len() > 1 {
            if index > 0 && writeln!(output).is_err() {
                eprintln!("head: cannot write output");
                return 2;
            }
            if writeln!(output, "==> {} <==", path).is_err() {
                eprintln!("head: cannot write output");
                return 2;
            }
        }
        let result = with_input(path, "head", |input| {
            copy_first_lines(input, count, &mut output)
        });
        if let Err(error) = result {
            eprintln!("head: {error}");
            had_error = true;
        }
    }
    if had_error {
        2
    } else {
        0
    }
}

pub fn tail(args: &[String]) -> i32 {
    let (count, paths) = match parse_lines(args, "tail", TAIL_HELP) {
        Ok(Some(options)) => options,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("tail: {error}\nTry 'tail --help' for usage.");
            return 2;
        }
    };
    let mut output = io::stdout().lock();
    let mut had_error = false;
    for (index, path) in paths.iter().enumerate() {
        if paths.len() > 1 {
            if index > 0 && writeln!(output).is_err() {
                eprintln!("tail: cannot write output");
                return 2;
            }
            if writeln!(output, "==> {} <==", path).is_err() {
                eprintln!("tail: cannot write output");
                return 2;
            }
        }
        let result = with_input(path, "tail", |input| {
            copy_last_lines(input, count, &mut output)
        });
        if let Err(error) = result {
            eprintln!("tail: {error}");
            had_error = true;
        }
    }
    if had_error {
        2
    } else {
        0
    }
}

fn parse_lines(
    args: &[String],
    command: &str,
    help: &str,
) -> Result<Option<(usize, Vec<String>)>, String> {
    let mut count = 10usize;
    let mut paths = Vec::new();
    let mut options = true;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if options && matches!(arg.as_str(), "-h" | "--help") {
            print!("{help}");
            return Ok(None);
        }
        if options && arg == "--" {
            options = false;
        } else if options && (arg == "-n" || arg == "--lines") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| format!("{arg} requires a line count"))?;
            count = parse_count(value, command)?;
        } else if options && arg.starts_with("--lines=") {
            count = parse_count(&arg[8..], command)?;
        } else if options && arg.starts_with("-n") && arg.len() > 2 {
            count = parse_count(&arg[2..], command)?;
        } else if options && arg.starts_with('-') && arg != "-" {
            return Err(format!("unknown option: {arg}"));
        } else {
            paths.push(arg.clone());
        }
        index += 1;
    }
    if paths.is_empty() {
        paths.push("-".to_string());
    }
    Ok(Some((count, paths)))
}

fn parse_count(value: &str, command: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("invalid line count for {command}: {value}"))
}

fn with_input<T>(
    path: &str,
    command: &str,
    operation: impl FnOnce(&mut dyn BufRead) -> io::Result<T>,
) -> io::Result<T> {
    if path == "-" {
        let stdin = io::stdin();
        return operation(&mut stdin.lock());
    }
    let file = File::open(Path::new(path))
        .map_err(|error| io::Error::new(error.kind(), format!("cannot open '{path}': {error}")))?;
    let mut input = BufReader::new(file);
    operation(&mut input).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot read '{path}' with {command}: {error}"),
        )
    })
}

fn copy_first_lines(
    input: &mut dyn BufRead,
    count: usize,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut line = Vec::new();
    for _ in 0..count {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        output.write_all(&line)?;
    }
    Ok(())
}

fn copy_last_lines(
    input: &mut dyn BufRead,
    count: usize,
    output: &mut dyn Write,
) -> io::Result<()> {
    if count == 0 {
        return Ok(());
    }
    let mut lines = VecDeque::with_capacity(count.min(1024));
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if lines.len() == count {
            lines.pop_front();
        }
        lines.push_back(line.clone());
    }
    for line in lines {
        output.write_all(&line)?;
    }
    Ok(())
}

const CAT_HELP: &str = "Usage: cat [FILE...]\n\nCopy files to standard output. With no FILE, read standard input. Use '-' for standard input.\n\nOptions:\n  -h, --help  Show this help\n";
const HEAD_HELP: &str = "Usage: head [-n N | --lines N] [FILE...]\n\nPrint the first N lines of each file (default: 10). With no FILE, read standard input. Use '-' for standard input.\n\nOptions:\n  -n, --lines N  Number of lines to print (zero is allowed)\n  -h, --help     Show this help\n";
const TAIL_HELP: &str = "Usage: tail [-n N | --lines N] [FILE...]\n\nPrint the last N lines of each file (default: 10). With no FILE, read standard input. Use '-' for standard input.\n\nOptions:\n  -n, --lines N  Number of lines to print (zero is allowed)\n  -h, --help     Show this help\n";
