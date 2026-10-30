use super::*;

fn chunk(text: &str) -> Chunk {
    Chunk {
        text: text.as_bytes().to_vec(),
        origin: Origin::Expr(1),
    }
}

fn parse_text(text: &str) -> Result<Script, ParseError> {
    parse(&[chunk(text)], Dialect::default())
}

fn message(text: &str) -> String {
    parse_text(text).unwrap_err().message
}

#[test]
fn parses_simple_commands_in_order() {
    let script = parse_text("p;d;=").unwrap();
    let kinds: Vec<&str> = script
        .commands
        .iter()
        .map(|c| match c.kind {
            Kind::Print => "p",
            Kind::Delete => "d",
            Kind::Equals => "=",
            _ => "?",
        })
        .collect();
    assert_eq!(kinds, ["p", "d", "="]);
}

#[test]
fn blocks_record_their_closing_brace() {
    let script = parse_text("1{p;{d}}").unwrap();
    let Kind::Block { end } = script.commands[0].kind else {
        panic!("expected block");
    };
    assert_eq!(end, 5);
    assert!(matches!(script.commands[5].kind, Kind::Nop));
    let Kind::Block { end: inner } = script.commands[2].kind else {
        panic!("expected block");
    };
    assert_eq!(inner, 4);
}

#[test]
fn labels_resolve_to_indexes() {
    let script = parse_text(":top\ns/a/b/\nt top\nb").unwrap();
    let Kind::Branch { target, .. } = script.commands[2].kind else {
        panic!("expected branch");
    };
    assert_eq!(target, 0);
    let Kind::Branch { target, .. } = script.commands[3].kind else {
        panic!("expected branch");
    };
    assert_eq!(target, script.commands.len());
}

#[test]
fn missing_label_is_status_four() {
    let error = parse_text("b nowhere").unwrap_err();
    assert_eq!(error.status, 4);
    assert!(error.message.contains("nowhere"));
}

#[test]
fn hash_n_first_line_sets_quiet() {
    assert!(parse_text("#n\np").unwrap().quiet);
    assert!(parse_text("#n").unwrap().quiet);
    assert!(!parse_text("#nope\np").unwrap().quiet);
    assert!(!parse_text(" #n\np").unwrap().quiet);
}

#[test]
fn one_line_text_forms() {
    let script = parse_text("a   hello\\tworld\ni\\\n  lead\nc\\X").unwrap();
    match &script.commands[0].kind {
        Kind::Append(text) => assert_eq!(text, b"hello\tworld\n"),
        other => panic!("unexpected {other:?}"),
    }
    match &script.commands[1].kind {
        Kind::Insert(text) => assert_eq!(text, b"  lead\n"),
        other => panic!("unexpected {other:?}"),
    }
    match &script.commands[2].kind {
        Kind::Change(text) => assert_eq!(text, b"X\n"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn multi_line_text_continues_with_backslash_newline() {
    let script = parse_text("a\\\nfirst\\\nsecond").unwrap();
    match &script.commands[0].kind {
        Kind::Append(text) => assert_eq!(text, b"first\nsecond\n"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn text_without_argument_is_an_error() {
    assert!(message("a").contains("expected \\ after"));
    assert!(message("a\n").contains("expected \\ after"));
}

#[test]
fn substitute_flags_are_recorded() {
    let script = parse_text("s/a/b/2gpI").unwrap();
    let Kind::Subst(subst) = &script.commands[0].kind else {
        panic!("expected subst");
    };
    assert!(subst.global && subst.print);
    assert_eq!(subst.occurrence, 2);
    assert!(subst.pattern.0.is_some());
}

#[test]
fn substitute_errors() {
    assert!(message("s/a/b").contains("unterminated `s' command"));
    assert!(message("s/a/b/k").contains("unknown option to `s'"));
    assert!(message("s/a/b/gg").contains("multiple `g'"));
    assert!(message("s/a/b/0").contains("may not be zero"));
    assert!(message("s/a/\\1/").contains("invalid reference"));
    assert!(message("s/a/b/e").contains("not supported"));
}

#[test]
fn escaped_delimiter_is_literal() {
    let script = parse_text("s,/,|,").unwrap();
    let Kind::Subst(subst) = &script.commands[0].kind else {
        panic!("expected subst");
    };
    assert_eq!(
        subst.replacement,
        vec![super::super::subst::Piece::Text(b"|".to_vec())]
    );
}

#[test]
fn translate_builds_table_and_checks_lengths() {
    let script = parse_text("y/abc/xyz/").unwrap();
    let Kind::Translate(table) = &script.commands[0].kind else {
        panic!("expected translate");
    };
    assert_eq!(table[usize::from(b'a')], b'x');
    assert_eq!(table[usize::from(b'z')], b'z');
    assert!(message("y/ab/c/").contains("different lengths"));
}

#[test]
fn address_forms_parse() {
    let script = parse_text("1,$p;/x/I,+2d;0~4p;2,~4p;\\,a,!p").unwrap();
    assert!(matches!(script.commands[0].selector.end, Some(Addr2::Last)));
    assert!(matches!(
        script.commands[1].selector.end,
        Some(Addr2::Plus(2))
    ));
    assert!(matches!(
        script.commands[2].selector.start,
        Some(Addr::Step(0, 4))
    ));
    assert!(script.commands[4].selector.negate);
}

#[test]
fn address_errors() {
    assert!(message("0p").contains("invalid usage of line address 0"));
    assert!(message("1,0p").contains("invalid usage of line address 0"));
    assert!(message("1!!p").contains("multiple `!'s"));
    assert!(message("1,2q").contains("only uses one address"));
    assert!(message("}").contains("unexpected `}'"));
    assert!(message("1:a").contains("doesn't want any addresses"));
}

#[test]
fn unmatched_braces_and_unknown_commands() {
    assert!(message("{p").contains("unmatched `{'"));
    assert!(message("k").contains("unknown command: `k'"));
    assert!(message("p x").contains("extra characters after command"));
}

#[test]
fn file_commands_share_write_files() {
    let script = parse_text("w out.txt\n/x/W out.txt\nw /dev/stdout").unwrap();
    assert_eq!(script.write_files, vec!["out.txt".to_string()]);
    assert!(matches!(
        script.commands[0].kind,
        Kind::Write(Sink::File(0))
    ));
    assert!(matches!(script.commands[2].kind, Kind::Write(Sink::Stdout)));
    assert!(message("r").contains("missing filename"));
}

#[test]
fn sandbox_rejects_file_and_exec_commands() {
    let dialect = Dialect {
        ere: false,
        sandbox: true,
    };
    for text in ["r x", "w x", "e ls", "s/a/b/w x"] {
        let error = parse(&[chunk(text)], dialect).unwrap_err();
        assert!(
            error.message.contains("sandbox"),
            "{text}: {}",
            error.message
        );
    }
}

#[test]
fn errors_report_expression_and_file_locations() {
    let chunks = [
        chunk("p"),
        Chunk {
            text: b"p\nk".to_vec(),
            origin: Origin::File("script.sed".into()),
        },
    ];
    let error = parse(&chunks, Dialect::default()).unwrap_err();
    assert!(
        error.message.starts_with("file script.sed line 2"),
        "{}",
        error.message
    );
    let error = parse(&[chunk("p;k")], Dialect::default()).unwrap_err();
    assert!(
        error.message.starts_with("-e expression #1, char 3"),
        "{}",
        error.message
    );
}

#[test]
fn ere_dialect_is_used_for_addresses() {
    let dialect = Dialect {
        ere: true,
        sandbox: false,
    };
    let script = parse(&[chunk("/a+/p")], dialect).unwrap();
    let Some(Addr::Re(Pattern(Some(regex)))) = &script.commands[0].selector.start else {
        panic!("expected regex address");
    };
    assert!(regex.is_match(b"aa"));
}

#[test]
fn empty_regex_needs_an_earlier_regex() {
    assert!(message("//p").contains("no previous regular expression"));
    assert!(parse_text("/a/p;//p").is_ok());
    assert!(parse_text("s/a/b/;s//c/").is_ok());
}

#[test]
fn comments_and_separators_are_skipped() {
    let script = parse_text("  ; # hello\n p ;; # again").unwrap();
    assert_eq!(script.commands.len(), 1);
}
