use super::*;
impl Runtime<'_> {
    pub(super) fn eval(&mut self, expression: &Expr) -> Result<Scalar> {
        Ok(match expression.as_ref() {
            Expression::Number(n) => Scalar::Number(*n),
            Expression::String(s) => Scalar::string(s.clone()),
            Expression::Regex(pattern) => {
                let text = self.field(0)?.bytes(&self.convfmt()?)?;
                boolean(self.cache.get(pattern)?.is_match(&text))
            }
            Expression::Var(id) => self.get_var(*id)?,
            Expression::Field(index) => {
                let index = variables::field_index(self.eval(index)?.number())?;
                self.field(index)?
            }
            Expression::Index(..) => {
                let target = self.target(expression)?;
                self.read_target(&target)?
            }
            Expression::Tuple(_) => {
                return Err(Error::from("multidimensional index used outside 'in'"))
            }
            Expression::Unary(op, value) => {
                let value = self.eval(value)?;
                match *op {
                    "!" => boolean(!value.truth()),
                    "-" => Scalar::Number(-value.number()),
                    _ => Scalar::Number(value.number()),
                }
            }
            Expression::Binary(op, left, right) => self.binary(op, left, right)?,
            Expression::In(key, array) => {
                let key = if let Expression::Tuple(keys) = key.as_ref() {
                    self.key(keys)?
                } else {
                    self.key(std::slice::from_ref(key))?
                };
                let found = self.array(*array)?.borrow().contains_key(&key);
                boolean(found)
            }
            Expression::Assign(op, left, right) => self.assignment(op, left, right)?,
            Expression::Increment(value, increment, prefix) => {
                let target = self.target(value)?;
                let old = self.read_target(&target)?.number();
                let new = old + increment;
                self.write_target(&target, Scalar::Number(new))?;
                Scalar::Number(if *prefix { new } else { old })
            }
            Expression::Conditional(condition, yes, no) => {
                if self.eval(condition)?.truth() {
                    self.eval(yes)?
                } else {
                    self.eval(no)?
                }
            }
            Expression::Call(id, args) => self.call(*id, args)?,
            Expression::Getline(target, source) => {
                self.getline(target.as_ref(), source.as_ref())?
            }
            Expression::Pipe(..) => {
                return Err(Error::from("command pipes are not supported by agentcmd"))
            }
        })
    }
    fn binary(&mut self, op: &str, left: &Expr, right: &Expr) -> Result<Scalar> {
        let left = self.eval(left)?;
        if op == "&&" && !left.truth() {
            return Ok(boolean(false));
        }
        if op == "||" && left.truth() {
            return Ok(boolean(true));
        }
        if matches!(op, "~" | "!~") {
            let pattern = self.regex_argument(right)?;
            let bytes = left.bytes(&self.convfmt()?)?;
            let found = self.cache.get(&pattern)?.is_match(&bytes);
            return Ok(boolean(found == (op == "~")));
        }
        let right = self.eval(right)?;
        if op == "concat" {
            let format = self.convfmt()?;
            let mut bytes = left.bytes(&format)?;
            bytes.extend_from_slice(&right.bytes(&format)?);
            return Ok(Scalar::string(bytes));
        }
        if matches!(op, "<" | "<=" | ">" | ">=" | "==" | "!=") {
            if left.numeric()
                && right.numeric()
                && (left.number().is_nan() || right.number().is_nan())
            {
                return Ok(boolean(op == "!="));
            }
            let ordering = left.compare(&right, &self.convfmt()?)?;
            return Ok(boolean(match op {
                "<" => ordering.is_lt(),
                "<=" => !ordering.is_gt(),
                ">" => ordering.is_gt(),
                ">=" => !ordering.is_lt(),
                "==" => ordering.is_eq(),
                _ => !ordering.is_eq(),
            }));
        }
        if matches!(op, "&&" | "||") {
            return Ok(boolean(right.truth()));
        }
        Ok(Scalar::Number(arithmetic(
            op,
            left.number(),
            right.number(),
        )?))
    }
    fn assignment(&mut self, op: &str, left: &Expr, right: &Expr) -> Result<Scalar> {
        let target = self.target(left)?;
        let value = if op == "=" {
            self.eval(right)?
        } else {
            let old = self.read_target(&target)?.number();
            let right = self.eval(right)?.number();
            Scalar::Number(arithmetic(op.trim_end_matches('='), old, right)?)
        };
        self.write_target(&target, value.clone())?;
        Ok(value)
    }
    pub(super) fn regex_argument(&mut self, arg: &Expr) -> Result<Vec<u8>> {
        if let Expression::Regex(pattern) = arg.as_ref() {
            Ok(pattern.clone())
        } else {
            let value = self.eval(arg)?;
            Ok(value.bytes(&self.convfmt()?)?)
        }
    }
    fn call(&mut self, id: Id, args: &[Expr]) -> Result<Scalar> {
        let program = self.program.clone();
        let name = &program.names[id];
        if super::super::analyze::builtin(name) {
            return self.builtin(name, args);
        }
        let function = program
            .functions
            .get(&id)
            .ok_or_else(|| Error::Message(format!("function '{name}' not defined")))?;
        if args.len() > function.params.len() {
            return Err(Error::from("function called with too many arguments"));
        }
        if self.locals.len() >= 1024 {
            return Err(Error::from("function call nesting limit exceeded"));
        }
        let mut frame = HashMap::new();
        for (index, param) in function.params.iter().enumerate() {
            let slot = if let Some(arg) = args.get(index) {
                if function.array_params[index] {
                    let Expression::Var(id) = arg.as_ref() else {
                        return Err(Error::from("array parameter requires an array variable"));
                    };
                    Slot::Array(self.array(*id)?)
                } else if let Expression::Var(id) = arg.as_ref() {
                    match self.slot(*id) {
                        Slot::Array(array) => Slot::Array(array.clone()),
                        _ => Slot::Scalar(self.eval(arg)?),
                    }
                } else {
                    Slot::Scalar(self.eval(arg)?)
                }
            } else {
                Slot::Unset
            };
            frame.insert(*param, slot);
        }
        self.locals.push(frame);
        let result = self.statement(&function.body);
        self.locals.pop();
        match result {
            Ok(()) => Ok(Scalar::Uninit),
            Err(Error::Return(value)) => Ok(value),
            Err(error) => Err(error),
        }
    }
}
pub(super) fn boolean(value: bool) -> Scalar {
    Scalar::Number(f64::from(value))
}
fn arithmetic(op: &str, a: f64, b: f64) -> Result<f64> {
    Ok(match op {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "/" | "%" if b == 0.0 => return Err(Error::from("division by zero attempted")),
        "/" => a / b,
        "%" => a % b,
        "^" | "**" => a.powf(b),
        _ => return Err(Error::from("invalid arithmetic operator")),
    })
}
