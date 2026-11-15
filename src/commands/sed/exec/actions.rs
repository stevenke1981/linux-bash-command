use super::super::{
    input::Input,
    options::Options,
    output::{self, Outputs, Stream},
    parse::{Action, Instruction, Program},
    subst::Substitution,
};
use super::{drain, Deferred, State};
use std::io::{self, Write};

enum Flow {
    Continue,
    Jump(usize),
    End,
    Quit(i32),
}
pub(super) struct Machine<'a, 'o> {
    pub options: &'a Options,
    pub state: &'a mut State,
    pub input: &'a mut Input,
    pub destinations: &'a mut Outputs,
    pub out: &'a mut Stream<&'o mut dyn Write>,
    pub quiet: bool,
    pub delimiter: u8,
}
impl Machine<'_, '_> {
    pub fn cycle(&mut self, program: &mut Program) -> io::Result<Option<i32>> {
        let mut pc = 0;
        while pc < program.instructions.len() {
            let instruction = &mut program.instructions[pc];
            let (selected, range_end) = self.selected(instruction)?;
            if !selected {
                pc = if let Action::Open(target) = instruction.action {
                    target
                } else {
                    pc + 1
                };
                continue;
            }
            match self.action(&instruction.action, &instruction.location, range_end)? {
                Flow::Continue => pc += 1,
                Flow::Jump(target) => pc = target,
                Flow::End => return Ok(None),
                Flow::Quit(code) => return Ok(Some(code)),
            }
            if self.options.unbuffered {
                self.out.flush()?;
            }
        }
        if !self.quiet {
            self.out
                .pattern(&self.state.text, self.state.terminated, false)?;
        }
        Ok(None)
    }
    fn selected(&mut self, instruction: &mut Instruction) -> io::Result<(bool, bool)> {
        let last = if instruction.selection.needs_last()
            || matches!(instruction.action, Action::Text(b'c', _))
        {
            self.input.last(self.state.file, self.options.separate)?
        } else {
            false
        };
        instruction
            .selection
            .select(
                self.state.line,
                last,
                &self.state.text,
                &mut self.state.previous,
            )
            .map_err(|e| script_error(&instruction.location, e))
    }
    fn action(&mut self, action: &Action, location: &str, range_end: bool) -> io::Result<Flow> {
        match action {
            Action::Open(_) | Action::Close | Action::Label => {}
            Action::Number | Action::Filename | Action::List(_) | Action::Print(_) => {
                self.print(action)?
            }
            Action::Text(kind, text) => return self.text(*kind, text, range_end),
            Action::Branch(kind, target) => return Ok(self.branch(*kind, *target)),
            Action::Delete => return Ok(Flow::End),
            Action::DeleteFirst => return Ok(self.delete_first()),
            Action::Get(_) | Action::Hold(_) | Action::Exchange | Action::Clear => {
                self.space(action)
            }
            Action::Next(append) => return self.next(*append),
            Action::Quit(print, code) => return self.quit(*print, *code),
            Action::Read(name, single) => self.read(name, *single)?,
            Action::Write(name, first) => self.write(name, *first)?,
            Action::Substitute(sub) => self.substitute(sub, location)?,
            Action::Translate(pairs) => {
                for byte in &mut self.state.text {
                    if let Some((_, to)) = pairs.iter().find(|(from, _)| from == byte) {
                        *byte = *to;
                    }
                }
            }
        }
        Ok(Flow::Continue)
    }
    fn print(&mut self, action: &Action) -> io::Result<()> {
        match action {
            Action::Number => {
                write!(self.out, "{}", self.state.line)?;
                self.out.write_all(&[self.delimiter])?;
            }
            Action::Filename => {
                write!(
                    self.out,
                    "{}",
                    crate::common::path::display_path(std::path::Path::new(&self.state.name))
                )?;
                self.out.write_all(&[self.delimiter])?;
            }
            Action::List(width) => output::list(
                &self.state.text,
                width.unwrap_or(self.options.line_length),
                self.delimiter,
                self.out,
            )?,
            Action::Print(first) => {
                self.out
                    .pattern(&self.state.text, self.state.terminated, *first)?
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    fn text(&mut self, kind: u8, text: &[u8], range_end: bool) -> io::Result<Flow> {
        if kind == b'a' {
            self.state.queue.push(Deferred::Text(text.to_vec()));
        } else if kind == b'i' || range_end {
            self.out.write_all(text)?;
            self.out.write_all(b"\n")?;
        }
        Ok(if kind == b'c' {
            Flow::End
        } else {
            Flow::Continue
        })
    }
    fn branch(&mut self, kind: u8, target: usize) -> Flow {
        let branch = match kind {
            b't' => self.state.substituted,
            b'T' => !self.state.substituted,
            _ => true,
        };
        if kind != b'b' {
            self.state.substituted = false;
        }
        if branch {
            Flow::Jump(target)
        } else {
            Flow::Continue
        }
    }
    fn delete_first(&mut self) -> Flow {
        if let Some(end) = self.state.text.iter().position(|&b| b == self.delimiter) {
            self.state.text.drain(..=end);
            Flow::Jump(0)
        } else {
            Flow::End
        }
    }
    fn space(&mut self, action: &Action) {
        let state = &mut self.state;
        match action {
            Action::Get(append) => {
                if *append {
                    state.text.push(self.delimiter);
                    state.text.extend_from_slice(&state.hold);
                } else {
                    state.text.clone_from(&state.hold);
                }
                state.terminated = state.hold_terminated;
            }
            Action::Hold(append) => {
                if *append {
                    state.hold.push(self.delimiter);
                    state.hold.extend_from_slice(&state.text);
                } else {
                    state.hold.clone_from(&state.text);
                }
                state.hold_terminated = state.terminated;
            }
            Action::Exchange => {
                std::mem::swap(&mut state.text, &mut state.hold);
                std::mem::swap(&mut state.terminated, &mut state.hold_terminated);
            }
            Action::Clear => state.text.clear(),
            _ => unreachable!(),
        }
    }
    fn next(&mut self, append: bool) -> io::Result<Flow> {
        if !append && !self.quiet {
            self.out
                .pattern(&self.state.text, self.state.terminated, false)?;
        }
        drain(self.state, self.destinations, self.out, self.delimiter)?;
        let Some(record) = self
            .input
            .take(Some(self.state.file), self.options.separate)?
        else {
            if append && !self.quiet && std::env::var_os("POSIXLY_CORRECT").is_none() {
                self.out
                    .pattern(&self.state.text, self.state.terminated, false)?;
            }
            return Ok(if self.options.separate {
                Flow::End
            } else {
                Flow::Quit(0)
            });
        };
        if append {
            self.state.text.push(self.delimiter);
            self.state.text.extend_from_slice(&record.bytes);
            self.state.terminated = record.terminated;
            self.state.name = record.name;
            self.state.file = record.file;
            self.state.line = self.state.line.saturating_add(1);
            self.state.substituted = false;
        } else {
            self.state.record(record, false);
        }
        Ok(Flow::Continue)
    }
    fn quit(&mut self, print: bool, code: i32) -> io::Result<Flow> {
        if print && !self.quiet {
            self.out
                .pattern(&self.state.text, self.state.terminated, false)?;
        }
        if !print {
            self.state.queue.clear();
        }
        Ok(Flow::Quit(code))
    }
    fn read(&mut self, name: &std::path::Path, single: bool) -> io::Result<()> {
        self.state.queue.push(if single {
            Deferred::Bytes(self.destinations.read_record(name, self.delimiter)?)
        } else {
            Deferred::Read(name.to_owned(), false)
        });
        Ok(())
    }
    fn write(&mut self, name: &std::path::Path, first: bool) -> io::Result<()> {
        self.destinations.write(
            name,
            &self.state.text,
            self.state.terminated,
            self.delimiter,
            first,
        )
    }
    fn substitute(&mut self, sub: &Substitution, location: &str) -> io::Result<()> {
        if sub
            .apply(&mut self.state.text, &mut self.state.previous)
            .map_err(|e| script_error(location, e))?
        {
            self.state.substituted = true;
            if sub.print {
                self.out
                    .pattern(&self.state.text, self.state.terminated, false)?;
            }
            if let Some(name) = &sub.write {
                self.write(name, false)?;
            }
        }
        Ok(())
    }
}
fn script_error(location: &str, message: String) -> io::Error {
    io::Error::other(super::super::ScriptError(format!("{location}: {message}")))
}
