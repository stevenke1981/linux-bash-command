mod expression;
use super::{
    ast::*,
    lexer::{Kind, Lexer, Token},
};

pub fn parse(source: Vec<u8>) -> Result<Program, String> {
    let mut lexer = Lexer::new(source);
    let current = lexer.next()?;
    let mut parser = Parser {
        lexer,
        current,
        program: Program::default(),
        loops: 0,
        function: false,
        phase: 1,
    };
    parser.program()?;
    super::analyze::functions(&mut parser.program)?;
    Ok(parser.program)
}
struct Parser {
    lexer: Lexer,
    current: Token,
    program: Program,
    loops: usize,
    function: bool,
    phase: u8,
}
impl Parser {
    fn advance(&mut self) -> Result<(), String> {
        self.current = self.lexer.next()?;
        Ok(())
    }
    fn symbol(&self, symbol: &str) -> bool {
        matches!(&self.current.kind,Kind::Symbol(s) if *s==symbol)
    }
    fn word(&self, word: &str) -> bool {
        matches!(&self.current.kind,Kind::Word(s) if s==word)
    }
    fn consume(&mut self, symbol: &str) -> Result<bool, String> {
        if self.symbol(symbol) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn expect(&mut self, symbol: &str) -> Result<(), String> {
        if self.consume(symbol)? {
            Ok(())
        } else {
            Err(self.error())
        }
    }
    fn newlines(&mut self) -> Result<(), String> {
        while self.current.kind == Kind::Newline {
            self.advance()?;
        }
        Ok(())
    }
    fn separators(&mut self) -> Result<(), String> {
        while self.current.kind == Kind::Newline || self.symbol(";") {
            self.advance()?;
        }
        Ok(())
    }
    fn boundary(&self) -> bool {
        matches!(self.current.kind, Kind::Newline | Kind::End)
            || self.symbol(";")
            || self.symbol("}")
    }
    fn identifier(&mut self) -> Result<Id, String> {
        let Kind::Word(name) = self.current.kind.clone() else {
            return Err(self.error());
        };
        if expression::reserved(&name) {
            return Err(self.error());
        }
        let id = self.program.intern(&name);
        self.advance()?;
        Ok(id)
    }
    fn error(&self) -> String {
        format!(
            "syntax error at source line {} near {:?}",
            self.current.line, self.current.kind
        )
    }
    fn program(&mut self) -> Result<(), String> {
        self.separators()?;
        while self.current.kind != Kind::End {
            if self.word("function") || self.word("func") {
                self.function()?;
            } else if self.word("BEGIN") || self.word("END") {
                let begin = self.word("BEGIN");
                self.advance()?;
                self.newlines()?;
                self.phase = if begin { 0 } else { 2 };
                let action = self.block()?;
                self.phase = 1;
                if begin {
                    self.program.begin.push(action);
                } else {
                    self.program.end.push(action);
                }
            } else {
                self.rule()?;
            }
            if !self.boundary() && !self.symbol("{") && !matches!(self.current.kind, Kind::Word(_))
            {
                return Err(self.error());
            }
            self.separators()?;
        }
        Ok(())
    }
    fn function(&mut self) -> Result<(), String> {
        self.advance()?;
        let name = self.identifier()?;
        if super::analyze::builtin(&self.program.names[name])
            || special_name(&self.program.names[name])
        {
            return Err(self.error());
        }
        self.expect("(")?;
        self.newlines()?;
        let mut params = Vec::new();
        if !self.symbol(")") {
            loop {
                let id = self.identifier()?;
                if id == name
                    || super::analyze::builtin(&self.program.names[id])
                    || special_name(&self.program.names[id])
                {
                    return Err(self.error());
                }
                if params.contains(&id) {
                    return Err(self.error());
                }
                params.push(id);
                if !self.consume(",")? {
                    break;
                }
                self.newlines()?;
            }
        }
        self.expect(")")?;
        self.newlines()?;
        self.function = true;
        let body = self.block()?;
        self.function = false;
        let array_params = vec![false; params.len()];
        if self
            .program
            .functions
            .insert(
                name,
                Function {
                    params,
                    body,
                    array_params,
                },
            )
            .is_some()
        {
            return Err(self.error());
        }
        Ok(())
    }
    fn rule(&mut self) -> Result<(), String> {
        let pattern = if self.symbol("{") {
            Pattern::Always
        } else {
            let first = self.expression(0, false)?;
            if self.consume(",")? {
                self.newlines()?;
                Pattern::Range(first, self.expression(0, false)?)
            } else {
                Pattern::Expression(first)
            }
        };
        let action = if self.symbol("{") {
            self.block()?
        } else {
            stmt(Statement::Print {
                formatted: false,
                args: Vec::new(),
                redirect: None,
            })
        };
        self.program.rules.push(Rule { pattern, action });
        Ok(())
    }
    fn block(&mut self) -> Result<Stmt, String> {
        self.expect("{")?;
        self.separators()?;
        let mut body = Vec::new();
        while !self.symbol("}") {
            if self.current.kind == Kind::End {
                return Err(self.error());
            }
            body.push(self.statement()?);
            self.separators()?;
        }
        self.expect("}")?;
        Ok(stmt(Statement::Block(body)))
    }
    fn statement(&mut self) -> Result<Stmt, String> {
        if self.symbol("{") {
            return self.block();
        }
        if self.consume(";")? {
            return Ok(stmt(Statement::Empty));
        }
        let node = if self.word("if") {
            return self.if_statement();
        } else if self.word("while") {
            return self.while_statement();
        } else if self.word("do") {
            return self.do_statement();
        } else if self.word("for") {
            return self.for_statement();
        } else if self.word("print") || self.word("printf") {
            return self.print_statement();
        } else if self.word("break") || self.word("continue") {
            if self.loops == 0 {
                return Err(self.error());
            }
            let is_break = self.word("break");
            self.advance()?;
            if is_break {
                Statement::Break
            } else {
                Statement::Continue
            }
        } else if self.word("next") || self.word("nextfile") {
            if !self.function && self.phase != 1 {
                return Err(self.error());
            }
            let next = self.word("next");
            self.advance()?;
            if next {
                Statement::Next
            } else {
                Statement::NextFile
            }
        } else if self.word("exit") || self.word("return") {
            let exit = self.word("exit");
            if !exit && !self.function {
                return Err(self.error());
            }
            self.advance()?;
            let value = if self.boundary() {
                None
            } else {
                Some(self.expression(0, false)?)
            };
            if exit {
                Statement::Exit(value)
            } else {
                Statement::Return(value)
            }
        } else if self.word("delete") {
            self.advance()?;
            let value = self.expression(150, false)?;
            match value.as_ref() {
                Expression::Var(id) => Statement::Delete(*id, None),
                Expression::Index(id, keys) => Statement::Delete(*id, Some(keys.clone())),
                _ => return Err(self.error()),
            }
        } else {
            Statement::Expression(self.expression(0, false)?)
        };
        Ok(stmt(node))
    }
    fn condition(&mut self) -> Result<Expr, String> {
        self.expect("(")?;
        self.newlines()?;
        let condition = self.expression(0, false)?;
        self.newlines()?;
        self.expect(")")?;
        self.newlines()?;
        Ok(condition)
    }
    fn loop_body(&mut self) -> Result<Stmt, String> {
        self.loops += 1;
        let body = self.statement();
        self.loops -= 1;
        body
    }
    fn if_statement(&mut self) -> Result<Stmt, String> {
        self.advance()?;
        let condition = self.condition()?;
        let yes = self.statement()?;
        self.separators()?;
        let no = if self.word("else") {
            self.advance()?;
            self.newlines()?;
            Some(self.statement()?)
        } else {
            None
        };
        Ok(stmt(Statement::If(condition, yes, no)))
    }
    fn while_statement(&mut self) -> Result<Stmt, String> {
        self.advance()?;
        let condition = self.condition()?;
        let body = self.loop_body()?;
        Ok(stmt(Statement::While(condition, body)))
    }
    fn do_statement(&mut self) -> Result<Stmt, String> {
        self.advance()?;
        self.newlines()?;
        let body = self.loop_body()?;
        self.separators()?;
        if !self.word("while") {
            return Err(self.error());
        }
        self.advance()?;
        let condition = self.condition()?;
        Ok(stmt(Statement::Do(body, condition)))
    }
    fn for_statement(&mut self) -> Result<Stmt, String> {
        self.advance()?;
        self.expect("(")?;
        self.newlines()?;
        let first = if self.symbol(";") {
            None
        } else {
            Some(self.expression(0, false)?)
        };
        if self.consume(")")? {
            let Some(first) = first else {
                return Err(self.error());
            };
            let Expression::In(index, array) = first.as_ref() else {
                return Err(self.error());
            };
            let Expression::Var(index) = index.as_ref() else {
                return Err(self.error());
            };
            self.newlines()?;
            let body = self.loop_body()?;
            return Ok(stmt(Statement::ForIn(*index, *array, body)));
        }
        self.expect(";")?;
        self.newlines()?;
        let condition = if self.symbol(";") {
            None
        } else {
            Some(self.expression(0, false)?)
        };
        self.expect(";")?;
        self.newlines()?;
        let update = if self.symbol(")") {
            None
        } else {
            Some(self.expression(0, false)?)
        };
        self.newlines()?;
        self.expect(")")?;
        self.newlines()?;
        let body = self.loop_body()?;
        Ok(stmt(Statement::For(first, condition, update, body)))
    }
    fn print_statement(&mut self) -> Result<Stmt, String> {
        let formatted = self.word("printf");
        self.advance()?;
        let mut args = Vec::new();
        if !self.boundary() && !matches!(self.current.kind, Kind::Symbol(">" | ">>" | "|")) {
            loop {
                args.push(self.expression(0, true)?);
                if !self.consume(",")? {
                    break;
                }
                self.newlines()?;
            }
        }
        if args.len() == 1 {
            if let Expression::Tuple(values) = args[0].as_ref() {
                args = values.clone();
            }
        } else if args
            .iter()
            .any(|arg| matches!(arg.as_ref(), Expression::Tuple(_)))
        {
            return Err(self.error());
        }
        if formatted && args.is_empty() {
            return Err(self.error());
        }
        let redirect = if let Kind::Symbol(kind @ (">" | ">>" | "|")) = self.current.kind {
            self.advance()?;
            Some((kind, self.expression(0, false)?))
        } else {
            None
        };
        Ok(stmt(Statement::Print {
            formatted,
            args,
            redirect,
        }))
    }
}

fn special_name(name: &str) -> bool {
    matches!(
        name,
        "NR" | "NF"
            | "FNR"
            | "FS"
            | "OFS"
            | "ORS"
            | "RS"
            | "FILENAME"
            | "SUBSEP"
            | "RSTART"
            | "RLENGTH"
            | "CONVFMT"
            | "OFMT"
            | "ENVIRON"
            | "ARGC"
            | "ARGV"
    )
}
pub(super) fn reserved_name(name: &str) -> bool {
    expression::reserved(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(text: &str) -> Program {
        parse(text.as_bytes().to_vec()).unwrap()
    }
    #[test]
    fn multiple_begin_end() {
        let p = p("BEGIN {x=1} BEGIN {x++} END {print x}");
        assert_eq!(p.begin.len(), 2);
        assert_eq!(p.end.len(), 1);
    }
    #[test]
    fn patterns_and_ranges() {
        assert_eq!(p("/a/,/b/ {print}\n$1>2").rules.len(), 2);
    }
    #[test]
    fn all_loop_forms() {
        assert_eq!(p("BEGIN {for(i=0;i<2;i++)print i;for(i in a)print i;while(i)break;do i++;while(i<3)}").begin.len(),1);
    }
    #[test]
    fn function_definition() {
        let p = p("function f(a,b) {if(a)return f(a-1);else return b} BEGIN {print f(2,1)}");
        assert_eq!(p.functions.len(), 1);
    }
    #[test]
    fn array_and_delete() {
        p("BEGIN {a[1,2]=3;delete a[1,2];delete a}");
    }
    #[test]
    fn redirections_and_getline() {
        p("{getline x < f;getline;print x > f;printf \"%s\",x >> f}");
    }
    #[test]
    fn pipes_parse_for_runtime_rejection() {
        p("BEGIN {\"cmd\" | getline x;print x | \"cmd\"}");
    }
    #[test]
    fn malformed_programs_report_line() {
        for text in [
            "BEGIN {x=}",
            "BEGIN {if(x}",
            "function f(a,a){}",
            "BEGIN {break}",
        ] {
            assert!(parse(text.as_bytes().to_vec())
                .unwrap_err()
                .contains("source line"));
        }
    }
}
