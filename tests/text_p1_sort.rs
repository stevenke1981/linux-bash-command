//! CLI tests for the phase 1 text commands: sort, uniq, comm and xargs.

mod common;

use common::{run, run_in, Sandbox};
use std::process::Output;

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Numbers in a pseudo-random order, one per line (deterministic, no RNG needed).
fn scrambled_numbers(count: u64) -> String {
    let mut value: u64 = 12345;
    let mut text = String::new();
    for _ in 0..count {
        value = value
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        text.push_str(&format!("{}\n", (value >> 33) % 100_000));
    }
    text
}

#[test]
fn sort_help_and_version_exit_zero() {
    let help = run("sort", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).starts_with("Usage: sort [OPTION]... [FILE]..."));
    let version = run("sort", &["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(stdout(&version).starts_with("sort (agentcmd) "));
}

#[test]
fn sort_unknown_option_is_usage_error() {
    let output = run("sort", &["--bogus"], b"");
    assert_eq!(code(&output), 2);
    let text = stderr(&output);
    assert!(text.contains("unrecognized option '--bogus'"), "{text}");
    assert!(text.contains("Try 'sort --help'"), "{text}");
}

#[test]
fn sort_missing_file_exits_two_and_still_sorts_others() {
    let sandbox = Sandbox::new();
    sandbox.write("ok.txt", b"b\na\n");
    let output = run_in(sandbox.path(), "sort", &["missing.txt", "ok.txt"], b"");
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "a\nb\n");
    assert!(stderr(&output).contains("sort: cannot read: missing.txt: No such file or directory"));
}

#[test]
fn sort_reads_stdin_and_adds_final_newline() {
    let output = run("sort", &[], b"b\na");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn sort_merges_several_files_in_one_order() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"pear\nfig\n");
    sandbox.write("b.txt", b"apple\nfig\n");
    let output = run_in(sandbox.path(), "sort", &["a.txt", "b.txt"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "apple\nfig\nfig\npear\n");
}

#[test]
fn sort_check_reports_first_disorder_with_line_number() {
    let sandbox = Sandbox::new();
    sandbox.write("f.txt", b"a\nc\nb\nd\n");
    let output = run_in(sandbox.path(), "sort", &["-c", "f.txt"], b"");
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("sort: f.txt:3: disorder: b"));
    let quiet = run_in(sandbox.path(), "sort", &["-C", "f.txt"], b"");
    assert_eq!(code(&quiet), 1);
    assert!(stderr(&quiet).is_empty());
}

#[test]
fn sort_check_extra_operand_is_usage_error() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"a\n");
    sandbox.write("b.txt", b"b\n");
    let output = run_in(sandbox.path(), "sort", &["-c", "a.txt", "b.txt"], b"");
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("extra operand 'b.txt' not allowed with -c"));
}

#[test]
fn sort_output_option_may_name_the_input() {
    let sandbox = Sandbox::new();
    sandbox.write("data.txt", b"c\na\nb\n");
    let output = run_in(sandbox.path(), "sort", &["-o", "data.txt", "data.txt"], b"");
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).is_empty());
    assert_eq!(sandbox.read("data.txt"), b"a\nb\nc\n");
}

#[test]
fn sort_external_merge_matches_in_memory_sort() {
    let sandbox = Sandbox::new();
    let input = scrambled_numbers(5000);
    sandbox.write("nums.txt", input.as_bytes());
    let in_memory = run_in(sandbox.path(), "sort", &["-n", "nums.txt"], b"");
    let spilled = run_in(sandbox.path(), "sort", &["-n", "-S", "1K", "nums.txt"], b"");
    assert_eq!(code(&spilled), 0);
    assert_eq!(stdout(&spilled), stdout(&in_memory));
    let lines: Vec<i64> = stdout(&spilled)
        .lines()
        .map(|l| l.parse().unwrap())
        .collect();
    assert_eq!(lines.len(), 5000);
    assert!(lines.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn sort_unique_external_merge_drops_duplicates() {
    let sandbox = Sandbox::new();
    let input = scrambled_numbers(3000);
    sandbox.write("nums.txt", input.as_bytes());
    let output = run_in(
        sandbox.path(),
        "sort",
        &["-n", "-u", "-S", "512", "nums.txt"],
        b"",
    );
    assert_eq!(code(&output), 0);
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    let mut expected = lines.clone();
    expected.dedup();
    assert_eq!(lines, expected);
}

#[test]
fn sort_temp_directory_must_exist_when_spilling() {
    let sandbox = Sandbox::new();
    sandbox.write("nums.txt", scrambled_numbers(200).as_bytes());
    let output = run_in(
        sandbox.path(),
        "sort",
        &["-T", "no-such-dir", "-S", "64", "nums.txt"],
        b"",
    );
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("cannot create temporary file in 'no-such-dir'"));
}

#[test]
fn sort_zero_terminated_round_trip() {
    let output = run("sort", &["-z"], b"b\0a\0\nz\0");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output).as_bytes(), b"\nz\0a\0b\0");
}

#[test]
fn sort_key_and_mode_errors_exit_two() {
    let bad_key = run("sort", &["-k", "0"], b"a\n");
    assert_eq!(code(&bad_key), 2);
    assert!(stderr(&bad_key).contains("field number is zero"));
    let incompatible = run("sort", &["-nM"], b"a\n");
    assert_eq!(code(&incompatible), 2);
    assert!(stderr(&incompatible).contains("options '-Mn' are incompatible"));
    let tab = run("sort", &["-t", "ab"], b"a\n");
    assert_eq!(code(&tab), 2);
    assert!(stderr(&tab).contains("multi-character tab 'ab'"));
}

#[test]
fn sort_size_and_batch_arguments_are_validated() {
    let bad_size = run("sort", &["-S", "3x"], b"a\n");
    assert_eq!(code(&bad_size), 2);
    let small_batch = run("sort", &["--batch-size=1"], b"a\n");
    assert_eq!(code(&small_batch), 2);
    assert!(stderr(&small_batch).contains("minimum --batch-size argument is '2'"));
    let ok = run(
        "sort",
        &["-S", "2K", "--batch-size=4", "--parallel=2"],
        b"b\na\n",
    );
    assert_eq!(stdout(&ok), "a\nb\n");
}

#[test]
fn sort_files0_from_reads_nul_separated_names() {
    let sandbox = Sandbox::new();
    sandbox.write("names", b"two\0one\0");
    sandbox.write("one", b"1\n");
    sandbox.write("two", b"2\n");
    let output = run_in(sandbox.path(), "sort", &["--files0-from=names"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "1\n2\n");
}

#[test]
fn sort_debug_option_is_unsupported() {
    let output = run("sort", &["--debug"], b"a\n");
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("option '--debug' is not supported by agentcmd"));
}

#[test]
fn sort_random_grouping_keeps_equal_keys_together() {
    let sandbox = Sandbox::new();
    sandbox.write("seed", b"0123456789abcdef");
    sandbox.write("in.txt", b"x\ny\nx\nz\nx\ny\n");
    let output = run_in(
        sandbox.path(),
        "sort",
        &["-R", "--random-source=seed", "in.txt"],
        b"",
    );
    assert_eq!(code(&output), 0);
    let lines: Vec<String> = stdout(&output).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 6);
    let x_positions: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| *l == "x")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(x_positions.last().unwrap() - x_positions[0], 2);
}

#[test]
fn sort_accepts_buffer_and_parallel_options() {
    let output = run("sort", &["-S", "1M", "--parallel=1", "-r"], b"a\nc\nb\n");
    assert_eq!(stdout(&output), "c\nb\na\n");
}

#[test]
fn uniq_help_version_and_unknown_option() {
    let help = run("uniq", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).starts_with("Usage: uniq [OPTION]... [INPUT [OUTPUT]]"));
    let version = run("uniq", &["--version"], b"");
    assert!(stdout(&version).starts_with("uniq (agentcmd) "));
    let bad = run("uniq", &["--bogus"], b"");
    assert_eq!(code(&bad), 1);
    assert!(stderr(&bad).contains("unrecognized option '--bogus'"));
}

#[test]
fn uniq_counts_adjacent_runs_from_stdin() {
    let output = run("uniq", &["-c"], b"a\na\nb\n");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "      2 a\n      1 b\n");
}

#[test]
fn uniq_repeated_and_unique_filters() {
    let input = b"a\na\nb\nc\nc\n";
    assert_eq!(stdout(&run("uniq", &["-d"], input)), "a\nc\n");
    assert_eq!(stdout(&run("uniq", &["-u"], input)), "b\n");
}

#[test]
fn uniq_all_repeated_and_group_modes() {
    let input = b"a\na\nb\n";
    assert_eq!(stdout(&run("uniq", &["-D"], input)), "a\na\n");
    assert_eq!(
        stdout(&run("uniq", &["--group=append"], input)),
        "a\na\n\nb\n\n"
    );
    let mixed = run("uniq", &["--group", "-c"], input);
    assert_eq!(code(&mixed), 1);
    assert!(stderr(&mixed).contains("--group is mutually exclusive with -c/-d/-D/-u"));
}

#[test]
fn uniq_skips_fields_and_chars_for_comparison() {
    assert_eq!(
        stdout(&run("uniq", &["-f", "1"], b"x a\ny a\nz b\n")),
        "x a\nz b\n"
    );
    assert_eq!(
        stdout(&run("uniq", &["-s", "1", "-w", "2"], b"abc\nxbc\nxyz\n")),
        "abc\nxyz\n"
    );
}

#[test]
fn uniq_input_and_output_files() {
    let sandbox = Sandbox::new();
    sandbox.write("in.txt", b"p\np\nq\n");
    let output = run_in(sandbox.path(), "uniq", &["in.txt", "out.txt"], b"");
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).is_empty());
    assert_eq!(sandbox.read("out.txt"), b"p\nq\n");
    let missing = run_in(sandbox.path(), "uniq", &["nofile.txt"], b"");
    assert_eq!(code(&missing), 1);
    assert!(stderr(&missing).contains("uniq: nofile.txt: No such file or directory"));
}

#[test]
fn uniq_rejects_extra_operands_and_bad_numbers() {
    let extra = run("uniq", &["a", "b", "c"], b"");
    assert_eq!(code(&extra), 1);
    assert!(stderr(&extra).contains("extra operand 'c'"));
    let bad = run("uniq", &["-f", "-1"], b"a\n");
    assert_eq!(code(&bad), 1);
    assert!(stderr(&bad).contains("-1: invalid number of fields to skip"));
}

#[test]
fn uniq_zero_terminated_records() {
    let output = run("uniq", &["-z"], b"a\0a\0b\0");
    assert_eq!(stdout(&output).as_bytes(), b"a\0b\0");
}

#[test]
fn comm_help_version_and_operand_errors() {
    let help = run("comm", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).starts_with("Usage: comm [OPTION]... FILE1 FILE2"));
    let missing = run("comm", &["one"], b"");
    assert_eq!(code(&missing), 1);
    assert!(stderr(&missing).contains("missing operand after 'one'"));
    let extra = run("comm", &["a", "b", "c"], b"");
    assert!(stderr(&extra).contains("extra operand 'c'"));
}

#[test]
fn comm_prints_three_columns_from_files() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"a\nb\nc\n");
    sandbox.write("b.txt", b"b\nc\nd\n");
    let output = run_in(sandbox.path(), "comm", &["a.txt", "b.txt"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "a\n\t\tb\n\t\tc\n\td\n");
    let common = run_in(sandbox.path(), "comm", &["-12", "a.txt", "b.txt"], b"");
    assert_eq!(stdout(&common), "b\nc\n");
}

#[test]
fn comm_reads_stdin_and_reports_unsorted_input() {
    let output = run("comm", &["-", "-"], b"a\n");
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "a\n");
    assert!(stderr(&output).contains("-: Bad file descriptor"));
}

#[test]
fn comm_check_order_stops_at_first_disorder() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"a\nb\nc\n");
    sandbox.write("c.txt", b"b\na\n");
    let output = run_in(
        sandbox.path(),
        "comm",
        &["--check-order", "a.txt", "c.txt"],
        b"",
    );
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "a\n\t\tb\n");
    assert!(stderr(&output).contains("comm: file 2 is not in sorted order"));
}

#[test]
fn comm_total_and_custom_delimiter() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"a\nb\n");
    sandbox.write("b.txt", b"b\nc\n");
    let output = run_in(
        sandbox.path(),
        "comm",
        &["--total", "--output-delimiter=:", "a.txt", "b.txt"],
        b"",
    );
    assert_eq!(stdout(&output), "a\n::b\n:c\n1:1:1:total\n");
}

#[test]
fn comm_rejects_conflicting_delimiters() {
    let output = run(
        "comm",
        &["--output-delimiter=:", "--output-delimiter=;", "a", "b"],
        b"",
    );
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("multiple output delimiters specified"));
}

#[test]
fn xargs_help_version_and_unknown_option() {
    let help = run("xargs", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).starts_with("Usage: xargs [OPTION]... COMMAND [INITIAL-ARGS]..."));
    let version = run("xargs", &["--version"], b"");
    assert!(stdout(&version).starts_with("xargs (agentcmd) "));
    let bad = run("xargs", &["-Z"], b"");
    assert_eq!(code(&bad), 1);
    assert!(stderr(&bad).contains("invalid option -- 'Z'"));
}

#[test]
fn xargs_runs_command_with_items_from_stdin() {
    let output = run("xargs", &["-n", "2"], b"a b c\nd\n");
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "a b\nc d\n");
}

#[test]
fn xargs_null_and_delimiter_inputs() {
    assert_eq!(stdout(&run("xargs", &["-0"], b"x y\0z\0")), "x y z\n");
    assert_eq!(stdout(&run("xargs", &["-d", ","], b"a,b")), "a b\n");
}

#[test]
fn xargs_replace_runs_once_per_line() {
    let output = run("xargs", &["-I", "{}", "echo", "<{}>"], b"one\n  two\n");
    assert_eq!(stdout(&output), "<one>\n<two>\n");
}

#[test]
#[cfg(unix)]
fn xargs_exit_status_mapping() {
    let missing = run("xargs", &["agentcmd-no-such-command-xyz"], b"a\n");
    assert_eq!(code(&missing), 127);
    let failing = run("xargs", &["sh", "-c", "exit 1"], b"a\n");
    assert_eq!(code(&failing), 123);
    let stop = run(
        "xargs",
        &["-n", "1", "sh", "-c", "echo ran; exit 255"],
        b"a\nb\n",
    );
    assert_eq!(code(&stop), 124);
    assert_eq!(stdout(&stop), "ran\n");
}

#[test]
fn xargs_quotes_and_unmatched_quote() {
    assert_eq!(
        stdout(&run("xargs", &[], b"'a b' \"c\" d\\ e\n")),
        "a b c d e\n"
    );
    let bad = run("xargs", &[], b"'open\n");
    assert_eq!(code(&bad), 1);
    assert!(stderr(&bad).contains("unmatched single quote"));
}

#[test]
fn xargs_empty_input_runs_once_unless_no_run_if_empty() {
    assert_eq!(stdout(&run("xargs", &[], b"")), "\n");
    assert_eq!(stdout(&run("xargs", &["-r"], b"")), "");
}

#[test]
fn xargs_arg_file_and_exit_on_overflow() {
    let sandbox = Sandbox::new();
    sandbox.write("items", b"p q\n");
    let from_file = run_in(sandbox.path(), "xargs", &["-a", "items"], b"");
    assert_eq!(stdout(&from_file), "p q\n");
    let overflow = run("xargs", &["-x", "-s", "10"], b"aaaaaaaaaa bbbbbbbbbb\n");
    assert_eq!(code(&overflow), 1);
}
