use super::ast::*;
use std::collections::{HashMap, HashSet};
pub fn builtin(name: &str) -> bool {
    matches!(
        name,
        "length"
            | "substr"
            | "index"
            | "split"
            | "sub"
            | "gsub"
            | "match"
            | "sprintf"
            | "sin"
            | "cos"
            | "atan2"
            | "exp"
            | "log"
            | "sqrt"
            | "int"
            | "rand"
            | "srand"
            | "tolower"
            | "toupper"
            | "fflush"
            | "close"
            | "system"
    )
}
pub fn functions(program: &mut Program) -> Result<(), String> {
    for (id, line) in &program.variable_lines {
        if program.functions.contains_key(id) {
            return Err(format!(
                "syntax error at source line {line} near function name '{}' used as a variable",
                program.names[*id]
            ));
        }
    }
    let mut calls = HashSet::new();
    for action in program.begin.iter().chain(&program.end) {
        visit_statement(action, &mut |e| {
            if let Expression::Call(id, _) = e {
                calls.insert(*id);
            }
        });
    }
    for rule in &program.rules {
        match &rule.pattern {
            Pattern::Expression(e) => visit_expression(e, &mut |e| {
                if let Expression::Call(id, _) = e {
                    calls.insert(*id);
                }
            }),
            Pattern::Range(a, b) => {
                for e in [a, b] {
                    visit_expression(e, &mut |e| {
                        if let Expression::Call(id, _) = e {
                            calls.insert(*id);
                        }
                    });
                }
            }
            _ => {}
        }
        visit_statement(&rule.action, &mut |e| {
            if let Expression::Call(id, _) = e {
                calls.insert(*id);
            }
        });
    }
    for function in program.functions.values() {
        visit_statement(&function.body, &mut |e| {
            if let Expression::Call(id, _) = e {
                calls.insert(*id);
            }
        });
    }
    for id in calls {
        if !program.functions.contains_key(&id) && !builtin(&program.names[id]) {
            return Err(format!("function '{}' not defined", program.names[id]));
        }
    }
    loop {
        let snapshot = program.functions.clone();
        let mut changed = false;
        for function in program.functions.values_mut() {
            let mut arrays = HashSet::new();
            arrays_in_statement(&function.body, &snapshot, &program.names, &mut arrays);
            for (index, id) in function.params.iter().enumerate() {
                if arrays.contains(id) && !function.array_params[index] {
                    function.array_params[index] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}
fn arrays_in_statement(
    statement: &Stmt,
    functions: &HashMap<Id, Function>,
    names: &[String],
    arrays: &mut HashSet<Id>,
) {
    visit_statement(statement, &mut |node| match node {
        Expression::Index(id, _) | Expression::In(_, id) => {
            arrays.insert(*id);
        }
        Expression::Call(id, args) => {
            let positions: Vec<_> = if names[*id] == "split" {
                vec![1]
            } else {
                functions.get(id).map_or(Vec::new(), |f| {
                    f.array_params
                        .iter()
                        .enumerate()
                        .filter_map(|(i, b)| b.then_some(i))
                        .collect()
                })
            };
            for index in positions {
                if let Some(arg) = args.get(index) {
                    if let Expression::Var(id) = arg.as_ref() {
                        arrays.insert(*id);
                    }
                }
            }
        }
        _ => {}
    });
    match statement.as_ref() {
        Statement::ForIn(_, id, body) => {
            arrays.insert(*id);
            arrays_in_statement(body, functions, names, arrays);
        }
        Statement::Delete(id, _) => {
            arrays.insert(*id);
        }
        Statement::Block(body) => {
            for statement in body {
                arrays_in_statement(statement, functions, names, arrays);
            }
        }
        Statement::If(_, yes, no) => {
            arrays_in_statement(yes, functions, names, arrays);
            if let Some(no) = no {
                arrays_in_statement(no, functions, names, arrays);
            }
        }
        Statement::While(_, body) | Statement::Do(body, _) | Statement::For(_, _, _, body) => {
            arrays_in_statement(body, functions, names, arrays)
        }
        _ => {}
    }
}
pub fn visit_expression(expression: &Expr, visitor: &mut impl FnMut(&Expression)) {
    let expression = expression.as_ref();
    visitor(expression);
    match expression {
        Expression::Field(e)
        | Expression::Unary(_, e)
        | Expression::Increment(e, _, _)
        | Expression::In(e, _) => visit_expression(e, visitor),
        Expression::Binary(_, a, b) | Expression::Assign(_, a, b) | Expression::Pipe(a, b) => {
            visit_expression(a, visitor);
            visit_expression(b, visitor);
        }
        Expression::Conditional(a, b, c) => {
            for e in [a, b, c] {
                visit_expression(e, visitor);
            }
        }
        Expression::Index(_, values) | Expression::Tuple(values) | Expression::Call(_, values) => {
            for e in values {
                visit_expression(e, visitor);
            }
        }
        Expression::Getline(target, source) => {
            for e in target.iter().chain(source) {
                visit_expression(e, visitor);
            }
        }
        _ => {}
    }
}
pub fn visit_statement(statement: &Stmt, visitor: &mut impl FnMut(&Expression)) {
    match statement.as_ref() {
        Statement::Expression(e) => visit_expression(e, visitor),
        Statement::Block(body) => {
            for statement in body {
                visit_statement(statement, visitor);
            }
        }
        Statement::If(e, yes, no) => {
            visit_expression(e, visitor);
            visit_statement(yes, visitor);
            if let Some(no) = no {
                visit_statement(no, visitor);
            }
        }
        Statement::While(e, body) | Statement::Do(body, e) => {
            visit_expression(e, visitor);
            visit_statement(body, visitor);
        }
        Statement::For(a, b, c, body) => {
            for e in a.iter().chain(b).chain(c) {
                visit_expression(e, visitor);
            }
            visit_statement(body, visitor);
        }
        Statement::ForIn(_, _, body) => visit_statement(body, visitor),
        Statement::Exit(e) | Statement::Return(e) => {
            if let Some(e) = e {
                visit_expression(e, visitor);
            }
        }
        Statement::Delete(_, Some(keys)) => {
            for e in keys {
                visit_expression(e, visitor);
            }
        }
        Statement::Print { args, redirect, .. } => {
            for e in args {
                visit_expression(e, visitor);
            }
            if let Some((_, e)) = redirect {
                visit_expression(e, visitor);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn array_formal_inference_propagates_through_calls() {
        let p = super::super::parser::parse(
            b"function a(x){b(x)} function b(y){y[1]=1} BEGIN{a(z)}".to_vec(),
        )
        .unwrap();
        assert!(p.functions.values().all(|f| f.array_params == [true]));
    }
    #[test]
    fn undeclared_functions_are_reported() {
        assert!(super::super::parser::parse(b"BEGIN { missing() }".to_vec())
            .unwrap_err()
            .contains("not defined"));
    }
}
