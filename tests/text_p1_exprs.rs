//! CLI tests for `printf`, `seq`, `test` and `[`: help, version, exit codes, messages and
//! the behaviour that the differential cases cannot express. Files live in a sandbox.

mod common;

use common::{run, run_in, Sandbox};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn code(output: &std::process::Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

// printf

#[test]
fn printf_help_and_version() {
    let help = run("printf", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).starts_with("Usage: printf FORMAT [ARGUMENT]..."));
    let version = run("printf", &["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(text(&version.stdout).starts_with("printf (agentcmd) "));
}

#[test]
fn printf_help_is_an_option_only_as_sole_argument() {
    let output = run("printf", &["%s\n", "--help"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout, b"--help\n");
}

#[test]
fn printf_missing_operand_is_a_usage_error() {
    let output = run("printf", &[], b"");
    assert_eq!(code(&output), 1);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("printf: missing operand"));
    assert!(stderr.contains("Try 'printf --help' for more information."));
}

#[test]
fn printf_numeric_error_prints_and_continues() {
    let output = run("printf", &["%d %d\n", "x", "5"], b"");
    assert_eq!(code(&output), 1);
    assert_eq!(output.stdout, b"0 5\n");
    assert!(text(&output.stderr).contains("printf: 'x': expected a numeric value"));
}

#[test]
fn printf_invalid_conversion_stops_with_prefix() {
    let output = run("printf", &["ab%ycd"], b"");
    assert_eq!(code(&output), 1);
    assert_eq!(output.stdout, b"ab");
    assert!(text(&output.stderr).contains("invalid conversion specification"));
}

#[test]
fn printf_ignores_stdin_and_writes_raw_bytes() {
    let output = run("printf", &["%b", "\\0101\\0"], b"ignored stdin");
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout, b"A\0");
}

#[test]
fn printf_warns_about_excess_arguments() {
    let output = run("printf", &["plain\n", "extra"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout, b"plain\n");
    assert!(
        text(&output.stderr).contains("warning: ignoring excess arguments, starting with 'extra'")
    );
}

#[test]
fn printf_missing_hex_in_escape_is_an_error() {
    let output = run("printf", &["%b|", "\\x"], b"");
    assert_eq!(code(&output), 1);
    assert!(text(&output.stderr).contains("missing hexadecimal number in escape"));
}

// seq

#[test]
fn seq_help_and_version() {
    let help = run("seq", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).starts_with("Usage: seq [OPTION]... LAST"));
    let version = run("seq", &["--version"], b"");
    assert!(text(&version.stdout).starts_with("seq (agentcmd) "));
}

#[test]
fn seq_missing_operand_and_extra_operand_are_usage_errors() {
    let missing = run("seq", &[], b"");
    assert_eq!(code(&missing), 1);
    assert!(text(&missing.stderr).contains("seq: missing operand"));
    let extra = run("seq", &["1", "2", "3", "4"], b"");
    assert_eq!(code(&extra), 1);
    assert!(text(&extra.stderr).contains("seq: extra operand '4'"));
}

#[test]
fn seq_zero_increment_is_rejected() {
    let output = run("seq", &["1", "0", "5"], b"");
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("invalid Zero increment value: '0'"));
}

#[test]
fn seq_separator_replaces_newlines_but_keeps_final_newline() {
    let output = run("seq", &["-s", ",", "3"], b"");
    assert_eq!(output.stdout, b"1,2,3\n");
}

#[test]
fn seq_equal_width_pads_with_zeroes() {
    let output = run("seq", &["-w", "8", "11"], b"");
    assert_eq!(output.stdout, b"08\n09\n10\n11\n");
}

#[test]
fn seq_decimal_steps_are_exact() {
    let output = run("seq", &["0", "0.1", "0.3"], b"");
    assert_eq!(output.stdout, b"0.0\n0.1\n0.2\n0.3\n");
}

#[test]
fn seq_format_errors_have_no_try_line() {
    let output = run("seq", &["-f", "abc", "1"], b"");
    assert_eq!(code(&output), 1);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("seq: format 'abc' has no % directive"));
    assert!(!stderr.contains("Try"));
}

#[test]
fn seq_nan_and_bad_numbers_are_rejected() {
    let nan = run("seq", &["nan"], b"");
    assert_eq!(code(&nan), 1);
    assert!(text(&nan.stderr).contains("not-a-number"));
    let bad = run("seq", &["1", "x"], b"");
    assert_eq!(code(&bad), 1);
    assert!(text(&bad.stderr).contains("invalid floating point argument: 'x'"));
}

#[test]
fn seq_empty_range_prints_nothing() {
    let output = run("seq", &["5", "1"], b"");
    assert_eq!(code(&output), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn seq_double_dash_allows_negative_last() {
    let output = run("seq", &["--", "-2", "-1"], b"");
    assert_eq!(output.stdout, b"-2\n-1\n");
}

// test and [

#[test]
fn test_has_no_help_option() {
    let output = run("test", &["--help"], b"");
    assert_eq!(code(&output), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_bad_integer_is_status_two() {
    let output = run("test", &["x", "-eq", "1"], b"");
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("test: invalid integer 'x'"));
}

#[test]
fn test_unknown_operator_is_status_two() {
    let output = run("test", &["-Q", "x"], b"");
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("'-Q': unary operator expected"));
}

#[test]
fn test_dash_t_is_false_when_stdin_is_a_pipe() {
    let output = run("test", &["-t", "0"], b"");
    assert_eq!(code(&output), 1);
}

#[test]
fn test_precedence_and_grouping() {
    assert_eq!(
        code(&run(
            "test",
            &["(", "a", "=", "a", ")", "-a", "!", "b", "=", "c"],
            b""
        )),
        0
    );
    assert_eq!(code(&run("test", &["a", "-o", "b", "-a", ""], b"")), 0);
    assert_eq!(code(&run("test", &["!", "a", "-a", "b"], b"")), 1);
}

#[test]
fn bracket_help_and_version_only_as_sole_argument() {
    let help = run("[", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).starts_with("Usage: [ EXPRESSION ]"));
    let version = run("[", &["--version"], b"");
    assert!(text(&version.stdout).starts_with("[ (agentcmd) "));
    let as_string = run("[", &["--help", "]"], b"");
    assert_eq!(code(&as_string), 0);
    assert!(as_string.stdout.is_empty());
}

#[test]
fn bracket_requires_closing_bracket() {
    let output = run("[", &["a", "=", "a"], b"");
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("[: missing ']'"));
}

#[test]
fn bracket_empty_expression_is_false() {
    assert_eq!(code(&run("[", &["]"], b"")), 1);
}

#[test]
fn file_predicates_in_a_sandbox() {
    let sandbox = Sandbox::new();
    sandbox.write("file.txt", b"data");
    sandbox.write("empty", b"");
    sandbox.mkdir("sub");
    let dir = sandbox.path();
    assert_eq!(code(&run_in(dir, "test", &["-f", "file.txt"], b"")), 0);
    assert_eq!(code(&run_in(dir, "test", &["-d", "sub"], b"")), 0);
    assert_eq!(code(&run_in(dir, "test", &["-s", "empty"], b"")), 1);
    assert_eq!(code(&run_in(dir, "test", &["-e", "missing"], b"")), 1);
    assert_eq!(code(&run_in(dir, "[", &["-e", "file.txt", "]"], b"")), 0);
}

#[cfg(unix)]
#[test]
fn symbolic_link_predicate_in_a_sandbox() {
    let sandbox = Sandbox::new();
    sandbox.write("target", b"x");
    if sandbox.symlink("target", "link").is_none() {
        return;
    }
    let dir = sandbox.path();
    assert_eq!(code(&run_in(dir, "test", &["-h", "link"], b"")), 0);
    assert_eq!(code(&run_in(dir, "test", &["-h", "target"], b"")), 1);
}
