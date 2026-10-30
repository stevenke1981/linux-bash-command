//! CLI tests for the Phase 3 text utilities: join, od, split, csplit, fmt and pr. Every
//! test works in a `Sandbox` directory.

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

fn version_line(util: &str) -> String {
    format!("{util} (agentcmd) {}\n", env!("CARGO_PKG_VERSION"))
}

fn join_files(sandbox: &Sandbox) {
    sandbox.write("a", b"1 apple\n2 banana\n3 cherry\n5 elder\n");
    sandbox.write("b", b"1 red\n3 green\n4 blue\n5 black\n");
}

// join

#[test]
fn join_pairs_on_first_field() {
    let sandbox = Sandbox::new();
    join_files(&sandbox);
    let output = run_in(sandbox.path(), "join", &["a", "b"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(
        stdout(&output),
        "1 apple red\n3 cherry green\n5 elder black\n"
    );
}

#[test]
fn join_prints_unpaired_lines_with_a() {
    let sandbox = Sandbox::new();
    join_files(&sandbox);
    let output = run_in(sandbox.path(), "join", &["-a", "1", "a", "b"], b"");
    assert_eq!(
        stdout(&output),
        "1 apple red\n2 banana\n3 cherry green\n5 elder black\n"
    );
}

#[test]
fn join_output_format_and_empty_fill() {
    let sandbox = Sandbox::new();
    join_files(&sandbox);
    let output = run_in(
        sandbox.path(),
        "join",
        &["-a", "2", "-e", "-", "-o", "0,2.2,1.2", "a", "b"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(
        stdout(&output),
        "1 red apple\n3 green cherry\n4 blue -\n5 black elder\n"
    );
}

#[test]
fn join_reports_unsorted_input_with_check_order() {
    let sandbox = Sandbox::new();
    sandbox.write("u1", b"1 a\n3 c\n2 b\n");
    sandbox.write("u2", b"1 x\n2 y\n3 z\n");
    let output = run_in(sandbox.path(), "join", &["--check-order", "u1", "u2"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("join: u1:3: is not sorted: 2 b"));
    assert_eq!(stdout(&output), "1 a x\n");
}

#[test]
fn join_missing_operand_and_both_stdin_are_usage_errors() {
    let output = run("join", &["only"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("missing operand after 'only'"));
    let output = run("join", &["-", "-"], b"1 a\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("both files cannot be standard input"));
}

#[test]
fn join_uses_nul_lines_with_z_and_reads_stdin() {
    let output = run("join", &["-z", "-", "/dev/null"], b"k1 a\0");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "");
    let output = run("join", &["-t", ":", "-", "-"], b"");
    assert_eq!(status(&output), 1);
}

#[test]
fn join_help_and_version() {
    let output = run("join", &["--help"], b"");
    assert_eq!(status(&output), 0);
    assert!(stdout(&output).starts_with("Usage: join [OPTION]... FILE1 FILE2"));
    let output = run("join", &["--version"], b"");
    assert_eq!(stdout(&output), version_line("join"));
}

// od

#[test]
fn od_default_is_octal_two_byte_units() {
    let output = run("od", &[], b"AB");
    assert_eq!(stdout(&output), "0000000 041101\n0000002\n");
}

#[test]
fn od_address_radix_and_hex_bytes() {
    let output = run("od", &["-A", "d", "-t", "x1"], b"AB");
    assert_eq!(stdout(&output), "0000000 41 42\n0000002\n");
    let output = run("od", &["-A", "n", "-t", "x1"], b"AB");
    assert_eq!(stdout(&output), " 41 42\n");
}

#[test]
fn od_character_format_with_ascii_column() {
    let output = run("od", &["-c", "-N", "3"], b"a\n\0");
    assert_eq!(stdout(&output), "0000000   a  \\n  \\0\n0000003\n");
    let output = run("od", &["-t", "x1z", "-A", "x"], b"abc");
    assert_eq!(
        stdout(&output),
        format!("000000 61 62 63{}  >abc<\n000003\n", " ".repeat(39))
    );
}

#[test]
fn od_marks_repeated_lines_with_star() {
    let input = vec![b'x'; 40];
    let output = run("od", &["-t", "x1", "-w4"], &input);
    let text = stdout(&output);
    assert!(text.starts_with("0000000 78 78 78 78\n*\n"), "{text}");
    let output = run("od", &["-v", "-t", "x1", "-w4"], &input);
    assert!(!stdout(&output).contains("\n*\n"));
}

#[test]
fn od_strings_need_nul_termination() {
    let output = run("od", &["-S", "3"], b"abc\0ab\n");
    assert_eq!(stdout(&output), "0000000 abc\n");
}

#[test]
fn od_skip_and_limit_offsets() {
    let output = run(
        "od",
        &["-j", "2", "-N", "2", "-A", "d", "-t", "c"],
        b"abcdef",
    );
    assert_eq!(stdout(&output), "0000002   c   d\n0000004\n");
}

#[test]
fn od_usage_errors_exit_one() {
    let output = run("od", &["-t", "z"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid character 'z' in type string 'z'"));
    let output = run("od", &["--bogus"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("unrecognized option '--bogus'"));
}

#[test]
fn od_missing_file_reports_and_continues() {
    let sandbox = Sandbox::new();
    sandbox.write("here", b"A");
    let output = run_in(
        sandbox.path(),
        "od",
        &["-A", "n", "-t", "x1", "missing", "here"],
        b"",
    );
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("missing: No such file or directory"));
    assert_eq!(stdout(&output), " 41\n");
}

#[test]
fn od_help_and_version() {
    let output = run("od", &["--help"], b"");
    assert!(stdout(&output).starts_with("Usage: od [OPTION]... [FILE]..."));
    let output = run("od", &["--version"], b"");
    assert_eq!(stdout(&output), version_line("od"));
}

// split

#[test]
fn split_default_keeps_small_input_in_one_piece() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n");
    let output = run_in(sandbox.path(), "split", &["in"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("xaa"), b"1\n2\n");
    assert!(!sandbox.path().join("xab").exists());
}

#[test]
fn split_lines_and_numeric_suffixes() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n3\n4\n5\n");
    let output = run_in(sandbox.path(), "split", &["-l", "2", "-d", "in", "p-"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("p-00"), b"1\n2\n");
    assert_eq!(sandbox.read("p-02"), b"5\n");
}

#[test]
fn split_bytes_and_additional_suffix() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"abcdefg");
    let output = run_in(
        sandbox.path(),
        "split",
        &["-b", "3", "--additional-suffix=.txt", "in"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("xaa.txt"), b"abc");
    assert_eq!(sandbox.read("xac.txt"), b"g");
}

#[test]
fn split_chunks_spread_remainder_over_first_pieces() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"0123456789");
    let output = run_in(sandbox.path(), "split", &["-n", "3", "in"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("xaa").len(), 4);
    assert_eq!(sandbox.read("xab").len(), 3);
    assert_eq!(sandbox.read("xac").len(), 3);
}

#[test]
fn split_round_robin_chunk_to_stdout() {
    let output = run("split", &["-n", "r/2/3", "-"], b"1\n2\n3\n4\n5\n6\n");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "2\n5\n");
}

#[test]
fn split_line_bytes_keeps_lines_whole_when_possible() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"ab\ncd\nef\n");
    let output = run_in(sandbox.path(), "split", &["-C", "5", "in"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("xaa"), b"ab\n");
    assert_eq!(sandbox.read("xab"), b"cd\n");
    assert_eq!(sandbox.read("xac"), b"ef\n");
}

#[test]
fn split_usage_errors_and_unsupported_filter() {
    let output = run("split", &["-l", "0"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid number of lines: '0'"));
    let output = run("split", &["-l", "2", "-b", "3"], b"");
    assert!(stderr(&output).contains("cannot split in more than one way"));
    let output = run("split", &["--filter=cat"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("option '--filter' is not supported by agentcmd"));
}

#[test]
fn split_verbose_names_each_file() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n");
    let output = run_in(
        sandbox.path(),
        "split",
        &["--verbose", "-l", "1", "in"],
        b"",
    );
    assert_eq!(
        stdout(&output),
        "creating file 'xaa'\ncreating file 'xab'\n"
    );
}

#[test]
fn split_help_and_version() {
    let output = run("split", &["--help"], b"");
    assert!(stdout(&output).starts_with("Usage: split [OPTION]... [FILE [PREFIX]]"));
    let output = run("split", &["--version"], b"");
    assert_eq!(stdout(&output), version_line("split"));
}

// csplit

#[test]
fn csplit_splits_at_line_numbers_and_prints_counts() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    let output = run_in(sandbox.path(), "csplit", &["in", "3", "7"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "4\n8\n9\n");
    assert_eq!(sandbox.read("xx00"), b"1\n2\n");
    assert_eq!(sandbox.read("xx02"), b"7\n8\n9\n10\n");
}

#[test]
fn csplit_regex_with_offset_and_suppressed_match() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    let output = run_in(
        sandbox.path(),
        "csplit",
        &["-s", "--suppress-matched", "in", "/5/+2"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("xx00"), b"1\n2\n3\n4\n5\n6\n");
    assert_eq!(sandbox.read("xx01"), b"8\n9\n10\n");
}

#[test]
fn csplit_skip_pattern_discards_leading_section() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n3\n4\n5\n6\n");
    let output = run_in(sandbox.path(), "csplit", &["-s", "in", "%4%"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("xx00"), b"4\n5\n6\n");
    assert!(!sandbox.path().join("xx01").exists());
}

#[test]
fn csplit_repeat_count_and_custom_names() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n3\n4\n5\n6\n");
    let output = run_in(
        sandbox.path(),
        "csplit",
        &["-f", "pre", "-b", "%03d.txt", "-s", "in", "2", "{2}"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("pre000.txt"), b"1\n");
    assert_eq!(sandbox.read("pre002.txt"), b"4\n5\n");
    assert_eq!(sandbox.read("pre003.txt"), b"6\n");
}

#[test]
fn csplit_errors_remove_files_unless_kept() {
    let sandbox = Sandbox::new();
    sandbox.write("in", b"1\n2\n3\n");
    let output = run_in(sandbox.path(), "csplit", &["-s", "in", "/nomatch/"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("'/nomatch/': match not found"));
    assert!(!sandbox.path().join("xx00").exists());
    let output = run_in(
        sandbox.path(),
        "csplit",
        &["-k", "-s", "in", "2", "/nomatch/"],
        b"",
    );
    assert_eq!(status(&output), 1);
    assert_eq!(sandbox.read("xx00"), b"1\n");
}

#[test]
fn csplit_usage_and_pattern_errors() {
    let output = run("csplit", &["file"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("missing operand after 'file'"));
    let output = run("csplit", &["file", "0"], b"");
    assert!(stderr(&output).contains("0: line number must be greater than zero"));
    let output = run("csplit", &["-b", "%s", "file", "1"], b"");
    assert!(stderr(&output).contains("invalid conversion specifier in suffix: s"));
}

#[test]
fn csplit_help_and_version() {
    let output = run("csplit", &["--help"], b"");
    assert!(stdout(&output).starts_with("Usage: csplit [OPTION]... FILE PATTERN..."));
    let output = run("csplit", &["--version"], b"");
    assert_eq!(stdout(&output), version_line("csplit"));
}

// fmt

#[test]
fn fmt_rewraps_a_paragraph_under_width() {
    let text = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog again.\n";
    let output = run("fmt", &["-w", "30"], text);
    assert_eq!(status(&output), 0);
    let text_out = stdout(&output);
    assert!(text_out.lines().count() >= 3);
    for line in text_out.lines() {
        assert!(line.len() < 30, "line too long: {line:?}");
    }
    assert_eq!(text_out.split_whitespace().count(), 19);
}

#[test]
fn fmt_uniform_spacing_puts_two_spaces_after_sentences() {
    let output = run("fmt", &["-u"], b"Hello  world.  This   is   a test.\n");
    assert_eq!(stdout(&output), "Hello world.  This is a test.\n");
}

#[test]
fn fmt_split_only_keeps_short_lines_intact() {
    let output = run(
        "fmt",
        &["-s", "-w", "20"],
        b"short\nalpha beta gamma delta epsilon\n",
    );
    assert_eq!(stdout(&output), "short\nalpha beta gamma\ndelta epsilon\n");
}

#[test]
fn fmt_reformats_only_prefixed_lines() {
    let output = run(
        "fmt",
        &["-p", "# ", "-w", "20"],
        b"# one two three four five\nkeep me\n",
    );
    assert_eq!(stdout(&output), "# one two three\n# four five\nkeep me\n");
}

#[test]
fn fmt_keeps_blank_lines_between_paragraphs() {
    let output = run("fmt", &[], b"one line\n\n\ntwo\n");
    assert_eq!(stdout(&output), "one line\n\n\ntwo\n");
}

#[test]
fn fmt_numeric_width_and_errors() {
    let output = run("fmt", &["-12"], b"aaa bbb ccc ddd\n");
    assert_eq!(stdout(&output), "aaa bbb\nccc ddd\n");
    let output = run("fmt", &["-w", "x"], b"a\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid width: 'x'"));
    let output = run("fmt", &["--bogus"], b"");
    assert_eq!(status(&output), 1);
}

#[test]
fn fmt_missing_file_exits_one() {
    let output = run("fmt", &["no-such-file"], b"");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("fmt: no-such-file: No such file or directory"));
}

#[test]
fn fmt_help_and_version() {
    let output = run("fmt", &["--help"], b"");
    assert!(stdout(&output).starts_with("Usage: fmt [-WIDTH] [OPTION]... [FILE]..."));
    let output = run("fmt", &["--version"], b"");
    assert_eq!(stdout(&output), version_line("fmt"));
}

// pr

#[test]
fn pr_default_page_has_header_body_and_footer() {
    let sandbox = Sandbox::new();
    sandbox.write("f3", b"alpha\nbeta\ngamma\n");
    let output = run_in(sandbox.path(), "pr", &["f3"], b"");
    assert_eq!(status(&output), 0);
    let text = stdout(&output);
    assert_eq!(text.lines().count(), 66);
    assert!(text.contains("Page 1"));
    assert!(text.contains("f3"));
    assert!(text.contains("\nalpha\nbeta\ngamma\n"));
}

#[test]
fn pr_omit_header_prints_only_text() {
    let sandbox = Sandbox::new();
    sandbox.write("f3", b"alpha\nbeta\ngamma\n");
    let output = run_in(sandbox.path(), "pr", &["-T", "f3"], b"");
    assert_eq!(stdout(&output), "alpha\nbeta\ngamma\n");
}

#[test]
fn pr_numbers_lines_and_double_spaces() {
    let output = run("pr", &["-T", "-n", "-d"], b"a\nb\n");
    assert_eq!(stdout(&output), "    1\ta\n\n    2\tb\n\n");
}

#[test]
fn pr_custom_header_replaces_file_name() {
    let sandbox = Sandbox::new();
    sandbox.write("f3", b"alpha\n");
    let output = run_in(sandbox.path(), "pr", &["-h", "HEAD", "f3"], b"");
    let text = stdout(&output);
    assert!(text.contains("HEAD"));
    assert!(!text.contains("f3"));
}

#[test]
fn pr_columns_down_then_across() {
    let input = b"1\n2\n3\n4\n5\n6\n7\n";
    let output = run("pr", &["-3", "-T"], input);
    assert_eq!(stdout(&output), "1\t\t\t4\t\t\t6\n2\t\t\t5\t\t\t7\n3\n");
    let output = run("pr", &["-a", "-3", "-T"], input);
    assert_eq!(stdout(&output), "1\t\t\t2\t\t\t3\n4\t\t\t5\t\t\t6\n7\n");
}

#[test]
fn pr_join_lines_and_merge_files() {
    let output = run("pr", &["-J", "-2", "-T"], b"1\n2\n3\n");
    assert_eq!(stdout(&output), "1\t3\n2\n");
    let sandbox = Sandbox::new();
    sandbox.write("a", b"x\ny\n");
    sandbox.write("b", b"1\n");
    let output = run_in(sandbox.path(), "pr", &["-m", "-T", "a", "b"], b"");
    assert_eq!(stdout(&output), "x\t\t\t\t    1\ny\t\t\t\t    \n");
}

#[test]
fn pr_page_range_past_the_end_reports_and_exits_zero() {
    let sandbox = Sandbox::new();
    sandbox.write("f3", b"alpha\n");
    let output = run_in(sandbox.path(), "pr", &["+5", "f3"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("starting page number 5 exceeds page count 1"));
}

#[test]
fn pr_missing_file_exits_one_and_help_version() {
    let output = run("pr", &["nope"], b"");
    assert_eq!(status(&output), 1);
    let output = run("pr", &["--help"], b"");
    assert!(stdout(&output).starts_with("Usage: pr [OPTION]... [FILE]..."));
    let output = run("pr", &["--version"], b"");
    assert_eq!(stdout(&output), version_line("pr"));
}
