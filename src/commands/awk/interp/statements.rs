use super::*;
impl Runtime<'_> {
    pub(super) fn statement(&mut self, statement: &Stmt) -> Result<()> {
        match statement.as_ref() {
            Statement::Empty => {}
            Statement::Expression(e) => {
                self.eval(e)?;
            }
            Statement::Block(body) => {
                for statement in body {
                    self.statement(statement)?;
                }
            }
            Statement::If(condition, yes, no) => {
                if self.eval(condition)?.truth() {
                    self.statement(yes)?;
                } else if let Some(no) = no {
                    self.statement(no)?;
                }
            }
            Statement::While(condition, body) => self.while_loop(condition, body, false)?,
            Statement::Do(body, condition) => self.while_loop(condition, body, true)?,
            Statement::For(init, condition, update, body) => {
                self.for_loop(init.as_ref(), condition.as_ref(), update.as_ref(), body)?
            }
            Statement::ForIn(index, array, body) => {
                let array = self.array(*array)?;
                let keys: Vec<_> = array.borrow().keys().cloned().collect();
                for key in keys {
                    self.set_var(*index, Scalar::string(key))?;
                    if !self.loop_body(body)? {
                        break;
                    }
                }
            }
            Statement::Break => return Err(Error::Break),
            Statement::Continue => return Err(Error::Continue),
            Statement::Next | Statement::NextFile => {
                if self.phase != 1 {
                    return Err(Error::from("next/nextfile used in BEGIN or END action"));
                }
                return Err(if matches!(statement.as_ref(), Statement::Next) {
                    Error::Next
                } else {
                    Error::NextFile
                });
            }
            Statement::Exit(value) => {
                if let Some(value) = value {
                    let value = self.eval(value)?.number();
                    self.exit_status = if value.is_finite() {
                        value.trunc().rem_euclid(256.0) as i32
                    } else {
                        0
                    };
                }
                return Err(Error::Exit);
            }
            Statement::Return(value) => {
                return Err(Error::Return(if let Some(value) = value {
                    self.eval(value)?
                } else {
                    Scalar::Uninit
                }))
            }
            Statement::Delete(array, keys) => {
                let key = if let Some(keys) = keys {
                    Some(self.key(keys)?)
                } else {
                    None
                };
                let array = self.array(*array)?;
                if let Some(key) = key {
                    array.borrow_mut().remove(&key);
                } else {
                    array.borrow_mut().clear();
                }
            }
            Statement::Print {
                formatted,
                args,
                redirect,
            } => self.print(*formatted, args, redirect.as_ref())?,
        }
        Ok(())
    }
    fn loop_body(&mut self, body: &Stmt) -> Result<bool> {
        match self.statement(body) {
            Ok(()) | Err(Error::Continue) => Ok(true),
            Err(Error::Break) => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn while_loop(&mut self, condition: &Expr, body: &Stmt, mut first: bool) -> Result<()> {
        while first || self.eval(condition)?.truth() {
            first = false;
            if !self.loop_body(body)? {
                break;
            }
        }
        Ok(())
    }
    fn for_loop(
        &mut self,
        init: Option<&Expr>,
        condition: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
    ) -> Result<()> {
        if let Some(init) = init {
            self.eval(init)?;
        }
        while if let Some(condition) = condition {
            self.eval(condition)?.truth()
        } else {
            true
        } {
            if !self.loop_body(body)? {
                break;
            }
            if let Some(update) = update {
                self.eval(update)?;
            }
        }
        Ok(())
    }
    fn print(
        &mut self,
        formatted: bool,
        args: &[Expr],
        redirect: Option<&(&'static str, Expr)>,
    ) -> Result<()> {
        let mut values = Vec::new();
        for arg in args {
            values.push(self.eval(arg)?);
        }
        let output = if formatted {
            let format = values
                .first()
                .ok_or("missing printf format")?
                .bytes(&self.convfmt()?)?;
            super::super::format::render(&format, &values[1..], &self.convfmt()?)?
        } else {
            if values.is_empty() {
                values.push(self.field(0)?);
            }
            let mut bytes = Vec::new();
            let ofs = self.text(self.special.ofs)?;
            let ofmt = self.text(self.special.ofmt)?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    bytes.extend_from_slice(&ofs);
                }
                bytes.extend_from_slice(&value.bytes(&ofmt)?);
            }
            bytes.extend_from_slice(&self.text(self.special.ors)?);
            bytes
        };
        if let Some((kind, file)) = redirect {
            if *kind == "|" {
                return Err(Error::from("command pipes are not supported by agentcmd"));
            }
            let name = self.eval(file)?.bytes(&self.convfmt()?)?;
            self.files.write(&name, *kind == ">>", &output, self.out)?;
        } else {
            self.out.write_all(&output)?;
        }
        Ok(())
    }
}
