//! Expression evaluation: operators, assignments, getline, and user function calls.
//! The statement executor and the variable model live in `interp`.

use std::rc::Rc;

use super::ast::{BinOp, Expr, GetlineSource, VarRef, NF};
use super::interp::{Fault, Flow, Interp, Slot, MAX_CALL_DEPTH};
use super::io::FileRead;
use super::value::{compare, Value};

impl Interp {
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
