//! CLI tests for `awk`: options, program sources, input handling, exit codes, error messages,
//! and the parts that the GNU differential cases cannot cover (pipes, `system()`, help text,
//! UTF-8 `length`, and the array form of `length`).

mod common;

use std::process::Output;

use common::{run, run_in, Sandbox};

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn status(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn awk(args: &[&str], stdin: &str) -> Output {
    run("awk", args, stdin.as_bytes())
}

#[test]
fn help_lists_options_and_exits_zero() {
    let output = awk(&["--help"], "");
    assert_eq!(status(&output), 0);
    let text = stdout(&output);
    assert!(text.starts_with("Usage: awk"));
    for option in ["-F FS", "-v VAR=VAL", "-f PROGFILE", "--help", "--version"] {
        assert!(text.contains(option), "help lacks {option}");
    }
}

#[test]
fn version_line_matches_convention() {
    let output = awk(&["--version"], "");
    assert_eq!(status(&output), 0);
    assert_eq!(
        stdout(&output),
        format!("awk (agentcmd) {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_option_is_a_usage_error_with_status_one() {
    let output = awk(&["-z", "BEGIN{}"], "");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid option -- 'z'"));
    assert!(stderr(&output).contains("Try 'awk --help'"));
}

#[test]
fn missing_program_is_a_usage_error() {
    let output = awk(&[], "");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).starts_with("Usage: awk"));
}

#[test]
fn field_separator_option_sets_fs() {
    let output = awk(&["-F:", "{print $2}"], "a:b:c\n");
    assert_eq!(stdout(&output), "b\n");
}

#[test]
fn field_separator_option_processes_escapes() {
    let output = awk(&["-F", "\\t", "{print $2}"], "a\tb c\n");
    assert_eq!(stdout(&output), "b c\n");
}

#[test]
fn variable_option_is_visible_in_begin() {
    let output = awk(
        &[
            "-v",
            "name=world",
            "-v",
            "n=3",
            "BEGIN{print \"hi\", name, n*2}",
        ],
        "",
    );
    assert_eq!(stdout(&output), "hi world 6\n");
}

#[test]
fn variable_option_rejects_a_bad_name() {
    let output = awk(&["-v", "1x=2", "BEGIN{}"], "");
    assert_eq!(status(&output), 2);
    assert!(stderr(&output).contains("invalid -v argument"));
}

#[test]
fn program_file_option_and_multiple_files() {
    let sandbox = Sandbox::new();
    sandbox.write("one.awk", b"function twice(x) { return 2 * x }\n");
    sandbox.write("two.awk", b"BEGIN { print twice(21) }\n");
    let output = run_in(
        sandbox.path(),
        "awk",
        &["-f", "one.awk", "-f", "two.awk"],
        b"",
    );
    assert_eq!(stdout(&output), "42\n");
}

#[test]
fn program_file_missing_is_fatal_status_two() {
    let sandbox = Sandbox::new();
    let output = run_in(sandbox.path(), "awk", &["-f", "nope.awk"], b"");
    assert_eq!(status(&output), 2);
    assert!(stderr(&output).contains("cannot open source file 'nope.awk'"));
}

#[test]
fn double_dash_ends_options_before_the_program() {
    let output = awk(&["--", "{print $1}"], "x y\n");
    assert_eq!(stdout(&output), "x\n");
}

#[test]
fn reads_standard_input_without_file_operands() {
    let output = awk(&["{ n++ } END { print n }"], "a\nb\nc\n");
    assert_eq!(stdout(&output), "3\n");
}

#[test]
fn dash_operand_is_standard_input() {
    let sandbox = Sandbox::new();
    sandbox.write("f.txt", b"file\n");
    let output = run_in(
        sandbox.path(),
        "awk",
        &["{print $0}", "f.txt", "-"],
        b"piped\n",
    );
    assert_eq!(stdout(&output), "file\npiped\n");
}

#[test]
fn filename_and_fnr_across_files() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"1\n2\n");
    sandbox.write("b.txt", b"3\n");
    let output = run_in(
        sandbox.path(),
        "awk",
        &["FNR == 1 { print FILENAME, NR, FNR }", "a.txt", "b.txt"],
        b"",
    );
    assert_eq!(stdout(&output), "a.txt 1 1\nb.txt 3 1\n");
}

#[test]
fn assignment_operands_apply_when_reached() {
    let sandbox = Sandbox::new();
    sandbox.write("f.txt", b"row\n");
    let output = run_in(
        sandbox.path(),
        "awk",
        &[
            "{ print tag, $0 }",
            "tag=first",
            "f.txt",
            "tag=second",
            "f.txt",
        ],
        b"",
    );
    assert_eq!(stdout(&output), "first row\nsecond row\n");
}

#[test]
fn missing_input_file_is_fatal_and_stops_processing() {
    let sandbox = Sandbox::new();
    sandbox.write("ok.txt", b"ok\n");
    let output = run_in(
        sandbox.path(),
        "awk",
        &["{ print }", "missing.txt", "ok.txt"],
        b"",
    );
    assert_eq!(status(&output), 2);
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("cannot open file `missing.txt' for reading"));
}

#[test]
fn syntax_error_reports_line_and_exits_two() {
    let output = awk(&["BEGIN {\n  x = (1 + 2\n}"], "");
    assert_eq!(status(&output), 2);
    let message = stderr(&output);
    assert!(
        message.contains("syntax error at source line 3"),
        "{message}"
    );
}

#[test]
fn exit_status_from_exit_statement() {
    let output = awk(&["{ if (NR == 2) exit 7 } { print }"], "a\nb\nc\n");
    assert_eq!(status(&output), 7);
    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn end_runs_after_exit_in_main_rules() {
    let output = awk(&["NR == 1 { exit } END { print \"end\", NR }"], "a\nb\n");
    assert_eq!(stdout(&output), "end 1\n");
}

#[test]
fn division_by_zero_is_fatal_at_run_time() {
    let output = awk(&["BEGIN { z = 0; print 1; print 1 / z }"], "");
    assert_eq!(status(&output), 2);
    assert_eq!(stdout(&output), "1\n");
    assert!(stderr(&output).contains("division by zero"));
}

#[test]
fn system_is_unsupported() {
    let output = awk(&["BEGIN { system(\"true\") }"], "");
    assert_eq!(status(&output), 2);
    assert!(stderr(&output).contains("system() is not supported"));
}

#[test]
fn output_pipe_is_unsupported() {
    let output = awk(&["BEGIN { print \"x\" | \"cat\" }"], "");
    assert_eq!(status(&output), 2);
    assert!(stderr(&output).contains("output pipes are not supported"));
}

#[test]
fn input_pipe_getline_is_unsupported() {
    let output = awk(&["BEGIN { \"echo hi\" | getline x; print x }"], "");
    assert_eq!(status(&output), 2);
    assert!(stderr(&output).contains("input pipes"));
}

#[test]
fn print_redirection_creates_files_in_the_sandbox() {
    let sandbox = Sandbox::new();
    let output = run_in(
        sandbox.path(),
        "awk",
        &["BEGIN { print \"one\" > \"out.txt\"; print \"two\" > \"out.txt\"; close(\"out.txt\"); print \"more\" >> \"out.txt\" }"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("out.txt"), b"one\ntwo\nmore\n");
}

#[test]
fn stderr_redirection_goes_to_stderr() {
    let output = awk(
        &["BEGIN { print \"warn\" > \"/dev/stderr\"; print \"out\" }"],
        "",
    );
    assert_eq!(stdout(&output), "out\n");
    assert_eq!(stderr(&output), "warn\n");
}

#[test]
fn getline_from_file_reads_records() {
    let sandbox = Sandbox::new();
    sandbox.write("data.txt", b"x 1\ny 2\n");
    let output = run_in(
        sandbox.path(),
        "awk",
        &["BEGIN { while ((getline line < \"data.txt\") > 0) n++; print n, line }"],
        b"",
    );
    assert_eq!(stdout(&output), "2 y 2\n");
}

#[test]
fn length_counts_utf8_characters_and_bytes_for_invalid_sequences() {
    let output = awk(&["BEGIN { print length(\"héllo\"), length(\"€\") }"], "");
    assert_eq!(stdout(&output), "5 1\n");
    let output = awk(&["{ print length($0) }"], "a\u{e9}b\n");
    assert_eq!(stdout(&output), "3\n");
}

#[test]
fn length_of_an_array_counts_elements() {
    let output = awk(&["BEGIN { a[\"x\"]; a[\"y\"]; print length(a) }"], "");
    assert_eq!(stdout(&output), "2\n");
}

#[test]
fn substr_handles_utf8_characters() {
    let output = awk(
        &["BEGIN { print substr(\"añb\", 2, 1), index(\"añb\", \"b\") }"],
        "",
    );
    assert_eq!(stdout(&output), "ñ 3\n");
}

#[test]
fn record_separator_paragraph_mode_across_chunks() {
    let mut input = String::new();
    for i in 0..5000 {
        input.push_str(&format!("line {i}\n"));
        if i % 100 == 99 {
            input.push('\n');
        }
    }
    let output = awk(&["BEGIN { RS = \"\" } { n++ } END { print n }"], &input);
    assert_eq!(stdout(&output), "50\n");
}

#[test]
fn printf_numeric_formats_through_the_cli() {
    let output = awk(
        &["BEGIN { printf \"%5.2f|%-4s|%03d|%e|%x|%c\\n\", 3.14159, \"ab\", 7, 1234.5, 255, 65 }"],
        "",
    );
    assert_eq!(stdout(&output), " 3.14|ab  |007|1.234500e+03|ff|A\n");
}

#[test]
fn large_input_sums_fields_quickly() {
    let mut input = String::new();
    for i in 1..=100_000 {
        input.push_str(&format!("{i} {}\n", i * 2));
    }
    let output = awk(&["{ s += $2 } END { print s }"], &input);
    assert_eq!(stdout(&output), "10000100000\n");
}

#[test]
fn deep_recursion_completes() {
    let output = awk(
        &["function d(n) { return n == 0 ? 0 : 1 + d(n - 1) } BEGIN { print d(2000) }"],
        "",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "2000\n");
}
