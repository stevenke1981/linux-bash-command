//! Expression grammar of the awk parser: assignment down to primaries.

use super::*;

impl Parser<'_> {
    // ----- expressions -------------------------------------------------------------------

    pub(super) fn expr(&mut self) -> PResult<Expr> {
        let lhs = self.ternary()?;
        let op = match self.tok() {
            Tok::Assign => None,
            Tok::AddAssign => Some(BinOp::Add),
            Tok::SubAssign => Some(BinOp::Sub),
            Tok::MulAssign => Some(BinOp::Mul),
            Tok::DivAssign => Some(BinOp::Div),
            Tok::ModAssign => Some(BinOp::Mod),
            Tok::PowAssign => Some(BinOp::Pow),
            _ => return Ok(lhs),
        };
        if !is_lvalue(&lhs) {
            return Err(self.error("assignment to a non-variable"));
        }
        self.advance();
        self.skip_newlines();
        let rhs = self.expr()?;
        Ok(match op {
            None => Expr::Assign(Box::new(lhs), Box::new(rhs)),
            Some(op) => Expr::AugAssign(op, Box::new(lhs), Box::new(rhs)),
        })
    }

    pub(super) fn ternary(&mut self) -> PResult<Expr> {
        let cond = self.or()?;
        if self.tok() != &Tok::Question {
            return Ok(cond);
        }
        self.advance();
        self.skip_newlines();
        let then = self.expr()?;
        self.skip_newlines();
        self.expect(&Tok::Colon)?;
        self.skip_newlines();
        let other = self.expr()?;
        Ok(Expr::Cond(Box::new(cond), Box::new(then), Box::new(other)))
    }

    pub(super) fn or(&mut self) -> PResult<Expr> {
        let mut lhs = self.and()?;
        while self.tok() == &Tok::Or {
            self.advance();
            self.skip_newlines();
            let rhs = self.and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    pub(super) fn and(&mut self) -> PResult<Expr> {
        let mut lhs = self.in_expr()?;
        while self.tok() == &Tok::And {
            self.advance();
            self.skip_newlines();
            let rhs = self.in_expr()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    pub(super) fn in_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.matching()?;
        while self.tok() == &Tok::Kw("in") {
            self.advance();
            let array = self.array_name()?;
            lhs = Expr::In(vec![lhs], array);
        }
        Ok(lhs)
    }

    pub(super) fn matching(&mut self) -> PResult<Expr> {
        let mut lhs = self.comparison()?;
        loop {
            let negate = match self.tok() {
                Tok::Match => false,
                Tok::NoMatch => true,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.comparison()?;
            lhs = Expr::Match {
                negate,
                subject: Box::new(lhs),
                pattern: Box::new(rhs),
            };
        }
    }

    pub(super) fn comparison(&mut self) -> PResult<Expr> {
        let mut lhs = self.concat()?;
        loop {
            if self.tok() == &Tok::Pipe && self.peek_tok(1) == &Tok::Kw("getline") {
                self.pos += 2;
                let target = self.getline_target()?;
                lhs = Expr::Getline {
                    source: GetlineSource::Command(Box::new(lhs)),
                    target,
                };
                continue;
            }
            let op = match self.tok() {
                Tok::Lt => BinOp::Lt,
                Tok::Le => BinOp::Le,
                Tok::Eq => BinOp::Eq,
                Tok::Ne => BinOp::Ne,
                Tok::Ge => BinOp::Ge,
                Tok::Gt if !self.no_gt => BinOp::Gt,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.concat()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    pub(super) fn concat(&mut self) -> PResult<Expr> {
        let mut lhs = self.additive()?;
        while self.starts_operand() {
            let rhs = self.additive()?;
            lhs = Expr::Binary(BinOp::Concat, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    pub(super) fn starts_operand(&self) -> bool {
        matches!(
            self.tok(),
            Tok::Number(_)
                | Tok::Str(_)
                | Tok::Regex(_)
                | Tok::Name(_)
                | Tok::FuncName(_)
                | Tok::Builtin(_)
                | Tok::Dollar
                | Tok::LParen
                | Tok::Incr
                | Tok::Decr
        )
    }

    pub(super) fn additive(&mut self) -> PResult<Expr> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.tok() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.multiplicative()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    pub(super) fn multiplicative(&mut self) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.tok() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.unary()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    pub(super) fn unary(&mut self) -> PResult<Expr> {
        match self.tok() {
            Tok::Not => {
                self.advance();
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Tok::Minus => {
                self.advance();
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Tok::Plus => {
                self.advance();
                Ok(Expr::Plus(Box::new(self.unary()?)))
            }
            _ => self.power(),
        }
    }

    pub(super) fn power(&mut self) -> PResult<Expr> {
        let base = self.postfix()?;
        if self.tok() != &Tok::Caret {
            return Ok(base);
        }
        self.advance();
        let exponent = self.unary()?;
        Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(exponent)))
    }

    pub(super) fn postfix(&mut self) -> PResult<Expr> {
        let expr = self.primary()?;
        if is_lvalue(&expr) {
            let delta = match self.tok() {
                Tok::Incr => 1.0,
                Tok::Decr => -1.0,
                _ => return Ok(expr),
            };
            self.advance();
            return Ok(Expr::IncDec {
                target: Box::new(expr),
                pre: false,
                delta,
            });
        }
        Ok(expr)
    }

    pub(super) fn primary(&mut self) -> PResult<Expr> {
        match self.tok().clone() {
            Tok::Number(n) => {
                self.advance();
                Ok(Expr::Num(n))
            }
            Tok::Str(text) => {
                self.advance();
                Ok(Expr::Str(text))
            }
            Tok::Regex(source) => {
                self.advance();
                Ok(Expr::Regex(source))
            }
            Tok::Dollar => {
                self.advance();
                let operand = match self.tok() {
                    Tok::Incr | Tok::Decr => self.prefix_incdec()?,
                    Tok::Minus => {
                        self.advance();
                        Expr::Neg(Box::new(self.primary()?))
                    }
                    _ => self.primary()?,
                };
                Ok(Expr::Field(Box::new(operand)))
            }
            Tok::Incr | Tok::Decr => self.prefix_incdec(),
            Tok::LParen => self.group(),
            Tok::Name(name) => {
                self.advance();
                let var = self.var_ref(&name);
                if self.tok() == &Tok::LBracket {
                    let subs = self.subscripts()?;
                    Ok(Expr::Index(var, subs))
                } else {
                    Ok(Expr::Var(var))
                }
            }
            Tok::FuncName(name) => {
                self.advance();
                let Some(&index) = self.functions.get(&name) else {
                    return Err(self.error(format!("calling undefined function {name}")));
                };
                let args = self.call_args()?;
                Ok(Expr::Call(index, args))
            }
            Tok::Builtin(name) => {
                self.advance();
                self.builtin(name)
            }
            Tok::Kw("getline") => {
                self.advance();
                let target = self.getline_target()?;
                if self.tok() == &Tok::Lt {
                    self.advance();
                    let file = self.primary()?;
                    Ok(Expr::Getline {
                        source: GetlineSource::File(Box::new(file)),
                        target,
                    })
                } else {
                    Ok(Expr::Getline {
                        source: GetlineSource::Main,
                        target,
                    })
                }
            }
            _ => Err(self.unexpected()),
        }
    }

    pub(super) fn prefix_incdec(&mut self) -> PResult<Expr> {
        let delta = if self.tok() == &Tok::Incr { 1.0 } else { -1.0 };
        self.advance();
        let target = self.primary()?;
        if !is_lvalue(&target) {
            return Err(self.error("++ or -- applied to a non-variable"));
        }
        Ok(Expr::IncDec {
            target: Box::new(target),
            pre: true,
            delta,
        })
    }

    /// `( expr )`, or a parenthesized list which is either `print` arguments or `(i, j) in a`.
    pub(super) fn group(&mut self) -> PResult<Expr> {
        self.advance();
        let saved = self.no_gt;
        self.no_gt = false;
        let items = self.expr_list(&Tok::RParen);
        self.no_gt = saved;
        let mut items = items?;
        self.expect(&Tok::RParen)?;
        if self.tok() == &Tok::Kw("in") {
            self.advance();
            let array = self.array_name()?;
            return Ok(Expr::In(items, array));
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap_or(Expr::Num(0.0)))
        } else {
            Ok(Expr::List(items))
        }
    }

    /// Comma-separated expressions up to (not including) `end`.
    pub(super) fn expr_list(&mut self, end: &Tok) -> PResult<Vec<Expr>> {
        self.skip_newlines();
        let mut items = Vec::new();
        if self.tok() == end {
            return Ok(items);
        }
        loop {
            items.push(self.expr()?);
            self.skip_newlines();
            if self.tok() != &Tok::Comma {
                return Ok(items);
            }
            self.advance();
            self.skip_newlines();
        }
    }

    pub(super) fn subscripts(&mut self) -> PResult<Vec<Expr>> {
        self.expect(&Tok::LBracket)?;
        let saved = self.no_gt;
        self.no_gt = false;
        let items = self.expr_list(&Tok::RBracket);
        self.no_gt = saved;
        let items = items?;
        if items.is_empty() {
            return Err(self.unexpected());
        }
        self.expect(&Tok::RBracket)?;
        Ok(items)
    }

    pub(super) fn call_args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(&Tok::LParen)?;
        let saved = self.no_gt;
        self.no_gt = false;
        let items = self.expr_list(&Tok::RParen);
        self.no_gt = saved;
        let items = items?;
        self.expect(&Tok::RParen)?;
        Ok(items)
    }

    pub(super) fn builtin(&mut self, name: &str) -> PResult<Expr> {
        let Some(builtin) = builtin_by_name(name) else {
            return Err(self.unexpected());
        };
        let args = if builtin == Builtin::Length && self.tok() != &Tok::LParen {
            Vec::new()
        } else {
            self.call_args()?
        };
        let (min, max) = arity(builtin);
        if args.len() < min || args.len() > max {
            return Err(self.error(format!("wrong number of arguments to {name}")));
        }
        if builtin == Builtin::Split && !matches!(args.get(1), Some(Expr::Var(_))) {
            return Err(self.error("split: second argument must be an array name"));
        }
        Ok(Expr::Builtin(builtin, args))
    }

    /// The optional variable after `getline`, or after `cmd |`.
    pub(super) fn getline_target(&mut self) -> PResult<Option<Box<Expr>>> {
        match self.tok().clone() {
            Tok::Name(name) => {
                self.advance();
                let var = self.var_ref(&name);
                if self.tok() == &Tok::LBracket {
                    let subs = self.subscripts()?;
                    Ok(Some(Box::new(Expr::Index(var, subs))))
                } else {
                    Ok(Some(Box::new(Expr::Var(var))))
                }
            }
            Tok::Dollar => {
                self.advance();
                let operand = self.primary()?;
                Ok(Some(Box::new(Expr::Field(Box::new(operand)))))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn array_name(&mut self) -> PResult<VarRef> {
        let Tok::Name(name) = self.tok().clone() else {
            return Err(self.unexpected());
        };
        self.advance();
        Ok(self.var_ref(&name))
    }

    pub(super) fn var_ref(&mut self, name: &str) -> VarRef {
        if let Some(&index) = self.locals.as_ref().and_then(|locals| locals.get(name)) {
            return VarRef::Local(index);
        }
        VarRef::Global(self.symbols.intern(name))
    }
}
