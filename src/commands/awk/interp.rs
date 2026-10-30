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
    BinOp, Expr, GetlineSource, Pattern, Program, Redirect, Rule, Stmt, Symbols, VarRef, ARGC,
    ARGV, CONVFMT, ENVIRON, FILENAME, FNR, NF, NR, OFMT, OFS, ORS, RLENGTH, RSTART, SUBSEP,
};
use super::builtins::Rng;
use super::fields::Record;
use super::format::sprintf;
use super::io::{FileRead, IoState};
use super::regex::RegexCache;
use super::value::{compare, Value};

const DEFAULT_FORMAT: &[u8] = b"%.6g";
/// Deepest function call nesting before the program is stopped.
const MAX_CALL_DEPTH: usize = 10_000;

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

    fn exec_block(&mut self, body: &[Stmt]) -> Result<Flow, Fault> {
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

    fn var_loc(&self, var: VarRef) -> Loc {
        match var {
            VarRef::Global(index) => Loc::Global(index),
            VarRef::Local(index) => Loc::Local(self.frames.len() - 1, index),
        }
    }

    fn slot_at(&self, loc: Loc) -> &Slot {
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

    fn read_var(&mut self, var: VarRef) -> Result<Value, Fault> {
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

    // ----- expressions -------------------------------------------------------------------

    pub(super) fn eval(&mut self, expr: &Expr) -> Result<Value, Fault> {
        match expr {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(bytes) => Ok(Value::Str(bytes.clone())),
            Expr::Regex(source) => {
                let regex = self.regex.get(source).map_err(Fault::Fatal)?;
                let matched = regex.is_match(self.record_text());
                Ok(Value::Num(f64::from(u8::from(matched))))
            }
            Expr::Var(var) => self.read_var(*var),
            Expr::Field(index) => {
                let number = self.eval(index)?.to_num();
                if number < 0.0 {
                    return Err(Fault::Fatal(format!(
                        "trying to access out of range field {}",
                        number as i64
                    )));
                }
                self.get_field(number as usize)
            }
            Expr::Index(var, subs) => {
                let key = self.subscript(subs)?;
                let array = self.array_of(*var)?;
                let value = array
                    .borrow_mut()
                    .entry(key)
                    .or_insert(Value::Uninit)
                    .clone();
                Ok(value)
            }
            Expr::Assign(target, value) => {
                let value = self.eval(value)?;
                let place = self.place(target)?;
                self.set_place(place, value.clone())?;
                Ok(value)
            }
            Expr::AugAssign(op, target, value) => {
                let place = self.place(target)?;
                let current = self.get_place(&place)?.to_num();
                let operand = self.eval(value)?.to_num();
                let next = Value::Num(arithmetic(*op, current, operand)?);
                self.set_place(place, next.clone())?;
                Ok(next)
            }
            Expr::IncDec { target, pre, delta } => {
                let place = self.place(target)?;
                let old = self.get_place(&place)?.to_num();
                let new = old + delta;
                self.set_place(place, Value::Num(new))?;
                Ok(Value::Num(if *pre { new } else { old }))
            }
            Expr::Binary(op, lhs, rhs) => {
                let left = self.eval(lhs)?;
                let right = self.eval(rhs)?;
                self.binary(*op, &left, &right)
            }
            Expr::And(lhs, rhs) => {
                let result = self.eval(lhs)?.truthy() && self.eval(rhs)?.truthy();
                Ok(Value::Num(f64::from(u8::from(result))))
            }
            Expr::Or(lhs, rhs) => {
                let result = self.eval(lhs)?.truthy() || self.eval(rhs)?.truthy();
                Ok(Value::Num(f64::from(u8::from(result))))
            }
            Expr::Not(operand) => {
                let result = !self.eval(operand)?.truthy();
                Ok(Value::Num(f64::from(u8::from(result))))
            }
            Expr::Neg(operand) => Ok(Value::Num(-self.eval(operand)?.to_num())),
            Expr::Plus(operand) => Ok(Value::Num(self.eval(operand)?.to_num())),
            Expr::Match {
                negate,
                subject,
                pattern,
            } => {
                let text = self.eval_string(subject)?;
                let regex = self.regex_of(pattern)?;
                let matched = regex.is_match(&text);
                Ok(Value::Num(f64::from(u8::from(matched != *negate))))
            }
            Expr::Cond(cond, then, other) => {
                if self.eval(cond)?.truthy() {
                    self.eval(then)
                } else {
                    self.eval(other)
                }
            }
            Expr::In(subs, var) => {
                let key = self.subscript(subs)?;
                let array = self.array_of(*var)?;
                let found = array.borrow().contains_key(&key);
                Ok(Value::Num(f64::from(u8::from(found))))
            }
            Expr::Call(index, args) => self.call_function(*index, args),
            Expr::Builtin(builtin, args) => self.call_builtin(*builtin, args),
            Expr::Getline { source, target } => self.getline(source, target.as_deref()),
            Expr::List(_) => Err(Fault::Fatal("syntax error: unexpected list".into())),
        }
    }

    fn binary(&mut self, op: BinOp, left: &Value, right: &Value) -> Result<Value, Fault> {
        let result = match op {
            BinOp::Concat => {
                let mut text = self.to_str(left).into_owned();
                text.extend_from_slice(&self.to_str(right));
                return Ok(Value::Str(text));
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                let ordering = compare(left, right, self.convfmt());
                let holds = match op {
                    BinOp::Lt => ordering.is_lt(),
                    BinOp::Le => ordering.is_le(),
                    BinOp::Gt => ordering.is_gt(),
                    BinOp::Ge => ordering.is_ge(),
                    BinOp::Eq => ordering.is_eq(),
                    _ => ordering.is_ne(),
                };
                f64::from(u8::from(holds))
            }
            _ => arithmetic(op, left.to_num(), right.to_num())?,
        };
        Ok(Value::Num(result))
    }

    fn getline(&mut self, source: &GetlineSource, target: Option<&Expr>) -> Result<Value, Fault> {
        let record = match source {
            GetlineSource::Main => match self.next_main_record()? {
                Some(record) => record,
                None => return Ok(Value::Num(0.0)),
            },
            GetlineSource::File(name) => {
                let name = self.eval_string(name)?;
                let mode = self.rs_mode()?;
                match self.read_named_file(&name, &mode) {
                    FileRead::Record(record) => record,
                    FileRead::Eof => return Ok(Value::Num(0.0)),
                    FileRead::Failed => return Ok(Value::Num(-1.0)),
                }
            }
            GetlineSource::Command(command) => {
                self.eval(command)?;
                return Err(Fault::Fatal(
                    "input pipes (cmd | getline) are not supported by agentcmd".into(),
                ));
            }
        };
        match target {
            Some(expr) => {
                let place = self.place(expr)?;
                self.set_place(place, Value::from_input(record))?;
            }
            None => self.set_record(record),
        }
        Ok(Value::Num(1.0))
    }

    /// Calls a user function. Scalars are passed by value; arrays and uninitialized
    /// variables by reference.
    fn call_function(&mut self, index: usize, args: &[Expr]) -> Result<Value, Fault> {
        let prog = Rc::clone(&self.prog);
        let function = &prog.functions[index];
        if args.len() > function.params {
            return Err(Fault::Fatal(format!(
                "function `{}' called with more arguments than declared",
                function.name
            )));
        }
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(Fault::Fatal("function call nesting too deep".into()));
        }
        let mut frame = Vec::with_capacity(function.params);
        for arg in args {
            frame.push(self.argument_slot(arg)?);
        }
        frame.resize(function.params, Slot::Uninit);
        self.frames.push(frame);
        let result = self.exec_block(&function.body);
        self.frames.pop();
        match result? {
            Flow::Return(value) => Ok(value),
            Flow::Normal | Flow::Break | Flow::Continue => Ok(Value::Uninit),
            jump @ (Flow::Next | Flow::NextFile | Flow::Exit) => Err(Fault::Jump(jump)),
        }
    }

    fn argument_slot(&mut self, arg: &Expr) -> Result<Slot, Fault> {
        let Expr::Var(var) = arg else {
            return Ok(Slot::Val(self.eval(arg)?));
        };
        if *var == VarRef::Global(NF) {
            return Ok(Slot::Val(self.read_var(*var)?));
        }
        let loc = self.var_loc(*var);
        Ok(match self.slot_at(loc).clone() {
            Slot::Array(array) => Slot::Array(array),
            Slot::Uninit => Slot::Ref(loc),
            Slot::Ref(target) => Slot::Ref(target),
            Slot::Val(value) => Slot::Val(value),
        })
    }
}

/// Arithmetic on numbers for the binary operators and compound assignment.
fn arithmetic(op: BinOp, x: f64, y: f64) -> Result<f64, Fault> {
    Ok(match op {
        BinOp::Add => x + y,
        BinOp::Sub => x - y,
        BinOp::Mul => x * y,
        BinOp::Div => {
            if y == 0.0 {
                return Err(Fault::Fatal("division by zero attempted".into()));
            }
            x / y
        }
        BinOp::Mod => {
            if y == 0.0 {
                return Err(Fault::Fatal("division by zero attempted in `%'".into()));
            }
            x % y
        }
        BinOp::Pow => x.powf(y),
        _ => x,
    })
}
