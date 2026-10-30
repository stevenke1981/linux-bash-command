//! The interpreter: program state, statements, expressions, variables and user functions.
//!
//! Locals of a function call live in a frame. An array argument is passed by reference. An
//! uninitialized variable passed as an argument is passed as a reference to its location, so
//! the callee can turn it into an array that the caller then sees.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::ast::{
    Expr, Pattern, Program, Redirect, Rule, Stmt, Symbols, VarRef, ARGC, ARGV, CONVFMT, ENVIRON,
    FILENAME, FNR, NF, NR, OFMT, OFS, ORS, RLENGTH, RSTART, SUBSEP,
};
use super::builtins::Rng;
use super::fields::Record;
use super::format::sprintf;
use super::io::IoState;
use super::regex::RegexCache;
use super::value::Value;

const DEFAULT_FORMAT: &[u8] = b"%.6g";
/// Deepest function call nesting before the program is stopped.
pub(super) const MAX_CALL_DEPTH: usize = 10_000;

/// Map from subscript to element value.
pub type Array = BTreeMap<Vec<u8>, Value>;
pub type ArrayRef = Rc<RefCell<Array>>;

/// Where a variable lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loc {
    Global(usize),
    /// A frame index (into `Interp::frames`) and a slot within it.
    Local(usize, usize),
}

/// The contents of a variable slot.
#[derive(Clone, Debug)]
pub enum Slot {
    Uninit,
    Val(Value),
    Array(ArrayRef),
    /// An uninitialized argument: the caller's variable, which becomes an array if used as one.
    Ref(Loc),
}

/// How a statement finished.
#[derive(Debug)]
pub enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
    Next,
    NextFile,
    Exit,
}

/// A failure that stops the statement or the program.
pub enum Fault {
    /// A run-time error: printed as `awk: MESSAGE`, exit status 2.
    Fatal(String),
    /// Stop quietly with this status (a closed output pipe).
    Silent(i32),
    /// `next`, `nextfile` or `exit` raised inside an expression (from a function body).
    Jump(Flow),
}

/// An assignable location.
pub enum Place {
    Var(VarRef),
    Field(usize),
    Elem(ArrayRef, Vec<u8>),
}

pub struct Interp {
    pub(super) prog: Rc<Program>,
    pub(super) symbols: Symbols,
    pub(super) globals: Vec<Slot>,
    pub(super) frames: Vec<Vec<Slot>>,
    pub(super) rec: Record,
    pub(super) regex: RegexCache,
    pub(super) io: IoState,
    pub(super) rng: Rng,
    pub(super) range_active: Vec<bool>,
    pub(super) exit_code: Option<i32>,
}

impl Interp {
    /// Creates the interpreter with its special variables, `ENVIRON` and `ARGV` set up.
    pub fn new(
        prog: Program,
        symbols: Symbols,
        argv: Vec<Vec<u8>>,
        environ: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Self {
        let count = symbols.len();
        let mut globals = vec![Slot::Uninit; count];
        let text = |s: &str| Slot::Val(Value::Str(s.as_bytes().to_vec()));
        let number = |n: f64| Slot::Val(Value::Num(n));
        globals[NR] = number(0.0);
        globals[NF] = number(0.0);
        globals[FNR] = number(0.0);
        globals[super::ast::FS] = text(" ");
        globals[OFS] = text(" ");
        globals[ORS] = text("\n");
        globals[super::ast::RS] = text("\n");
        globals[FILENAME] = text("");
        globals[SUBSEP] = text("\x1c");
        globals[RSTART] = number(0.0);
        globals[RLENGTH] = number(-1.0);
        globals[CONVFMT] = text("%.6g");
        globals[OFMT] = text("%.6g");
        globals[ARGC] = number(argv.len() as f64);
        let mut env_map = Array::new();
        for (name, value) in environ {
            env_map.insert(name, Value::from_input(value));
        }
        globals[ENVIRON] = Slot::Array(Rc::new(RefCell::new(env_map)));
        let mut argv_map = Array::new();
        for (index, arg) in argv.into_iter().enumerate() {
            argv_map.insert(index.to_string().into_bytes(), Value::from_input(arg));
        }
        globals[ARGV] = Slot::Array(Rc::new(RefCell::new(argv_map)));
        let rule_count = prog.rules.len();
        Self {
            prog: Rc::new(prog),
            symbols,
            globals,
            frames: Vec::new(),
            rec: Record::new(),
            regex: RegexCache::new(),
            io: IoState::new(),
            rng: Rng::new(),
            range_active: vec![false; rule_count],
            exit_code: None,
        }
    }

    /// Runs BEGIN, the main loop and END. Returns the exit status.
    pub fn run(&mut self) -> i32 {
        let outcome = self.run_program();
        self.close_all();
        match outcome {
            Ok(()) | Err(Fault::Jump(_)) => {}
            Err(Fault::Silent(code)) => return code,
            Err(Fault::Fatal(message)) => {
                eprintln!("awk: {message}");
                return 2;
            }
        }
        self.exit_code.unwrap_or(0)
    }

    fn run_program(&mut self) -> Result<(), Fault> {
        let prog = Rc::clone(&self.prog);
        let mut exiting = false;
        for block in &prog.begin {
            if matches!(self.exec_top(block)?, Flow::Exit) {
                exiting = true;
                break;
            }
        }
        if !exiting && (!prog.rules.is_empty() || !prog.end.is_empty()) {
            self.main_loop(&prog)?;
        }
        for block in &prog.end {
            if matches!(self.exec_top(block)?, Flow::Exit) {
                break;
            }
        }
        Ok(())
    }

    /// Processes records. Returns true when `exit` was executed.
    fn main_loop(&mut self, prog: &Program) -> Result<bool, Fault> {
        'records: while let Some(record) = self.next_main_record()? {
            self.set_record(record);
            for (index, rule) in prog.rules.iter().enumerate() {
                if !self.rule_matches(index, rule)? {
                    continue;
                }
                match &rule.action {
                    None => {
                        let mut out = self.record_text().to_vec();
                        out.extend_from_slice(&self.special_bytes(ORS));
                        self.write_stdout(&out)?;
                    }
                    Some(body) => match self.exec_top(body)? {
                        Flow::Next => continue 'records,
                        Flow::NextFile => {
                            self.close_main_file();
                            continue 'records;
                        }
                        Flow::Exit => return Ok(true),
                        _ => {}
                    },
                }
            }
        }
        Ok(false)
    }

    fn rule_matches(&mut self, index: usize, rule: &Rule) -> Result<bool, Fault> {
        match &rule.pattern {
            Pattern::All => Ok(true),
            Pattern::Expr(expr) => Ok(self.eval(expr)?.truthy()),
            Pattern::Range(start, end) => {
                if self.range_active[index] {
                    if self.eval(end)?.truthy() {
                        self.range_active[index] = false;
                    }
                    Ok(true)
                } else if self.eval(start)?.truthy() {
                    if !self.eval(end)?.truthy() {
                        self.range_active[index] = true;
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// Runs a block, turning jumps raised inside expressions into flow results.
    fn exec_top(&mut self, body: &[Stmt]) -> Result<Flow, Fault> {
        match self.exec_block(body) {
            Err(Fault::Jump(flow)) => Ok(flow),
            other => other,
        }
    }

    pub(super) fn exec_block(&mut self, body: &[Stmt]) -> Result<Flow, Fault> {
        for statement in body {
            match self.exec(statement)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec(&mut self, statement: &Stmt) -> Result<Flow, Fault> {
        match statement {
            Stmt::Expr(expr) => {
                self.eval(expr)?;
            }
            Stmt::Print(args, redirect) => self.exec_print(args, redirect.as_ref())?,
            Stmt::Printf(args, redirect) => self.exec_printf(args, redirect.as_ref())?,
            Stmt::If(cond, then, other) => {
                if self.eval(cond)?.truthy() {
                    return self.exec(then);
                } else if let Some(other) = other {
                    return self.exec(other);
                }
            }
            Stmt::While(cond, body) => {
                while self.eval(cond)?.truthy() {
                    match self.exec(body)? {
                        Flow::Break => break,
                        Flow::Normal | Flow::Continue => {}
                        other => return Ok(other),
                    }
                }
            }
            Stmt::Do(body, cond) => loop {
                match self.exec(body)? {
                    Flow::Break => break,
                    Flow::Normal | Flow::Continue => {}
                    other => return Ok(other),
                }
                if !self.eval(cond)?.truthy() {
                    break;
                }
            },
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(init) = init {
                    self.eval(init)?;
                }
                loop {
                    if let Some(cond) = cond {
                        if !self.eval(cond)?.truthy() {
                            break;
                        }
                    }
                    match self.exec(body)? {
                        Flow::Break => break,
                        Flow::Normal | Flow::Continue => {}
                        other => return Ok(other),
                    }
                    if let Some(step) = step {
                        self.eval(step)?;
                    }
                }
            }
            Stmt::ForIn(var, array, body) => {
                let keys: Vec<Vec<u8>> = self.array_of(*array)?.borrow().keys().cloned().collect();
                for key in keys {
                    self.write_var(*var, Value::Str(key))?;
                    match self.exec(body)? {
                        Flow::Break => break,
                        Flow::Normal | Flow::Continue => {}
                        other => return Ok(other),
                    }
                }
            }
            Stmt::Block(body) => return self.exec_block(body),
            Stmt::Next => return Ok(Flow::Next),
            Stmt::NextFile => return Ok(Flow::NextFile),
            Stmt::Break => return Ok(Flow::Break),
            Stmt::Continue => return Ok(Flow::Continue),
            Stmt::Exit(code) => {
                if let Some(code) = code {
                    let value = self.eval(code)?.to_num();
                    self.exit_code = Some(value as i64 as i32);
                }
                return Ok(Flow::Exit);
            }
            Stmt::Return(value) => {
                let value = match value {
                    Some(expr) => self.eval(expr)?,
                    None => Value::Uninit,
                };
                return Ok(Flow::Return(value));
            }
            Stmt::Delete(array, subs) => {
                let array = self.array_of(*array)?;
                match subs {
                    None => array.borrow_mut().clear(),
                    Some(subs) => {
                        let key = self.subscript(subs)?;
                        array.borrow_mut().remove(&key);
                    }
                }
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_print(&mut self, args: &[Expr], redirect: Option<&Redirect>) -> Result<(), Fault> {
        let mut out = Vec::new();
        if args.is_empty() {
            out.extend_from_slice(self.record_text());
        } else {
            let ofs = self.special_bytes(OFS);
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.extend_from_slice(&ofs);
                }
                let value = self.eval(arg)?;
                out.extend_from_slice(&value.to_output(self.ofmt()));
            }
        }
        out.extend_from_slice(&self.special_bytes(ORS));
        self.emit(redirect, &out)
    }

    fn exec_printf(&mut self, args: &[Expr], redirect: Option<&Redirect>) -> Result<(), Fault> {
        let format = self.eval_string(&args[0])?;
        let mut values = Vec::with_capacity(args.len() - 1);
        for arg in &args[1..] {
            values.push(self.eval(arg)?);
        }
        let out = sprintf(&format, &values, self.convfmt());
        self.emit(redirect, &out)
    }

    // ----- values and conversions --------------------------------------------------------

    /// `CONVFMT` as bytes (the default when it is not a string).
    pub(super) fn convfmt(&self) -> &[u8] {
        match &self.globals[CONVFMT] {
            Slot::Val(Value::Str(bytes) | Value::StrNum(bytes, _)) => bytes,
            _ => DEFAULT_FORMAT,
        }
    }

    pub(super) fn ofmt(&self) -> &[u8] {
        match &self.globals[OFMT] {
            Slot::Val(Value::Str(bytes) | Value::StrNum(bytes, _)) => bytes,
            _ => DEFAULT_FORMAT,
        }
    }

    /// String value of `value` using `CONVFMT`.
    pub(super) fn to_str<'a>(&'a self, value: &'a Value) -> Cow<'a, [u8]> {
        value.to_str(self.convfmt())
    }

    /// The string value of a special variable (or of any scalar global).
    pub(super) fn special_bytes(&self, index: usize) -> Vec<u8> {
        match &self.globals[index] {
            Slot::Val(value) => value.to_str(self.convfmt()).into_owned(),
            _ => Vec::new(),
        }
    }

    pub(super) fn global_number(&self, index: usize) -> f64 {
        match &self.globals[index] {
            Slot::Val(value) => value.to_num(),
            _ => 0.0,
        }
    }

    pub(super) fn set_global_value(&mut self, index: usize, value: Value) {
        self.globals[index] = Slot::Val(value);
    }

    pub(super) fn add_to_global(&mut self, index: usize, delta: f64) {
        let next = self.global_number(index) + delta;
        self.globals[index] = Slot::Val(Value::Num(next));
    }

    /// Converts a string argument (for `-v`, operands and `FS`) and assigns it to a variable.
    pub(super) fn assign_from_text(&mut self, name: &str, raw: &[u8]) -> Result<(), Fault> {
        let Some(index) = self.symbols.lookup(name) else {
            return Ok(());
        };
        let value = Value::from_input(super::lexer::unescape(raw));
        self.write_var(VarRef::Global(index), value)
    }

    /// Sets a variable to a string without escape processing (for `-F`).
    pub(super) fn assign_plain(&mut self, name: &str, text: Vec<u8>) {
        if let Some(index) = self.symbols.lookup(name) {
            self.globals[index] = Slot::Val(Value::Str(text));
        }
    }

    /// The `ARGV` element at `index`, when present.
    pub(super) fn argv_element(&self, index: usize) -> Option<Vec<u8>> {
        let Slot::Array(array) = &self.globals[ARGV] else {
            return None;
        };
        let map = array.borrow();
        map.get(index.to_string().as_bytes())
            .map(|value| value.to_str(DEFAULT_FORMAT).into_owned())
    }

    // ----- variables ---------------------------------------------------------------------

    pub(super) fn var_is_array(&self, var: VarRef) -> bool {
        matches!(self.slot_at(self.var_loc(var)), Slot::Array(_))
    }

    pub(super) fn var_loc(&self, var: VarRef) -> Loc {
        match var {
            VarRef::Global(index) => Loc::Global(index),
            VarRef::Local(index) => Loc::Local(self.frames.len() - 1, index),
        }
    }

    pub(super) fn slot_at(&self, loc: Loc) -> &Slot {
        match loc {
            Loc::Global(index) => &self.globals[index],
            Loc::Local(frame, index) => &self.frames[frame][index],
        }
    }

    fn set_slot(&mut self, loc: Loc, slot: Slot) {
        match loc {
            Loc::Global(index) => self.globals[index] = slot,
            Loc::Local(frame, index) => self.frames[frame][index] = slot,
        }
    }

    pub(super) fn read_var(&mut self, var: VarRef) -> Result<Value, Fault> {
        if var == VarRef::Global(NF) {
            return Ok(Value::Num(self.nf()? as f64));
        }
        match self.slot_at(self.var_loc(var)) {
            Slot::Uninit | Slot::Ref(_) => Ok(Value::Uninit),
            Slot::Val(value) => Ok(value.clone()),
            Slot::Array(_) => Err(Fault::Fatal(
                "attempt to use array in a scalar context".into(),
            )),
        }
    }

    fn write_var(&mut self, var: VarRef, value: Value) -> Result<(), Fault> {
        if var == VarRef::Global(NF) {
            let count = value.to_num().max(0.0) as usize;
            return self.set_nf(count);
        }
        let loc = self.var_loc(var);
        if matches!(self.slot_at(loc), Slot::Array(_)) {
            return Err(Fault::Fatal(
                "attempt to use array in a scalar context".into(),
            ));
        }
        self.set_slot(loc, Slot::Val(value));
        Ok(())
    }

    /// The array a variable refers to, creating it when the variable is still uninitialized.
    pub(super) fn array_of(&mut self, var: VarRef) -> Result<ArrayRef, Fault> {
        let loc = self.var_loc(var);
        self.array_at(loc)
    }

    fn array_at(&mut self, loc: Loc) -> Result<ArrayRef, Fault> {
        let slot = self.slot_at(loc).clone();
        match slot {
            Slot::Array(array) => Ok(array),
            Slot::Uninit => {
                let array: ArrayRef = Rc::new(RefCell::new(Array::new()));
                self.set_slot(loc, Slot::Array(Rc::clone(&array)));
                Ok(array)
            }
            Slot::Ref(target) => {
                let array = self.array_at(target)?;
                self.set_slot(loc, Slot::Array(Rc::clone(&array)));
                Ok(array)
            }
            Slot::Val(_) => Err(Fault::Fatal("attempt to use scalar as array".into())),
        }
    }

    /// The subscript for `arr[subs]`: `SUBSEP` joins several subscripts.
    pub(super) fn subscript(&mut self, subs: &[Expr]) -> Result<Vec<u8>, Fault> {
        if let [single] = subs {
            let value = self.eval(single)?;
            return Ok(self.to_str(&value).into_owned());
        }
        let separator = self.special_bytes(SUBSEP);
        let mut key = Vec::new();
        for (index, sub) in subs.iter().enumerate() {
            if index > 0 {
                key.extend_from_slice(&separator);
            }
            let value = self.eval(sub)?;
            key.extend_from_slice(&self.to_str(&value));
        }
        Ok(key)
    }

    /// Resolves an assignable expression, evaluating its subscripts once.
    pub(super) fn place(&mut self, expr: &Expr) -> Result<Place, Fault> {
        match expr {
            Expr::Var(var) => Ok(Place::Var(*var)),
            Expr::Field(index) => {
                let number = self.eval(index)?.to_num();
                if number < 0.0 {
                    return Err(Fault::Fatal(format!(
                        "trying to access out of range field {}",
                        number as i64
                    )));
                }
                Ok(Place::Field(number as usize))
            }
            Expr::Index(var, subs) => {
                let key = self.subscript(subs)?;
                let array = self.array_of(*var)?;
                Ok(Place::Elem(array, key))
            }
            _ => Err(Fault::Fatal("assignment to a non-variable".into())),
        }
    }

    pub(super) fn get_place(&mut self, place: &Place) -> Result<Value, Fault> {
        match place {
            Place::Var(var) => self.read_var(*var),
            Place::Field(index) => self.get_field(*index),
            Place::Elem(array, key) => Ok(array
                .borrow_mut()
                .entry(key.clone())
                .or_insert(Value::Uninit)
                .clone()),
        }
    }

    pub(super) fn set_place(&mut self, place: Place, value: Value) -> Result<(), Fault> {
        match place {
            Place::Var(var) => self.write_var(var, value),
            Place::Field(index) => self.set_field(index, value),
            Place::Elem(array, key) => {
                array.borrow_mut().insert(key, value);
                Ok(())
            }
        }
    }
}
