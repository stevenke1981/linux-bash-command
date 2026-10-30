//! CLI tests for the Phase 1 simple text utilities: true, false, echo, yes, sleep,
//! basename, dirname, env, printenv and tee. File operations run in a `Sandbox`.

mod common;

use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

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

fn version_line(util: &str) -> String {
    format!("{util} (agentcmd) {}\n", env!("CARGO_PKG_VERSION"))
}

/// Starts `agentcmd <util> <args>` with stdout piped, reads exactly `count` bytes and
/// leaves the process running (for commands with unbounded output).
fn first_bytes(util: &str, args: &[&str], count: usize) -> Vec<u8> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .arg(util)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn agentcmd");
    let mut bytes = vec![0u8; count];
    child
        .stdout
        .as_mut()
        .expect("stdout")
        .read_exact(&mut bytes)
        .expect("read output");
    let _ = child.kill();
    let _ = child.wait();
    bytes
}

// ---------------------------------------------------------------- true / false

#[test]
fn true_exits_zero_and_ignores_arguments() {
    let output = run("true", &["a", "-x", "--bogus"], b"");
    assert_eq!(status(&output), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn true_ignores_stdin() {
    assert_eq!(status(&run("true", &[], b"data\n")), 0);
}

#[test]
fn true_help_prints_usage_and_exits_zero() {
    let output = run("true", &["--help"], b"");
    assert_eq!(status(&output), 0);
    assert!(stdout(&output).starts_with("Usage: true "));
}

#[test]
fn true_help_next_to_other_arguments_is_ignored() {
    let output = run("true", &["--help", "x"], b"");
    assert_eq!(status(&output), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn true_version_uses_agentcmd_convention() {
    let output = run("true", &["--version"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), version_line("true"));
}

#[test]
fn false_exits_one_and_ignores_arguments() {
    let output = run("false", &["a", "b"], b"");
    assert_eq!(status(&output), 1);
    assert!(output.stdout.is_empty());
}

#[test]
fn false_help_prints_usage_and_still_exits_one() {
    let output = run("false", &["--help"], b"");
    assert_eq!(status(&output), 1);
    assert!(stdout(&output).starts_with("Usage: false "));
}

#[test]
fn false_version_still_exits_one() {
    let output = run("false", &["--version"], b"");
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), version_line("false"));
}

#[test]
fn false_help_next_to_other_arguments_is_silent() {
    let output = run("false", &["--help", "x"], b"");
    assert_eq!(status(&output), 1);
    assert!(output.stdout.is_empty());
}

// ---------------------------------------------------------------------- echo

#[test]
fn echo_joins_arguments_with_spaces_and_newline() {
    assert_eq!(stdout(&run("echo", &["a", "b", "c"], b"")), "a b c\n");
    assert_eq!(stdout(&run("echo", &[], b"")), "\n");
}

#[test]
fn echo_n_suppresses_the_newline() {
    assert_eq!(stdout(&run("echo", &["-n", "x"], b"")), "x");
}

#[test]
fn echo_e_interprets_escapes() {
    assert_eq!(stdout(&run("echo", &["-e", "a\\tb"], b"")), "a\tb\n");
    assert_eq!(stdout(&run("echo", &["-ne", "x\\101"], b"")), "xA");
}

#[test]
fn echo_backslash_c_stops_all_output() {
    assert_eq!(stdout(&run("echo", &["-e", "one\\c", "two"], b"")), "one");
}

#[test]
fn echo_capital_e_disables_escapes_and_is_last_wins() {
    assert_eq!(stdout(&run("echo", &["-e", "-E", "a\\tb"], b"")), "a\\tb\n");
}

#[test]
fn echo_unknown_option_letters_are_text() {
    assert_eq!(stdout(&run("echo", &["-x", "y"], b"")), "-x y\n");
    assert_eq!(stdout(&run("echo", &["-nx"], b"")), "-nx\n");
}

#[test]
fn echo_help_and_version_only_as_sole_argument() {
    let help = run("echo", &["--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: echo "));
    assert_eq!(stdout(&run("echo", &["--help", "x"], b"")), "--help x\n");
    assert_eq!(
        stdout(&run("echo", &["--version"], b"")),
        version_line("echo")
    );
}

#[test]
fn echo_ignores_stdin() {
    assert_eq!(stdout(&run("echo", &["z"], b"ignored\n")), "z\n");
}

// ----------------------------------------------------------------------- yes

#[test]
fn yes_repeats_y_by_default() {
    assert_eq!(first_bytes("yes", &[], 6), b"y\ny\ny\n");
}

#[test]
fn yes_joins_operands_with_spaces() {
    assert_eq!(first_bytes("yes", &["a", "b"], 8), b"a b\na b\n");
}

#[test]
fn yes_double_dash_makes_option_like_operand_text() {
    assert_eq!(first_bytes("yes", &["--", "-y"], 6), b"-y\n-y\n");
}

#[test]
fn yes_invalid_option_exits_one_with_usage_hint() {
    let output = run("yes", &["-x"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid option -- 'x'"));
    assert!(stderr(&output).contains("Try 'yes --help'"));
}

#[test]
fn yes_help_after_operand_and_version() {
    let help = run("yes", &["a", "--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: yes "));
    assert_eq!(
        stdout(&run("yes", &["--version"], b"")),
        version_line("yes")
    );
}

#[test]
fn yes_unknown_long_option_exits_one() {
    let output = run("yes", &["--bogus"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("unrecognized option '--bogus'"));
}

#[test]
fn yes_exits_zero_when_the_reader_goes_away() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .args(["yes", "a"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn agentcmd");
    let mut first = [0u8; 2];
    child
        .stdout
        .as_mut()
        .expect("stdout")
        .read_exact(&mut first)
        .expect("read");
    drop(child.stdout.take());
    let finished = child.wait_with_output().expect("wait");
    assert_eq!(status(&finished), 0);
    assert!(finished.stderr.is_empty());
}

// --------------------------------------------------------------------- sleep

#[test]
fn sleep_zero_and_sum_of_operands_succeed() {
    assert_eq!(status(&run("sleep", &["0"], b"")), 0);
    assert_eq!(status(&run("sleep", &["0", "0.01", "0.0s"], b"")), 0);
}

#[test]
fn sleep_suffixes_are_accepted() {
    for operand in ["0s", "0m", "0h", "0d"] {
        assert_eq!(status(&run("sleep", &[operand], b"")), 0, "{operand}");
    }
}

#[test]
fn sleep_without_operand_is_a_usage_error() {
    let output = run("sleep", &[], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("missing operand"));
}

#[test]
fn sleep_invalid_interval_is_reported() {
    let output = run("sleep", &["1x"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid time interval '1x'"));
}

#[test]
fn sleep_negative_number_is_an_option_error() {
    let output = run("sleep", &["-1"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid option -- '1'"));
}

#[test]
fn sleep_help_after_operand_and_version() {
    let help = run("sleep", &["0", "--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: sleep "));
    assert_eq!(
        stdout(&run("sleep", &["--version"], b"")),
        version_line("sleep")
    );
}

#[test]
fn sleep_infinity_does_not_return() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .args(["sleep", "infinity"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn agentcmd");
    std::thread::sleep(Duration::from_millis(300));
    assert!(child.try_wait().expect("try_wait").is_none());
    let _ = child.kill();
    let _ = child.wait();
}

// ------------------------------------------------------------------ basename

#[test]
fn basename_strips_directories_and_trailing_slashes() {
    assert_eq!(stdout(&run("basename", &["/usr/lib/"], b"")), "lib\n");
    assert_eq!(stdout(&run("basename", &["a/b"], b"")), "b\n");
}

#[test]
fn basename_two_operands_remove_suffix() {
    assert_eq!(stdout(&run("basename", &["a/b.txt", ".txt"], b"")), "b\n");
    assert_eq!(stdout(&run("basename", &[".txt", ".txt"], b"")), ".txt\n");
}

#[test]
fn basename_suffix_option_and_multiple_names() {
    assert_eq!(
        stdout(&run("basename", &["-s", ".c", "a.c", "b.c"], b"")),
        "a\nb\n"
    );
    assert_eq!(
        stdout(&run("basename", &["-a", "x/y", "z/w"], b"")),
        "y\nw\n"
    );
}

#[test]
fn basename_zero_terminates_records() {
    assert_eq!(stdout(&run("basename", &["-z", "a/b"], b"")), "b\0");
}

#[test]
fn basename_usage_errors_exit_one() {
    let missing = run("basename", &[], b"");
    assert_eq!(status(&missing), 1);
    assert!(stderr(&missing).contains("missing operand"));
    let extra = run("basename", &["a", "b", "c"], b"");
    assert_eq!(status(&extra), 1);
    assert!(stderr(&extra).contains("extra operand 'c'"));
    assert_eq!(status(&run("basename", &["-x", "a"], b"")), 1);
    assert_eq!(status(&run("basename", &["-s"], b"")), 1);
}

#[test]
fn basename_does_not_permute_options() {
    // After the first operand `-a` is a NAME, so this is the two-operand form.
    assert_eq!(stdout(&run("basename", &["a/b", "-a"], b"")), "b\n");
    assert_eq!(stdout(&run("basename", &["--", "-a"], b"")), "-a\n");
}

#[test]
fn basename_help_and_version() {
    assert!(stdout(&run("basename", &["--help"], b"")).starts_with("Usage: basename "));
    assert_eq!(
        stdout(&run("basename", &["--version"], b"")),
        version_line("basename")
    );
}

// ------------------------------------------------------------------- dirname

#[test]
fn dirname_returns_parent_directory() {
    assert_eq!(stdout(&run("dirname", &["a/b/c"], b"")), "a/b\n");
    assert_eq!(stdout(&run("dirname", &["file"], b"")), ".\n");
    assert_eq!(stdout(&run("dirname", &["/"], b"")), "/\n");
    assert_eq!(stdout(&run("dirname", &["//a"], b"")), "/\n");
}

#[test]
fn dirname_handles_each_operand() {
    assert_eq!(
        stdout(&run("dirname", &["a/b", "c", "/d"], b"")),
        "a\n.\n/\n"
    );
}

#[test]
fn dirname_zero_terminates_records() {
    assert_eq!(stdout(&run("dirname", &["-z", "a/b", "c"], b"")), "a\0.\0");
}

#[test]
fn dirname_option_is_permuted_after_operands() {
    assert_eq!(stdout(&run("dirname", &["a/b", "-z"], b"")), "a\0");
}

#[test]
fn dirname_usage_errors_exit_one() {
    let missing = run("dirname", &[], b"");
    assert_eq!(status(&missing), 1);
    assert!(stderr(&missing).contains("missing operand"));
    assert_eq!(status(&run("dirname", &["-x"], b"")), 1);
    assert_eq!(stdout(&run("dirname", &["--", "-a"], b"")), ".\n");
}

#[test]
fn dirname_help_and_version() {
    assert!(stdout(&run("dirname", &["--help"], b"")).starts_with("Usage: dirname "));
    assert_eq!(
        stdout(&run("dirname", &["--version"], b"")),
        version_line("dirname")
    );
}

// ----------------------------------------------------------------------- env

#[test]
fn env_prints_assignments_on_an_empty_environment() {
    assert_eq!(
        stdout(&run("env", &["-i", "A=1", "B=2"], b"")),
        "A=1\nB=2\n"
    );
}

#[test]
fn env_null_terminates_printed_entries() {
    assert_eq!(stdout(&run("env", &["-i", "-0", "A=1"], b"")), "A=1\0");
}

#[test]
fn env_later_assignment_replaces_earlier() {
    assert_eq!(stdout(&run("env", &["-i", "A=1", "A=2"], b"")), "A=2\n");
}

#[test]
fn env_unset_removes_variable() {
    assert_eq!(stdout(&run("env", &["-i", "A=1", "-u", "A"], b"")), "");
}

#[test]
fn env_lone_dash_means_ignore_environment() {
    assert_eq!(stdout(&run("env", &["-", "A=1"], b"")), "A=1\n");
}

#[test]
fn env_unset_with_equals_is_a_failure() {
    let output = run("env", &["-u", "A=B", "true"], b"");
    assert_eq!(status(&output), 125);
    assert!(stderr(&output).contains("cannot unset 'A=B'"));
}

#[test]
fn env_chdir_into_a_directory_then_print() {
    let sandbox = Sandbox::new();
    sandbox.mkdir("inner");
    let output = run_in(sandbox.path(), "env", &["-i", "-C", "inner", "X=1"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "X=1\n");
}

#[test]
fn env_chdir_missing_directory_exits_125() {
    let sandbox = Sandbox::new();
    let output = run_in(sandbox.path(), "env", &["-C", "missing-dir", "true"], b"");
    assert_eq!(status(&output), 125);
    assert!(stderr(&output).contains("cannot change directory to 'missing-dir'"));
}

#[test]
fn env_command_not_found_exits_127() {
    let output = run("env", &["agentcmd-no-such-command-xyz"], b"");
    assert_eq!(status(&output), 127);
    assert!(stderr(&output).contains("No such file or directory"));
}

#[test]
fn env_invalid_option_and_missing_argument_exit_125() {
    let invalid = run("env", &["-x"], b"");
    assert_eq!(status(&invalid), 125);
    assert!(stderr(&invalid).contains("invalid option -- 'x'"));
    assert_eq!(status(&run("env", &["-u"], b"")), 125);
    assert_eq!(status(&run("env", &["--nope"], b"")), 125);
}

#[test]
fn env_null_with_command_is_a_usage_error() {
    let output = run("env", &["-0", "true"], b"");
    assert_eq!(status(&output), 125);
    assert!(stderr(&output).contains("cannot specify --null (-0) with command"));
}

#[test]
fn env_split_string_expands_into_options_and_command() {
    let output = run("env", &["-S", "-i A=9 agentcmd-no-such-command-xyz"], b"");
    assert_eq!(status(&output), 127);
}

#[test]
fn env_help_and_version() {
    let help = run("env", &["--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: env "));
    assert_eq!(
        stdout(&run("env", &["--version"], b"")),
        version_line("env")
    );
}

#[cfg(unix)]
#[test]
fn env_runs_command_with_the_new_environment() {
    let output = run(
        "env",
        &["-i", "A=7", "/bin/sh", "-c", "printf %s \"$A\""],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "7");
}

#[cfg(unix)]
#[test]
fn env_passes_through_the_command_exit_status() {
    let output = run("env", &["-i", "/bin/sh", "-c", "exit 4"], b"");
    assert_eq!(status(&output), 4);
}

#[cfg(unix)]
#[test]
fn env_non_executable_command_exits_126() {
    let output = run("env", &["/dev/null"], b"");
    assert_eq!(status(&output), 126);
}

#[cfg(unix)]
#[test]
fn env_split_string_runs_quoted_command() {
    let output = run("env", &["-S", "-i A=9 /bin/sh -c 'exit 6'"], b"");
    assert_eq!(status(&output), 6);
}

// ------------------------------------------------------------------ printenv

#[test]
fn printenv_prints_value_of_named_variable() {
    let path = std::env::var("PATH").expect("PATH is set in the test environment");
    assert_eq!(
        stdout(&run("printenv", &["PATH"], b"")),
        format!("{path}\n")
    );
}

#[test]
fn printenv_missing_variable_exits_one() {
    let output = run("printenv", &["AGENTCMD_NO_SUCH_VARIABLE_XYZ"], b"");
    assert_eq!(status(&output), 1);
    assert!(output.stdout.is_empty());
}

#[test]
fn printenv_null_terminates_values() {
    let path = std::env::var("PATH").expect("PATH is set in the test environment");
    assert_eq!(
        stdout(&run("printenv", &["-0", "PATH"], b"")),
        format!("{path}\0")
    );
}

#[test]
fn printenv_without_names_lists_the_environment() {
    let output = run("printenv", &[], b"");
    assert_eq!(status(&output), 0);
    // Windows reports the variable as `Path`, so compare case-insensitively.
    assert!(stdout(&output)
        .lines()
        .any(|line| line.to_ascii_uppercase().starts_with("PATH=")));
}

#[test]
fn printenv_option_after_a_name_is_a_name() {
    let path = std::env::var("PATH").expect("PATH is set in the test environment");
    let output = run("printenv", &["PATH", "-0"], b"");
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), format!("{path}\n"));
}

#[test]
fn printenv_invalid_option_exits_one() {
    let output = run("printenv", &["-x"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid option -- 'x'"));
}

#[test]
fn printenv_help_and_version() {
    assert!(stdout(&run("printenv", &["--help"], b"")).starts_with("Usage: printenv "));
    assert_eq!(
        stdout(&run("printenv", &["--version"], b"")),
        version_line("printenv")
    );
}

// ----------------------------------------------------------------------- tee

#[test]
fn tee_copies_stdin_to_stdout_and_file() {
    let sandbox = Sandbox::new();
    let output = run_in(sandbox.path(), "tee", &["out.txt"], b"hello\n");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "hello\n");
    assert_eq!(sandbox.read("out.txt"), b"hello\n");
}

#[test]
fn tee_truncates_by_default_and_appends_with_a() {
    let sandbox = Sandbox::new();
    sandbox.write("f.txt", b"old contents\n");
    run_in(sandbox.path(), "tee", &["f.txt"], b"new\n");
    assert_eq!(sandbox.read("f.txt"), b"new\n");
    run_in(sandbox.path(), "tee", &["-a", "f.txt"], b"more\n");
    assert_eq!(sandbox.read("f.txt"), b"new\nmore\n");
}

#[test]
fn tee_reports_a_bad_file_and_still_writes_the_others() {
    let sandbox = Sandbox::new();
    let output = run_in(sandbox.path(), "tee", &["no-dir/x", "ok.txt"], b"data\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("no-dir/x"));
    assert_eq!(stdout(&output), "data\n");
    assert_eq!(sandbox.read("ok.txt"), b"data\n");
}

#[test]
fn tee_dash_is_an_ordinary_file_name() {
    let sandbox = Sandbox::new();
    run_in(sandbox.path(), "tee", &["-"], b"d\n");
    assert_eq!(sandbox.read("-"), b"d\n");
}

#[test]
fn tee_double_dash_allows_option_like_file_names() {
    let sandbox = Sandbox::new();
    run_in(sandbox.path(), "tee", &["--", "-dash"], b"x\n");
    assert_eq!(sandbox.read("-dash"), b"x\n");
}

#[test]
fn tee_writes_each_chunk_before_input_ends() {
    let sandbox = Sandbox::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .args(["tee", "live.txt"])
        .current_dir(sandbox.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn agentcmd");
    let mut input = child.stdin.take().expect("stdin");
    input.write_all(b"ab\n").expect("write");
    input.flush().expect("flush");
    let mut echoed = [0u8; 3];
    child
        .stdout
        .as_mut()
        .expect("stdout")
        .read_exact(&mut echoed)
        .expect("read echoed chunk while stdin is still open");
    assert_eq!(&echoed, b"ab\n");
    drop(input);
    assert_eq!(status(&child.wait_with_output().expect("wait")), 0);
    assert_eq!(sandbox.read("live.txt"), b"ab\n");
}

#[test]
fn tee_output_error_modes() {
    let sandbox = Sandbox::new();
    let warn = run_in(
        sandbox.path(),
        "tee",
        &["--output-error=warn", "w.txt"],
        b"w\n",
    );
    assert_eq!(status(&warn), 0);
    let prefix = run_in(
        sandbox.path(),
        "tee",
        &["--output-error=exit-", "p.txt"],
        b"p\n",
    );
    assert_eq!(status(&prefix), 0);
    assert_eq!(sandbox.read("p.txt"), b"p\n");
}

#[test]
fn tee_invalid_output_error_mode_is_rejected() {
    let output = run("tee", &["--output-error=bogus"], b"x\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid argument 'bogus' for '--output-error'"));
    assert!(stderr(&output).contains("Valid arguments are:"));
}

#[test]
fn tee_accepts_p_and_i_options() {
    let sandbox = Sandbox::new();
    assert_eq!(
        status(&run_in(sandbox.path(), "tee", &["-p", "a.txt"], b"a\n")),
        0
    );
    assert_eq!(
        status(&run_in(sandbox.path(), "tee", &["-i", "b.txt"], b"b\n")),
        0
    );
    assert_eq!(sandbox.read("b.txt"), b"b\n");
}

#[test]
fn tee_unknown_option_exits_one() {
    let output = run("tee", &["-x"], b"x\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid option -- 'x'"));
}

#[test]
fn tee_help_and_version() {
    assert!(stdout(&run("tee", &["--help"], b"")).starts_with("Usage: tee "));
    assert_eq!(
        stdout(&run("tee", &["--version"], b"")),
        version_line("tee")
    );
}

// ------------------------------------------------- additional coverage (long options)

#[test]
fn true_accepts_double_dash_dash_and_missing_files() {
    assert_eq!(status(&run("true", &["--", "-"], b"")), 0);
    assert_eq!(status(&run("true", &["/nonexistent/agentcmd/x"], b"")), 0);
    assert!(stderr(&run("true", &["--version"], b"")).is_empty());
}

#[test]
fn false_accepts_double_dash_stdin_and_missing_files() {
    assert_eq!(status(&run("false", &["--"], b"")), 1);
    assert_eq!(
        status(&run("false", &["/nonexistent/agentcmd/x"], b"data\n")),
        1
    );
    assert!(stderr(&run("false", &["--version"], b"")).is_empty());
}

#[test]
fn yes_long_prefix_of_version_is_accepted() {
    let output = run("yes", &["--vers"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), version_line("yes"));
}

#[test]
fn sleep_sums_suffixed_operands_quickly() {
    assert_eq!(status(&run("sleep", &["0m", "0h", "0d"], b"")), 0);
}

#[test]
fn basename_long_multiple_zero_and_suffix_forms() {
    assert_eq!(
        stdout(&run(
            "basename",
            &["--multiple", "--zero", "a/b", "c/d"],
            b""
        )),
        "b\0d\0"
    );
    assert_eq!(
        stdout(&run("basename", &["--suffix", ".c", "x.c"], b"")),
        "x\n"
    );
}

#[test]
fn dirname_long_zero_and_dot_forms() {
    assert_eq!(stdout(&run("dirname", &["--zero", "/x/y"], b"")), "/x\0");
    assert_eq!(
        stdout(&run("dirname", &[".", "..", "./"], b"")),
        ".\n.\n.\n"
    );
    assert_eq!(stdout(&run("dirname", &["a//"], b"")), ".\n");
}

#[test]
fn printenv_long_null_option() {
    let path = std::env::var("PATH").expect("PATH is set in the test environment");
    assert_eq!(
        stdout(&run("printenv", &["--null", "PATH"], b"")),
        format!("{path}\0")
    );
}

#[test]
fn env_long_ignore_environment_and_null() {
    assert_eq!(
        stdout(&run("env", &["--ignore-environment", "A=1"], b"")),
        "A=1\n"
    );
    assert_eq!(
        stdout(&run("env", &["--ignore-environment", "--null", "A=1"], b"")),
        "A=1\0"
    );
}

#[test]
fn env_long_chdir_option() {
    let sandbox = Sandbox::new();
    sandbox.mkdir("inner");
    let output = run_in(sandbox.path(), "env", &["--chdir=inner", "-i", "X=1"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "X=1\n");
}

#[cfg(unix)]
#[test]
fn env_long_unset_option_removes_from_inherited_environment() {
    let output = run(
        "env",
        &["--unset=HOME", "/bin/sh", "-c", "echo ${HOME-unset}"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "unset\n");
}

#[test]
fn env_long_debug_writes_to_stderr_only() {
    let output = run("env", &["--debug", "-i", "A=1"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "A=1\n");
    assert!(stderr(&output).contains("setenv:"));
}

#[test]
fn env_long_split_string_option() {
    assert_eq!(
        stdout(&run("env", &["--split-string=-i A=3"], b"")),
        "A=3\n"
    );
}

#[test]
fn env_long_help_prefix_is_accepted() {
    let output = run("env", &["--he"], b"");
    assert_eq!(status(&output), 0);
    assert!(stdout(&output).starts_with("Usage: env "));
}

#[test]
fn tee_long_append_and_ignore_interrupts() {
    let sandbox = Sandbox::new();
    sandbox.write("log.txt", b"old\n");
    let output = run_in(
        sandbox.path(),
        "tee",
        &["--append", "--ignore-interrupts", "log.txt"],
        b"more\n",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("log.txt"), b"old\nmore\n");
}
