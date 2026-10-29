use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

const HELP: &str = "Usage: less [OPTIONS] [FILE ...]\n\nDisplay files one screen at a time. With no FILE, copy standard input to standard output.\n\nOptions:\n  -N, --LINE-NUMBERS  Show line numbers\n  -h, --help          Show this help\n\nInteractive keys (when FILE is a terminal): Space/PageDown next page, Enter/j one line,\nb/PageUp/k previous page, g first page, G last page, q quit. Long lines are clipped.\n";

struct TerminalSession;

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, Clear(ClearType::All)) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
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

fn page(lines: &[String], numbered: bool) -> io::Result<()> {
    let _terminal = TerminalSession::enter()?;
    let (_, rows) = terminal::size()?;
    let page_lines = usize::from(rows.saturating_sub(1)).max(1);
    let mut start = 0usize;
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

fn numbered_output(contents: &[u8]) -> io::Result<()> {
    let text = String::from_utf8_lossy(contents);
    let mut output = io::stdout().lock();
    for (index, line) in text.lines().enumerate() {
        writeln!(output, "{:>6} {line}", index + 1)?;
    }
    Ok(())
}

pub fn run(args: &[String]) -> i32 {
    let mut numbered = false;
    let mut paths = Vec::new();
    let mut options = true;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if options && (arg == "-h" || arg == "--help") {
            print!("{HELP}");
            return 0;
        } else if options && (arg == "-N" || arg == "--LINE-NUMBERS") {
            numbered = true;
        } else if options && arg == "--" {
            options = false;
        } else if options && arg.starts_with('-') && arg != "-" {
            eprintln!("less: unknown option: {arg}\nUse 'less --help' for usage.");
            return 2;
        } else {
            paths.push(PathBuf::from(arg));
        }
        index += 1;
    }

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

    if io::stdin().is_terminal() && io::stdout().is_terminal() && !paths.is_empty() {
        let text = String::from_utf8_lossy(&contents);
        let lines: Vec<String> = text
            .split_terminator('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect();
        if let Err(error) = page(&lines, numbered) {
            eprintln!("less: pager failed: {error}");
            return 2;
        }
        return 0;
    }

    let result = if numbered {
        numbered_output(&contents)
    } else {
        io::stdout().lock().write_all(&contents)
    };
    if let Err(error) = result {
        eprintln!("less: cannot write output: {error}");
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::read_sources;
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
}
