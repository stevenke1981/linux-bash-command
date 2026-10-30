//! Script parser: sed program text to a flat command list.
//!
//! Blocks (`{`/`}`) are recorded as indexes into the list and labels are resolved to
//! indexes once the whole script is read, so the executor only jumps by index.

use std::collections::HashMap;
use std::rc::Rc;

use super::addr::{Addr, Addr2, Selector};
use super::pattern::{compile, Pattern};
use super::subst::{max_group, new_subst, parse_replacement};

pub use super::script::{
    BranchKind, Chunk, Command, Dialect, Kind, Origin, ParseError, Script, Sink,
};

/// Parses the chunks (joined with newlines) into a [`Script`].
pub fn parse(chunks: &[Chunk], dialect: Dialect) -> Result<Script, ParseError> {
    let mut src = Vec::new();
    let mut starts = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        if index > 0 {
            src.push(b'\n');
        }
        starts.push((src.len(), chunk.origin.clone()));
        src.extend_from_slice(&chunk.text);
    }
    let quiet = src.starts_with(b"#n") && (src.len() == 2 || src[2] == b'\n');
    let mut parser = Parser {
        src: &src,
        pos: 0,
        starts,
        dialect,
        commands: Vec::new(),
        blocks: Vec::new(),
        write_files: Vec::new(),
        regex_seen: false,
    };
    parser.run()?;
    let commands = resolve_labels(parser.commands)?;
    Ok(Script {
        commands,
        write_files: parser.write_files,
        quiet,
    })
}

fn resolve_labels(mut commands: Vec<Command>) -> Result<Vec<Command>, ParseError> {
    let mut labels = HashMap::new();
    for (index, command) in commands.iter().enumerate() {
        if let Kind::Label(name) = &command.kind {
            if labels.insert(name.clone(), index).is_some() {
                return Err(ParseError {
                    message: format!("duplicate label `{name}'"),
                    status: 1,
                });
            }
        }
    }
    let end = commands.len();
    for command in &mut commands {
        if let Kind::Branch { label, target, .. } = &mut command.kind {
            *target = if label.is_empty() {
                end
            } else {
                *labels.get(label.as_str()).ok_or_else(|| ParseError {
                    message: format!("can't find label for jump to `{label}'"),
                    status: 4,
                })?
            };
        }
    }
    Ok(commands)
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    /// Start offset and origin of each chunk within `src`.
    starts: Vec<(usize, Origin)>,
    dialect: Dialect,
    commands: Vec<Command>,
    /// Indexes of the `{` commands still open.
    blocks: Vec<usize>,
    write_files: Vec<String>,
    /// A non-empty regex has been compiled, so `//` has something to reuse.
    regex_seen: bool,
}

impl Parser<'_> {
    fn run(&mut self) -> Result<(), ParseError> {
        loop {
            self.skip_separators();
            let Some(c) = self.peek() else {
                break;
            };
            if c == b'#' {
                self.skip_line();
                continue;
            }
            let selector = self.parse_selector()?;
            let Some(command) = self.next() else {
                return Err(self.error("missing command"));
            };
            self.command(command, selector)?;
        }
        if !self.blocks.is_empty() {
            return Err(self.error("unmatched `{'"));
        }
        Ok(())
    }

    fn command(&mut self, command: u8, selector: Selector) -> Result<(), ParseError> {
        let has_address = selector.start.is_some();
        let kind = match command {
            b'#' => return Err(self.error("comments don't accept any addresses")),
            b'{' => {
                self.blocks.push(self.commands.len());
                Kind::Block { end: 0 }
            }
            b'}' => {
                if has_address {
                    return Err(self.error("unexpected `}'"));
                }
                let open = self
                    .blocks
                    .pop()
                    .ok_or_else(|| self.error("unexpected `}'"))?;
                self.commands[open].kind = Kind::Block {
                    end: self.commands.len(),
                };
                self.end_of_command()?;
                Kind::Nop
            }
            b':' => {
                if has_address {
                    return Err(self.error(": doesn't want any addresses"));
                }
                let label = self.read_label(false);
                if label.is_empty() {
                    return Err(self.error("\":\" lacks a label"));
                }
                self.end_of_command()?;
                Kind::Label(label)
            }
            b'b' => self.branch(BranchKind::Always)?,
            b't' => self.branch(BranchKind::IfReplaced)?,
            b'T' => self.branch(BranchKind::IfNotReplaced)?,
            b'a' => Kind::Append(self.read_text()?),
            b'i' => Kind::Insert(self.read_text()?),
            b'c' => Kind::Change(self.read_text()?),
            b'=' => self.simple(Kind::Equals)?,
            b'd' => self.simple(Kind::Delete)?,
            b'D' => self.simple(Kind::DeleteFirst)?,
            b'F' => self.simple(Kind::FileName)?,
            b'g' => self.simple(Kind::Get)?,
            b'G' => self.simple(Kind::GetAppend)?,
            b'h' => self.simple(Kind::Hold)?,
            b'H' => self.simple(Kind::HoldAppend)?,
            b'n' => self.simple(Kind::Next)?,
            b'N' => self.simple(Kind::NextAppend)?,
            b'p' => self.simple(Kind::Print)?,
            b'P' => self.simple(Kind::PrintFirst)?,
            b'x' => self.simple(Kind::Exchange)?,
            b'z' => self.simple(Kind::Zap)?,
            b'l' => {
                let width = self.optional_number()?;
                self.end_of_command()?;
                Kind::List(width.map(|n| n as usize))
            }
            b'q' | b'Q' => {
                if selector.end.is_some() {
                    return Err(self.error("command only uses one address"));
                }
                let code = self.optional_number()?.unwrap_or(0) as i32;
                self.end_of_command()?;
                if command == b'q' {
                    Kind::Quit(code)
                } else {
                    Kind::QuitSilent(code)
                }
            }
            b'e' => {
                if self.dialect.sandbox {
                    return Err(self.error("e/r/w commands disabled in sandbox mode"));
                }
                return Err(self.error("e command is not supported by agentcmd"));
            }
            b'r' | b'R' | b'w' | b'W' => self.file_command(command)?,
            b's' => self.substitute()?,
            b'y' => self.translate()?,
            other => {
                return Err(self.error(&format!("unknown command: `{}'", char::from(other))));
            }
        };
        self.commands.push(Command { selector, kind });
        Ok(())
    }

    fn simple(&mut self, kind: Kind) -> Result<Kind, ParseError> {
        self.end_of_command()?;
        Ok(kind)
    }

    fn branch(&mut self, kind: BranchKind) -> Result<Kind, ParseError> {
        let label = self.read_label(true);
        self.end_of_command()?;
        Ok(Kind::Branch {
            kind,
            label,
            target: 0,
        })
    }

    fn file_command(&mut self, command: u8) -> Result<Kind, ParseError> {
        self.check_sandbox()?;
        let name = self.read_filename()?;
        Ok(match command {
            b'r' => Kind::ReadFile(name),
            b'R' => Kind::ReadLine(name),
            b'w' => Kind::Write(self.sink(name)),
            _ => Kind::WriteFirst(self.sink(name)),
        })
    }

    fn check_sandbox(&self) -> Result<(), ParseError> {
        if self.dialect.sandbox {
            Err(self.error("e/r/w commands disabled in sandbox mode"))
        } else {
            Ok(())
        }
    }

    fn sink(&mut self, name: String) -> Sink {
        match name.as_str() {
            "/dev/stdout" => Sink::Stdout,
            "/dev/stderr" => Sink::Stderr,
            _ => {
                let index = match self.write_files.iter().position(|file| *file == name) {
                    Some(index) => index,
                    None => {
                        self.write_files.push(name);
                        self.write_files.len() - 1
                    }
                };
                Sink::File(index)
            }
        }
    }

    fn substitute(&mut self) -> Result<Kind, ParseError> {
        let delim = self.delimiter("unterminated `s' command")?;
        let regex_source = self.read_delimited(delim, true, "`s' command")?;
        let replacement_source = self.read_delimited(delim, false, "`s' command")?;
        let replacement = parse_replacement(&replacement_source).map_err(|m| self.error(&m))?;
        let (mut icase, mut multi) = (false, false);
        let (mut global, mut print, mut occurrence) = (false, false, None);
        let mut write = None;
        loop {
            self.skip_blanks();
            match self.peek() {
                Some(b'g') => {
                    self.pos += 1;
                    if global {
                        return Err(self.error("multiple `g' options to `s' command"));
                    }
                    global = true;
                }
                Some(b'p') => {
                    self.pos += 1;
                    if print {
                        return Err(self.error("multiple `p' options to `s' command"));
                    }
                    print = true;
                }
                Some(b'i' | b'I') => {
                    self.pos += 1;
                    icase = true;
                }
                Some(b'm' | b'M') => {
                    self.pos += 1;
                    multi = true;
                }
                Some(b'0'..=b'9') => {
                    if occurrence.is_some() {
                        return Err(self.error("multiple number options to `s' command"));
                    }
                    let number = self.number()?;
                    if number == 0 {
                        return Err(self.error("number option to `s' command may not be zero"));
                    }
                    occurrence = Some(number);
                }
                Some(b'e') => {
                    self.check_sandbox()?;
                    return Err(self.error("e flag to `s' command is not supported by agentcmd"));
                }
                Some(b'w') => {
                    self.pos += 1;
                    self.check_sandbox()?;
                    let name = self.read_filename()?;
                    write = Some(self.sink(name));
                    break;
                }
                _ => {
                    self.finish_flags()?;
                    break;
                }
            }
        }
        let pattern = self.make_pattern(&regex_source, icase, multi)?;
        if let Pattern(Some(regex)) = &pattern {
            let group = max_group(&replacement);
            if group >= regex.captures_len() {
                return Err(
                    self.error(&format!("invalid reference \\{group} on `s' command's RHS"))
                );
            }
        }
        let mut subst = new_subst(pattern, replacement);
        subst.global = global;
        subst.print = print;
        subst.occurrence = occurrence.unwrap_or(1);
        subst.write = write;
        Ok(Kind::Subst(Box::new(subst)))
    }

    /// Ends the `s` flag list: only a command end may follow.
    fn finish_flags(&mut self) -> Result<(), ParseError> {
        self.skip_blanks();
        match self.peek() {
            None | Some(b'\n' | b';' | b'}' | b'#') => self.end_of_command(),
            Some(_) => Err(self.error("unknown option to `s'")),
        }
    }

    fn translate(&mut self) -> Result<Kind, ParseError> {
        let delim = self.delimiter("unterminated `y' command")?;
        let from = unescape_translate(&self.read_delimited(delim, false, "`y' command")?)
            .map_err(|m| self.error(&m))?;
        let to = unescape_translate(&self.read_delimited(delim, false, "`y' command")?)
            .map_err(|m| self.error(&m))?;
        if from.len() != to.len() {
            return Err(self.error("strings for `y' command are different lengths"));
        }
        let mut table = Box::new([0u8; 256]);
        for (index, slot) in table.iter_mut().enumerate() {
            *slot = index as u8;
        }
        for (&source, &target) in from.iter().zip(&to) {
            table[usize::from(source)] = target;
        }
        self.end_of_command()?;
        Ok(Kind::Translate(table))
    }

    /// Reads the delimiter of `s`, `y` or a custom address regex.
    fn delimiter(&mut self, message: &str) -> Result<u8, ParseError> {
        match self.next() {
            Some(delim) if delim != b'\n' && delim != b'\\' => Ok(delim),
            _ => Err(self.error(message)),
        }
    }

    /// Reads text up to the next unescaped `delim`. In a regex a backslash-newline is a
    /// newline; in a replacement it is kept so the replacement parser sees it.
    fn read_delimited(
        &mut self,
        delim: u8,
        regex: bool,
        what: &str,
    ) -> Result<Vec<u8>, ParseError> {
        let mut out = Vec::new();
        loop {
            let Some(c) = self.next() else {
                return Err(self.error(&format!("unterminated {what}")));
            };
            if c == delim {
                return Ok(out);
            }
            match c {
                b'\n' => return Err(self.error(&format!("unterminated {what}"))),
                b'\\' => {
                    let Some(next) = self.next() else {
                        return Err(self.error(&format!("unterminated {what}")));
                    };
                    if next == delim {
                        out.push(next);
                    } else if next == b'\n' {
                        if regex {
                            out.push(b'\n');
                        } else {
                            out.extend_from_slice(b"\\\n");
                        }
                    } else {
                        out.push(b'\\');
                        out.push(next);
                    }
                }
                _ => out.push(c),
            }
        }
    }

    /// Compiles a regex operand. The empty regex is allowed only after some regex has
    /// been written earlier in the script (GNU checks this at compile time).
    fn make_pattern(
        &mut self,
        source: &[u8],
        icase: bool,
        multi: bool,
    ) -> Result<Pattern, ParseError> {
        if source.is_empty() {
            if !self.regex_seen {
                return Err(self.error("no previous regular expression"));
            }
            return Ok(Pattern::default());
        }
        let regex = compile(source, self.dialect.ere, icase, multi).map_err(|m| self.error(&m))?;
        self.regex_seen = true;
        Ok(Pattern(Some(Rc::new(regex))))
    }

    fn parse_selector(&mut self) -> Result<Selector, ParseError> {
        let start = self.parse_addr()?;
        let mut end = None;
        if start.is_some() {
            self.skip_blanks();
            if self.peek() == Some(b',') {
                self.pos += 1;
                self.skip_blanks();
                end = Some(self.parse_addr2()?);
            }
        }
        let zero_start = matches!(start, Some(Addr::Line(0) | Addr::Step(0, 0)));
        let zero_end = matches!(end, Some(Addr2::Line(0)));
        if (zero_start && !matches!(end, Some(Addr2::Re(_)))) || zero_end {
            return Err(self.error("invalid usage of line address 0"));
        }
        let mut selector = Selector {
            start,
            end,
            negate: false,
        };
        self.skip_blanks();
        while self.peek() == Some(b'!') {
            if selector.negate {
                return Err(self.error("multiple `!'s"));
            }
            selector.negate = true;
            self.pos += 1;
            self.skip_blanks();
        }
        Ok(selector)
    }

    fn parse_addr(&mut self) -> Result<Option<Addr>, ParseError> {
        match self.peek() {
            Some(b'0'..=b'9') => {
                let first = self.number()?;
                if self.peek() == Some(b'~') {
                    self.pos += 1;
                    let step = if matches!(self.peek(), Some(b'0'..=b'9')) {
                        self.number()?
                    } else {
                        0
                    };
                    return Ok(Some(Addr::Step(first, step)));
                }
                Ok(Some(Addr::Line(first)))
            }
            Some(b'$') => {
                self.pos += 1;
                Ok(Some(Addr::Last))
            }
            Some(b'/' | b'\\') => Ok(Some(Addr::Re(self.parse_regex_addr()?))),
            _ => Ok(None),
        }
    }

    fn parse_addr2(&mut self) -> Result<Addr2, ParseError> {
        match self.peek() {
            Some(b'0'..=b'9') => Ok(Addr2::Line(self.number()?)),
            Some(b'+') => {
                self.pos += 1;
                Ok(Addr2::Plus(self.number()?))
            }
            Some(b'~') => {
                self.pos += 1;
                Ok(Addr2::Tilde(self.number()?))
            }
            Some(b'$') => {
                self.pos += 1;
                Ok(Addr2::Last)
            }
            Some(b'/' | b'\\') => Ok(Addr2::Re(self.parse_regex_addr()?)),
            _ => Err(self.error("unexpected `,'")),
        }
    }

    fn parse_regex_addr(&mut self) -> Result<Pattern, ParseError> {
        let opener = self.next();
        let delim = if opener == Some(b'\\') {
            self.delimiter("unexpected end of script")?
        } else {
            b'/'
        };
        let source = self.read_delimited(delim, true, "address regex")?;
        let (mut icase, mut multi) = (false, false);
        loop {
            match self.peek() {
                Some(b'I') => icase = true,
                Some(b'M') => multi = true,
                _ => break,
            }
            self.pos += 1;
        }
        self.make_pattern(&source, icase, multi)
    }

    /// Reads a label for `:` (ends at blanks or `;`) or a branch (also at `}`).
    fn read_label(&mut self, branch: bool) -> String {
        self.skip_blanks();
        let start = self.pos;
        while let Some(c) = self.peek() {
            let stop = matches!(c, b' ' | b'\t' | b'\n' | b';') || (branch && c == b'}');
            if stop {
                break;
            }
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    /// Reads a file name: the rest of the line, blanks skipped first.
    fn read_filename(&mut self) -> Result<String, ParseError> {
        self.skip_blanks();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'\n' {
                break;
            }
            self.pos += 1;
        }
        let name = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        if self.peek() == Some(b'\n') {
            self.pos += 1;
        }
        if name.is_empty() {
            return Err(self.error("missing filename in r/R/w/W commands"));
        }
        Ok(name)
    }

    /// Reads the text of `a`, `i` or `c`, in both the one-line GNU form and the
    /// traditional `a\` form. The result ends with a newline.
    fn read_text(&mut self) -> Result<Vec<u8>, ParseError> {
        self.skip_blanks();
        match self.peek() {
            None | Some(b'\n') => return Err(self.error("expected \\ after `a', `c' or `i'")),
            Some(b'\\') => {
                self.pos += 1;
                match self.peek() {
                    None => return Ok(Vec::new()),
                    Some(b'\n') => self.pos += 1,
                    Some(_) => {}
                }
            }
            Some(_) => {}
        }
        let mut text = Vec::new();
        while let Some(c) = self.next() {
            match c {
                b'\n' => break,
                b'\\' => match self.next() {
                    None => break,
                    Some(b'\n') => text.push(b'\n'),
                    Some(escaped) => text.push(text_escape(escaped)),
                },
                _ => text.push(c),
            }
        }
        text.push(b'\n');
        Ok(text)
    }

    fn number(&mut self) -> Result<u64, ParseError> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.error("expected number"));
        }
        let digits = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("0");
        digits
            .parse::<u64>()
            .map_err(|_| self.error("number too large"))
    }

    fn optional_number(&mut self) -> Result<Option<u64>, ParseError> {
        self.skip_blanks();
        if matches!(self.peek(), Some(b'0'..=b'9')) {
            Ok(Some(self.number()?))
        } else {
            Ok(None)
        }
    }

    /// Ends a command: blanks, then `;`, a newline, `}`, `#` or the end of the script.
    fn end_of_command(&mut self) -> Result<(), ParseError> {
        self.skip_blanks();
        match self.peek() {
            None => Ok(()),
            Some(b'\n' | b';') => {
                self.pos += 1;
                Ok(())
            }
            Some(b'}' | b'#') => Ok(()),
            Some(_) => Err(self.error("extra characters after command")),
        }
    }

    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) {
            self.pos += 1;
        }
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n' | b';')) {
            self.pos += 1;
        }
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.next() {
            if c == b'\n' {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    /// An error located like GNU sed: `-e expression #N, char M` or `file F line L`.
    fn error(&self, message: &str) -> ParseError {
        let (start, origin) = self
            .starts
            .iter()
            .rev()
            .find(|(start, _)| *start <= self.pos)
            .cloned()
            .unwrap_or((0, Origin::Expr(1)));
        let place = match origin {
            Origin::Expr(number) => format!(
                "-e expression #{number}, char {}",
                self.pos.saturating_sub(start)
            ),
            Origin::File(name) => {
                let end = self.pos.min(self.src.len());
                let lines = self.src[start..end].iter().filter(|&&b| b == b'\n').count();
                format!("file {name} line {}", lines + 1)
            }
        };
        ParseError {
            message: format!("{place}: {message}"),
            status: 1,
        }
    }
}

/// Escapes in `a`, `i` and `c` text: the usual control escapes, anything else literal.
fn text_escape(escaped: u8) -> u8 {
    match escaped {
        b'a' => 0x07,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0b,
        other => other,
    }
}

/// Unescapes one side of `y`: `\\` is a backslash, `\n` a newline, and a
/// backslash-newline a newline.
fn unescape_translate(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(source.len());
    let mut i = 0;
    while i < source.len() {
        if source[i] != b'\\' || i + 1 == source.len() {
            out.push(source[i]);
            i += 1;
            continue;
        }
        let escaped = source[i + 1];
        i += 2;
        match escaped {
            b'\\' => out.push(b'\\'),
            b'n' | b'\n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            _ => return Err("unknown option to `y' command".into()),
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
