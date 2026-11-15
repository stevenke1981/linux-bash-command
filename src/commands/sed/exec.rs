mod actions;
use super::{
    input::{Input, Record},
    options::Options,
    output::{Outputs, Stream},
    parse::Program,
    pattern::Compiled,
};
use std::io::{self, Write};

enum Deferred {
    Text(Vec<u8>),
    Read(std::path::PathBuf, bool),
    Bytes(Vec<u8>),
}
struct State {
    text: Vec<u8>,
    terminated: bool,
    hold: Vec<u8>,
    hold_terminated: bool,
    line: u64,
    file: usize,
    name: String,
    substituted: bool,
    previous: Option<Compiled>,
    queue: Vec<Deferred>,
}
pub struct Outcome {
    pub status: i32,
    pub quit: bool,
    pub input_failed: bool,
}
impl State {
    fn new() -> Self {
        Self {
            text: Vec::new(),
            terminated: true,
            hold: Vec::new(),
            hold_terminated: true,
            line: 0,
            file: 0,
            name: String::new(),
            substituted: false,
            previous: None,
            queue: Vec::new(),
        }
    }
    fn record(&mut self, record: Record, separate: bool) -> bool {
        let changed = record.file != self.file;
        if changed && separate {
            self.line = 0;
            self.hold.clear();
            self.hold_terminated = true;
        }
        self.file = record.file;
        self.name = record.name;
        self.text = record.bytes;
        self.terminated = record.terminated;
        self.line = self.line.saturating_add(1);
        self.substituted = false;
        changed
    }
}

pub fn run(
    options: &Options,
    program: &mut Program,
    files: &[String],
    out: &mut dyn Write,
) -> io::Result<i32> {
    let mut destinations = Outputs::new(program, if options.null { 0 } else { b'\n' })?;
    execute(options, program, files, out, &mut destinations).map(|outcome| outcome.status)
}
pub fn execute(
    options: &Options,
    program: &mut Program,
    files: &[String],
    out: &mut dyn Write,
    destinations: &mut Outputs,
) -> io::Result<Outcome> {
    let delimiter = if options.null { 0 } else { b'\n' };
    let mut output = Stream::new(out, delimiter);
    let mut input = Input::new(files, delimiter, options.unbuffered)?;
    let mut state = State::new();
    let quiet = options.quiet || program.quiet;
    while let Some(record) = input.take(None, false)? {
        if state.record(record, options.separate) && options.separate {
            for instruction in &mut program.instructions {
                instruction.selection.reset();
            }
            destinations.reset_readers();
        }
        let result = cycle(
            options,
            program,
            &mut state,
            &mut input,
            destinations,
            &mut output,
            quiet,
        )?;
        drain(&mut state, destinations, &mut output, delimiter)?;
        if options.unbuffered {
            output.flush()?;
        }
        if let Some(code) = result {
            destinations.flush()?;
            return Ok(Outcome {
                status: if code == 0 && input.failed { 2 } else { code },
                quit: true,
                input_failed: input.failed,
            });
        }
    }
    destinations.flush()?;
    Ok(Outcome {
        status: if input.failed { 2 } else { 0 },
        quit: false,
        input_failed: input.failed,
    })
}

fn cycle(
    options: &Options,
    program: &mut Program,
    state: &mut State,
    input: &mut Input,
    destinations: &mut Outputs,
    out: &mut Stream<&mut dyn Write>,
    quiet: bool,
) -> io::Result<Option<i32>> {
    actions::Machine {
        options,
        state,
        input,
        destinations,
        out,
        quiet,
        delimiter: if options.null { 0 } else { b'\n' },
    }
    .cycle(program)
}

fn drain(
    state: &mut State,
    destinations: &mut Outputs,
    out: &mut dyn Write,
    delimiter: u8,
) -> io::Result<()> {
    for item in state.queue.drain(..) {
        match item {
            Deferred::Text(text) => {
                out.write_all(&text)?;
                out.write_all(b"\n")?;
            }
            Deferred::Read(name, single) => destinations.read(&name, single, delimiter, out)?,
            Deferred::Bytes(bytes) => out.write_all(&bytes)?,
        }
    }
    Ok(())
}
