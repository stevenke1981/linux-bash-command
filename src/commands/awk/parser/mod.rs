//! Recursive-descent parser for awk programs.
//!
//! Precedence, lowest first: assignment, `?:`, `||`, `&&`, `in`, `~ !~`, relational and
//! `cmd | getline`, concatenation, `+ -`, `* / %`, unary `! - +`, `^` (right associative),
//! postfix `++ --`, then `$`, grouping and primaries. Inside `print` arguments a bare `>`
//! is a redirection, so comparisons with `>` need parentheses there.

use std::collections::HashMap;

mod expr;

use super::ast::{
    BinOp, Builtin, Expr, Function, GetlineSource, Pattern, Program, RedirKind, Redirect, Rule,
    Stmt, Symbols, VarRef,
};
use super::lexer::{tokenize, Tok, Token};

/// A syntax error at a source line.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

type PResult<T> = Result<T, ParseError>;

/// Parses a whole program. Global names are interned into `symbols`.
pub fn parse_program(src: &[u8], symbols: &mut Symbols) -> PResult<Program> {
    let toks = tokenize(src).map_err(|e| ParseError {
        line: e.line,
        message: e.message,
    })?;
    let mut parser = Parser {
        toks,
        pos: 0,
        symbols,
        functions: HashMap::new(),
        locals: None,
        no_gt: false,
        loop_depth: 0,
        in_function: false,
        in_begin_end: false,
    };
    parser.program()
}

struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    symbols: &'a mut Symbols,
    functions: HashMap<String, usize>,
    locals: Option<HashMap<String, usize>>,
    /// Set while parsing `print` arguments: a bare `>` starts a redirection.
    no_gt: bool,
    loop_depth: usize,
    in_function: bool,
    in_begin_end: bool,
}

fn is_lvalue(expr: &Expr) -> bool {
    matches!(expr, Expr::Var(_) | Expr::Field(_) | Expr::Index(..))
}

fn builtin_by_name(name: &str) -> Option<Builtin> {
    Some(match name {
        "length" => Builtin::Length,
        "substr" => Builtin::Substr,
        "index" => Builtin::Index,
        "split" => Builtin::Split,
        "sub" => Builtin::Sub,
        "gsub" => Builtin::Gsub,
        "match" => Builtin::Match,
        "sprintf" => Builtin::Sprintf,
        "sin" => Builtin::Sin,
        "cos" => Builtin::Cos,
        "atan2" => Builtin::Atan2,
        "exp" => Builtin::Exp,
        "log" => Builtin::Log,
        "sqrt" => Builtin::Sqrt,
        "int" => Builtin::Int,
        "rand" => Builtin::Rand,
        "srand" => Builtin::Srand,
        "tolower" => Builtin::Tolower,
        "toupper" => Builtin::Toupper,
        "close" => Builtin::Close,
        "fflush" => Builtin::Fflush,
        "system" => Builtin::System,
        _ => return None,
    })
}

/// Allowed argument counts (inclusive) for a builtin.
fn arity(builtin: Builtin) -> (usize, usize) {
    match builtin {
        Builtin::Length | Builtin::Srand | Builtin::Fflush => (0, 1),
        Builtin::Rand => (0, 0),
        Builtin::Substr | Builtin::Split | Builtin::Sub | Builtin::Gsub => (2, 3),
        Builtin::Index | Builtin::Match | Builtin::Atan2 => (2, 2),
        Builtin::Sprintf => (1, usize::MAX),
        _ => (1, 1),
    }
}

impl Parser<'_> {
    fn program(&mut self) -> PResult<Program> {
        self.collect_functions()?;
        let mut program = Program::default();
        loop {
            self.skip_terms();
            match self.tok() {
                Tok::Eof => break,
                Tok::Kw("BEGIN") => {
                    self.advance();
                    self.in_begin_end = true;
                    let body = self.block()?;
                    self.in_begin_end = false;
                    program.begin.push(body);
                }
                Tok::Kw("END") => {
                    self.advance();
                    self.in_begin_end = true;
                    let body = self.block()?;
                    self.in_begin_end = false;
                    program.end.push(body);
                }
                Tok::Kw("function") => {
                    let function = self.function_def()?;
                    program.functions.push(function);
                }
                _ => {
                    let rule = self.rule()?;
                    program.rules.push(rule);
                }
            }
        }
        Ok(program)
    }

    /// Records every function name first, so calls may precede definitions.
    fn collect_functions(&mut self) -> PResult<()> {
        let mut index = 0;
        for i in 0..self.toks.len().saturating_sub(1) {
            if self.toks[i].tok != Tok::Kw("function") {
                continue;
            }
            let name = match &self.toks[i + 1].tok {
                Tok::Name(name) | Tok::FuncName(name) => name.clone(),
                _ => {
                    return Err(ParseError {
                        line: self.toks[i + 1].line,
                        message: "function name expected".into(),
                    })
                }
            };
            if self.functions.insert(name.clone(), index).is_some() {
                return Err(ParseError {
                    line: self.toks[i + 1].line,
                    message: format!("function '{name}' previously defined"),
                });
            }
            index += 1;
        }
        Ok(())
    }

    fn function_def(&mut self) -> PResult<Function> {
        self.advance();
        let name = match self.tok().clone() {
            Tok::Name(name) | Tok::FuncName(name) => name,
            _ => return Err(self.unexpected()),
        };
        self.advance();
        self.expect(&Tok::LParen)?;
        let mut params = Vec::new();
        self.skip_newlines();
        while self.tok() != &Tok::RParen {
            let Tok::Name(param) = self.tok().clone() else {
                return Err(self.unexpected());
            };
            if params.contains(&param) {
                return Err(self.error(format!("duplicate parameter '{param}'")));
            }
            params.push(param);
            self.advance();
            self.skip_newlines();
            if self.tok() == &Tok::Comma {
                self.advance();
                self.skip_newlines();
            } else if self.tok() != &Tok::RParen {
                return Err(self.unexpected());
            }
        }
        self.advance();
        self.skip_newlines();
        let locals: HashMap<String, usize> = params
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i))
            .collect();
        self.locals = Some(locals);
        self.in_function = true;
        let body = self.block();
        self.in_function = false;
        self.locals = None;
        Ok(Function {
            name,
            params: params.len(),
            body: body?,
        })
    }

    fn rule(&mut self) -> PResult<Rule> {
        let pattern = if self.tok() == &Tok::LBrace {
            Pattern::All
        } else {
            let first = self.expr()?;
            if self.tok() == &Tok::Comma {
                self.advance();
                self.skip_newlines();
                Pattern::Range(first, self.expr()?)
            } else {
                Pattern::Expr(first)
            }
        };
        if self.tok() == &Tok::LBrace {
            let body = self.block()?;
            return Ok(Rule {
                pattern,
                action: Some(body),
            });
        }
        match self.tok() {
            Tok::Newline | Tok::Semi | Tok::Eof => Ok(Rule {
                pattern,
                action: None,
            }),
            _ => Err(self.unexpected()),
        }
    }

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect(&Tok::LBrace)?;
        let mut body = Vec::new();
        loop {
            self.skip_terms();
            match self.tok() {
                Tok::RBrace => {
                    self.advance();
                    return Ok(body);
                }
                Tok::Eof => return Err(self.unexpected()),
                _ => body.push(self.statement()?),
            }
        }
    }

    fn statement(&mut self) -> PResult<Stmt> {
        match self.tok().clone() {
            Tok::LBrace => Ok(Stmt::Block(self.block()?)),
            Tok::Semi => {
                self.advance();
                Ok(Stmt::Block(Vec::new()))
            }
            Tok::Kw("if") => self.if_stmt(),
            Tok::Kw("while") => {
                self.advance();
                let cond = self.paren_expr()?;
                let body = self.loop_body()?;
                Ok(Stmt::While(cond, Box::new(body)))
            }
            Tok::Kw("do") => {
                self.advance();
                let body = self.loop_body()?;
                self.skip_terms();
                self.expect(&Tok::Kw("while"))?;
                let cond = self.paren_expr()?;
                self.end_simple()?;
                Ok(Stmt::Do(Box::new(body), cond))
            }
            Tok::Kw("for") => self.for_stmt(),
            Tok::Kw("break") => {
                self.advance();
                self.check_loop("break")?;
                self.end_simple()?;
                Ok(Stmt::Break)
            }
            Tok::Kw("continue") => {
                self.advance();
                self.check_loop("continue")?;
                self.end_simple()?;
                Ok(Stmt::Continue)
            }
            Tok::Kw("next") | Tok::Kw("nextfile") => {
                let is_next = self.tok() == &Tok::Kw("next");
                self.advance();
                if self.in_begin_end {
                    return Err(self.error(if is_next {
                        "next used in BEGIN or END action"
                    } else {
                        "nextfile used in BEGIN or END action"
                    }));
                }
                self.end_simple()?;
                Ok(if is_next { Stmt::Next } else { Stmt::NextFile })
            }
            Tok::Kw("exit") => {
                self.advance();
                let value = self.optional_expr()?;
                self.end_simple()?;
                Ok(Stmt::Exit(value))
            }
            Tok::Kw("return") => {
                if !self.in_function {
                    return Err(self.error("return outside function body"));
                }
                self.advance();
                let value = self.optional_expr()?;
                self.end_simple()?;
                Ok(Stmt::Return(value))
            }
            Tok::Kw("delete") => self.delete_stmt(),
            Tok::Kw("print") => self.print_stmt(false),
            Tok::Kw("printf") => self.print_stmt(true),
            _ => {
                let expr = self.expr()?;
                self.end_simple()?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn check_loop(&self, word: &str) -> PResult<()> {
        if self.loop_depth == 0 {
            Err(self.error(format!("{word} is not allowed outside a loop")))
        } else {
            Ok(())
        }
    }

    fn loop_body(&mut self) -> PResult<Stmt> {
        self.skip_newlines();
        self.loop_depth += 1;
        let body = if self.tok() == &Tok::Semi {
            self.advance();
            Ok(Stmt::Block(Vec::new()))
        } else {
            self.statement()
        };
        self.loop_depth -= 1;
        body
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        self.advance();
        let cond = self.paren_expr()?;
        self.skip_newlines();
        let then = self.statement()?;
        let save = self.pos;
        self.skip_terms();
        if self.tok() == &Tok::Kw("else") {
            self.advance();
            self.skip_newlines();
            let other = self.statement()?;
            Ok(Stmt::If(cond, Box::new(then), Some(Box::new(other))))
        } else {
            self.pos = save;
            Ok(Stmt::If(cond, Box::new(then), None))
        }
    }

    fn for_stmt(&mut self) -> PResult<Stmt> {
        self.advance();
        self.expect(&Tok::LParen)?;
        if let (Tok::Name(var), Tok::Kw("in"), Tok::Name(array), Tok::RParen) = (
            self.peek_tok(0).clone(),
            self.peek_tok(1).clone(),
            self.peek_tok(2).clone(),
            self.peek_tok(3).clone(),
        ) {
            self.pos += 4;
            let var = self.var_ref(&var);
            let array = self.var_ref(&array);
            let body = self.loop_body()?;
            return Ok(Stmt::ForIn(var, array, Box::new(body)));
        }
        let init = self.optional_expr_until(&Tok::Semi)?;
        self.expect(&Tok::Semi)?;
        self.skip_newlines();
        let cond = self.optional_expr_until(&Tok::Semi)?;
        self.expect(&Tok::Semi)?;
        self.skip_newlines();
        let step = self.optional_expr_until(&Tok::RParen)?;
        self.expect(&Tok::RParen)?;
        let body = self.loop_body()?;
        Ok(Stmt::For {
            init,
            cond,
            step,
            body: Box::new(body),
        })
    }

    fn delete_stmt(&mut self) -> PResult<Stmt> {
        self.advance();
        let Tok::Name(name) = self.tok().clone() else {
            return Err(self.unexpected());
        };
        self.advance();
        let var = self.var_ref(&name);
        let subs = if self.tok() == &Tok::LBracket {
            Some(self.subscripts()?)
        } else {
            None
        };
        self.end_simple()?;
        Ok(Stmt::Delete(var, subs))
    }

    fn print_stmt(&mut self, is_printf: bool) -> PResult<Stmt> {
        self.advance();
        let mut args = Vec::new();
        if !self.at_print_end() {
            let saved = self.no_gt;
            self.no_gt = true;
            args.push(self.expr()?);
            while self.tok() == &Tok::Comma {
                self.advance();
                self.skip_newlines();
                args.push(self.expr()?);
            }
            self.no_gt = saved;
        }
        if let [Expr::List(_)] = args.as_slice() {
            if let Some(Expr::List(items)) = args.pop() {
                args = items;
            }
        }
        if is_printf && args.is_empty() {
            return Err(self.error("printf: no format"));
        }
        let kind = match self.tok() {
            Tok::Gt => Some(RedirKind::File),
            Tok::Append => Some(RedirKind::Append),
            Tok::Pipe => Some(RedirKind::Pipe),
            _ => None,
        };
        let redirect = match kind {
            Some(kind) => {
                self.advance();
                Some(Redirect {
                    kind,
                    target: self.concat()?,
                })
            }
            None => None,
        };
        self.end_simple()?;
        Ok(if is_printf {
            Stmt::Printf(args, redirect)
        } else {
            Stmt::Print(args, redirect)
        })
    }

    fn at_print_end(&self) -> bool {
        matches!(
            self.tok(),
            Tok::Semi | Tok::Newline | Tok::RBrace | Tok::Eof | Tok::Gt | Tok::Append | Tok::Pipe
        )
    }

    fn if_expr_end(&self) -> bool {
        matches!(
            self.tok(),
            Tok::Semi | Tok::Newline | Tok::RBrace | Tok::Eof
        )
    }

    fn optional_expr(&mut self) -> PResult<Option<Expr>> {
        if self.if_expr_end() {
            Ok(None)
        } else {
            Ok(Some(self.expr()?))
        }
    }

    fn optional_expr_until(&mut self, end: &Tok) -> PResult<Option<Expr>> {
        if self.tok() == end {
            Ok(None)
        } else {
            Ok(Some(self.expr()?))
        }
    }

    fn paren_expr(&mut self) -> PResult<Expr> {
        self.expect(&Tok::LParen)?;
        let saved = self.no_gt;
        self.no_gt = false;
        let expr = self.expr();
        self.no_gt = saved;
        let expr = expr?;
        self.expect(&Tok::RParen)?;
        Ok(expr)
    }

    /// A statement terminator: `;` or newline, or nothing before `}` or end of file.
    fn end_simple(&mut self) -> PResult<()> {
        match self.tok() {
            Tok::Semi | Tok::Newline => {
                self.advance();
                Ok(())
            }
            Tok::RBrace | Tok::Eof => Ok(()),
            _ => Err(self.unexpected()),
        }
    }
}

impl Parser<'_> {
    // ----- token helpers -----------------------------------------------------------------

    fn tok(&self) -> &Tok {
        self.toks.get(self.pos).map(|t| &t.tok).unwrap_or(&Tok::Eof)
    }

    fn peek_tok(&self, offset: usize) -> &Tok {
        self.toks
            .get(self.pos + offset)
            .map(|t| &t.tok)
            .unwrap_or(&Tok::Eof)
    }

    fn line(&self) -> usize {
        self.toks
            .get(self.pos)
            .or(self.toks.last())
            .map_or(1, |t| t.line)
    }

    fn advance(&mut self) {
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, tok: &Tok) -> PResult<()> {
        if self.tok() == tok {
            self.advance();
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    fn skip_newlines(&mut self) {
        while self.tok() == &Tok::Newline {
            self.advance();
        }
    }

    fn skip_terms(&mut self) {
        while matches!(self.tok(), Tok::Newline | Tok::Semi) {
            self.advance();
        }
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            line: self.line(),
            message: message.into(),
        }
    }

    fn unexpected(&self) -> ParseError {
        let shown = describe(self.tok());
        self.error(format!("unexpected {shown}"))
    }
}

/// Human-readable name of a token for error messages.
fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Newline => "newline".into(),
        Tok::Eof => "end of program".into(),
        Tok::Str(text) => format!("string \"{}\"", String::from_utf8_lossy(text)),
        Tok::Regex(text) => format!("regex /{}/", String::from_utf8_lossy(text)),
        Tok::Number(n) => format!("number {n}"),
        Tok::Name(name) | Tok::FuncName(name) => format!("'{name}'"),
        Tok::Builtin(name) | Tok::Kw(name) => format!("'{name}'"),
        other => format!("'{}'", symbol(other)),
    }
}

fn symbol(tok: &Tok) -> &'static str {
    match tok {
        Tok::LBrace => "{",
        Tok::RBrace => "}",
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBracket => "[",
        Tok::RBracket => "]",
        Tok::Semi => ";",
        Tok::Comma => ",",
        Tok::Plus => "+",
        Tok::Minus => "-",
        Tok::Star => "*",
        Tok::Slash => "/",
        Tok::Percent => "%",
        Tok::Caret => "^",
        Tok::Not => "!",
        Tok::Gt => ">",
        Tok::Lt => "<",
        Tok::Ge => ">=",
        Tok::Le => "<=",
        Tok::Eq => "==",
        Tok::Ne => "!=",
        Tok::Match => "~",
        Tok::NoMatch => "!~",
        Tok::And => "&&",
        Tok::Or => "||",
        Tok::Question => "?",
        Tok::Colon => ":",
        Tok::Assign => "=",
        Tok::AddAssign => "+=",
        Tok::SubAssign => "-=",
        Tok::MulAssign => "*=",
        Tok::DivAssign => "/=",
        Tok::ModAssign => "%=",
        Tok::PowAssign => "^=",
        Tok::Incr => "++",
        Tok::Decr => "--",
        Tok::Dollar => "$",
        Tok::Append => ">>",
        Tok::Pipe => "|",
        _ => "?",
    }
}

#[cfg(test)]
mod tests;
