//! The execution engine: the input cycle, pattern and hold spaces, the append queue,
//! `l` formatting and the output streams.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufWriter, Read};
use std::rc::Rc;

use ::regex::bytes::Regex;

use crate::common::error::io_error_text;
use crate::common::io::{is_broken_pipe, read_record, stdout_writer};

use super::addr::{self, Env, Range, Selector};
use super::inplace::{self, InPlace};
use super::input::{open_source, Input, Line};
use super::output::Output;
use super::parse::{BranchKind, Kind, Script, Sink};
use super::pattern::resolve;
use super::subst::{substitute, Subst};

/// Options that affect execution (the script itself is already parsed).
pub struct Options {
    /// `-n` (the script's `#n` is added by the caller).
    pub quiet: bool,
    /// `-s` or `-i`: `$`, line numbers and the in-place target are per file.
    pub separate: bool,
    /// `-i[SUFFIX]`: `Some("")` for no backup.
    pub in_place: Option<String>,
    pub null_data: bool,
    pub unbuffered: bool,
    pub line_len: usize,
    pub follow: bool,
    pub inputs: Vec<String>,
}

/// Runs `script` over the inputs and returns the exit status.
pub fn run(script: &Script, options: &Options) -> i32 {
    match run_script(script, options) {
        Ok(status) => status,
        Err(error) if is_broken_pipe(&error) => 0,
        Err(error) if error.kind() == io::ErrorKind::Other => {
            eprintln!("sed: {error}");
            1
        }
        Err(error) => {
            eprintln!("sed: {}", io_error_text(&error));
            4
        }
    }
}

fn run_script(script: &Script, options: &Options) -> io::Result<i32> {
    let mut files = Vec::with_capacity(script.write_files.len());
    for name in &script.write_files {
        match File::create(name) {
            Ok(file) => files.push(Output::new(Box::new(BufWriter::new(file)))),
            Err(error) => {
                eprintln!("sed: couldn't open file {name}: {}", io_error_text(&error));
                return Ok(4);
            }
        }
    }
    let delim = if options.null_data { 0 } else { b'\n' };
    let mut engine = Engine {
        script,
        opts: options,
        quiet: options.quiet || script.quiet,
        delim,
        input: Input::new(options.inputs.clone(), delim),
        line_no: 0,
        file_index: None,
        ps: Line {
            text: Vec::new(),
            chomped: true,
            file: 0,
        },
        hold: Line {
            text: Vec::new(),
            chomped: true,
            file: 0,
        },
        replaced: false,
        ranges: script
            .commands
            .iter()
            .map(|c| Range::initial(&c.selector))
            .collect(),
        last_re: None,
        appends: Vec::new(),
        stdout: Output::new(Box::new(stdout_writer())),
        stderr: Output::new(Box::new(io::stderr())),
        files,
        readers: HashMap::new(),
        inplace: None,
    };
    let status = engine.process()?;
    // Release the input (a quit leaves it open) before the in-place file is renamed.
    engine.input.close();
    engine.commit_in_place()?;
    engine.flush_all()?;
    if status == 0 && engine.input.failed {
        return Ok(2);
    }
    Ok(status)
}

/// Pending output queued for the end of the cycle (`a`, `r`, `R`).
enum Append {
    Text(Vec<u8>),
    File(String),
}

/// How a cycle ended.
enum Flow {
    /// End of script: autoprint, then the next cycle.
    EndCycle,
    /// `d`, `c`: next cycle without autoprint.
    Delete,
    /// `D` with a newline: next cycle without reading input.
    Restart,
    /// `q`: autoprint, flush the queue, exit with the status.
    Quit(i32),
    /// `Q`: exit at once.
    QuitNow(i32),
}

struct Engine<'a> {
    script: &'a Script,
    opts: &'a Options,
    quiet: bool,
    delim: u8,
    input: Input,
    line_no: u64,
    /// Index of the file the current line came from.
    file_index: Option<usize>,
    ps: Line,
    hold: Line,
    replaced: bool,
    ranges: Vec<Range>,
    last_re: Option<Rc<Regex>>,
    appends: Vec<Append>,
    stdout: Output,
    stderr: Output,
    files: Vec<Output>,
    readers: HashMap<String, Option<Box<dyn BufRead>>>,
    inplace: Option<InPlace>,
}

impl Engine<'_> {
    fn process(&mut self) -> io::Result<i32> {
        let mut restart = false;
        loop {
            if !restart && !self.read_pattern_space()? {
                break;
            }
            restart = false;
            let flow = self.execute()?;
            if let Some(error) = self.input.error.take() {
                return Err(error);
            }
            match flow {
                Flow::EndCycle => self.end_cycle_print()?,
                Flow::Delete => {}
                Flow::Restart => restart = true,
                Flow::Quit(code) => {
                    self.end_cycle_print()?;
                    self.dump_appends()?;
                    return Ok(code);
                }
                Flow::QuitNow(code) => return Ok(code),
            }
            if self.opts.unbuffered {
                main_slot(&mut self.inplace, &mut self.stdout).flush()?;
            }
        }
        Ok(0)
    }

    /// Reads the next line into the pattern space. Queued output is written first.
    fn read_pattern_space(&mut self) -> io::Result<bool> {
        self.dump_appends()?;
        self.replaced = false;
        let Some(line) = self.input.read()? else {
            return Ok(false);
        };
        self.enter_file(line.file)?;
        self.line_no += 1;
        self.ps = line;
        Ok(true)
    }

    /// Notes that the next line comes from file `file`: starts a new line count with
    /// `-s`, and switches the in-place output.
    fn enter_file(&mut self, file: usize) -> io::Result<()> {
        if self.file_index == Some(file) {
            return Ok(());
        }
        self.file_index = Some(file);
        if self.opts.separate {
            self.line_no = 0;
        }
        if self.opts.in_place.is_some() {
            self.commit_in_place()?;
            let name = self.input.name(file).to_string();
            if name != "-" {
                self.inplace = Some(inplace::start(&name, self.opts.follow)?);
            }
        }
        Ok(())
    }

    /// Finishes the in-place file in progress, if any. The input reader is not touched:
    /// when this runs at a file switch it already belongs to the next file.
    fn commit_in_place(&mut self) -> io::Result<()> {
        if let Some(edit) = self.inplace.take() {
            inplace::commit(edit, self.opts.in_place.as_deref())?;
        }
        Ok(())
    }

    fn flush_all(&mut self) -> io::Result<()> {
        self.stdout.flush()?;
        self.stderr.flush()?;
        for file in &mut self.files {
            file.flush()?;
        }
        Ok(())
    }

    fn end_cycle_print(&mut self) -> io::Result<()> {
        if !self.quiet {
            let out = main_slot(&mut self.inplace, &mut self.stdout);
            out.line(&self.ps.text, self.ps.chomped, self.delim)?;
        }
        Ok(())
    }

    fn dump_appends(&mut self) -> io::Result<()> {
        if self.appends.is_empty() {
            return Ok(());
        }
        for append in std::mem::take(&mut self.appends) {
            let out = main_slot(&mut self.inplace, &mut self.stdout);
            match append {
                Append::Text(text) => out.text(&text)?,
                Append::File(name) => {
                    if let Some(data) = read_whole_file(&name) {
                        if !data.is_empty() {
                            out.text(&data)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Runs the script over the pattern space once.
    fn execute(&mut self) -> io::Result<Flow> {
        let script = self.script;
        let commands = &script.commands;
        let mut pc = 0;
        while pc < commands.len() {
            let command = &commands[pc];
            let mut next = pc + 1;
            if !self.selected(pc, &command.selector)? {
                if let Kind::Block { end } = command.kind {
                    next = end + 1;
                }
                pc = next;
                continue;
            }
            match &command.kind {
                Kind::Block { .. } | Kind::Nop | Kind::Label(_) => {}
                Kind::Branch { kind, target, .. } => {
                    if self.branch_taken(*kind) {
                        next = *target;
                    }
                }
                Kind::Subst(subst) => self.substitute_command(subst)?,
                Kind::Equals => {
                    let mut text = self.line_no.to_string().into_bytes();
                    text.push(self.delim);
                    main_slot(&mut self.inplace, &mut self.stdout).text(&text)?;
                }
                Kind::Append(text) => self.appends.push(Append::Text(text.clone())),
                Kind::Insert(text) => {
                    main_slot(&mut self.inplace, &mut self.stdout).text(text)?;
                }
                Kind::Change(text) => {
                    if !self.ranges[pc].active {
                        main_slot(&mut self.inplace, &mut self.stdout).text(text)?;
                    }
                    return Ok(Flow::Delete);
                }
                Kind::Delete => return Ok(Flow::Delete),
                Kind::DeleteFirst => {
                    let Some(at) = self.ps.text.iter().position(|&b| b == self.delim) else {
                        return Ok(Flow::Delete);
                    };
                    self.ps.text.drain(..=at);
                    return Ok(Flow::Restart);
                }
                Kind::FileName => {
                    let name = match self.file_index {
                        Some(index) => self.input.name(index).to_string(),
                        None => "-".to_string(),
                    };
                    main_slot(&mut self.inplace, &mut self.stdout)
                        .text(format!("{name}\n").as_bytes())?;
                }
                Kind::Get => {
                    self.ps.text.clone_from(&self.hold.text);
                    self.ps.chomped = self.hold.chomped;
                }
                Kind::GetAppend => {
                    self.ps.text.push(self.delim);
                    self.ps.text.extend_from_slice(&self.hold.text);
                }
                Kind::Hold => {
                    self.hold.text.clone_from(&self.ps.text);
                    self.hold.chomped = self.ps.chomped;
                }
                Kind::HoldAppend => {
                    self.hold.text.push(self.delim);
                    self.hold.text.extend_from_slice(&self.ps.text);
                }
                Kind::List(width) => {
                    let width = width.unwrap_or(self.opts.line_len);
                    let text = list_text(&self.ps.text, width, self.delim);
                    main_slot(&mut self.inplace, &mut self.stdout).text(&text)?;
                }
                Kind::Next => {
                    if !self.input.has_more(true) {
                        return Ok(Flow::EndCycle);
                    }
                    if !self.quiet {
                        self.end_cycle_print()?;
                    }
                    self.read_pattern_space()?;
                }
                Kind::NextAppend => {
                    if !self.input.has_more(true) {
                        return Ok(Flow::EndCycle);
                    }
                    let mut joined = std::mem::take(&mut self.ps.text);
                    self.read_pattern_space()?;
                    joined.push(self.delim);
                    joined.append(&mut self.ps.text);
                    self.ps.text = joined;
                }
                Kind::Print => {
                    main_slot(&mut self.inplace, &mut self.stdout).line(
                        &self.ps.text,
                        self.ps.chomped,
                        self.delim,
                    )?;
                }
                Kind::PrintFirst => {
                    let out = main_slot(&mut self.inplace, &mut self.stdout);
                    print_first_line(out, &self.ps, self.delim)?;
                }
                Kind::Quit(code) => return Ok(Flow::Quit(*code)),
                Kind::QuitSilent(code) => return Ok(Flow::QuitNow(*code)),
                Kind::ReadFile(name) => self.appends.push(Append::File(name.clone())),
                Kind::ReadLine(name) => self.queue_line(name)?,
                Kind::Write(sink) => {
                    let out = sink_slot(*sink, &mut self.stdout, &mut self.stderr, &mut self.files);
                    out.line(&self.ps.text, self.ps.chomped, self.delim)?;
                }
                Kind::WriteFirst(sink) => {
                    let out = sink_slot(*sink, &mut self.stdout, &mut self.stderr, &mut self.files);
                    print_first_line(out, &self.ps, self.delim)?;
                }
                Kind::Exchange => std::mem::swap(&mut self.ps, &mut self.hold),
                Kind::Translate(table) => {
                    for byte in &mut self.ps.text {
                        *byte = table[usize::from(*byte)];
                    }
                }
                Kind::Zap => self.ps.text.clear(),
            }
            pc = next;
        }
        if let Some(error) = self.input.error.take() {
            return Err(error);
        }
        Ok(Flow::EndCycle)
    }

    fn branch_taken(&mut self, kind: BranchKind) -> bool {
        match kind {
            BranchKind::Always => true,
            BranchKind::IfReplaced => std::mem::replace(&mut self.replaced, false),
            BranchKind::IfNotReplaced => {
                if self.replaced {
                    self.replaced = false;
                    false
                } else {
                    true
                }
            }
        }
    }

    /// Evaluates the selector of command `pc` against the current line.
    fn selected(&mut self, pc: usize, selector: &Selector) -> io::Result<bool> {
        let cross_files = !self.opts.separate;
        let input = &mut self.input;
        let mut is_last = || !input.has_more(cross_files);
        let mut env = Env {
            line: self.line_no,
            space: &self.ps.text,
            last_re: &mut self.last_re,
            is_last: &mut is_last,
        };
        addr::selects(selector, &mut self.ranges[pc], &mut env).map_err(io::Error::other)
    }

    fn substitute_command(&mut self, subst: &Subst) -> io::Result<()> {
        let regex = resolve(&subst.pattern, &mut self.last_re).map_err(io::Error::other)?;
        let Some(result) = substitute(&regex, &self.ps.text, subst) else {
            return Ok(());
        };
        self.ps.text = result;
        self.replaced = true;
        if subst.print {
            main_slot(&mut self.inplace, &mut self.stdout).line(
                &self.ps.text,
                self.ps.chomped,
                self.delim,
            )?;
        }
        if let Some(sink) = subst.write {
            let out = sink_slot(sink, &mut self.stdout, &mut self.stderr, &mut self.files);
            out.line(&self.ps.text, self.ps.chomped, self.delim)?;
        }
        Ok(())
    }

    /// `R FILE`: queues the next line of the file (nothing at its end).
    fn queue_line(&mut self, name: &str) -> io::Result<()> {
        let delim = self.delim;
        let reader = self
            .readers
            .entry(name.to_string())
            .or_insert_with(|| open_source(name).ok());
        let Some(reader) = reader.as_mut() else {
            return Ok(());
        };
        let mut buffer = Vec::new();
        if read_record(reader.as_mut(), delim, &mut buffer)? {
            self.appends.push(Append::Text(buffer));
        }
        Ok(())
    }
}

/// The stream the pattern space goes to: the in-place file when editing, else stdout.
fn main_slot<'b>(inplace: &'b mut Option<InPlace>, stdout: &'b mut Output) -> &'b mut Output {
    match inplace {
        Some(edit) => &mut edit.output,
        None => stdout,
    }
}

fn sink_slot<'b>(
    sink: Sink,
    stdout: &'b mut Output,
    stderr: &'b mut Output,
    files: &'b mut [Output],
) -> &'b mut Output {
    match sink {
        Sink::Stdout => stdout,
        Sink::Stderr => stderr,
        Sink::File(index) => &mut files[index],
    }
}

/// `P` and `W`: the text up to the first delimiter, which is always written.
fn print_first_line(out: &mut Output, line: &Line, delim: u8) -> io::Result<()> {
    match line.text.iter().position(|&b| b == delim) {
        Some(at) => out.line(&line.text[..at], true, delim),
        None => out.line(&line.text, line.chomped, delim),
    }
}

fn read_whole_file(name: &str) -> Option<Vec<u8>> {
    if name == "/dev/stdin" || name == "-" {
        let mut data = Vec::new();
        return io::stdin().read_to_end(&mut data).ok().map(|_| data);
    }
    fs::read(name).ok()
}

/// Formats the pattern space for `l`: escapes, octal for other bytes, lines wrapped
/// with a trailing `\` so that no output line is longer than `width` (0: no wrap).
/// The end marker `$` is followed by the record delimiter, as in GNU sed.
pub fn list_text(text: &[u8], width: usize, delim: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 8);
    let mut column = 0;
    for &byte in text {
        let piece = escape_for_list(byte);
        if width > 0 && column + piece.len() > width - 1 {
            out.extend_from_slice(b"\\\n");
            column = 0;
        }
        column += piece.len();
        out.extend_from_slice(&piece);
    }
    out.push(b'$');
    out.push(delim);
    out
}

fn escape_for_list(byte: u8) -> Vec<u8> {
    let short = match byte {
        b'\\' => b'\\',
        0x07 => b'a',
        0x08 => b'b',
        0x0c => b'f',
        b'\n' => b'n',
        b'\r' => b'r',
        b'\t' => b't',
        0x0b => b'v',
        0x20..=0x7e => return vec![byte],
        _ => return format!("\\{byte:03o}").into_bytes(),
    };
    vec![b'\\', short]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_escapes_control_and_non_printable_bytes() {
        assert_eq!(
            list_text(b"a\tb\\\x01\xff", 70, b'\n'),
            b"a\\tb\\\\\\001\\377$\n".to_vec()
        );
        assert_eq!(list_text(b"\n\r\x07", 70, b'\n'), b"\\n\\r\\a$\n".to_vec());
    }

    #[test]
    fn list_wraps_at_width_minus_one() {
        assert_eq!(
            list_text(b"abcdefghijkl", 5, b'\n'),
            b"abcd\\\nefgh\\\nijkl$\n".to_vec()
        );
    }

    #[test]
    fn list_width_zero_never_wraps() {
        let long = vec![b'x'; 200];
        let text = list_text(&long, 0, b'\n');
        assert_eq!(text.len(), 202);
    }

    #[test]
    fn list_does_not_split_an_escape() {
        assert_eq!(list_text(b"ab\t", 4, b'\n'), b"ab\\\n\\t$\n".to_vec());
    }

    #[test]
    fn list_of_empty_text_is_just_the_end_marker() {
        assert_eq!(list_text(b"", 70, b'\n'), b"$\n".to_vec());
    }

    #[test]
    fn list_ends_with_the_null_delimiter_under_dash_z() {
        assert_eq!(list_text(b"a", 70, 0), b"a$\0".to_vec());
    }
}
