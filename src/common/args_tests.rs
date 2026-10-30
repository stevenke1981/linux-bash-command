use super::*;

fn specs() -> Vec<OptSpec> {
    vec![
        OptSpec::flag("all", Some('a'), Some("all")),
        OptSpec::flag("brief", Some('b'), Some("brief")),
        OptSpec::req("lines", Some('n'), Some("lines")),
        OptSpec::flag("verbose", Some('v'), Some("verbose")),
        OptSpec::flag("verbosity", None, Some("verbosity")),
        OptSpec::flag("quiet", Some('q'), Some("quiet")),
        OptSpec::opt("color", None, Some("color")),
        OptSpec::req("width", None, Some("width")),
    ]
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn opt(id: &'static str, value: Option<&str>) -> Item {
    Item::Opt {
        id,
        value: value.map(str::to_owned),
    }
}

fn operand(text: &str) -> Item {
    Item::Operand(text.to_owned())
}

fn parse(list: &[&str]) -> Result<Vec<Item>, UsageError> {
    let table = specs();
    Parser::new("demo", &table).parse_with(&args(list), true)
}

fn parse_error(list: &[&str]) -> String {
    parse(list).unwrap_err().message
}

#[test]
fn combined_short_flags() {
    assert_eq!(
        parse(&["-ab"]).unwrap(),
        vec![opt("all", None), opt("brief", None)]
    );
}

#[test]
fn short_value_attached_or_separate() {
    assert_eq!(parse(&["-n5"]).unwrap(), vec![opt("lines", Some("5"))]);
    assert_eq!(parse(&["-n", "5"]).unwrap(), vec![opt("lines", Some("5"))]);
}

#[test]
fn short_value_after_cluster() {
    assert_eq!(
        parse(&["-vn5"]).unwrap(),
        vec![opt("verbose", None), opt("lines", Some("5"))]
    );
    assert_eq!(
        parse(&["-vn", "5"]).unwrap(),
        vec![opt("verbose", None), opt("lines", Some("5"))]
    );
}

#[test]
fn required_short_value_may_look_like_an_option() {
    assert_eq!(
        parse(&["-n", "-a"]).unwrap(),
        vec![opt("lines", Some("-a"))]
    );
}

#[test]
fn long_flag_and_value_forms() {
    assert_eq!(parse(&["--all"]).unwrap(), vec![opt("all", None)]);
    assert_eq!(
        parse(&["--lines=7"]).unwrap(),
        vec![opt("lines", Some("7"))]
    );
    assert_eq!(
        parse(&["--lines", "7"]).unwrap(),
        vec![opt("lines", Some("7"))]
    );
}

#[test]
fn required_long_value_may_be_empty_or_dash_prefixed() {
    assert_eq!(parse(&["--lines="]).unwrap(), vec![opt("lines", Some(""))]);
    assert_eq!(
        parse(&["--lines", "-1"]).unwrap(),
        vec![opt("lines", Some("-1"))]
    );
}

#[test]
fn optional_value_only_accepts_equals_form() {
    assert_eq!(parse(&["--color"]).unwrap(), vec![opt("color", None)]);
    assert_eq!(
        parse(&["--color=auto"]).unwrap(),
        vec![opt("color", Some("auto"))]
    );
    assert_eq!(
        parse(&["--color", "auto"]).unwrap(),
        vec![opt("color", None), operand("auto")]
    );
}

#[test]
fn unique_prefix_abbreviation() {
    assert_eq!(parse(&["--bri"]).unwrap(), vec![opt("brief", None)]);
    assert_eq!(parse(&["--wid=3"]).unwrap(), vec![opt("width", Some("3"))]);
}

#[test]
fn exact_match_wins_over_prefix() {
    assert_eq!(parse(&["--verbose"]).unwrap(), vec![opt("verbose", None)]);
    assert_eq!(parse(&["--verbosi"]).unwrap(), vec![opt("verbosity", None)]);
}

#[test]
fn ambiguous_prefix_lists_possibilities() {
    assert_eq!(
        parse_error(&["--ver"]),
        "option '--ver' is ambiguous; possibilities: '--verbose' '--verbosity'"
    );
}

#[test]
fn double_dash_ends_options() {
    assert_eq!(
        parse(&["-a", "--", "-b", "--all"]).unwrap(),
        vec![opt("all", None), operand("-b"), operand("--all")]
    );
}

#[test]
fn lone_dash_is_an_operand() {
    assert_eq!(
        parse(&["-", "-a"]).unwrap(),
        vec![operand("-"), opt("all", None)]
    );
}

#[test]
fn options_are_permuted_after_operands() {
    assert_eq!(
        parse(&["file", "-a", "other"]).unwrap(),
        vec![operand("file"), opt("all", None), operand("other")]
    );
}

#[test]
fn no_permute_stops_at_first_operand() {
    let table = specs();
    let items = Parser::new("demo", &table)
        .parse_with(&args(&["file", "-a"]), false)
        .unwrap();
    assert_eq!(items, vec![operand("file"), operand("-a")]);
}

#[test]
fn no_permute_builder_disables_permutation() {
    let table = specs();
    let parser = Parser::new("demo", &table).no_permute();
    assert!(!parser.permute);
}

#[test]
fn numeric_short_option_is_an_operand_when_enabled() {
    let table = specs();
    let mut parser = Parser::new("demo", &table);
    parser.numeric_short = true;
    assert_eq!(
        parser.parse_with(&args(&["-5"]), true).unwrap(),
        vec![operand("-5")]
    );
    assert_eq!(parse_error(&["-5"]), "invalid option -- '5'");
}

#[test]
fn unknown_short_option() {
    assert_eq!(parse_error(&["-x"]), "invalid option -- 'x'");
    assert_eq!(parse_error(&["-ax"]), "invalid option -- 'x'");
}

#[test]
fn unknown_long_option_shows_full_argument() {
    assert_eq!(parse_error(&["--xyz"]), "unrecognized option '--xyz'");
    assert_eq!(parse_error(&["--xyz=3"]), "unrecognized option '--xyz=3'");
}

#[test]
fn empty_long_name_is_unrecognized() {
    assert_eq!(parse_error(&["--=x"]), "unrecognized option '--=x'");
}

#[test]
fn missing_short_argument() {
    assert_eq!(parse_error(&["-n"]), "option requires an argument -- 'n'");
    assert_eq!(parse_error(&["-an"]), "option requires an argument -- 'n'");
}

#[test]
fn missing_long_argument() {
    assert_eq!(
        parse_error(&["--lines"]),
        "option '--lines' requires an argument"
    );
}

#[test]
fn flag_rejects_inline_value() {
    assert_eq!(
        parse_error(&["--all=3"]),
        "option '--all' doesn't allow an argument"
    );
}

#[test]
fn optional_short_without_value_is_none() {
    let table = [OptSpec::opt("color", Some('c'), None)];
    let items = Parser::new("demo", &table)
        .parse_with(&args(&["-c"]), true)
        .unwrap();
    assert_eq!(items, vec![opt("color", None)]);
}

#[test]
fn optional_short_takes_rest_of_cluster() {
    let table = [
        OptSpec::flag("all", Some('a'), None),
        OptSpec::opt("color", Some('c'), None),
    ];
    let items = Parser::new("demo", &table)
        .parse_with(&args(&["-acauto"]), true)
        .unwrap();
    assert_eq!(items, vec![opt("all", None), opt("color", Some("auto"))]);
}

#[test]
fn usage_error_report_returns_code() {
    let err = UsageError {
        message: "invalid option -- 'x'".to_owned(),
    };
    assert_eq!(report_usage("demo", &err, 1), 1);
    assert_eq!(report_usage("demo", &err, 2), 2);
}

#[test]
fn expand_keeps_exact_and_unknown_names() {
    let longs = ["lines", "line-number"];
    assert_eq!(expand_long_abbrev("--lines", &longs).unwrap(), "--lines");
    assert_eq!(expand_long_abbrev("--bogus", &longs).unwrap(), "--bogus");
    assert_eq!(expand_long_abbrev("-n", &longs).unwrap(), "-n");
    assert_eq!(expand_long_abbrev("--", &longs).unwrap(), "--");
}

#[test]
fn expand_prefix_keeps_value() {
    let longs = ["lines", "line-number"];
    assert_eq!(
        expand_long_abbrev("--line-n", &longs).unwrap(),
        "--line-number"
    );
    assert_eq!(
        expand_long_abbrev("--line-n=3", &longs).unwrap(),
        "--line-number=3"
    );
    assert_eq!(
        expand_long_abbrev("--li=", &longs).unwrap_err(),
        "option '--li' is ambiguous; possibilities: '--lines' '--line-number'"
    );
}

#[test]
fn expand_ambiguity_message() {
    let longs = ["xa", "xb"];
    assert_eq!(
        expand_long_abbrev("--x", &longs).unwrap_err(),
        "option '--x' is ambiguous; possibilities: '--xa' '--xb'"
    );
}

#[test]
fn expand_ignores_empty_name_and_non_long() {
    let longs = ["lines"];
    assert_eq!(expand_long_abbrev("--=3", &longs).unwrap(), "--=3");
    assert_eq!(expand_long_abbrev("file", &longs).unwrap(), "file");
}

fn legacy(list: &[&str], posix: bool) -> Result<Vec<String>, String> {
    expand_legacy_with(
        &args(list),
        &["lines", "line-number", "color"],
        &["lines", "color"],
        "nw",
        posix,
    )
}

#[test]
fn legacy_expands_abbreviations_and_keeps_values() {
    assert_eq!(
        legacy(&["--line-n", "--line", "5", "file"], false).unwrap_err(),
        "option '--line' is ambiguous; possibilities: '--lines' '--line-number'"
    );
    assert_eq!(
        legacy(&["--line-n", "--lin", "5", "file"], false).unwrap_err(),
        "option '--lin' is ambiguous; possibilities: '--lines' '--line-number'"
    );
    assert_eq!(
        legacy(&["--line-nu", "--lines", "5", "file"], false).unwrap(),
        args(&["--line-number", "--lines", "5", "file"])
    );
}

#[test]
fn legacy_passes_short_values_through() {
    assert_eq!(
        legacy(&["-n", "-a", "file"], false).unwrap(),
        args(&["-n", "-a", "file"])
    );
    assert_eq!(
        legacy(&["-vn", "3", "-a"], false).unwrap(),
        args(&["-vn", "3", "-a"])
    );
    assert_eq!(legacy(&["-n3", "x"], false).unwrap(), args(&["-n3", "x"]));
}

#[test]
fn legacy_permutes_by_default() {
    assert_eq!(
        legacy(&["file", "-a"], false).unwrap(),
        args(&["file", "-a"])
    );
}

#[test]
fn legacy_posix_inserts_double_dash_before_first_operand() {
    assert_eq!(
        legacy(&["-a", "file", "-a"], true).unwrap(),
        args(&["-a", "--", "file", "-a"])
    );
    assert_eq!(
        legacy(&["file", "-a"], true).unwrap(),
        args(&["--", "file", "-a"])
    );
}

#[test]
fn legacy_keeps_existing_double_dash() {
    assert_eq!(legacy(&["--", "-a"], true).unwrap(), args(&["--", "-a"]));
}

#[test]
fn legacy_reports_ambiguity_as_error() {
    let result = expand_legacy_with(&args(&["--l"]), &["lines", "line-number"], &[], "", false);
    assert!(result.unwrap_err().starts_with("option '--l' is ambiguous"));
}

#[test]
fn abbrev_only_pass_keeps_operands_and_stops_at_double_dash() {
    let list = args(&["aux", "--sor=pid", "--", "--sor"]);
    let expanded = expand_abbrevs(&list, &["sort", "pid"]).unwrap();
    assert_eq!(expanded, args(&["aux", "--sort=pid", "--", "--sor"]));
    assert!(expand_abbrevs(&args(&["--s"]), &["sort", "suffix"]).is_err());
}

#[test]
fn prepare_reports_ambiguity_with_usage_code() {
    let list = args(&["--l"]);
    let code = prepare_legacy("demo", &list, &["lines", "line-number"], &[], "", 2);
    assert_eq!(code, Err(2));
    let ok = prepare_legacy("demo", &args(&["--line-n"]), &["line-number"], &[], "", 1);
    assert_eq!(ok, Ok(args(&["--line-number"])));
}

#[test]
fn legacy_stdin_operand_is_not_an_option() {
    assert_eq!(
        legacy(&["-", "-a"], true).unwrap(),
        args(&["--", "-", "-a"])
    );
}
