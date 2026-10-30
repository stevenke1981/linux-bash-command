//! CLI tests for the Phase 3 utilities: shuf, tsort, numfmt, factor, expr, pathchk and ptx.
//! Random output is checked by properties (same multiset, reproducibility), never by value.
//! Files live in a sandbox.

mod common;

use std::process::Output;

use common::{run, run_in, Sandbox};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn lines(bytes: &[u8]) -> Vec<String> {
    text(bytes).lines().map(str::to_owned).collect()
}

fn sorted_lines(bytes: &[u8]) -> Vec<String> {
    let mut found = lines(bytes);
    found.sort();
    found
}

// ------------------------------------------------------------------ shuf

#[test]
fn shuf_help_and_version() {
    let help = run("shuf", &["--help"], b"");
    assert_eq!(code(&help), 0);
    let body = text(&help.stdout);
    for option in [
        "--echo",
        "--input-range",
        "--head-count",
        "--output",
        "--random-source",
        "--repeat",
        "--zero-terminated",
    ] {
        assert!(body.contains(option), "help lacks {option}");
    }
    let version = run("shuf", &["--version"], b"");
    assert!(text(&version.stdout).starts_with("shuf (agentcmd) "));
}

#[test]
fn shuf_permutes_stdin_lines_without_loss() {
    let input = "alpha\nbeta\ngamma\ndelta\nalpha\n";
    let output = run("shuf", &[], input.as_bytes());
    assert_eq!(code(&output), 0);
    let mut expected: Vec<String> = input.lines().map(str::to_owned).collect();
    expected.sort();
    assert_eq!(sorted_lines(&output.stdout), expected);
}

#[test]
fn shuf_head_count_limits_output_and_keeps_members() {
    let output = run("shuf", &["-n", "2", "-e", "a", "b", "c", "d"], b"");
    let found = lines(&output.stdout);
    assert_eq!(found.len(), 2);
    assert!(found
        .iter()
        .all(|line| ["a", "b", "c", "d"].contains(&line.as_str())));
}

#[test]
fn shuf_range_is_a_permutation_of_integers() {
    let output = run("shuf", &["-i", "1-20"], b"");
    let mut numbers: Vec<u64> = lines(&output.stdout)
        .iter()
        .map(|l| l.parse().unwrap())
        .collect();
    numbers.sort_unstable();
    assert_eq!(numbers, (1..=20).collect::<Vec<u64>>());
}

#[test]
fn shuf_huge_range_samples_without_materialising() {
    let output = run("shuf", &["-i", "1-18446744073709551614", "-n", "5"], b"");
    assert_eq!(code(&output), 0);
    let mut values: Vec<u128> = lines(&output.stdout)
        .iter()
        .map(|l| l.parse().unwrap())
        .collect();
    assert_eq!(values.len(), 5);
    values.sort_unstable();
    values.dedup();
    assert_eq!(values.len(), 5, "sampled values must be distinct");
}

#[test]
fn shuf_random_source_is_reproducible() {
    let dir = Sandbox::new();
    dir.write("seed.bin", b"fixed seed material for the test");
    dir.write("other.bin", b"different seed material");
    let first = run_in(
        dir.path(),
        "shuf",
        &["--random-source=seed.bin", "-i", "1-50"],
        b"",
    );
    let second = run_in(
        dir.path(),
        "shuf",
        &["--random-source=seed.bin", "-i", "1-50"],
        b"",
    );
    let other = run_in(
        dir.path(),
        "shuf",
        &["--random-source=other.bin", "-i", "1-50"],
        b"",
    );
    assert_eq!(code(&first), 0);
    assert_eq!(first.stdout, second.stdout);
    assert_ne!(first.stdout, other.stdout);
}

#[test]
fn shuf_empty_random_source_is_end_of_file_error() {
    let dir = Sandbox::new();
    dir.write("empty.bin", b"");
    let output = run_in(
        dir.path(),
        "shuf",
        &["--random-source=empty.bin", "-e", "a"],
        b"",
    );
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("end of file"));
}

#[test]
fn shuf_zero_terminated_records() {
    let output = run("shuf", &["-z", "-n", "2", "-e", "x", "y"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout.iter().filter(|&&b| b == 0).count(), 2);
}

#[test]
fn shuf_repeat_draws_from_the_source() {
    let output = run("shuf", &["-r", "-n", "6", "-e", "only"], b"");
    assert_eq!(lines(&output.stdout), vec!["only"; 6]);
}

#[test]
fn shuf_repeat_without_lines_is_an_error() {
    let output = run("shuf", &["-r", "-n", "1"], b"");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("no lines to repeat"));
}

#[test]
fn shuf_writes_to_output_file() {
    let dir = Sandbox::new();
    let output = run_in(
        dir.path(),
        "shuf",
        &["-n", "0", "-o", "out.txt", "-e", "a"],
        b"",
    );
    assert_eq!(code(&output), 0);
    assert!(dir.read("out.txt").is_empty());
}

#[test]
fn shuf_usage_errors_exit_one() {
    let combined = run("shuf", &["-e", "-i", "1-3"], b"");
    assert_eq!(code(&combined), 1);
    assert!(text(&combined.stderr).contains("cannot combine -e and -i options"));
    let bogus = run("shuf", &["--bogus"], b"");
    assert_eq!(code(&bogus), 1);
    assert!(text(&bogus.stderr).contains("unrecognized option '--bogus'"));
}

// ------------------------------------------------------------------ tsort

#[test]
fn tsort_help_and_version() {
    assert_eq!(code(&run("tsort", &["--help"], b"")), 0);
    let version = run("tsort", &["--version"], b"");
    assert!(text(&version.stdout).starts_with("tsort (agentcmd) "));
}

#[test]
fn tsort_orders_a_dag() {
    let output = run("tsort", &[], b"shirt tie\ntie jacket\njacket belt\n");
    assert_eq!(lines(&output.stdout), ["shirt", "tie", "jacket", "belt"]);
    assert_eq!(code(&output), 0);
}

#[test]
fn tsort_reports_loop_members_on_stderr_and_exits_one() {
    let output = run("tsort", &[], b"a b\nb a\n");
    assert_eq!(code(&output), 1);
    let err = text(&output.stderr);
    assert!(err.contains("input contains a loop:"));
    assert!(err.contains("tsort: a") && err.contains("tsort: b"));
    assert_eq!(lines(&output.stdout), ["a", "b"]);
}

#[test]
fn tsort_odd_token_count_prints_nothing() {
    let output = run("tsort", &[], b"a b\nc\n");
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("odd number of tokens"));
}

#[test]
fn tsort_reads_a_file_operand() {
    let dir = Sandbox::new();
    dir.write("pairs.txt", b"x y\ny z\n");
    let output = run_in(dir.path(), "tsort", &["pairs.txt"], b"");
    assert_eq!(lines(&output.stdout), ["x", "y", "z"]);
}

#[test]
fn tsort_missing_file_is_an_error() {
    let output = run("tsort", &["no-such-file"], b"");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("no-such-file"));
}

#[test]
fn tsort_empty_input_is_empty_output() {
    let output = run("tsort", &[], b"");
    assert!(output.stdout.is_empty());
    assert_eq!(code(&output), 0);
}

// ------------------------------------------------------------------ numfmt

#[test]
fn numfmt_help_and_version() {
    let help = run("numfmt", &["--help"], b"");
    assert_eq!(code(&help), 0);
    let body = text(&help.stdout);
    for option in [
        "--delimiter",
        "--field",
        "--format",
        "--from",
        "--to-unit",
        "--header",
        "--invalid",
        "--padding",
        "--round",
        "--suffix",
    ] {
        assert!(body.contains(option), "help lacks {option}");
    }
    assert!(text(&run("numfmt", &["--version"], b"").stdout).starts_with("numfmt (agentcmd) "));
}

#[test]
fn numfmt_bad_choice_lists_valid_arguments() {
    let output = run("numfmt", &["--round=sideways", "1"], b"");
    assert_eq!(code(&output), 1);
    let err = text(&output.stderr);
    assert!(err.contains("invalid argument 'sideways' for '--round'"));
    assert!(err.contains("'from-zero'"));
}

#[test]
fn numfmt_format_rules_are_checked() {
    assert_eq!(code(&run("numfmt", &["--format=%d", "1"], b"")), 1);
    assert_eq!(code(&run("numfmt", &["--format=%f %f", "1"], b"")), 1);
    assert_eq!(code(&run("numfmt", &["--format=%.3f", "1500"], b"")), 0);
}

#[test]
fn numfmt_abort_keeps_earlier_output_and_exits_two() {
    let output = run("numfmt", &["--to=iec"], b"2048\nbad\n4096\n");
    assert_eq!(code(&output), 2);
    assert_eq!(text(&output.stdout), "2.0K\n");
    assert!(text(&output.stderr).contains("invalid number: 'bad'"));
}

#[test]
fn numfmt_negative_values_keep_their_sign() {
    let output = run("numfmt", &["--to=si", "--", "-1500"], b"");
    assert_eq!(text(&output.stdout), "-1.5K\n");
}

#[test]
fn numfmt_suffix_without_from_is_rejected() {
    let output = run("numfmt", &["1K"], b"");
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("rejecting suffix in input: '1K'"));
}

#[test]
fn numfmt_unsupported_option_is_usage_error() {
    let output = run("numfmt", &["--no-such-option"], b"");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("unrecognized option"));
}

// ------------------------------------------------------------------ factor

#[test]
fn factor_help_and_version() {
    let help = run("factor", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).contains("--exponents"));
    assert!(text(&run("factor", &["--version"], b"").stdout).starts_with("factor (agentcmd) "));
}

#[test]
fn factor_prints_factors_in_order_with_repeats() {
    let output = run("factor", &["360", "97"], b"");
    assert_eq!(text(&output.stdout), "360: 2 2 2 3 3 5\n97: 97\n");
}

#[test]
fn factor_exponent_form_collapses_repeats() {
    let output = run("factor", &["-h", "360"], b"");
    assert_eq!(text(&output.stdout), "360: 2^3 3^2 5\n");
}

#[test]
fn factor_reads_stdin_tokens_and_reports_bad_ones() {
    let output = run("factor", &[], b"6 x 35\n");
    assert_eq!(code(&output), 1);
    assert_eq!(text(&output.stdout), "6: 2 3\n35: 5 7\n");
    assert!(text(&output.stderr).contains("'x' is not a valid positive integer"));
}

#[test]
fn factor_property_product_of_factors_is_the_input() {
    for number in [2u128, 1001, 65_537, 1_000_000_007 * 3, 600_851_475_143] {
        let output = run("factor", &[&number.to_string()], b"");
        let line = text(&output.stdout);
        let (head, tail) = line.trim_end().split_once(':').unwrap();
        assert_eq!(head.parse::<u128>().unwrap(), number);
        let product: u128 = tail
            .split_whitespace()
            .map(|f| f.parse::<u128>().unwrap())
            .product();
        assert_eq!(product, number);
    }
}

#[test]
fn factor_too_large_is_reported() {
    let output = run("factor", &["340282366920938463463374607431768211457"], b"");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("is too large"));
}

// ------------------------------------------------------------------ expr

#[test]
fn expr_help_and_version_only_as_sole_argument() {
    let help = run("expr", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).starts_with("Usage: expr EXPRESSION"));
    assert!(text(&run("expr", &["--version"], b"").stdout).starts_with("expr (agentcmd) "));
    assert_eq!(code(&run("expr", &["5", "--help"], b"")), 2);
}

#[test]
fn expr_exit_status_follows_the_value() {
    assert_eq!(code(&run("expr", &["2", "+", "2"], b"")), 0);
    assert_eq!(code(&run("expr", &["0"], b"")), 1);
    assert_eq!(code(&run("expr", &[""], b"")), 1);
}

#[test]
fn expr_syntax_errors_exit_two() {
    let output = run("expr", &["1", "2"], b"");
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("syntax error: unexpected argument '2'"));
    assert_eq!(code(&run("expr", &["1", "+"], b"")), 2);
    assert_eq!(code(&run("expr", &[], b"")), 2);
}

#[test]
fn expr_evaluation_errors_exit_three() {
    let division = run("expr", &["4", "/", "0"], b"");
    assert_eq!(code(&division), 3);
    assert!(text(&division.stderr).contains("division by zero"));
    let overflow = run(
        "expr",
        &["170141183460469231731687303715884105727", "+", "1"],
        b"",
    );
    assert_eq!(code(&overflow), 3);
    assert!(text(&overflow.stderr).contains("integer overflow"));
}

#[test]
fn expr_bad_regular_expression_is_a_syntax_error() {
    let output = run("expr", &["abc", ":", "a\\("], b"");
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("Unmatched"));
}

#[test]
fn expr_arguments_are_operands_not_options() {
    let output = run("expr", &["-5", "<", "3"], b"");
    assert_eq!(text(&output.stdout), "1\n");
}

// ------------------------------------------------------------------ pathchk

#[test]
fn pathchk_help_and_version() {
    let help = run("pathchk", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).contains("--portability"));
    assert!(text(&run("pathchk", &["--version"], b"").stdout).starts_with("pathchk (agentcmd) "));
}

#[test]
fn pathchk_does_not_touch_the_file_system() {
    let dir = Sandbox::new();
    let output = run_in(dir.path(), "pathchk", &["-p", "not/there/yet"], b"");
    assert_eq!(code(&output), 0);
}

#[test]
fn pathchk_reports_each_failing_name_and_exits_one() {
    let output = run("pathchk", &["-p", "a b", "fine", "c d"], b"");
    assert_eq!(code(&output), 1);
    let err = text(&output.stderr);
    assert_eq!(err.matches("non-portable character").count(), 2);
}

#[test]
fn pathchk_leading_dash_check_is_per_component() {
    assert_eq!(code(&run("pathchk", &["-P", "--", "-abc"], b"")), 1);
    assert_eq!(code(&run("pathchk", &["-p", "--", "-abc"], b"")), 0);
}

#[test]
fn pathchk_missing_operand_exits_one() {
    let output = run("pathchk", &[], b"");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("missing operand"));
}

#[test]
fn pathchk_component_limits_in_posix_mode() {
    let long_component = "x".repeat(15);
    assert_eq!(code(&run("pathchk", &["-p", &long_component], b"")), 1);
    assert_eq!(code(&run("pathchk", &[&long_component], b"")), 0);
}

// ------------------------------------------------------------------ ptx

#[test]
fn ptx_help_and_version() {
    let help = run("ptx", &["--help"], b"");
    assert_eq!(code(&help), 0);
    let body = text(&help.stdout);
    for option in [
        "--auto-reference",
        "--traditional",
        "--flag-truncation",
        "--macro-name",
        "--gap-size",
        "--width",
        "--references",
    ] {
        assert!(body.contains(option), "help lacks {option}");
    }
    assert!(text(&run("ptx", &["--version"], b"").stdout).starts_with("ptx (agentcmd) "));
}

#[test]
fn ptx_traditional_lines_are_roff_macros() {
    let output = run("ptx", &["-G"], b"a b c\n");
    assert_eq!(
        lines(&output.stdout),
        [
            ".xx \"\" \"\" \"a b c\" \"\"",
            ".xx \"\" \"a\" \"b c\" \"\"",
            ".xx \"\" \"a b\" \"c\" \"\""
        ]
    );
}

#[test]
fn ptx_default_layout_puts_keywords_at_half_width_plus_gap() {
    let output = run("ptx", &[], b"cat\n");
    let line = text(&output.stdout);
    assert_eq!(line, format!("{}cat\n", " ".repeat(39)));
}

#[test]
fn ptx_roff_option_and_macro_name() {
    let output = run("ptx", &["-O", "-M", "DD"], b"hello world\n");
    assert!(text(&output.stdout).starts_with(".DD "));
}

#[test]
fn ptx_tex_format_wraps_groups() {
    let output = run("ptx", &["-T"], b"hello world\n");
    assert!(text(&output.stdout).starts_with("\\xx {}"));
}

#[test]
fn ptx_ignore_and_only_files_select_keywords() {
    let dir = Sandbox::new();
    dir.write("ignore.txt", b"the\n");
    dir.write("only.txt", b"dog\n");
    let ignored = run_in(dir.path(), "ptx", &["-G", "-i", "ignore.txt"], b"the dog\n");
    let rows = lines(&ignored.stdout);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("\"dog\""));
    let only = run_in(dir.path(), "ptx", &["-G", "-o", "only.txt"], b"the dog\n");
    assert_eq!(lines(&only.stdout).len(), 1);
}

#[test]
fn ptx_zero_width_or_gap_is_rejected() {
    let output = run("ptx", &["-w", "0"], b"a\n");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("invalid line width"));
    assert_eq!(code(&run("ptx", &["-g", "x"], b"a\n")), 1);
}

#[test]
fn ptx_word_regexp_uses_extended_syntax() {
    let output = run("ptx", &["-G", "-W", "o+"], b"foo\n");
    assert_eq!(lines(&output.stdout), [".xx \"\" \"f\" \"oo\" \"\""]);
}

#[test]
fn ptx_missing_file_is_reported() {
    let output = run("ptx", &["no-such-input"], b"");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("no-such-input"));
}
