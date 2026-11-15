use std::{collections::HashMap, rc::Rc};
pub type Id = usize;
pub type Expr = Rc<Expression>;
pub type Stmt = Rc<Statement>;
#[derive(Clone, Debug)]
pub enum Expression {
    Number(f64),
    String(Vec<u8>),
    Regex(Vec<u8>),
    Var(Id),
    Field(Expr),
    Index(Id, Vec<Expr>),
    Tuple(Vec<Expr>),
    Unary(&'static str, Expr),
    Binary(&'static str, Expr, Expr),
    In(Expr, Id),
    Assign(&'static str, Expr, Expr),
    Increment(Expr, f64, bool),
    Conditional(Expr, Expr, Expr),
    Call(Id, Vec<Expr>),
    Getline(Option<Expr>, Option<Expr>),
    Pipe(Expr, Expr),
}
#[derive(Clone, Debug)]
pub enum Statement {
    Empty,
    Block(Vec<Stmt>),
    Expression(Expr),
    If(Expr, Stmt, Option<Stmt>),
    While(Expr, Stmt),
    Do(Stmt, Expr),
    For(Option<Expr>, Option<Expr>, Option<Expr>, Stmt),
    ForIn(Id, Id, Stmt),
    Break,
    Continue,
    Next,
    NextFile,
    Exit(Option<Expr>),
    Return(Option<Expr>),
    Delete(Id, Option<Vec<Expr>>),
    Print {
        formatted: bool,
        args: Vec<Expr>,
        redirect: Option<(&'static str, Expr)>,
    },
}
#[derive(Clone, Debug)]
pub struct Function {
    pub params: Vec<Id>,
    pub body: Stmt,
    pub array_params: Vec<bool>,
}
#[derive(Clone, Debug)]
pub enum Pattern {
    Always,
    Expression(Expr),
    Range(Expr, Expr),
}
#[derive(Clone, Debug)]
pub struct Rule {
    pub pattern: Pattern,
    pub action: Stmt,
}
#[derive(Default, Debug)]
pub struct Program {
    pub begin: Vec<Stmt>,
    pub end: Vec<Stmt>,
    pub rules: Vec<Rule>,
    pub functions: HashMap<Id, Function>,
    pub names: Vec<String>,
    pub identifiers: HashMap<String, Id>,
    pub variable_lines: HashMap<Id, usize>,
}
impl Program {
    pub fn intern(&mut self, name: &str) -> Id {
        if let Some(id) = self.identifiers.get(name) {
            return *id;
        }
        let id = self.names.len();
        self.names.push(name.into());
        self.identifiers.insert(name.into(), id);
        id
    }
}
pub fn expr(node: Expression) -> Expr {
    Rc::new(node)
}
pub fn stmt(node: Statement) -> Stmt {
    Rc::new(node)
}
pub fn lvalue(value: &Expr) -> bool {
    matches!(
        value.as_ref(),
        Expression::Var(_) | Expression::Field(_) | Expression::Index(..)
    )
}
