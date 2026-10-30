use crate::common::error::io_text as io_message;
use crate::common::io::{is_closed_pipe, report_failure, report_open};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::thread;
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

const DEFAULT_SLEEP: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------- cat

#[derive(Clone, Copy, Default)]
struct CatOptions {
    number: bool,
    number_nonblank: bool,
    squeeze: bool,
    show_ends: bool,
    show_tabs: bool,
    show_nonprinting: bool,
}

impl CatOptions {
    fn is_plain(self) -> bool {
        !(self.number
            || self.number_nonblank
            || self.squeeze
            || self.show_ends
            || self.show_tabs
            || self.show_nonprinting)
    }
}

#[derive(Default)]
struct CatState {
    line: u64,
    previous_blank: bool,
}

pub fn cat(args: &[String]) -> i32 {
    let (options, paths) = match parse_cat(args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("cat: {error}\nTry 'cat --help' for more information.");
            return 1;
        }
    };
    let inputs = if paths.is_empty() {
        vec!["-".to_string()]
    } else {
        paths
    };
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut state = CatState::default();
    let mut had_error = false;
    for path in &inputs {
        let result = match Source::open(path) {
            Ok(mut source) => cat_source(source.reader(), &options, &mut state, &mut output),
            Err(error) => {
                report_open("cat", path, &error);
                had_error = true;
                continue;
            }
        };
        if let Err(error) = result {
            if is_closed_pipe(&error) {
                return 0;
            }
            report_failure("cat", path, &error);
            had_error = true;
        }
    }
    status(had_error)
}

fn parse_cat(args: &[String]) -> Result<Option<(CatOptions, Vec<String>)>, String> {
    let mut options = CatOptions::default();
    let mut paths = Vec::new();
    let mut options_open = true;
    for arg in args {
        let expanded;
        let arg = if options_open && arg.starts_with("--") && arg != "--" {
            expanded = crate::common::args::expand_long(
                arg,
                &[
                    "number",
                    "number-nonblank",
                    "squeeze-blank",
                    "show-ends",
                    "show-tabs",
                    "show-nonprinting",
                    "show-all",
                    "unbuffered",
                    "help",
                ],
            )?;
            &expanded
        } else {
            arg
        };
        if options_open && arg == "--" {
            options_open = false;
        } else if options_open && (arg == "-h" || arg == "--help") {
            print!("{CAT_HELP}");
            return Ok(None);
        } else if options_open && arg.starts_with("--") {
            apply_cat_long(arg, &mut options)?;
        } else if options_open && arg.len() > 1 && arg.starts_with('-') {
            for letter in arg[1..].chars() {
                apply_cat_short(letter, &mut options)?;
            }
        } else {
            paths.push(arg.clone());
            if std::env::var_os("POSIXLY_CORRECT").is_some() {
                options_open = false;
            }
        }
    }
    Ok(Some((options, paths)))
}

fn apply_cat_long(arg: &str, options: &mut CatOptions) -> Result<(), String> {
    let letter = match arg {
        "--number" => 'n',
        "--number-nonblank" => 'b',
        "--squeeze-blank" => 's',
        "--show-ends" => 'E',
        "--show-tabs" => 'T',
        "--show-nonprinting" => 'v',
        "--show-all" => 'A',
        "--unbuffered" => 'u',
        _ => return Err(format!("unknown option: {arg}")),
    };
    apply_cat_short(letter, options)
}

fn apply_cat_short(letter: char, options: &mut CatOptions) -> Result<(), String> {
    match letter {
        'n' => options.number = true,
        'b' => options.number_nonblank = true,
        's' => options.squeeze = true,
        'E' => options.show_ends = true,
        'T' => options.show_tabs = true,
        'v' => options.show_nonprinting = true,
        'A' => set_all(options, true, true, true),
        'e' => set_all(options, false, true, true),
        't' => set_all(options, true, false, true),
        'u' => {}
        _ => return Err(format!("unknown option: -{letter}")),
    }
    Ok(())
}

/// Sets the `-A`/`-e`/`-t` style groups; each flag is a (tabs, ends, nonprinting) triple.
fn set_all(options: &mut CatOptions, tabs: bool, ends: bool, nonprinting: bool) {
    options.show_tabs |= tabs;
    options.show_ends |= ends;
    options.show_nonprinting |= nonprinting;
}

fn cat_source(
    input: &mut dyn BufRead,
    options: &CatOptions,
    state: &mut CatState,
    output: &mut dyn Write,
) -> io::Result<()> {
    if options.is_plain() {
        io::copy(input, output).map(|_| ())
    } else {
        cat_lines(input, options, state, output)
    }
}

fn cat_lines(
    input: &mut dyn BufRead,
    options: &CatOptions,
    state: &mut CatState,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut line = Vec::new();
    let mut rendered = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        let has_newline = line.last() == Some(&b'\n');
        let body = if has_newline {
            &line[..line.len() - 1]
        } else {
            &line[..]
        };
        let blank = has_newline && body.is_empty();
        if options.squeeze && blank && state.previous_blank {
            continue;
        }
        state.previous_blank = blank;
        rendered.clear();
        push_line_number(options, state, blank, &mut rendered);
        render_body(body, options, &mut rendered);
        if has_newline {
            if options.show_ends {
                rendered.push(b'$');
            }
            rendered.push(b'\n');
        }
        output.write_all(&rendered)?;
    }
}

/// `-b` numbers non-empty lines only and takes precedence over `-n`.
fn push_line_number(options: &CatOptions, state: &mut CatState, blank: bool, out: &mut Vec<u8>) {
    let numbered = if options.number_nonblank {
        !blank
    } else {
        options.number
    };
    if numbered {
        state.line += 1;
        out.extend_from_slice(format!("{:>6}\t", state.line).as_bytes());
    }
}

fn render_body(body: &[u8], options: &CatOptions, out: &mut Vec<u8>) {
    for &byte in body {
        match byte {
            b'\t' if options.show_tabs => out.extend_from_slice(b"^I"),
            b'\t' => out.push(b'\t'),
            _ if options.show_nonprinting => push_visible(byte, out),
            _ => out.push(byte),
        }
    }
}

/// GNU `-v` notation: control bytes as `^X`, bytes 128-255 with an `M-` prefix.
fn push_visible(byte: u8, out: &mut Vec<u8>) {
    let (meta, low) = if byte >= 128 {
        (&b"M-"[..], byte - 128)
    } else {
        (&b""[..], byte)
    };
    out.extend_from_slice(meta);
    match low {
        0..=31 => {
            out.push(b'^');
            out.push(low + 64);
        }
        127 => out.extend_from_slice(b"^?"),
        _ => out.push(low),
    }
}

// ---------------------------------------------------------- head / tail

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sign {
    Unsigned,
    Plus,
    Minus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Lines,
    Bytes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Follow {
    No,
    Descriptor,
    Name,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HeaderMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy)]
struct Count {
    sign: Sign,
    value: u64,
}

#[derive(Clone, Copy)]
struct LineOptions {
    delimiter: u8,
    unit: Unit,
    count: Count,
    headers: HeaderMode,
    follow: Follow,
    retry: bool,
    sleep: Duration,
    pid: Option<u32>,
}

impl LineOptions {
    fn new() -> Self {
        Self {
            delimiter: b'\n',
            unit: Unit::Lines,
            count: Count {
                sign: Sign::Unsigned,
                value: 10,
            },
            headers: HeaderMode::Auto,
            follow: Follow::No,
            retry: false,
            sleep: DEFAULT_SLEEP,
            pid: None,
        }
    }

    fn follows(&self) -> bool {
        self.follow != Follow::No
    }

    fn apply_short(
        &mut self,
        arg: &str,
        next: Option<&String>,
        is_tail: bool,
    ) -> Result<bool, String> {
        let flags = &arg[1..];
        if flags.bytes().all(|b| b.is_ascii_digit()) {
            // Legacy `-N` form: same as `-n N`.
            self.apply_value("--lines", flags)?;
            return Ok(false);
        }
        for (offset, letter) in flags.char_indices() {
            let rest = &flags[offset + letter.len_utf8()..];
            match letter {
                'n' => return self.take_value("--lines", rest, next),
                'c' => return self.take_value("--bytes", rest, next),
                's' if is_tail => return self.take_value("--sleep-interval", rest, next),
                'q' => self.headers = HeaderMode::Never,
                'z' => self.delimiter = 0,
                'v' => self.headers = HeaderMode::Always,
                'f' if is_tail => self.follow = Follow::Descriptor,
                'F' if is_tail => {
                    self.follow = Follow::Name;
                    self.retry = true;
                }
                _ => return Err(format!("unknown option: -{letter}")),
            }
        }
        Ok(false)
    }

    fn apply_long(
        &mut self,
        arg: &str,
        next: Option<&String>,
        is_tail: bool,
    ) -> Result<bool, String> {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (arg, None),
        };
        let takes_value = matches!(name, "--lines" | "--bytes")
            || (is_tail && matches!(name, "--sleep-interval" | "--pid"));
        if takes_value {
            return match inline {
                Some(value) => {
                    self.apply_value(name, value)?;
                    Ok(false)
                }
                None => self.take_value(name, "", next),
            };
        }
        match (name, inline) {
            ("--zero-terminated", None) => self.delimiter = 0,
            ("--quiet" | "--silent", None) => self.headers = HeaderMode::Never,
            ("--verbose", None) => self.headers = HeaderMode::Always,
            ("--follow", None | Some("descriptor")) if is_tail => {
                self.follow = Follow::Descriptor;
            }
            ("--follow", Some("name")) if is_tail => self.follow = Follow::Name,
            ("--retry", None) if is_tail => self.retry = true,
            _ => return Err(format!("unknown option: {arg}")),
        }
        Ok(false)
    }

    /// Consumes the attached value (`-n5`) or the following argument (`-n 5`).
    fn take_value(
        &mut self,
        option: &str,
        attached: &str,
        next: Option<&String>,
    ) -> Result<bool, String> {
        if !attached.is_empty() {
            self.apply_value(option, attached)?;
            return Ok(false);
        }
        let value = next.ok_or_else(|| format!("option '{option}' requires an argument"))?;
        self.apply_value(option, value)?;
        Ok(true)
    }

    fn apply_value(&mut self, option: &str, value: &str) -> Result<(), String> {
        match option {
            "--lines" | "--bytes" => {
                self.count = parse_count(value).ok_or_else(|| {
                    let kind = if option == "--lines" {
                        "lines"
                    } else {
                        "bytes"
                    };
                    format!("invalid number of {kind}: '{value}'")
                })?;
                self.unit = if option == "--lines" {
                    Unit::Lines
                } else {
                    Unit::Bytes
                };
            }
            "--sleep-interval" => {
                self.sleep = parse_seconds(value)
                    .ok_or_else(|| format!("invalid number of seconds: '{value}'"))?;
            }
            "--pid" => {
                let pid = value
                    .parse::<u32>()
                    .map_err(|_| format!("invalid process ID: '{value}'"))?;
                self.pid = Some(pid);
            }
            _ => return Err(format!("unknown option: {option}")),
        }
        Ok(())
    }
}

/// Parses `N`, `+N`, `-N` with an optional b/K/M/G suffix (512, 1024, 1024^2, 1024^3).
fn parse_count(text: &str) -> Option<Count> {
    let (sign, unsigned) = match text.as_bytes().first() {
        Some(b'+') => (Sign::Plus, &text[1..]),
        Some(b'-') => (Sign::Minus, &text[1..]),
        _ => (Sign::Unsigned, text),
    };
    let (digits, multiplier) = crate::common::size::split_suffix(
        unsigned,
        &[
            ("b", 512),
            ("k", 1 << 10),
            ("K", 1 << 10),
            ("m", 1 << 20),
            ("M", 1 << 20),
            ("g", 1 << 30),
            ("G", 1 << 30),
        ],
        1,
    );
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: u64 = digits.parse().unwrap_or(u64::MAX);
    Some(Count {
        sign,
        value: number.saturating_mul(multiplier),
    })
}

fn parse_seconds(text: &str) -> Option<Duration> {
    let seconds: f64 = text.parse().ok()?;
    Duration::try_from_secs_f64(seconds).ok()
}

fn parse_text_args(
    args: &[String],
    help: &str,
    is_tail: bool,
) -> Result<Option<(LineOptions, Vec<String>)>, String> {
    let mut options = LineOptions::new();
    let mut paths = Vec::new();
    let mut options_open = true;
    let mut index = 0;
    while index < args.len() {
        let expanded;
        let mut long_options = vec![
            "lines",
            "bytes",
            "quiet",
            "silent",
            "verbose",
            "zero-terminated",
            "help",
        ];
        if is_tail {
            long_options.extend(["sleep-interval", "pid", "follow", "retry"]);
        }
        let arg = if options_open && args[index].starts_with("--") && args[index] != "--" {
            expanded = crate::common::args::expand_long(&args[index], &long_options)?;
            expanded.as_str()
        } else {
            args[index].as_str()
        };
        let next = args.get(index + 1);
        let mut consumed = false;
        if options_open && arg == "--" {
            options_open = false;
        } else if options_open && (arg == "-h" || arg == "--help") {
            print!("{help}");
            return Ok(None);
        } else if options_open && arg.starts_with("--") {
            consumed = options.apply_long(arg, next, is_tail)?;
        } else if options_open && arg.len() > 1 && arg.starts_with('-') {
            consumed = options.apply_short(arg, next, is_tail)?;
        } else {
            paths.push(arg.to_string());
            if std::env::var_os("POSIXLY_CORRECT").is_some() {
                options_open = false;
            }
        }
        index += if consumed { 2 } else { 1 };
    }
    if paths.is_empty() {
        paths.push("-".to_string());
    }
    Ok(Some((options, paths)))
}

/// An input that is either standard input or an owned, buffered file.
enum Source {
    Stdin(io::StdinLock<'static>),
    File(BufReader<File>),
}

impl Source {
    fn open(path: &str) -> io::Result<Self> {
        if path == "-" {
            Ok(Self::Stdin(io::stdin().lock()))
        } else {
            Ok(Self::File(BufReader::new(File::open(path)?)))
        }
    }

    fn reader(&mut self) -> &mut dyn BufRead {
        match self {
            Self::Stdin(lock) => lock,
            Self::File(reader) => reader,
        }
    }
}

/// Tracks `==> name <==` headers. `last` is the index of the last file announced.
struct Headers {
    enabled: bool,
    last: Option<usize>,
}

impl Headers {
    fn new(mode: HeaderMode, file_count: usize) -> Self {
        let enabled = match mode {
            HeaderMode::Always => true,
            HeaderMode::Never => false,
            HeaderMode::Auto => file_count > 1,
        };
        Self {
            enabled,
            last: None,
        }
    }

    fn enter(&mut self, output: &mut dyn Write, index: usize, path: &str) -> io::Result<()> {
        if !self.enabled || self.last == Some(index) {
            return Ok(());
        }
        if self.last.is_some() {
            writeln!(output)?;
        }
        let name = if path == "-" { "standard input" } else { path };
        writeln!(output, "==> {name} <==")?;
        self.last = Some(index);
        Ok(())
    }
}

pub fn head(args: &[String]) -> i32 {
    let (options, paths) = match parse_text_args(args, HEAD_HELP, false) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("head: {error}\nTry 'head --help' for more information.");
            return 1;
        }
    };
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut headers = Headers::new(options.headers, paths.len());
    let mut had_error = false;
    for (index, path) in paths.iter().enumerate() {
        let result = match Source::open(path) {
            Ok(mut source) => headers
                .enter(&mut output, index, path)
                .and_then(|()| head_one(source.reader(), &options, &mut output)),
            Err(error) => {
                report_open("head", path, &error);
                had_error = true;
                continue;
            }
        };
        if let Err(error) = result {
            if is_closed_pipe(&error) {
                return 0;
            }
            report_failure("head", path, &error);
            had_error = true;
        }
    }
    status(had_error)
}

fn head_one(
    input: &mut dyn BufRead,
    options: &LineOptions,
    output: &mut dyn Write,
) -> io::Result<()> {
    let value = options.count.value;
    match (options.unit, options.count.sign) {
        (Unit::Lines, Sign::Minus) => {
            head_all_but_last_lines(input, as_usize(value), options.delimiter, output)
        }
        (Unit::Lines, _) => head_first_lines(input, value, options.delimiter, output),
        (Unit::Bytes, Sign::Minus) => head_all_but_last_bytes(input, as_usize(value), output),
        (Unit::Bytes, _) => io::copy(&mut (&mut *input).take(value), output).map(|_| ()),
    }
}

fn head_first_lines(
    input: &mut dyn BufRead,
    count: u64,
    delimiter: u8,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut line = Vec::new();
    let mut remaining = count;
    while remaining > 0 {
        line.clear();
        if input.read_until(delimiter, &mut line)? == 0 {
            break;
        }
        output.write_all(&line)?;
        remaining -= 1;
    }
    Ok(())
}

/// Streams output while holding back the last `keep` lines.
fn head_all_but_last_lines(
    input: &mut dyn BufRead,
    keep: usize,
    delimiter: u8,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut pending = VecDeque::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(delimiter, &mut line)? == 0 {
            return Ok(());
        }
        pending.push_back(std::mem::take(&mut line));
        if pending.len() > keep {
            if let Some(oldest) = pending.pop_front() {
                output.write_all(&oldest)?;
            }
        }
    }
}

fn head_all_but_last_bytes(
    input: &mut dyn BufRead,
    keep: usize,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut pending: VecDeque<u8> = VecDeque::new();
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(());
        }
        let consumed = chunk.len();
        pending.extend(chunk.iter().copied());
        input.consume(consumed);
        let excess = pending.len().saturating_sub(keep);
        if excess > 0 {
            let spill: Vec<u8> = pending.drain(..excess).collect();
            output.write_all(&spill)?;
        }
    }
}

pub fn tail(args: &[String]) -> i32 {
    let (options, paths) = match parse_text_args(args, TAIL_HELP, true) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("tail: {error}\nTry 'tail --help' for more information.");
            return 1;
        }
    };
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut headers = Headers::new(options.headers, paths.len());
    let mut followers = Vec::new();
    let mut had_error = false;
    for (index, path) in paths.iter().enumerate() {
        match Source::open(path) {
            Ok(mut source) => {
                let result = headers
                    .enter(&mut output, index, path)
                    .and_then(|()| tail_initial(source.reader(), &options, &mut output));
                if let Err(error) = result {
                    if is_closed_pipe(&error) {
                        return 0;
                    }
                    report_failure("tail", path, &error);
                    had_error = true;
                    continue;
                }
                if let (true, Source::File(reader)) = (options.follows(), source) {
                    match Follower::open(index, path, reader) {
                        Ok(follower) => followers.push(follower),
                        Err(error) => {
                            report_failure("tail", path, &error);
                            had_error = true;
                        }
                    }
                }
            }
            Err(error) => {
                report_open("tail", path, &error);
                had_error = true;
                if options.follows() && options.retry {
                    followers.push(Follower::waiting(index, path));
                }
            }
        }
    }
    if followers.is_empty() {
        return status(had_error);
    }
    let code = follow(&mut followers, &options, &mut headers, &mut output);
    if had_error {
        1
    } else {
        code
    }
}

fn tail_initial(
    input: &mut dyn BufRead,
    options: &LineOptions,
    output: &mut dyn Write,
) -> io::Result<()> {
    let value = options.count.value;
    match (options.unit, options.count.sign) {
        (Unit::Lines, Sign::Plus) => tail_from_line(input, value, options.delimiter, output),
        (Unit::Lines, _) => tail_last_lines(input, as_usize(value), options.delimiter, output),
        (Unit::Bytes, Sign::Plus) => tail_from_byte(input, value, output),
        (Unit::Bytes, _) => tail_last_bytes(input, as_usize(value), output),
    }
}

/// `-n +N`: output starting at 1-based line N (`+0` behaves like `+1`).
fn tail_from_line(
    input: &mut dyn BufRead,
    start: u64,
    delimiter: u8,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut line = Vec::new();
    let mut to_skip = start.saturating_sub(1);
    while to_skip > 0 {
        line.clear();
        if input.read_until(delimiter, &mut line)? == 0 {
            return Ok(());
        }
        to_skip -= 1;
    }
    io::copy(input, output).map(|_| ())
}

/// `-c +N`: output starting at 1-based byte N.
fn tail_from_byte(input: &mut dyn BufRead, start: u64, output: &mut dyn Write) -> io::Result<()> {
    let to_skip = start.saturating_sub(1);
    io::copy(&mut (&mut *input).take(to_skip), &mut io::sink())?;
    io::copy(input, output).map(|_| ())
}

fn tail_last_lines(
    input: &mut dyn BufRead,
    keep: usize,
    delimiter: u8,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut lines: VecDeque<Vec<u8>> = VecDeque::with_capacity(keep.min(1024));
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(delimiter, &mut line)? == 0 {
            break;
        }
        lines.push_back(std::mem::take(&mut line));
        if lines.len() > keep {
            lines.pop_front();
        }
    }
    for line in &lines {
        output.write_all(line)?;
    }
    Ok(())
}

fn tail_last_bytes(input: &mut dyn BufRead, keep: usize, output: &mut dyn Write) -> io::Result<()> {
    let mut ring: VecDeque<u8> = VecDeque::new();
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        let consumed = chunk.len();
        ring.extend(chunk.iter().copied());
        input.consume(consumed);
        let excess = ring.len().saturating_sub(keep);
        if excess > 0 {
            ring.drain(..excess);
        }
    }
    let (front, back) = ring.as_slices();
    output.write_all(front)?;
    output.write_all(back)
}

/// A file being followed (`-f`/`-F`). `reader` is `None` while a `-F` file is missing.
struct Follower {
    index: usize,
    path: String,
    reader: Option<BufReader<File>>,
    offset: u64,
}

impl Follower {
    fn open(index: usize, path: &str, mut reader: BufReader<File>) -> io::Result<Self> {
        let offset = reader.stream_position()?;
        Ok(Self {
            index,
            path: path.to_string(),
            reader: Some(reader),
            offset,
        })
    }

    fn waiting(index: usize, path: &str) -> Self {
        Self {
            index,
            path: path.to_string(),
            reader: None,
            offset: 0,
        }
    }

    /// Appends any new bytes to `buffer`. Handles `-F` reappearance, replacement and truncation.
    fn read_new(&mut self, retry: bool, buffer: &mut Vec<u8>) -> io::Result<()> {
        if self.reader.is_none() {
            self.try_reappear(retry);
        }
        if retry {
            self.check_replaced();
        }
        let Some(reader) = self.reader.as_mut() else {
            return Ok(());
        };
        if reader.get_ref().metadata()?.len() < self.offset {
            eprintln!("tail: '{}': file truncated", self.path);
            reader.seek(SeekFrom::Start(0))?;
            self.offset = 0;
        }
        let read = reader.read_to_end(buffer)?;
        self.offset += read as u64;
        Ok(())
    }

    fn try_reappear(&mut self, retry: bool) {
        if !retry {
            return;
        }
        // A missing file is expected while retrying; the next poll tries again.
        if let Ok(file) = File::open(&self.path) {
            eprintln!("tail: '{}' has appeared; following new file", self.path);
            self.reader = Some(BufReader::new(file));
            self.offset = 0;
        }
    }

    fn check_replaced(&mut self) {
        let Some(current) = self.reader.as_ref() else {
            return;
        };
        if let Some(replacement) = replacement(&self.path, current.get_ref()) {
            eprintln!(
                "tail: '{}' has been replaced; following new file",
                self.path
            );
            self.reader = Some(replacement);
            self.offset = 0;
        }
    }
}

/// On Unix, a path that now names a different inode was replaced (rotation).
#[cfg(unix)]
fn replacement(path: &str, current: &File) -> Option<BufReader<File>> {
    use std::os::unix::fs::MetadataExt;
    let now = std::fs::metadata(path).ok()?;
    let before = current.metadata().ok()?;
    if now.dev() == before.dev() && now.ino() == before.ino() {
        return None;
    }
    File::open(path).ok().map(BufReader::new)
}

/// Replacement detection needs file identity, which is only portable through Unix metadata.
#[cfg(not(unix))]
fn replacement(_path: &str, _current: &File) -> Option<BufReader<File>> {
    None
}

fn follow(
    followers: &mut [Follower],
    options: &LineOptions,
    headers: &mut Headers,
    output: &mut dyn Write,
) -> i32 {
    let mut system = System::new();
    let mut buffer = Vec::new();
    loop {
        let alive = match options.pid {
            Some(pid) => process_alive(&mut system, pid),
            None => true,
        };
        for follower in followers.iter_mut() {
            buffer.clear();
            if let Err(error) = follower.read_new(options.retry, &mut buffer) {
                eprintln!(
                    "tail: cannot read '{}': {}",
                    follower.path,
                    io_message(&error)
                );
                return 1;
            }
            if buffer.is_empty() {
                continue;
            }
            let written = headers
                .enter(output, follower.index, &follower.path)
                .and_then(|()| write_flushed(output, &buffer));
            if let Err(error) = written {
                return if is_closed_pipe(&error) {
                    0
                } else {
                    eprintln!("tail: cannot write output: {}", io_message(&error));
                    1
                };
            }
        }
        if !alive {
            return 0;
        }
        thread::sleep(options.sleep);
    }
}

fn write_flushed(output: &mut dyn Write, bytes: &[u8]) -> io::Result<()> {
    output.write_all(bytes)?;
    output.flush()
}

fn process_alive(system: &mut System, pid: u32) -> bool {
    let pid = Pid::from_u32(pid);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}

// ------------------------------------------------------------- helpers

fn as_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn status(had_error: bool) -> i32 {
    if had_error {
        1
    } else {
        0
    }
}

const CAT_HELP: &str = "Usage: cat [OPTION]... [FILE]...\n\nConcatenate FILE(s) to standard output. With no FILE, or when FILE is -, read standard input.\n\nOptions:\n  -A, --show-all          Same as -vET\n  -b, --number-nonblank   Number non-empty output lines (overrides -n)\n  -e                      Same as -vE\n  -E, --show-ends         Show $ at the end of each line\n  -n, --number            Number all output lines (format: %6d TAB)\n  -s, --squeeze-blank     Suppress repeated empty output lines\n  -t                      Same as -vT\n  -T, --show-tabs         Show TAB characters as ^I\n  -u                      Accepted and ignored\n  -v, --show-nonprinting  Show control characters with ^ and M- notation\n  -h, --help              Show this help\n\nExit status: 0 on success, 1 if an input could not be read.\n";

const HEAD_HELP: &str = "Usage: head [OPTION]... [FILE]...\n\nPrint the first 10 lines of each FILE (or of standard input for no FILE or -).\n\nOptions:\n  -n, --lines=[-]N      Print the first N lines; with -N, all but the last N lines\n  -c, --bytes=[-]N      Print the first N bytes; with -N, all but the last N bytes\n  -N                    Same as -n N (for example -5)\n  -z, --zero-terminated  Records end with NUL instead of newline\n  -q, --quiet, --silent Never print headers giving file names\n  -v, --verbose         Always print headers giving file names\n  -h, --help            Show this help\n\nN may end in b (512), K (1024), M (1024*1024) or G (1024*1024*1024). +N is the same as N.\nExit status: 0 on success, 1 if an input could not be read.\n";

const TAIL_HELP: &str = "Usage: tail [OPTION]... [FILE]...\n\nPrint the last 10 lines of each FILE (or of standard input for no FILE or -).\n\nOptions:\n  -n, --lines=[+]N      Print the last N lines; with +N, start at line N\n  -c, --bytes=[+]N      Print the last N bytes; with +N, start at byte N\n  -N                    Same as -n N (for example -5)\n  -f, --follow[=name|descriptor]\n                        Print appended data as the file grows\n  -F                    Same as --follow=name --retry\n      --retry           Keep trying to open a file that is missing (with -F)\n  -s, --sleep-interval=S\n                        Poll interval in seconds while following (default 0.5)\n      --pid=PID         With -f, stop following when process PID exits\n  -z, --zero-terminated  Records end with NUL instead of newline\n  -q, --quiet, --silent Never print headers giving file names\n  -v, --verbose         Always print headers giving file names\n  -h, --help            Show this help\n\nN may end in b (512), K (1024), M (1024*1024) or G (1024*1024*1024).\nFollowing stdin is not supported and ends at end of input, like GNU tail.\nReplacement detection with -F uses file identity on Unix only.\nExit status: 0 on success, 1 if an input could not be read.\n";

#[cfg(test)]
mod phase1_tests {
    use super::*;
    fn render(tail: bool, args: &[&str], data: &[u8]) -> Vec<u8> {
        let args: Vec<String> = args.iter().map(|s| (*s).into()).collect();
        let (options, _) = parse_text_args(&args, "", tail).unwrap().unwrap();
        let mut input = io::Cursor::new(data);
        let mut output = Vec::new();
        if tail {
            tail_initial(&mut input, &options, &mut output).unwrap();
        } else {
            head_one(&mut input, &options, &mut output).unwrap();
        }
        output
    }
    #[test]
    fn head_nul_first() {
        assert_eq!(render(false, &["-zn1"], b"a\0b\0"), b"a\0");
    }
    #[test]
    fn head_nul_partial() {
        assert_eq!(render(false, &["-zn2"], b"a\0b"), b"a\0b");
    }
    #[test]
    fn head_nul_hold() {
        assert_eq!(render(false, &["-z", "-n-1"], b"a\0b\0"), b"a\0");
    }
    #[test]
    fn head_nul_zero() {
        assert!(render(false, &["-zn0"], b"a\0").is_empty());
    }
    #[test]
    fn head_nul_newline() {
        assert_eq!(
            render(false, &["--zero-terminated", "-n1"], b"a\nb\0c\0"),
            b"a\nb\0"
        );
    }
    #[test]
    fn tail_nul_last() {
        assert_eq!(render(true, &["-zn1"], b"a\0b\0"), b"b\0");
    }
    #[test]
    fn tail_nul_partial() {
        assert_eq!(render(true, &["-zn1"], b"a\0b"), b"b");
    }
    #[test]
    fn tail_nul_from() {
        assert_eq!(render(true, &["-z", "-n+2"], b"a\0b\0"), b"b\0");
    }
    #[test]
    fn tail_nul_zero() {
        assert!(render(true, &["-zn0"], b"a\0").is_empty());
    }
    #[test]
    fn tail_nul_newline() {
        assert_eq!(
            render(true, &["--zero-terminated", "-n1"], b"a\0b\nc\0"),
            b"b\nc\0"
        );
    }
    #[test]
    fn cat_u_is_plain() {
        let mut options = CatOptions::default();
        apply_cat_short('u', &mut options).unwrap();
        assert!(options.is_plain());
    }
}
