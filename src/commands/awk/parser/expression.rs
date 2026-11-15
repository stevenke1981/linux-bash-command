use super::*;
impl Parser {
    pub(super) fn expression(&mut self, minimum: u8, printing: bool) -> Result<Expr, String> {
        let mut left = self.prefix()?;
        loop {
            if self.symbol("[") {
                left = self.index(left)?;
                continue;
            }
            if matches!(self.current.kind, Kind::Symbol("++" | "--")) && minimum <= 150 {
                let increment = if self.symbol("++") { 1.0 } else { -1.0 };
                if !lvalue(&left) {
                    return Err(self.error());
                }
                self.advance()?;
                left = expr(Expression::Increment(left, increment, false));
                continue;
            }
            if printing && matches!(self.current.kind, Kind::Symbol(">" | ">>" | "|")) {
                break;
            }
            if self.symbol("?") && minimum <= 40 {
                if matches!(left.as_ref(), Expression::Tuple(_)) {
                    return Err(self.error());
                }
                self.advance()?;
                self.newlines()?;
                let yes = self.expression(0, false)?;
                self.expect(":")?;
                self.newlines()?;
                left = expr(Expression::Conditional(
                    left,
                    yes,
                    self.expression(40, printing)?,
                ));
                continue;
            }
            let Some((operator, power, right)) = self.operator() else {
                if minimum <= 100 && self.operand_start() {
                    if matches!(left.as_ref(), Expression::Tuple(_)) {
                        return Err(self.error());
                    }
                    let right = self.expression(101, printing)?;
                    left = expr(Expression::Binary("concat", left, right));
                    continue;
                }
                break;
            };
            if power < minimum {
                break;
            }
            if operator != "in" && matches!(left.as_ref(), Expression::Tuple(_)) {
                return Err(self.error());
            }
            self.advance()?;
            self.newlines()?;
            if operator == "in" {
                let array = self.identifier()?;
                left = expr(Expression::In(left, array));
                continue;
            }
            let rhs = self.expression(if right { power } else { power + 1 }, printing)?;
            left = if power == 30 {
                if !lvalue(&left) {
                    return Err(self.error());
                }
                expr(Expression::Assign(operator, left, rhs))
            } else if operator == "|" {
                if !matches!(rhs.as_ref(), Expression::Getline(..)) {
                    return Err(self.error());
                }
                expr(Expression::Pipe(left, rhs))
            } else {
                expr(Expression::Binary(operator, left, rhs))
            };
        }
        if matches!(left.as_ref(), Expression::Tuple(_)) && (!printing || minimum != 0) {
            return Err(self.error());
        }
        Ok(left)
    }
    fn prefix(&mut self) -> Result<Expr, String> {
        let token = self.current.clone();
        match token.kind {
            Kind::Number(number) => {
                self.advance()?;
                Ok(expr(Expression::Number(number)))
            }
            Kind::String(bytes) => {
                self.advance()?;
                Ok(expr(Expression::String(bytes)))
            }
            Kind::Symbol("/" | "/=") => {
                let regex = self.lexer.regex(token.start)?;
                super::super::regexp::Pattern::new(&regex).map_err(|message| {
                    format!(
                        "syntax error at source line {} near regexp: {message}",
                        token.line
                    )
                })?;
                self.advance()?;
                Ok(expr(Expression::Regex(regex)))
            }
            Kind::Symbol("(") => {
                self.advance()?;
                self.newlines()?;
                let mut values = vec![self.expression(0, false)?];
                while self.consume(",")? {
                    self.newlines()?;
                    values.push(self.expression(0, false)?);
                }
                self.newlines()?;
                self.expect(")")?;
                Ok(if values.len() == 1 {
                    values.pop().unwrap()
                } else {
                    expr(Expression::Tuple(values))
                })
            }
            Kind::Symbol("$") => {
                self.advance()?;
                Ok(expr(Expression::Field(self.expression(160, false)?)))
            }
            Kind::Symbol(op @ ("+" | "-" | "!")) => {
                self.advance()?;
                Ok(expr(Expression::Unary(op, self.expression(130, false)?)))
            }
            Kind::Symbol(op @ ("++" | "--")) => {
                self.advance()?;
                let target = self.expression(150, false)?;
                if !lvalue(&target) {
                    return Err(self.error());
                }
                Ok(expr(Expression::Increment(
                    target,
                    if op == "++" { 1.0 } else { -1.0 },
                    true,
                )))
            }
            Kind::Word(ref name) if name == "getline" => self.getline(),
            Kind::Word(name) if !reserved(&name) => {
                self.advance()?;
                let id = self.program.intern(&name);
                if self.symbol("(")
                    && (super::super::analyze::builtin(&name)
                        || self.current.start == token.start + name.len())
                {
                    self.advance()?;
                    let args = self.arguments(")")?;
                    Ok(expr(Expression::Call(id, args)))
                } else if name == "length" {
                    Ok(expr(Expression::Call(id, Vec::new())))
                } else if super::super::analyze::builtin(&name) {
                    Err(self.error())
                } else {
                    self.program.variable_lines.entry(id).or_insert(token.line);
                    Ok(expr(Expression::Var(id)))
                }
            }
            _ => Err(self.error()),
        }
    }
    fn arguments(&mut self, end: &str) -> Result<Vec<Expr>, String> {
        self.newlines()?;
        let mut args = Vec::new();
        if !self.symbol(end) {
            loop {
                args.push(self.expression(0, false)?);
                if !self.consume(",")? {
                    break;
                }
                self.newlines()?;
            }
        }
        self.newlines()?;
        self.expect(end)?;
        Ok(args)
    }
    fn index(&mut self, left: Expr) -> Result<Expr, String> {
        let Expression::Var(id) = left.as_ref() else {
            return Err(self.error());
        };
        self.advance()?;
        let keys = self.arguments("]")?;
        if keys.is_empty() {
            return Err(self.error());
        }
        Ok(expr(Expression::Index(*id, keys)))
    }
    fn getline(&mut self) -> Result<Expr, String> {
        self.advance()?;
        let target = if self.symbol("$")
            || matches!(&self.current.kind,Kind::Word(name) if !reserved(name))
        {
            let target = self.expression(150, false)?;
            if !lvalue(&target) {
                return Err(self.error());
            }
            Some(target)
        } else {
            None
        };
        let source = if self.consume("<")? {
            Some(self.expression(91, false)?)
        } else {
            None
        };
        Ok(expr(Expression::Getline(target, source)))
    }
    fn operand_start(&self) -> bool {
        matches!(
            self.current.kind,
            Kind::Number(_) | Kind::String(_) | Kind::Symbol("(" | "$")
        ) || matches!(&self.current.kind,Kind::Word(name) if !reserved(name)&&name!="in")
    }
    fn operator(&self) -> Option<(&'static str, u8, bool)> {
        if self.word("in") {
            return Some(("in", 70, false));
        }
        let Kind::Symbol(operator) = self.current.kind else {
            return None;
        };
        let (power, right) = match operator {
            "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "^=" | "**=" => (30, true),
            "||" => (50, false),
            "&&" => (60, false),
            "~" | "!~" => (80, false),
            "<" | "<=" | ">" | ">=" | "==" | "!=" => (90, false),
            "+" | "-" => (110, false),
            "*" | "/" | "%" => (120, false),
            "^" | "**" => (140, true),
            "|" => (20, false),
            _ => return None,
        };
        Some((operator, power, right))
    }
}
pub(super) fn reserved(name: &str) -> bool {
    matches!(
        name,
        "if" | "else"
            | "while"
            | "do"
            | "for"
            | "break"
            | "continue"
            | "next"
            | "nextfile"
            | "exit"
            | "return"
            | "delete"
            | "print"
            | "printf"
            | "BEGIN"
            | "END"
            | "function"
            | "func"
            | "in"
            | "getline"
    )
}
