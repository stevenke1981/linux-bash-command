//! CLI tests for the P1 option gaps of existing commands (05-existing-gaps §4): `head`
//! and `tail` `-z`, `cat -u`, and the `grep` additions, plus `egrep` and `fgrep`.
//! Files live in a sandbox; differential behaviour is covered by `tests/gnu_cases`.

mod common;

use common::{run, run_in, Sandbox};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn code(output: &std::process::Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

// head -z

#[test]
fn head_zero_terminated_takes_records_not_lines() {
    let out = run("head", &["-z", "-n", "2"], b"a\nb\0c\0d\0");
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"a\nb\0c\0");
}

#[test]
fn head_zero_long_option_and_all_but_last() {
    let out = run("head", &["--zero-terminated", "-n", "-1"], b"a\0b\0c\0");
    assert_eq!(out.stdout, b"a\0b\0");
    let out = run("head", &["--zero-term", "-n", "1"], b"a\0b\0");
    assert_eq!(out.stdout, b"a\0");
}

#[test]
fn head_zero_keeps_unterminated_last_record_as_is() {
    let out = run("head", &["-z", "-n", "5"], b"x\0y");
    assert_eq!(out.stdout, b"x\0y");
}

#[test]
fn head_zero_does_not_change_byte_counts() {
    let out = run("head", &["-z", "-c", "4"], b"abc\0def\0");
    assert_eq!(out.stdout, b"abc\0");
}

#[test]
fn head_help_lists_zero_terminated() {
    let out = run("head", &["--help"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out.stdout).contains("-z, --zero-terminated"));
}

#[test]
fn head_zero_unknown_option_is_usage_error() {
    let out = run("head", &["-z", "--zero-bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(text(&out.stderr).contains("--zero-bogus"));
}

// tail -z

#[test]
fn tail_zero_terminated_last_records() {
    let out = run("tail", &["-z", "-n", "2"], b"a\0b\0c\0");
    assert_eq!(out.stdout, b"b\0c\0");
    let out = run("tail", &["-z", "-n", "+2"], b"a\0b\0c\0");
    assert_eq!(out.stdout, b"b\0c\0");
}

#[test]
fn tail_zero_without_trailing_terminator() {
    let out = run("tail", &["-z", "-n", "1"], b"one\0two");
    assert_eq!(out.stdout, b"two");
}

#[test]
fn tail_zero_headers_and_verbose() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"a1\0a2\0");
    sb.write("b.txt", b"b1\0");
    let out = run_in(sb.path(), "tail", &["-z", "-n", "1", "a.txt", "b.txt"], b"");
    assert_eq!(out.stdout, b"==> a.txt <==\na2\0\n==> b.txt <==\nb1\0");
    let out = run_in(sb.path(), "tail", &["-zv", "-n", "1", "b.txt"], b"");
    assert_eq!(out.stdout, b"==> b.txt <==\nb1\0");
}

#[test]
fn tail_help_lists_zero_terminated() {
    let out = run("tail", &["--help"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out.stdout).contains("-z, --zero-terminated"));
}

#[test]
fn tail_zero_missing_file_is_exit_one() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "tail", &["-z", "missing.txt"], b"");
    assert_eq!(code(&out), 1);
    assert!(text(&out.stderr).contains("missing.txt"));
}

// cat -u

#[test]
fn cat_unbuffered_is_accepted_and_ignored() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"one\ntwo\n");
    let out = run_in(sb.path(), "cat", &["-u", "a.txt"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"one\ntwo\n");
    let out = run_in(sb.path(), "cat", &["-un", "a.txt"], b"");
    assert_eq!(out.stdout, b"     1\tone\n     2\ttwo\n");
}

#[test]
fn cat_unbuffered_stdin_and_clustered_flags() {
    let out = run("cat", &["-u"], b"x\ny\n");
    assert_eq!(out.stdout, b"x\ny\n");
    let out = run("cat", &["-uE"], b"x\n");
    assert_eq!(out.stdout, b"x$\n");
}

#[test]
fn cat_help_lists_unbuffered() {
    let out = run("cat", &["--help"], b"");
    assert!(text(&out.stdout).contains("Accepted and ignored"));
}

// grep: -z, -Z, -b, -T, --label and the remaining accepted options

#[test]
fn grep_null_data_reads_and_writes_nul_records() {
    let out = run("grep", &["-z", "hello"], b"one\ntwo hello\0three\0");
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"one\ntwo hello\0");
    let out = run("grep", &["-zc", "e"], b"a\0e\0ee\0");
    assert_eq!(out.stdout, b"2\n");
}

#[test]
fn grep_null_data_dot_spans_newlines_and_anchors_are_per_record() {
    let out = run("grep", &["-z", "one.two"], b"one\ntwo\0x\0");
    assert_eq!(out.stdout, b"one\ntwo\0");
    let out = run("grep", &["-z", "^two"], b"one\ntwo\0");
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
}

#[test]
fn grep_null_data_only_matching_and_byte_offset() {
    let out = run("grep", &["-zo", "t[a-z]*"], b"one\0two\0three\0");
    assert_eq!(out.stdout, b"two\0three\0");
    let out = run("grep", &["-zb", "three"], b"one\0two\0three\0");
    assert_eq!(out.stdout, b"8:three\0");
}

#[test]
fn grep_null_names_separate_file_names_with_nul() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"hello\n");
    sb.write("b.txt", b"no\n");
    sb.write("c.txt", b"hello\n");
    let out = run_in(
        sb.path(),
        "grep",
        &["-lZ", "hello", "a.txt", "b.txt", "c.txt"],
        b"",
    );
    assert_eq!(out.stdout, b"a.txt\0c.txt\0");
    let out = run_in(sb.path(), "grep", &["-Z", "hello", "a.txt", "b.txt"], b"");
    assert_eq!(out.stdout, b"a.txt\0hello\n");
    let out = run_in(sb.path(), "grep", &["-cZ", "hello", "a.txt", "b.txt"], b"");
    assert_eq!(out.stdout, b"a.txt\x001\nb.txt\x000\n");
}

#[test]
fn grep_byte_offset_for_lines_and_matches() {
    let out = run("grep", &["-b", "hello"], b"hello\nxx hello\n");
    assert_eq!(out.stdout, b"0:hello\n6:xx hello\n");
    let out = run("grep", &["-bo", "hello"], b"xx hello\n");
    assert_eq!(out.stdout, b"3:hello\n");
}

#[test]
fn grep_initial_tab_pads_numbers_to_the_input_size_width() {
    let sb = Sandbox::new();
    // 106 bytes: three digits, so line numbers are right-aligned in a width-3 field.
    let mut body = "x".repeat(19);
    body.push('\n');
    body = body.repeat(5);
    body.push_str("hello\n");
    sb.write("pad.txt", body.as_bytes());
    let out = run_in(sb.path(), "grep", &["-T", "-n", "hello", "pad.txt"], b"");
    assert_eq!(out.stdout, b"  6:\thello\n");
    // Without any prefix there is nothing to align, so no tab is written.
    let out = run_in(sb.path(), "grep", &["-T", "hello", "pad.txt"], b"");
    assert_eq!(out.stdout, b"hello\n");
}

#[test]
fn grep_label_names_standard_input() {
    let out = run("grep", &["--label=LBL", "-H", "hello"], b"hello\n");
    assert_eq!(out.stdout, b"LBL:hello\n");
    let out = run(
        "grep",
        &["--label", "LBL", "-Hn", "hello", "-"],
        b"x\nhello\n",
    );
    assert_eq!(out.stdout, b"LBL:2:hello\n");
}

#[test]
fn grep_line_buffered_and_binary_flags_are_accepted() {
    let out = run("grep", &["--line-buffered", "-U", "hi"], b"hi\n");
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"hi\n");
}

#[test]
fn grep_case_flags_y_and_no_ignore_case() {
    let out = run("grep", &["-y", "HELLO"], b"hello\n");
    assert_eq!(out.stdout, b"hello\n");
    let out = run(
        "grep",
        &["-i", "--no-ignore-case", "HELLO"],
        b"hello\nHELLO\n",
    );
    assert_eq!(out.stdout, b"HELLO\n");
}

#[test]
fn grep_exclude_from_reads_globs_from_a_file() {
    let sb = Sandbox::new();
    sb.write("skip.lst", b"*.log\n");
    sb.write("d/a.log", b"hello\n");
    sb.write("d/b.txt", b"hello\n");
    let out = run_in(
        sb.path(),
        "grep",
        &["-r", "--exclude-from=skip.lst", "hello", "d"],
        b"",
    );
    assert_eq!(out.stdout, b"d/b.txt:hello\n");
}

#[test]
fn grep_exclude_from_missing_file_is_exit_two() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"hello\n");
    let out = run_in(
        sb.path(),
        "grep",
        &["--exclude-from", "nope.lst", "hello", "a.txt"],
        b"",
    );
    assert_eq!(code(&out), 2);
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("nope.lst: No such file or directory"));
}

#[test]
fn grep_devices_unknown_method_is_exit_two() {
    let out = run("grep", &["-D", "bogus", "x"], b"");
    assert_eq!(code(&out), 2);
    assert!(text(&out.stderr).contains("unknown devices method"));
}

/// `/dev/null` is a character device on Unix; the device check does not apply to Windows.
#[cfg(unix)]
#[test]
fn grep_devices_skip_and_read_on_dev_null() {
    let out = run("grep", &["-D", "skip", "hello", "/dev/null"], b"");
    assert_eq!(code(&out), 1);
    let out = run("grep", &["--devices=read", "hello", "/dev/null"], b"");
    assert_eq!(code(&out), 1);
}

#[test]
fn grep_directories_actions() {
    let sb = Sandbox::new();
    sb.write("d/x.txt", b"hello\n");
    sb.write("a.txt", b"hello\n");
    let out = run_in(
        sb.path(),
        "grep",
        &["-d", "skip", "hello", "d", "a.txt"],
        b"",
    );
    assert_eq!(out.stdout, b"a.txt:hello\n");
    let out = run_in(sb.path(), "grep", &["-d", "recurse", "hello", "d"], b"");
    assert_eq!(out.stdout, b"d/x.txt:hello\n");
    let out = run_in(sb.path(), "grep", &["hello", "d"], b"");
    assert_eq!(code(&out), 2);
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("d: Is a directory"));
}

#[test]
fn grep_directories_unknown_action_is_usage_error() {
    let out = run("grep", &["-d", "bogus", "x"], b"");
    assert_eq!(code(&out), 2);
    assert!(text(&out.stderr).contains("invalid argument 'bogus' for '--directories'"));
}

#[test]
fn grep_help_lists_every_new_option() {
    let out = run("grep", &["--help"], b"");
    assert_eq!(code(&out), 0);
    let help = text(&out.stdout);
    for needle in [
        "--no-ignore-case",
        "-b, --byte-offset",
        "-T, --initial-tab",
        "-Z, --null ",
        "-z, --null-data",
        "--label=LABEL",
        "--line-buffered",
        "-U, --binary",
        "--exclude-from=FILE",
        "-d, --directories=ACTION",
        "-D, --devices=ACTION",
    ] {
        assert!(help.contains(needle), "help is missing {needle}");
    }
}

#[test]
fn grep_long_option_abbreviation_for_new_option() {
    let out = run("grep", &["--null-d", "hello"], b"hello\0x\0");
    assert_eq!(out.stdout, b"hello\0");
}

// egrep and fgrep

#[test]
fn egrep_is_extended_grep() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"abbc\nac\nx\n");
    let out = run_in(sb.path(), "egrep", &["ab+c", "a.txt"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"abbc\n");
    let out = run("egrep", &["-c", "a|x"], b"a\nb\nx\n");
    assert_eq!(out.stdout, b"2\n");
}

#[test]
fn egrep_without_pattern_is_usage_error() {
    let out = run("egrep", &[], b"");
    assert_eq!(code(&out), 2);
}

#[test]
fn egrep_accepts_grep_options_with_z() {
    let out = run("egrep", &["-z", "two|four"], b"one\0two\0four\0");
    assert_eq!(out.stdout, b"two\0four\0");
}

#[test]
fn fgrep_matches_literal_text() {
    let out = run("fgrep", &["a.b"], b"axb\na.b\n");
    assert_eq!(out.stdout, b"a.b\n");
    let out = run("fgrep", &["[x]"], b"[x]\nx\n");
    assert_eq!(out.stdout, b"[x]\n");
}

#[test]
fn fgrep_newline_separates_patterns_and_counts() {
    let out = run("fgrep", &["one\ntwo"], b"one\ntwo\nthree\n");
    assert_eq!(out.stdout, b"one\ntwo\n");
    let out = run("fgrep", &["-c", "-e", "a", "-e", "b"], b"a\nb\nc\n");
    assert_eq!(out.stdout, b"2\n");
}

#[test]
fn fgrep_empty_pattern_matches_every_line() {
    let out = run("fgrep", &[""], b"one\n\n");
    assert_eq!(out.stdout, b"one\n\n");
}

#[test]
fn fgrep_help_and_missing_file_exit_status() {
    let out = run("fgrep", &["--help"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out.stdout).starts_with("Usage: grep"));
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "fgrep", &["x", "missing.txt"], b"");
    assert_eq!(code(&out), 2);
}
