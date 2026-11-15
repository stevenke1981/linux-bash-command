use super::super::value::character_count;
use super::*;
impl Runtime<'_> {
    pub(super) fn builtin(&mut self, name: &str, args: &[Expr]) -> Result<Scalar> {
        let (minimum, maximum) = match name {
            "length" | "srand" | "fflush" => (0, 1),
            "rand" => (0, 0),
            "substr" | "split" | "sub" | "gsub" => (2, 3),
            "index" | "match" | "atan2" => (2, 2),
            "sprintf" => (1, usize::MAX),
            _ => (1, 1),
        };
        if args.len() < minimum || args.len() > maximum {
            return Err(Error::Message(format!(
                "invalid number of arguments to {name}"
            )));
        }
        match name {
            "system" => Err(Error::from("system() is not supported by agentcmd")),
            "length" => self.length(args),
            "substr" => self.substr(args),
            "index" => self.index(args),
            "split" => self.split(args),
            "sub" | "gsub" => self.substitute(args, name == "gsub"),
            "match" => self.match_builtin(args),
            "sprintf" => {
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.eval(arg)?);
                }
                let format = values[0].bytes(&self.convfmt()?)?;
                Ok(Scalar::string(super::super::format::render(
                    &format,
                    &values[1..],
                    &self.convfmt()?,
                )?))
            }
            "tolower" | "toupper" => {
                let mut bytes = self.eval(&args[0])?.bytes(&self.convfmt()?)?;
                if name == "tolower" {
                    bytes.make_ascii_lowercase();
                } else {
                    bytes.make_ascii_uppercase();
                }
                Ok(Scalar::string(bytes))
            }
            "close" => {
                let name = self.eval(&args[0])?.bytes(&self.convfmt()?)?;
                Ok(Scalar::Number(f64::from(self.files.close(&name, self.out))))
            }
            "fflush" => self.fflush(args),
            "rand" | "srand" => self.random_builtin(name, args),
            _ => self.math(name, args),
        }
    }
    fn length(&mut self, args: &[Expr]) -> Result<Scalar> {
        if let Some(arg) = args.first() {
            if let Expression::Var(id) = arg.as_ref() {
                if let Slot::Array(array) = self.slot(*id) {
                    return Ok(Scalar::Number(array.borrow().len() as f64));
                }
            }
        }
        let value = if let Some(arg) = args.first() {
            self.eval(arg)?
        } else {
            self.field(0)?
        };
        let bytes = value.bytes(&self.convfmt()?)?;
        Ok(Scalar::Number(character_count(&bytes) as f64))
    }
    fn substr(&mut self, args: &[Expr]) -> Result<Scalar> {
        let bytes = self.eval(&args[0])?.bytes(&self.convfmt()?)?;
        let from = self.eval(&args[1])?.number().trunc();
        let start = if from <= 1.0 {
            0
        } else {
            (from as usize).saturating_sub(1).min(bytes.len())
        };
        let end = if let Some(length) = args.get(2) {
            let length = self.eval(length)?.number().trunc();
            if length <= 0.0 {
                start
            } else {
                start.saturating_add(length as usize).min(bytes.len())
            }
        } else {
            bytes.len()
        };
        Ok(Scalar::string(bytes[start..end].to_vec()))
    }
    fn index(&mut self, args: &[Expr]) -> Result<Scalar> {
        let bytes = self.eval(&args[0])?.bytes(&self.convfmt()?)?;
        let needle = self.eval(&args[1])?.bytes(&self.convfmt()?)?;
        let index = if needle.is_empty() {
            1
        } else {
            bytes
                .windows(needle.len())
                .position(|w| w == needle)
                .map_or(0, |i| i + 1)
        };
        Ok(Scalar::Number(index as f64))
    }
    fn split(&mut self, args: &[Expr]) -> Result<Scalar> {
        let bytes = self.eval(&args[0])?.bytes(&self.convfmt()?)?;
        let separator = if let Some(arg) = args.get(2) {
            self.regex_argument(arg)?
        } else {
            self.text(self.special.fs)?.to_vec()
        };
        let Expression::Var(id) = args[1].as_ref() else {
            return Err(Error::from(
                "split second argument is not an array variable",
            ));
        };
        let fields = super::super::fields::split(&bytes, &separator, false, &mut self.cache)?;
        let length = fields.len();
        let array = self.array(*id)?;
        let mut array = array.borrow_mut();
        array.clear();
        for (index, field) in fields.into_iter().enumerate() {
            array.insert((index + 1).to_string().into_bytes(), Scalar::input(field));
        }
        Ok(Scalar::Number(length as f64))
    }
    fn match_builtin(&mut self, args: &[Expr]) -> Result<Scalar> {
        let bytes = self.eval(&args[0])?.bytes(&self.convfmt()?)?;
        let pattern = self.regex_argument(&args[1])?;
        let (start, length) = if let Some((start, end)) = self.cache.get(&pattern)?.find(&bytes, 0)
        {
            ((start + 1) as f64, (end - start) as f64)
        } else {
            (0.0, -1.0)
        };
        self.set_var(self.special.rstart, Scalar::Number(start))?;
        self.set_var(self.special.rlength, Scalar::Number(length))?;
        Ok(Scalar::Number(start))
    }
    fn substitute(&mut self, args: &[Expr], global: bool) -> Result<Scalar> {
        let pattern = self.regex_argument(&args[0])?;
        let replacement = self.eval(&args[1])?.bytes(&self.convfmt()?)?;
        let target = if let Some(arg) = args.get(2) {
            self.target(arg)?
        } else {
            variables::Target::Field(0)
        };
        let text = self.read_target(&target)?.bytes(&self.convfmt()?)?;
        let regex = self.cache.get(&pattern)?;
        let (mut out, mut count, mut copied, mut at, mut previous) = (Vec::new(), 0, 0, 0, None);
        while let Some((start, end)) = regex.find(&text, at) {
            if start != end || previous != Some(start) {
                out.extend_from_slice(&text[copied..start]);
                expand(&replacement, &text[start..end], &mut out);
                copied = end;
                count += 1;
                if !global {
                    break;
                }
            }
            previous = Some(end);
            if start == end {
                if end == text.len() {
                    break;
                }
                at = end + 1;
            } else {
                at = end;
            }
        }
        if count > 0 {
            out.extend_from_slice(&text[copied..]);
            self.write_target(&target, Scalar::string(out))?;
        }
        Ok(Scalar::Number(f64::from(count)))
    }
    fn math(&mut self, name: &str, args: &[Expr]) -> Result<Scalar> {
        let x = self.eval(&args[0])?.number();
        Ok(Scalar::Number(match name {
            "sin" => x.sin(),
            "cos" => x.cos(),
            "atan2" => x.atan2(self.eval(&args[1])?.number()),
            "exp" => x.exp(),
            "log" => x.ln(),
            "sqrt" => x.sqrt(),
            "int" => x.trunc(),
            _ => return Err(Error::from("unknown builtin function")),
        }))
    }
    fn random_builtin(&mut self, name: &str, args: &[Expr]) -> Result<Scalar> {
        if name == "srand" {
            let seed = if let Some(arg) = args.first() {
                self.eval(arg)?.number()
            } else {
                0.0
            };
            let previous = self.seed;
            self.seed = seed;
            self.random = seed.to_bits() ^ 0x9e3779b97f4a7c15;
            return Ok(Scalar::Number(previous));
        }
        self.random = self
            .random
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        Ok(Scalar::Number(
            (self.random >> 11) as f64 / (1u64 << 53) as f64,
        ))
    }
    fn fflush(&mut self, args: &[Expr]) -> Result<Scalar> {
        let name = if let Some(arg) = args.first() {
            Some(self.eval(arg)?.bytes(&self.convfmt()?)?)
        } else {
            None
        };
        if name
            .as_deref()
            .is_none_or(|n| n.is_empty() || n == b"/dev/stdout")
        {
            self.out.flush()?;
        }
        if matches!(name.as_deref(), Some(b"/dev/stdout" | b"/dev/stderr")) {
            return Ok(Scalar::Number(0.0));
        }
        Ok(Scalar::Number(if self.files.flush(name.as_deref())? {
            0.0
        } else {
            -1.0
        }))
    }
}
fn expand(replacement: &[u8], matched: &[u8], out: &mut Vec<u8>) {
    let mut at = 0;
    while at < replacement.len() {
        let byte = replacement[at];
        at += 1;
        if byte == b'&' {
            out.extend_from_slice(matched);
        } else if byte == b'\\' && at < replacement.len() {
            let next = replacement[at];
            at += 1;
            if !matches!(next, b'&' | b'\\') {
                out.push(b'\\');
            }
            out.push(next);
        } else {
            out.push(byte);
        }
    }
}
