use super::*;
impl Runtime<'_> {
    pub(super) fn read_main(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            if self.input.is_some() {
                let separator = self.text(self.special.rs)?;
                if let Some(record) = self
                    .input
                    .as_mut()
                    .unwrap()
                    .next(&separator, &mut self.cache)?
                {
                    for id in [self.special.nr, self.special.fnr] {
                        let n = self.get_var(id)?.number() + 1.0;
                        self.set_var(id, Scalar::Number(n))?;
                    }
                    return Ok(Some(record));
                }
                self.input = None;
            }
            if self.open_argument()? {
                continue;
            }
            if !self.opened_input && !self.stdin_done {
                self.stdin_done = true;
                self.open(b"-")?;
                continue;
            }
            return Ok(None);
        }
    }
    fn open_argument(&mut self) -> Result<bool> {
        loop {
            let argc = self.get_var(self.special.argc)?.number();
            if !argc.is_finite() || argc < 0.0 {
                return Err(Error::from("invalid ARGC"));
            }
            if self.input_index as f64 >= argc {
                return Ok(false);
            }
            let key = self.input_index.to_string().into_bytes();
            self.input_index += 1;
            let value = self.array(self.special.argv)?.borrow().get(&key).cloned();
            let Some(value) = value else {
                continue;
            };
            let bytes = value.bytes(&self.convfmt()?)?;
            if bytes.is_empty() {
                continue;
            }
            if let Some((name, value)) = super::super::io::assignment(&bytes) {
                self.assign(name, value)?;
                continue;
            }
            self.open(&bytes)?;
            return Ok(true);
        }
    }
    fn open(&mut self, name: &[u8]) -> Result<()> {
        let path = super::super::io::path(name);
        let reader = super::super::io::reader(&path, true).map_err(|e| {
            Error::Message(format!(
                "cannot open file '{}': {}",
                crate::common::path::display_path(&path),
                crate::common::error::io_text(&e)
            ))
        })?;
        self.input = Some(Records::new(reader));
        self.opened_input = true;
        self.set_var(
            self.special.filename,
            Scalar::string(crate::common::path::display_path(&path).into_bytes()),
        )?;
        self.set_var(self.special.fnr, Scalar::Number(0.0))?;
        Ok(())
    }
    pub(super) fn getline(
        &mut self,
        target: Option<&Expr>,
        source: Option<&Expr>,
    ) -> Result<Scalar> {
        let target = if let Some(target) = target {
            Some(self.target(target)?)
        } else {
            None
        };
        let record = if let Some(source) = source {
            let name = self.eval(source)?.bytes(&self.convfmt()?)?;
            let separator = self.text(self.special.rs)?;
            match self.files.getline(&name, &separator, &mut self.cache) {
                Ok(record) => record,
                Err(_) => return Ok(Scalar::Number(-1.0)),
            }
        } else {
            self.read_main()?
        };
        if let Some(record) = record {
            if let Some(target) = target {
                self.write_target(&target, Scalar::input(record))?;
            } else {
                self.set_record(record)?;
            }
            Ok(Scalar::Number(1.0))
        } else {
            Ok(Scalar::Number(0.0))
        }
    }
}
