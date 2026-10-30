//! CLI tests for `sed`: script semantics through the binary, option handling,
//! in-place editing, exit statuses and messages. All files live in a `Sandbox`.

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

fn sed(args: &[&str], stdin: &str) -> Output {
    run("sed", args, stdin.as_bytes())
}

#[test]
fn help_goes_to_stdout_with_status_zero() {
    let output = sed(&["--help"], "");
    assert_eq!(status(&output), 0);
    assert!(stdout(&output).starts_with("Usage: sed [OPTION]..."));
    assert!(stdout(&output).contains("--in-place"));
}

#[test]
fn version_names_the_command() {
    let output = sed(&["--version"], "");
    assert_eq!(status(&output), 0);
    assert_eq!(
        stdout(&output),
        format!("sed (agentcmd) {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn no_script_is_a_usage_error() {
    let output = sed(&[], "");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("Usage:"));
}

#[test]
fn unknown_option_is_a_usage_error() {
    let output = sed(&["-k", "p"], "");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid option -- 'k'"));
    assert!(stderr(&output).contains("Try 'sed --help'"));
}

#[test]
fn unknown_long_option_is_a_usage_error() {
    let output = sed(&["--nonexistent", "p"], "");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("unrecognized option '--nonexistent'"));
}

#[test]
fn debug_option_is_reported_unsupported() {
    let output = sed(&["--debug", "p"], "x\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("not supported by agentcmd"));
}

#[test]
fn substitution_and_quiet_print() {
    let output = sed(&["-n", "s/a/A/gp"], "banana\nxyz\n");
    assert_eq!(stdout(&output), "bAnAnA\n");
    assert_eq!(status(&output), 0);
}

#[test]
fn missing_trailing_newline_is_preserved() {
    let output = sed(&["s/x/y/"], "x");
    assert_eq!(stdout(&output), "y");
    let output = sed(&["p"], "x");
    assert_eq!(stdout(&output), "x\nx");
}

#[test]
fn multiple_expressions_run_in_order() {
    let output = sed(&["-e", "s/a/b/", "-e", "s/b/c/"], "a\n");
    assert_eq!(stdout(&output), "c\n");
}

#[test]
fn script_file_and_stdin_script() {
    let sandbox = Sandbox::new();
    sandbox.write("script.sed", b"1d\ns/two/2/\n");
    sandbox.write("data.txt", b"one\ntwo\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["-f", "script.sed", "data.txt"],
        b"",
    );
    assert_eq!(stdout(&output), "2\n");
    let output = run_in(sandbox.path(), "sed", &["-f", "-", "data.txt"], b"$a end\n");
    assert_eq!(stdout(&output), "one\ntwo\nend\n");
}

#[test]
fn missing_script_file_is_status_four() {
    let output = sed(&["-f", "no-such-script.sed"], "");
    assert_eq!(status(&output), 4);
    assert!(stderr(&output).contains("couldn't open file no-such-script.sed"));
}

#[test]
fn missing_input_file_continues_and_exits_two() {
    let sandbox = Sandbox::new();
    sandbox.write("here.txt", b"hi\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["p", "missing.txt", "here.txt"],
        b"",
    );
    assert_eq!(status(&output), 2);
    assert_eq!(stdout(&output), "hi\nhi\n");
    assert!(stderr(&output).contains("can't read missing.txt: No such file or directory"));
}

#[test]
fn syntax_errors_exit_one_with_gnu_style_messages() {
    let cases = [
        ("s/a/b", "unterminated `s' command"),
        ("k", "unknown command: `k'"),
        ("{p", "unmatched `{'"),
        ("}", "unexpected `}'"),
        ("y/ab/c/", "strings for `y' command are different lengths"),
        ("p x", "extra characters after command"),
        ("1,2q", "command only uses one address"),
        ("0p", "invalid usage of line address 0"),
    ];
    for (script, message) in cases {
        let output = sed(&[script], "a\n");
        assert_eq!(status(&output), 1, "{script}");
        assert!(
            stderr(&output).contains(message),
            "{script}: {}",
            stderr(&output)
        );
        assert!(
            stderr(&output).starts_with("sed: -e expression #1, char "),
            "{script}"
        );
    }
}

#[test]
fn missing_label_exits_four() {
    let output = sed(&["b nowhere"], "a\n");
    assert_eq!(status(&output), 4);
    assert!(stderr(&output).contains("can't find label for jump to `nowhere'"));
}

#[test]
fn backreference_in_pattern_is_rejected() {
    let output = sed(&["s/\\(a\\)\\1/X/"], "aa\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("unsupported backreference in regex"));
}

#[test]
fn e_command_and_flag_are_unsupported() {
    let output = sed(&["e echo hi"], "a\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("e command is not supported"));
    let output = sed(&["s/a/b/e"], "a\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("e flag"));
}

#[test]
fn sandbox_refuses_file_and_exec_commands() {
    let output = sed(&["--sandbox", "w out.txt"], "a\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("disabled in sandbox mode"));
    let output = sed(&["--sandbox", "s/a/b/"], "a\n");
    assert_eq!(stdout(&output), "b\n");
}

#[test]
fn extended_regex_options_are_equivalent() {
    for flag in ["-E", "-r", "--regexp-extended"] {
        let output = sed(&[flag, "s/(a|b)+/X/"], "aabc\n");
        assert_eq!(stdout(&output), "Xc\n", "{flag}");
    }
}

#[test]
fn null_data_splits_records_on_nul() {
    let output = run("sed", &["-z", "s/^/>/"], b"a\0b\0");
    assert_eq!(output.stdout, b">a\0>b\0");
    let output = run("sed", &["-z", "-n", "$="], b"a\0b\0");
    assert_eq!(output.stdout, b"2\0");
}

#[test]
fn list_command_escapes_and_wraps() {
    let output = sed(&["-n", "l"], "a\tb\\\n");
    assert_eq!(stdout(&output), "a\\tb\\\\$\n");
    let output = sed(&["-n", "-l", "5", "l"], "abcdefgh\n");
    assert_eq!(stdout(&output), "abcd\\\nefgh$\n");
    let output = sed(
        &["-n", "l 0"],
        "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz\n",
    );
    assert_eq!(stdout(&output).lines().count(), 1);
}

#[test]
fn separate_restarts_line_numbers_and_last_line() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"a\nb\n");
    sandbox.write("b.txt", b"c\nd\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["-s", "-n", "$p;1p", "a.txt", "b.txt"],
        b"",
    );
    assert_eq!(stdout(&output), "a\nb\nc\nd\n");
    let output = run_in(sandbox.path(), "sed", &["-n", "3p", "a.txt", "b.txt"], b"");
    assert_eq!(stdout(&output), "c\n");
}

#[test]
fn quit_sets_status_and_quiet_quit_prints_nothing() {
    let output = sed(&["2q5"], "1\n2\n3\n");
    assert_eq!(stdout(&output), "1\n2\n");
    assert_eq!(status(&output), 5);
    let output = sed(&["2Q"], "1\n2\n3\n");
    assert_eq!(stdout(&output), "1\n");
    assert_eq!(status(&output), 0);
}

#[test]
fn write_and_read_files_round_trip() {
    let sandbox = Sandbox::new();
    sandbox.write("in.txt", b"x1\nx2\n");
    sandbox.write("extra.txt", b"EXTRA\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["-n", "/x2/w out.txt\n1r extra.txt", "in.txt"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("out.txt"), b"x2\n");
    let output = run_in(sandbox.path(), "sed", &["1r extra.txt", "in.txt"], b"");
    assert_eq!(stdout(&output), "x1\nEXTRA\nx2\n");
}

#[test]
fn write_to_stdout_interleaves_with_normal_output() {
    let output = sed(&["w /dev/stdout"], "a\nb\n");
    assert_eq!(stdout(&output), "a\na\nb\nb\n");
}

#[test]
fn in_place_edits_the_file_and_keeps_a_backup() {
    let sandbox = Sandbox::new();
    sandbox.write("doc.txt", b"hello\nworld\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["-i.bak", "s/o/0/g", "doc.txt"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert!(stdout(&output).is_empty());
    assert_eq!(sandbox.read("doc.txt"), b"hell0\nw0rld\n");
    assert_eq!(sandbox.read("doc.txt.bak"), b"hello\nworld\n");
}

#[test]
fn in_place_star_suffix_names_the_backup() {
    let sandbox = Sandbox::new();
    sandbox.write("doc.txt", b"a\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["-iold-*", "s/a/b/", "doc.txt"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("old-doc.txt"), b"a\n");
    assert_eq!(sandbox.read("doc.txt"), b"b\n");
}

#[test]
fn in_place_files_are_edited_independently() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"a\nb\n");
    sandbox.write("b.txt", b"c\nd\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["-i", "1s/^/>/;$s/$/!/", "a.txt", "b.txt"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("a.txt"), b">a\nb!\n");
    assert_eq!(sandbox.read("b.txt"), b">c\nd!\n");
}

#[test]
fn in_place_quit_truncates_the_rest_of_the_file() {
    let sandbox = Sandbox::new();
    sandbox.write("f.txt", b"1\n2\n3\n");
    let output = run_in(sandbox.path(), "sed", &["-i", "2q", "f.txt"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("f.txt"), b"1\n2\n");
}

#[test]
fn in_place_without_files_is_refused() {
    let output = sed(&["-i", "s/a/b/"], "a\n");
    assert_eq!(status(&output), 4);
    assert!(stderr(&output).contains("no input files"));
}

#[test]
fn in_place_follow_symlinks_edits_the_target() {
    let sandbox = Sandbox::new();
    sandbox.write("real.txt", b"a\n");
    if sandbox.symlink("real.txt", "link.txt").is_none() {
        return;
    }
    let output = run_in(
        sandbox.path(),
        "sed",
        &["--follow-symlinks", "-i", "s/a/z/", "link.txt"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("real.txt"), b"z\n");
}

#[test]
fn w_file_is_truncated_even_when_not_written() {
    let sandbox = Sandbox::new();
    sandbox.write("out.txt", b"old\n");
    sandbox.write("in.txt", b"x\n");
    let output = run_in(
        sandbox.path(),
        "sed",
        &["/nomatch/w out.txt", "in.txt"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert!(sandbox.read("out.txt").is_empty());
}

#[test]
fn empty_regex_reuses_the_last_regex_at_run_time() {
    let output = sed(&["-n", "/o/{s//0/p}"], "foo\nbar\n");
    assert_eq!(stdout(&output), "f0o\n");
}

#[test]
fn hash_n_first_line_acts_as_quiet() {
    let output = sed(&["#n\n2p"], "1\n2\n3\n");
    assert_eq!(stdout(&output), "2\n");
}

#[test]
fn append_insert_and_change_text() {
    let output = sed(&["-e", "1i\\  head", "-e", "$a tail"], "x\ny\n");
    assert_eq!(stdout(&output), "  head\nx\ny\ntail\n");
    let output = sed(&["2c\\changed"], "1\n2\n3\n");
    assert_eq!(stdout(&output), "1\nchanged\n3\n");
}
