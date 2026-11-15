use super::*;
pub(super) enum Target {
    Variable(Id),
    Field(usize),
    Element(Array, Vec<u8>),
}
impl Runtime<'_> {
    pub(super) fn slot(&mut self, id: Id) -> &mut Slot {
        if self
            .locals
            .last()
            .is_some_and(|frame| frame.contains_key(&id))
        {
            self.locals.last_mut().unwrap().get_mut(&id).unwrap()
        } else {
            &mut self.globals[id]
        }
    }
    pub(super) fn get_var(&mut self, id: Id) -> Result<Scalar> {
        if id == self.special.nf {
            return Ok(Scalar::Number(self.fields.count(&mut self.cache)? as f64));
        }
        let slot = self.slot(id);
        if matches!(slot, Slot::Unset) {
            *slot = Slot::Scalar(Scalar::Uninit);
        }
        match slot {
            Slot::Scalar(value) => Ok(value.clone()),
            _ => Err(Error::Message(format!(
                "attempt to use array '{}' in a scalar context",
                self.program.names[id]
            ))),
        }
    }
    pub(super) fn set_var(&mut self, id: Id, value: Scalar) -> Result<()> {
        if id == self.special.nf {
            let n = field_index(value.number())?;
            self.fields.resize(n, &mut self.cache)?;
            let ofs = self.text(self.special.ofs)?;
            self.fields.capture_ofs(&ofs);
            return Ok(());
        }
        let slot = self.slot(id);
        if matches!(slot, Slot::Array(_)) {
            return Err(Error::from("attempt to assign a scalar value to an array"));
        }
        *slot = Slot::Scalar(value);
        Ok(())
    }
    pub(super) fn array(&mut self, id: Id) -> Result<Array> {
        let slot = self.slot(id);
        if matches!(slot, Slot::Unset) {
            *slot = Slot::Array(Rc::new(std::cell::RefCell::new(
                std::collections::BTreeMap::new(),
            )));
        }
        match slot {
            Slot::Array(array) => Ok(array.clone()),
            _ => Err(Error::Message(format!(
                "attempt to use scalar '{}' as an array",
                self.program.names[id]
            ))),
        }
    }
    pub(super) fn convfmt(&mut self) -> Result<Rc<Vec<u8>>> {
        Ok(self.get_var(self.special.convfmt)?.shared_bytes(b"%.6g")?)
    }
    pub(super) fn text(&mut self, id: Id) -> Result<Rc<Vec<u8>>> {
        let value = self.get_var(id)?;
        Ok(value.shared_bytes(&self.convfmt()?)?)
    }
    pub(super) fn field(&mut self, index: usize) -> Result<Scalar> {
        if index != 0 {
            return Ok(self.fields.get(index, &[], b"%.6g", &mut self.cache)?);
        }
        let ofs = self.text(self.special.ofs)?;
        let format = self.convfmt()?;
        Ok(self.fields.get(index, &ofs, &format, &mut self.cache)?)
    }
    pub(super) fn key(&mut self, expressions: &[Expr]) -> Result<Vec<u8>> {
        let separator = self.text(self.special.subsep)?;
        let mut key = Vec::new();
        for (index, expression) in expressions.iter().enumerate() {
            if index > 0 {
                key.extend_from_slice(&separator);
            }
            let value = self.eval(expression)?;
            key.extend_from_slice(&value.bytes(&self.convfmt()?)?);
        }
        Ok(key)
    }
    pub(super) fn target(&mut self, expression: &Expr) -> Result<Target> {
        Ok(match expression.as_ref() {
            Expression::Var(id) => Target::Variable(*id),
            Expression::Field(index) => Target::Field(field_index(self.eval(index)?.number())?),
            Expression::Index(id, keys) => {
                let key = self.key(keys)?;
                Target::Element(self.array(*id)?, key)
            }
            _ => return Err(Error::from("invalid assignment target")),
        })
    }
    pub(super) fn read_target(&mut self, target: &Target) -> Result<Scalar> {
        match target {
            Target::Variable(id) => self.get_var(*id),
            Target::Field(index) => self.field(*index),
            Target::Element(array, key) => {
                Ok(array.borrow_mut().entry(key.clone()).or_default().clone())
            }
        }
    }
    pub(super) fn write_target(&mut self, target: &Target, value: Scalar) -> Result<()> {
        match target {
            Target::Variable(id) => self.set_var(*id, value),
            Target::Field(index) => {
                let fs = self.text(self.special.fs)?;
                let paragraph = self.text(self.special.rs)?.is_empty();
                let format = self.convfmt()?;
                self.fields
                    .set(*index, value, &fs, paragraph, &format, &mut self.cache)?;
                let ofs = self.text(self.special.ofs)?;
                self.fields.capture_ofs(&ofs);
                Ok(())
            }
            Target::Element(array, key) => {
                array.borrow_mut().insert(key.clone(), value);
                Ok(())
            }
        }
    }
}
pub(super) fn field_index(number: f64) -> Result<usize> {
    if !number.is_finite() || number < 0.0 || number >= usize::MAX as f64 {
        Err(Error::from("negative or oversized field index"))
    } else {
        Ok(number.trunc() as usize)
    }
}
