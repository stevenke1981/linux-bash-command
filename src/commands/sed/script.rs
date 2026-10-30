//! Compiled script: the command list, its options and the types the parser produces.

use super::addr::Selector;
use super::subst::Subst;

/// Where a piece of script text came from, for error messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The n-th `-e` expression (1-based).
    Expr(usize),
    /// A `-f` script file.
    File(String),
}

/// One piece of script text as given on the command line.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub text: Vec<u8>,
    pub origin: Origin,
}

/// Options that change how the script is read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dialect {
    /// `-E`: extended regular expressions.
    pub ere: bool,
    /// `--sandbox`: reject `e`, `r`, `R`, `w`, `W` and the `s` flags `w` and `e`.
    pub sandbox: bool,
}

/// Where an output command writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sink {
    Stdout,
    Stderr,
    /// Index into [`Script::write_files`].
    File(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    Always,
    /// `t`: taken when a substitution was made since the last input line or `t`.
    IfReplaced,
    /// `T`: taken when no substitution was made.
    IfNotReplaced,
}

/// One command with its operation.
#[derive(Debug)]
pub enum Kind {
    /// `{`; `end` is the index of the matching `}`.
    Block {
        end: usize,
    },
    /// `}`: a no-op that keeps the block's end index valid.
    Nop,
    /// `:label`.
    Label(String),
    /// `b`, `t`, `T`; `target` is the index to jump to (the list length for the end).
    Branch {
        kind: BranchKind,
        label: String,
        target: usize,
    },
    Equals,
    Append(Vec<u8>),
    Insert(Vec<u8>),
    Change(Vec<u8>),
    Delete,
    DeleteFirst,
    FileName,
    Get,
    GetAppend,
    Hold,
    HoldAppend,
    /// `l [N]`.
    List(Option<usize>),
    Next,
    NextAppend,
    Print,
    PrintFirst,
    /// `q [EXIT]`: prints the pattern space, then exits.
    Quit(i32),
    /// `Q [EXIT]`: exits without printing.
    QuitSilent(i32),
    /// `r FILE`.
    ReadFile(String),
    /// `R FILE`: one line per execution.
    ReadLine(String),
    Write(Sink),
    WriteFirst(Sink),
    Exchange,
    /// `y`: a byte-to-byte translation table.
    Translate(Box<[u8; 256]>),
    Subst(Box<Subst>),
    Zap,
}

#[derive(Debug)]
pub struct Command {
    pub selector: Selector,
    pub kind: Kind,
}

/// A compiled script.
#[derive(Debug)]
pub struct Script {
    pub commands: Vec<Command>,
    /// Files named by `w`, `W` and `s///w`, opened (truncated) before processing.
    pub write_files: Vec<String>,
    /// The script starts with `#n`, which acts like `-n`.
    pub quiet: bool,
}

/// A script error. `status` is the exit status (1 for syntax errors, 4 for a missing
/// label, as GNU sed reports).
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub status: i32,
}
