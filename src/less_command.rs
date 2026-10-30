use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

const HELP: &str = "Usage: less [OPTIONS] [+CMD] [FILE ...]\n\nDisplay files one screen at a time. With no FILE, copy standard input to standard output.\nWhen standard output is not a terminal, the content is copied unchanged, like cat.\n\nOptions:\n  -N, --LINE-NUMBERS       Show line numbers\n  -F, --quit-if-one-screen Exit at once when the file fits on one screen\n  -X                       Do not switch to the alternate screen\n  -R, -r                   Raw control characters (output already passes through)\n  -S -i -I -M -m -E -e -K -Q\n                           Accepted and ignored (see the long options below)\n      --no-init, --chop-long-lines, --RAW-CONTROL-CHARS, --ignore-case,\n      --IGNORE-CASE, --quit-at-eof, --quit-on-intr, --silent, --QUIET,\n      --no-keypad          Accepted and ignored\n  +N                       Start at line N (interactive only)\n  +G                       Start at the last page (interactive only)\n  +/PATTERN                Start at the first line containing PATTERN (interactive only)\n  -h, --help               Show this help\n\nInteractive keys (when FILE is a terminal): Space/PageDown next page, Enter/j one line,\nb/PageUp/k previous page, g first page, G last page, q quit. Long lines are clipped.\n";

/// Where the interactive pager starts, from a `+CMD` argument.
#[derive(Default)]
enum Start {
    #[default]
    Top,
    Line(usize),
    End,
    Pattern(String),
}

struct LessOptions {
    numbered: bool,
    quit_if_one_screen: bool,
    alternate_screen: bool,
    start: Start,
}

impl Default for LessOptions {
    fn default() -> Self {
        Self {
            numbered: false,
            quit_if_one_screen: false,
            alternate_screen: true,
            start: Start::Top,
        }
    }
}

struct TerminalSession {
    alternate_screen: bool,
}

impl TerminalSession {
    fn enter(alternate_screen: bool) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if alternate_screen {
            if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, Clear(ClearType::All))
            {
                let _ = terminal::disable_raw_mode();
                return Err(error);
            }
        }
        Ok(Self { alternate_screen })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.alternate_screen {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
        let _ = terminal::disable_raw_mode();
    }
}

fn read_sources(paths: &[PathBuf]) -> io::Result<Vec<u8>> {
    let mut contents = Vec::new();
    for path in paths {
        let bytes = if path == Path::new("-") {
            let mut bytes = Vec::new();
            io::stdin().read_to_end(&mut bytes)?;
            bytes
        } else {
            fs::read(path)?
        };
        contents.extend_from_slice(&bytes);
    }
    Ok(contents)
}

fn render_page(
    lines: &[String],
    start: usize,
    page_lines: usize,
    numbered: bool,
) -> io::Result<()> {
    let (width, _) = terminal::size()?;
    let mut output = io::stdout();
    execute!(output, Clear(ClearType::All), MoveTo(0, 0))?;
    for (offset, line) in lines.iter().skip(start).take(page_lines).enumerate() {
        if numbered {
            write!(output, "{:>6} ", start + offset + 1)?;
        }
        let available = usize::from(width).saturating_sub(if numbered { 7 } else { 0 });
        let clipped: String = line.chars().take(available).collect();
        writeln!(output, "{clipped}")?;
    }
    execute!(
        output,
        MoveTo(0, page_lines as u16),
        Clear(ClearType::CurrentLine)
    )?;
    write!(
        output,
        "-- More -- ({}/{})  Space next, b back, q quit",
        start.saturating_add(1).min(lines.len()),
        lines.len()
    )?;
    output.flush()
}

fn initial_start(start: &Start, lines: &[String], page_lines: usize) -> usize {
    match start {
        Start::Top => 0,
        Start::Line(line) => (*line).min(lines.len().saturating_sub(1)),
        Start::End => lines.len().saturating_sub(page_lines),
        Start::Pattern(pattern) => lines
            .iter()
            .position(|line| line.contains(pattern.as_str()))
            .unwrap_or(0),
    }
}

fn page(lines: &[String], options: &LessOptions) -> io::Result<()> {
    let _terminal = TerminalSession::enter(options.alternate_screen)?;
    let (_, rows) = terminal::size()?;
    let page_lines = usize::from(rows.saturating_sub(1)).max(1);
    let numbered = options.numbered;
    let mut start = initial_start(&options.start, lines, page_lines);
    render_page(lines, start, page_lines, numbered)?;

    loop {
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char(' ') | KeyCode::PageDown => {
                    if start.saturating_add(page_lines) >= lines.len() {
                        break;
                    }
                    start += page_lines;
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Char('j') => {
                    if start.saturating_add(page_lines) >= lines.len() {
                        break;
                    }
                    start += 1;
                }
                KeyCode::PageUp | KeyCode::Char('b') => start = start.saturating_sub(page_lines),
                KeyCode::Up | KeyCode::Char('k') => start = start.saturating_sub(1),
                KeyCode::Char('g') => start = 0,
                KeyCode::Char('G') => start = lines.len().saturating_sub(page_lines),
                _ => continue,
            }
            render_page(lines, start, page_lines, numbered)?;
        }
    }
    Ok(())
}

/// Writes `contents` with `-N` numbering. Works on bytes, so non-UTF-8 input is preserved.
fn numbered_output(contents: &[u8]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    for (index, line) in contents.split_inclusive(|&byte| byte == b'\n').enumerate() {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        write!(output, "{:>6} ", index + 1)?;
        output.write_all(line)?;
        output.write_all(b"\n")?;
    }
    Ok(())
}

/// Non-interactive output: byte-exact copy, or numbered lines with `-N`.
fn write_plain(contents: &[u8], numbered: bool) -> i32 {
    let result = if numbered {
        numbered_output(contents)
    } else {
        io::stdout().lock().write_all(contents)
    };
    match result {
        Ok(()) => 0,
        // A reader that stopped early (for example `less file | head`) is not a failure.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("less: cannot write output: {error}");
            2
        }
    }
}

fn fits_one_screen(line_count: usize) -> bool {
    match terminal::size() {
        Ok((_, rows)) => line_count < usize::from(rows),
        // Without a size the pager is the safer choice.
        Err(_) => false,
    }
}

fn show_pager(contents: &[u8], options: &LessOptions) -> i32 {
    let text = String::from_utf8_lossy(contents);
    let lines: Vec<String> = text
        .split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    if options.quit_if_one_screen && fits_one_screen(lines.len()) {
        return write_plain(contents, options.numbered);
    }
    if let Err(error) = page(&lines, options) {
        eprintln!("less: pager failed: {error}");
        return 2;
    }
    0
}

fn parse_args(args: &[String]) -> Result<Option<(LessOptions, Vec<PathBuf>)>, String> {
    let mut options = LessOptions::default();
    let mut paths = Vec::new();
    let mut options_open = true;
    for arg in args {
        if options_open && (arg == "-h" || arg == "--help") {
            print!("{HELP}");
            return Ok(None);
        } else if options_open && arg == "--" {
            options_open = false;
        } else if options_open && arg.starts_with("--") {
            apply_long(arg, &mut options)?;
        } else if options_open && arg.len() > 1 && arg.starts_with('-') {
            for letter in arg[1..].chars() {
                apply_short(letter, &mut options)?;
            }
        } else if options_open && arg.len() > 1 && arg.starts_with('+') {
            options.start = parse_start(&arg[1..])?;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    Ok(Some((options, paths)))
}

fn apply_short(letter: char, options: &mut LessOptions) -> Result<(), String> {
    match letter {
        'N' => options.numbered = true,
        'F' => options.quit_if_one_screen = true,
        'X' => options.alternate_screen = false,
        // Accepted for compatibility; these do not change the output here.
        'R' | 'r' | 'S' | 'i' | 'I' | 'M' | 'm' | 'E' | 'e' | 'K' | 'Q' => {}
        _ => return Err(format!("unknown option: -{letter}")),
    }
    Ok(())
}

fn apply_long(arg: &str, options: &mut LessOptions) -> Result<(), String> {
    match arg {
        "--LINE-NUMBERS" => options.numbered = true,
        "--quit-if-one-screen" => options.quit_if_one_screen = true,
        "--no-init"
        | "--chop-long-lines"
        | "--RAW-CONTROL-CHARS"
        | "--ignore-case"
        | "--IGNORE-CASE"
        | "--quit-at-eof"
        | "--quit-on-intr"
        | "--silent"
        | "--QUIET"
        | "--no-keypad" => {}
        _ => return Err(format!("unknown option: {arg}")),
    }
    Ok(())
}

/// Parses the text after `+`: `N` (1-based line), `G` (last page) or `/PATTERN`.
fn parse_start(text: &str) -> Result<Start, String> {
    if text == "G" {
        return Ok(Start::End);
    }
    if let Some(pattern) = text.strip_prefix('/') {
        return Ok(Start::Pattern(pattern.to_string()));
    }
    if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
        let line: usize = text.parse().unwrap_or(usize::MAX);
        return Ok(Start::Line(line.saturating_sub(1)));
    }
    Err(format!("unsupported initial command: +{text}"))
}

pub fn run(args: &[String]) -> i32 {
    let (options, paths) = match parse_args(args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("less: {error}\nUse 'less --help' for usage.");
            return 2;
        }
    };

    let contents = if paths.is_empty() {
        let mut contents = Vec::new();
        if let Err(error) = io::stdin().read_to_end(&mut contents) {
            eprintln!("less: cannot read standard input: {error}");
            return 2;
        }
        contents
    } else {
        match read_sources(&paths) {
            Ok(contents) => contents,
            Err(error) => {
                eprintln!("less: cannot read input: {error}");
                return 2;
            }
        }
    };

    let interactive = !paths.is_empty() && io::stdin().is_terminal() && io::stdout().is_terminal();
    if interactive {
        return show_pager(&contents, &options);
    }
    write_plain(&contents, options.numbered)
}

#[cfg(test)]
mod tests {
    use super::{parse_start, read_sources, Start};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn multiple_files_are_concatenated_without_added_bytes() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("less-concat-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        fs::write(&first, [b'a', 0, b'b']).unwrap();
        fs::write(&second, [0xff, b'c']).unwrap();

        let result = read_sources(&[PathBuf::from(&first), PathBuf::from(&second)]).unwrap();
        assert_eq!(result, [b'a', 0, b'b', 0xff, b'c']);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plus_commands_parse_line_end_and_pattern() {
        assert!(matches!(parse_start("G"), Ok(Start::End)));
        assert!(matches!(parse_start("3"), Ok(Start::Line(2))));
        assert!(matches!(parse_start("0"), Ok(Start::Line(0))));
        assert!(matches!(parse_start("/abc"), Ok(Start::Pattern(p)) if p == "abc"));
        assert!(parse_start("Q").is_err());
    }
}
