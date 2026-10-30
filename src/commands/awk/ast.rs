//! Syntax tree and symbol table for awk programs.
//!
//! Variables are resolved at parse time: globals (including the special variables, which
//! occupy fixed slots) and function-local parameters.

use std::collections::HashMap;

pub const NR: usize = 0;
pub const NF: usize = 1;
pub const FNR: usize = 2;
pub const FS: usize = 3;
pub const OFS: usize = 4;
pub const ORS: usize = 5;
pub const RS: usize = 6;
pub const FILENAME: usize = 7;
pub const SUBSEP: usize = 8;
pub const RSTART: usize = 9;
pub const RLENGTH: usize = 10;
pub const CONVFMT: usize = 11;
pub const OFMT: usize = 12;
pub const ENVIRON: usize = 13;
pub const ARGC: usize = 14;
pub const ARGV: usize = 15;

/// Names of the special variables, indexed by the constants above.
pub const SPECIAL_NAMES: [&str; 16] = [
    "NR", "NF", "FNR", "FS", "OFS", "ORS", "RS", "FILENAME", "SUBSEP", "RSTART", "RLENGTH",
    "CONVFMT", "OFMT", "ENVIRON", "ARGC", "ARGV",
];

/// A reference to a variable: a global slot or a parameter of the running function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarRef {
    Global(usize),
    Local(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Concat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Length,
    Substr,
    Index,
    Split,
    Sub,
    Gsub,
    Match,
    Sprintf,
    Sin,
    Cos,
    Atan2,
    Exp,
    Log,
    Sqrt,
    Int,
    Rand,
    Srand,
    Tolower,
    Toupper,
    Close,
    Fflush,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirKind {
    File,
    Append,
    Pipe,
}

#[derive(Debug)]
pub enum GetlineSource {
    /// `getline` or `getline var`: the main input (ARGV files or stdin).
    Main,
    /// `getline < file`.
    File(Box<Expr>),
    /// `cmd | getline`. Not supported at run time.
    Command(Box<Expr>),
}

#[derive(Debug)]
pub enum Expr {
    Num(f64),
    Str(Vec<u8>),
    /// A regex literal. As a value it matches against `$0`.
    Regex(Vec<u8>),
    Var(VarRef),
    Field(Box<Expr>),
    Index(VarRef, Vec<Expr>),
    /// Plain assignment: target, value.
    Assign(Box<Expr>, Box<Expr>),
    AugAssign(BinOp, Box<Expr>, Box<Expr>),
    IncDec {
        target: Box<Expr>,
        pre: bool,
        delta: f64,
    },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Plus(Box<Expr>),
    Match {
        negate: bool,
        subject: Box<Expr>,
        pattern: Box<Expr>,
    },
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    In(Vec<Expr>, VarRef),
    Call(usize, Vec<Expr>),
    Builtin(Builtin, Vec<Expr>),
    Getline {
        source: GetlineSource,
        target: Option<Box<Expr>>,
    },
    /// A parenthesized comma list. Only `print`/`printf` arguments and `(i, j) in a` accept it.
    List(Vec<Expr>),
}

#[derive(Debug)]
pub struct Redirect {
    pub kind: RedirKind,
    pub target: Expr,
}

#[derive(Debug)]
pub enum Stmt {
    Expr(Expr),
    Print(Vec<Expr>, Option<Redirect>),
    Printf(Vec<Expr>, Option<Redirect>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    Do(Box<Stmt>, Expr),
    For {
        init: Option<Expr>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Box<Stmt>,
    },
    ForIn(VarRef, VarRef, Box<Stmt>),
    Block(Vec<Stmt>),
    Next,
    NextFile,
    Break,
    Continue,
    Exit(Option<Expr>),
    Return(Option<Expr>),
    Delete(VarRef, Option<Vec<Expr>>),
}

#[derive(Debug)]
pub enum Pattern {
    All,
    Expr(Expr),
    Range(Expr, Expr),
}

#[derive(Debug)]
pub struct Rule {
    pub pattern: Pattern,
    /// `None` means the default action: print `$0`.
    pub action: Option<Vec<Stmt>>,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: usize,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Default)]
pub struct Program {
    pub begin: Vec<Vec<Stmt>>,
    pub end: Vec<Vec<Stmt>>,
    pub rules: Vec<Rule>,
    pub functions: Vec<Function>,
}

/// Global variable names. The special variables are interned first, in `SPECIAL_NAMES` order.
#[derive(Debug)]
pub struct Symbols {
    names: Vec<String>,
    index: HashMap<String, usize>,
}

impl Symbols {
    pub fn new() -> Self {
        let mut symbols = Self {
            names: Vec::new(),
            index: HashMap::new(),
        };
        for name in SPECIAL_NAMES {
            symbols.intern(name);
        }
        symbols
    }

    pub fn intern(&mut self, name: &str) -> usize {
        if let Some(&index) = self.index.get(name) {
            return index;
        }
        let index = self.names.len();
        self.names.push(name.to_owned());
        self.index.insert(name.to_owned(), index);
        index
    }

    pub fn lookup(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

impl Default for Symbols {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_variables_have_fixed_slots() {
        let symbols = Symbols::new();
        assert_eq!(symbols.lookup("NR"), Some(NR));
        assert_eq!(symbols.lookup("ARGV"), Some(ARGV));
        assert_eq!(symbols.lookup("CONVFMT"), Some(CONVFMT));
        assert_eq!(symbols.len(), SPECIAL_NAMES.len());
    }

    #[test]
    fn intern_is_idempotent() {
        let mut symbols = Symbols::new();
        let first = symbols.intern("total");
        assert_eq!(symbols.intern("total"), first);
        assert!(first >= SPECIAL_NAMES.len());
        assert!(!symbols.is_empty());
    }
}
