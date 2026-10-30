//! Parser unit tests.

use super::*;

fn parse(src: &str) -> Result<Program, ParseError> {
    let mut symbols = Symbols::new();
    parse_program(src.as_bytes(), &mut symbols)
}

fn ok(src: &str) -> Program {
    parse(src).unwrap_or_else(|e| panic!("parse failed: {e:?} in {src:?}"))
}

fn fails(src: &str) {
    assert!(parse(src).is_err(), "expected a syntax error in {src:?}");
}

/// The first expression statement of the BEGIN block (or of the first rule action).
fn first_expr(src: &str) -> Expr {
    let mut program = ok(src);
    let mut body = if let Some(block) = program.begin.pop() {
        block
    } else {
        program.rules.remove(0).action.expect("rule with an action")
    };
    match body.remove(0) {
        Stmt::Expr(expr) => expr,
        other => panic!("not an expression statement: {other:?}"),
    }
}

#[test]
fn begin_end_and_rules() {
    let program = ok("BEGIN { x = 1 } END { print x } /a/ { n++ } NR > 2");
    assert_eq!(program.begin.len(), 1);
    assert_eq!(program.end.len(), 1);
    assert_eq!(program.rules.len(), 2);
    assert!(program.rules[1].action.is_none());
}

#[test]
fn range_pattern_and_newline_separated_items() {
    let program = ok("/start/,/stop/ { print }\n\n$1 == \"x\"\n");
    assert!(matches!(program.rules[0].pattern, Pattern::Range(..)));
    assert_eq!(program.rules.len(), 2);
}

#[test]
fn precedence_of_power_unary_and_concatenation() {
    // -2^2 is -(2^2); 2^3^2 is 2^(3^2).
    assert!(matches!(first_expr("BEGIN { -2^2 }"), Expr::Neg(_)));
    match first_expr("BEGIN { 2^3^2 }") {
        Expr::Binary(BinOp::Pow, _, rhs) => assert!(matches!(*rhs, Expr::Binary(BinOp::Pow, ..))),
        other => panic!("unexpected {other:?}"),
    }
    // a b + c is a (b + c).
    match first_expr("BEGIN { a b + c }") {
        Expr::Binary(BinOp::Concat, _, rhs) => {
            assert!(matches!(*rhs, Expr::Binary(BinOp::Add, ..)));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn ternary_and_assignment_are_right_associative() {
    assert!(matches!(
        first_expr("BEGIN { x = y = 3 }"),
        Expr::Assign(_, _)
    ));
    assert!(matches!(
        first_expr("BEGIN { a ? b : c ? d : e }"),
        Expr::Cond(..)
    ));
}

#[test]
fn print_redirection_and_grouping() {
    let program = ok("BEGIN { print a, b > \"out\" ; print (1,2) > \"f\"; printf(\"%d\\n\", 3) }");
    let body = &program.begin[0];
    match &body[0] {
        Stmt::Print(args, Some(redirect)) => {
            assert_eq!(args.len(), 2);
            assert_eq!(redirect.kind, RedirKind::File);
        }
        other => panic!("unexpected {other:?}"),
    }
    match &body[1] {
        Stmt::Print(args, Some(_)) => assert_eq!(args.len(), 2),
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(&body[2], Stmt::Printf(args, None) if args.len() == 2));
}

#[test]
fn if_else_with_separators_and_newlines() {
    ok("BEGIN { if (x) print 1; else print 2 }");
    ok("BEGIN {\n if (x)\n print 1\n else\n print 2\n}");
    ok("BEGIN { if (x) { print 1 } else if (y) print 2 }");
}

#[test]
fn loops_and_for_in() {
    ok("BEGIN { for (i = 0; i < 3; i++) s += i }");
    ok("BEGIN { for (;;) break }");
    ok("BEGIN { do { n++ } while (n < 3) }");
    ok("BEGIN { while (x) ; }");
    ok("BEGIN { for (k in arr) delete arr[k] }");
    ok("BEGIN { delete arr }");
}

#[test]
fn functions_with_locals_and_forward_calls() {
    let program = ok(
        "function f(a, b,\n c) { return a + b }\nBEGIN { print g(1) }\nfunction g(x) { return x }",
    );
    assert_eq!(program.functions.len(), 2);
    assert_eq!(program.functions[0].params, 3);
    let calls = ok("function h(x) { return x } BEGIN { h(1) }");
    assert!(matches!(&calls.begin[0][0], Stmt::Expr(Expr::Call(0, args)) if args.len() == 1));
}

#[test]
fn getline_forms_parse() {
    let program = ok("BEGIN { getline; getline line; getline < \"f\"; getline x < \"f\"; \"cmd\" | getline; \"cmd\" | getline y }");
    assert_eq!(program.begin[0].len(), 6);
}

#[test]
fn in_operator_and_multi_subscripts() {
    assert!(
        matches!(first_expr("BEGIN { (i, j) in arr }"), Expr::In(items, _) if items.len() == 2)
    );
    assert!(matches!(first_expr("BEGIN { k in arr }"), Expr::In(..)));
    assert!(matches!(
        first_expr("BEGIN { a[1,2] = 3 }"),
        Expr::Assign(..)
    ));
}

#[test]
fn regex_and_match_operators() {
    assert!(matches!(
        first_expr("BEGIN { x ~ /a/ }"),
        Expr::Match { negate: false, .. }
    ));
    assert!(matches!(
        first_expr("BEGIN { x !~ \"a\" }"),
        Expr::Match { negate: true, .. }
    ));
}

#[test]
fn increment_and_field_precedence() {
    assert!(matches!(
        first_expr("BEGIN { $i++ }"),
        Expr::IncDec { pre: false, .. }
    ));
    match first_expr("BEGIN { $NF-1 }") {
        Expr::Binary(BinOp::Sub, lhs, _) => assert!(matches!(*lhs, Expr::Field(_))),
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        first_expr("BEGIN { ++x }"),
        Expr::IncDec { pre: true, .. }
    ));
}

#[test]
fn builtins_and_arity_checks() {
    assert!(
        matches!(first_expr("BEGIN { length }"), Expr::Builtin(Builtin::Length, a) if a.is_empty())
    );
    assert!(matches!(
        first_expr("BEGIN { split(s, a, /,/) }"),
        Expr::Builtin(Builtin::Split, _)
    ));
    fails("BEGIN { substr(s) }");
    fails("BEGIN { split(s, \"x\") }");
    fails("BEGIN { rand(1) }");
}

#[test]
fn syntax_errors_are_reported_with_lines() {
    let error = parse("BEGIN {\n x = (1 + 2\n}").unwrap_err();
    assert_eq!(error.line, 3);
    fails("BEGIN { print ( }");
    fails("BEGIN { break }");
    fails("BEGIN { next }");
    fails("{ return 1 }");
    fails("BEGIN { 1 = 2 }");
    fails("BEGIN { undefined_fn(1) }");
    fails("function f(a, a) { } BEGIN { }");
    fails("BEGIN");
}

#[test]
fn locals_shadow_globals() {
    let program = ok("function f(x) { x = 1 } BEGIN { x = 2 }");
    match &program.functions[0].body[0] {
        Stmt::Expr(Expr::Assign(target, _)) => {
            assert!(matches!(**target, Expr::Var(VarRef::Local(0))));
        }
        other => panic!("unexpected {other:?}"),
    }
}
