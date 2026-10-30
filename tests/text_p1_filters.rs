//! CLI tests for the Phase 1 text filters: wc, cut, tr, paste, fold, expand, unexpand, nl, tac.
//!
//! Each test runs the multi-call binary inside a temporary sandbox directory.

mod common;

use std::process::Output;

use common::{run, run_in, Sandbox};

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// ---------------------------------------------------------------- wc

#[test]
fn wc_default_counts_stdin_with_padding() {
    let out = run("wc", &[], b"hello world\nfoo\n");
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "      2       3      16\n");
}

#[test]
fn wc_single_count_of_one_file_is_unpadded() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"hello world\nfoo\n");
    let out = run_in(sb.path(), "wc", &["-l", "a.txt"], b"");
    assert_eq!(stdout(&out), "2 a.txt\n");
}

#[test]
fn wc_multiple_files_pad_to_size_digits_and_total() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"hello world\nfoo\n");
    sb.write("b.txt", b"x\n");
    let out = run_in(sb.path(), "wc", &["-lw", "a.txt", "b.txt"], b"");
    assert_eq!(stdout(&out), " 2  3 a.txt\n 1  1 b.txt\n 3  4 total\n");
}

#[test]
fn wc_total_only_prints_bare_totals() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"hello world\nfoo\n");
    sb.write("b.txt", b"x\n");
    let out = run_in(sb.path(), "wc", &["--total=only", "a.txt", "b.txt"], b"");
    assert_eq!(stdout(&out), "3 4 18\n");
}

#[test]
fn wc_invalid_total_is_a_usage_error() {
    let out = run("wc", &["--total=bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("invalid argument 'bogus' for '--total'"));
}

#[test]
fn wc_missing_file_is_reported_and_others_still_counted() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"one\n");
    let out = run_in(sb.path(), "wc", &["-l", "nope.txt", "a.txt"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("wc: nope.txt: No such file or directory"));
    assert!(stdout(&out).contains("1 a.txt"));
}

#[test]
fn wc_directory_operand_prints_zero_row_and_fails() {
    let sb = Sandbox::new();
    sb.mkdir("dd");
    let out = run_in(sb.path(), "wc", &["dd"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("wc: dd: Is a directory"));
    assert_eq!(stdout(&out), "      0       0       0 dd\n");
}

#[test]
fn wc_files0_from_reads_nul_separated_names() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"one\ntwo\n");
    sb.write("list", b"a.txt\0");
    let out = run_in(sb.path(), "wc", &["--files0-from=list", "-l"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "2 a.txt\n");
}

#[test]
fn wc_files0_from_rejects_file_operands() {
    let out = run("wc", &["--files0-from=-", "a.txt"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("file operands cannot be combined with --files0-from"));
}

#[test]
fn wc_max_line_length_expands_tabs() {
    let out = run("wc", &["-L"], b"a\tb\n");
    assert_eq!(stdout(&out), "9\n");
}

#[test]
fn wc_chars_count_utf8_sequences_once() {
    let out = run("wc", &["-m"], "é€\n".as_bytes());
    assert_eq!(stdout(&out), "3\n");
}

#[test]
fn wc_help_and_version_exit_zero() {
    let help = run("wc", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).starts_with("Usage: wc [OPTION]... [FILE]..."));
    let version = run("wc", &["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(stdout(&version).starts_with("wc (agentcmd) "));
}

#[test]
fn wc_unknown_option_is_a_usage_error() {
    let out = run("wc", &["-x"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("wc: invalid option -- 'x'"));
}

// ---------------------------------------------------------------- cut

#[test]
fn cut_selects_fields_and_keeps_lines_without_delimiter() {
    let out = run("cut", &["-d,", "-f2"], b"a,b,c\nplain\n");
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "b\nplain\n");
}

#[test]
fn cut_only_delimited_drops_lines_without_delimiter() {
    let out = run("cut", &["-d,", "-s", "-f2"], b"a,b\nplain\n");
    assert_eq!(stdout(&out), "b\n");
}

#[test]
fn cut_complement_and_output_delimiter() {
    let out = run(
        "cut",
        &["-d,", "--complement", "-f2", "--output-delimiter=;"],
        b"a,b,c\n",
    );
    assert_eq!(stdout(&out), "a;c\n");
}

#[test]
fn cut_bytes_inserts_output_delimiter_between_gaps() {
    let out = run("cut", &["-b", "1,3", "--output-delimiter=:"], b"abcdef\n");
    assert_eq!(stdout(&out), "a:c\n");
}

#[test]
fn cut_chars_is_byte_selection_and_zero_terminates() {
    let out = run("cut", &["-c", "2-3"], b"abcd\n");
    assert_eq!(stdout(&out), "bc\n");
    let zero = run("cut", &["-z", "-d,", "-f2"], b"a,b\0c,d\0");
    assert_eq!(zero.stdout, b"b\0d\0");
}

#[test]
fn cut_requires_exactly_one_list() {
    let none = run("cut", &[], b"");
    assert_eq!(code(&none), 1);
    assert!(stderr(&none).contains("you must specify a list of bytes, characters, or fields"));
    let two = run("cut", &["-b1", "-f1"], b"");
    assert!(stderr(&two).contains("only one list may be specified"));
}

#[test]
fn cut_rejects_zero_and_decreasing_ranges() {
    let zero = run("cut", &["-f0"], b"");
    assert_eq!(code(&zero), 1);
    assert!(stderr(&zero).contains("fields are numbered from 1"));
    let decreasing = run("cut", &["-b3-1"], b"");
    assert!(stderr(&decreasing).contains("invalid decreasing range"));
}

#[test]
fn cut_missing_file_fails_but_continues() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"abc\n");
    let out = run_in(sb.path(), "cut", &["-b1", "nope.txt", "t.txt"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(stdout(&out), "a\n");
    assert!(stderr(&out).contains("cut: nope.txt: No such file or directory"));
}

#[test]
fn cut_help_and_version() {
    let help = run("cut", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).contains("--output-delimiter=STRING"));
    assert_eq!(code(&run("cut", &["--version"], b"")), 0);
}

// ---------------------------------------------------------------- tr

#[test]
fn tr_translates_ranges_and_pads_set2() {
    let out = run("tr", &["a-c", "x"], b"abcd\n");
    assert_eq!(out.stdout, b"xxxd\n");
}

#[test]
fn tr_deletes_classes_and_complements() {
    let out = run("tr", &["-d", "[:digit:]"], b"a1b2\n");
    assert_eq!(out.stdout, b"ab\n");
    let complement = run("tr", &["-cd", "a-z\n"], b"a1B\n");
    assert_eq!(complement.stdout, b"a\n");
}

#[test]
fn tr_squeezes_repeats() {
    let out = run("tr", &["-s", " "], b"a   b\n");
    assert_eq!(out.stdout, b"a b\n");
}

#[test]
fn tr_truncate_set1_limits_mapping() {
    let out = run("tr", &["-t", "abc", "xy"], b"abc\n");
    assert_eq!(out.stdout, b"xyc\n");
}

#[test]
fn tr_octal_escapes_and_repeat_fill() {
    let out = run("tr", &["\\101-\\103", "[x*]"], b"ABCD\n");
    assert_eq!(out.stdout, b"xxxD\n");
}

#[test]
fn tr_operand_count_errors_exit_one() {
    let missing = run("tr", &["a"], b"");
    assert_eq!(code(&missing), 1);
    assert!(stderr(&missing).contains("missing operand after 'a'"));
    let extra = run("tr", &["a", "b", "c"], b"");
    assert!(stderr(&extra).contains("extra operand 'c'"));
}

#[test]
fn tr_reverse_range_and_bad_class_are_errors() {
    let reverse = run("tr", &["z-a", "x"], b"");
    assert_eq!(code(&reverse), 1);
    assert!(stderr(&reverse)
        .contains("range-endpoints of 'z-a' are in reverse collating sequence order"));
    let class = run("tr", &["[:nope:]", "x"], b"");
    assert!(stderr(&class).contains("invalid character class 'nope'"));
}

#[test]
fn tr_help_and_version() {
    let help = run("tr", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).starts_with("Usage: tr [OPTION]... SET1 [SET2]"));
    assert_eq!(code(&run("tr", &["--version"], b"")), 0);
}

// ---------------------------------------------------------------- paste

#[test]
fn paste_joins_parallel_lines_with_tabs() {
    let sb = Sandbox::new();
    sb.write("p1", b"1\n2\n3\n");
    sb.write("p2", b"a\nb\n");
    let out = run_in(sb.path(), "paste", &["p1", "p2"], b"");
    assert_eq!(out.stdout, b"1\ta\n2\tb\n3\t\n");
}

#[test]
fn paste_cycles_delimiters_and_escapes() {
    let sb = Sandbox::new();
    sb.write("p1", b"1\n2\n");
    sb.write("p2", b"a\nb\n");
    let out = run_in(sb.path(), "paste", &["-d", "\\n\\t", "p1", "p2", "p1"], b"");
    assert_eq!(out.stdout, b"1\na\t1\n2\nb\t2\n");
}

#[test]
fn paste_serial_joins_each_file_on_one_line() {
    let sb = Sandbox::new();
    sb.write("p1", b"1\n2\n3\n");
    let out = run_in(sb.path(), "paste", &["-s", "-d", ",", "p1"], b"");
    assert_eq!(out.stdout, b"1,2,3\n");
}

#[test]
fn paste_zero_terminated_output() {
    let out = run("paste", &["-z"], b"x\ny\n");
    assert_eq!(out.stdout, b"x\ny\n\0");
}

#[test]
fn paste_missing_file_in_parallel_produces_no_output() {
    let sb = Sandbox::new();
    sb.write("p2", b"a\n");
    let out = run_in(sb.path(), "paste", &["nope.txt", "p2"], b"");
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
    assert!(stderr(&out).contains("paste: nope.txt: No such file or directory"));
}

#[test]
fn paste_serial_missing_file_does_not_stop_others() {
    let sb = Sandbox::new();
    sb.write("p2", b"a\nb\n");
    let out = run_in(sb.path(), "paste", &["-s", "p2", "nope.txt"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(out.stdout, b"a\tb\n");
}

#[test]
fn paste_dash_operands_share_standard_input() {
    let out = run("paste", &["-", "-"], b"l1\nl2\nl3\n");
    assert_eq!(out.stdout, b"l1\tl2\nl3\t\n");
}

#[test]
fn paste_trailing_backslash_is_an_error() {
    let out = run("paste", &["-d", "a\\"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("delimiter list ends with an unescaped backslash"));
}

#[test]
fn paste_help_and_version() {
    assert_eq!(code(&run("paste", &["--help"], b"")), 0);
    assert_eq!(code(&run("paste", &["--version"], b"")), 0);
}

// ---------------------------------------------------------------- fold

#[test]
fn fold_wraps_at_width() {
    let out = run("fold", &["-w", "5"], b"abcdefghij\n");
    assert_eq!(out.stdout, b"abcde\nfghij\n");
}

#[test]
fn fold_spaces_breaks_after_blanks() {
    let out = run("fold", &["-s", "-w", "8"], b"hello world foo\n");
    assert_eq!(out.stdout, b"hello \nworld \nfoo\n");
}

#[test]
fn fold_bytes_ignores_tab_columns() {
    let out = run("fold", &["-b", "-w", "4"], b"a\tbc\td\n");
    assert_eq!(out.stdout, b"a\tbc\n\td\n");
}

#[test]
fn fold_columns_count_tabs_to_next_stop() {
    let out = run("fold", &["-w", "4"], b"a\tbc\n");
    assert_eq!(out.stdout, b"a\n\t\nbc\n");
}

#[test]
fn fold_legacy_numeric_width() {
    let out = run("fold", &["-4"], b"abcdefgh\n");
    assert_eq!(out.stdout, b"abcd\nefgh\n");
}

#[test]
fn fold_invalid_width_is_an_error() {
    let zero = run("fold", &["-w", "0"], b"abc\n");
    assert_eq!(code(&zero), 1);
    assert!(stderr(&zero).contains("invalid number of columns: '0'"));
    let text = run("fold", &["-w", "x"], b"abc\n");
    assert!(stderr(&text).contains("invalid number of columns: 'x'"));
}

#[test]
fn fold_missing_file_fails_but_continues() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"abcd\n");
    let out = run_in(sb.path(), "fold", &["-w", "2", "nope.txt", "a.txt"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(stdout(&out), "ab\ncd\n");
}

#[test]
fn fold_help_and_version() {
    assert_eq!(code(&run("fold", &["--help"], b"")), 0);
    assert_eq!(code(&run("fold", &["--version"], b"")), 0);
}

// ---------------------------------------------------------------- expand / unexpand

#[test]
fn expand_converts_tabs_to_eight_column_stops() {
    let out = run("expand", &[], b"a\tb\n");
    assert_eq!(out.stdout, b"a       b\n");
}

#[test]
fn expand_tab_list_and_increment() {
    assert_eq!(run("expand", &["-t", "4"], b"a\tb\n").stdout, b"a   b\n");
    assert_eq!(run("expand", &["-t", "2,+3"], b"a\tb\n").stdout, b"a b\n");
}

#[test]
fn expand_initial_keeps_tabs_after_text() {
    let out = run("expand", &["-i"], b"a\tb\n  \tc\n");
    assert_eq!(out.stdout, b"a\tb\n        c\n");
}

#[test]
fn expand_rejects_bad_tab_lists() {
    let zero = run("expand", &["-t", "0"], b"");
    assert_eq!(code(&zero), 1);
    assert!(stderr(&zero).contains("tab size cannot be 0"));
    let descending = run("expand", &["-t", "5,3"], b"");
    assert!(stderr(&descending).contains("tab sizes must be ascending"));
}

#[test]
fn expand_help_and_version() {
    assert_eq!(code(&run("expand", &["--help"], b"")), 0);
    assert_eq!(code(&run("expand", &["--version"], b"")), 0);
}

#[test]
fn unexpand_converts_leading_blanks_by_default() {
    let out = run("unexpand", &[], b"        a       b\n");
    assert_eq!(out.stdout, b"\ta       b\n");
}

#[test]
fn unexpand_all_and_tabs_imply_full_conversion() {
    assert_eq!(
        run("unexpand", &["-a"], b"        a       b\n").stdout,
        b"\ta\tb\n"
    );
    assert_eq!(
        run("unexpand", &["-t", "4"], b"        a\n").stdout,
        b"\t\ta\n"
    );
}

#[test]
fn unexpand_first_only_overrides_all() {
    let out = run("unexpand", &["-a", "--first-only"], b"        a       b\n");
    assert_eq!(out.stdout, b"\ta       b\n");
}

#[test]
fn unexpand_zero_tab_size_is_an_error() {
    let out = run("unexpand", &["-t", "0"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("tab size cannot be 0"));
}

#[test]
fn unexpand_help_and_version() {
    assert_eq!(code(&run("unexpand", &["--help"], b"")), 0);
    assert_eq!(code(&run("unexpand", &["--version"], b"")), 0);
}

// ---------------------------------------------------------------- nl

#[test]
fn nl_numbers_non_empty_lines_by_default() {
    let out = run("nl", &[], b"one\n\ntwo\n");
    assert_eq!(stdout(&out), "     1\tone\n       \n     2\ttwo\n");
}

#[test]
fn nl_body_all_and_format_options() {
    assert_eq!(
        stdout(&run("nl", &["-ba"], b"x\n\n")),
        "     1\tx\n     2\t\n"
    );
    assert_eq!(
        stdout(&run("nl", &["-n", "rz", "-w", "3"], b"x\n")),
        "001\tx\n"
    );
    assert_eq!(
        stdout(&run("nl", &["-n", "ln", "-w", "3", "-s", "|"], b"x\n")),
        "1  |x\n"
    );
}

#[test]
fn nl_start_increment_and_separator() {
    let out = run("nl", &["-v", "10", "-i", "5", "-s", "::"], b"a\nb\n");
    assert_eq!(stdout(&out), "    10::a\n    15::b\n");
}

#[test]
fn nl_section_delimiters_restart_numbering() {
    let input = b"\\:\\:\\:\nh1\n\\:\\:\nb1\n\\:\nf1\n";
    let out = run("nl", &["-ha"], input);
    assert_eq!(stdout(&out), "\n     1\th1\n\n     1\tb1\n\n       f1\n");
}

#[test]
fn nl_no_renumber_keeps_counting_across_sections() {
    let out = run("nl", &["-ba", "-p"], b"a\n\\:\\:\nb\n");
    assert_eq!(stdout(&out), "     1\ta\n\n     2\tb\n");
}

#[test]
fn nl_rejects_bad_styles_and_formats() {
    let style = run("nl", &["-b", "x"], b"");
    assert_eq!(code(&style), 1);
    assert!(stderr(&style).contains("invalid body numbering style: 'x'"));
    let format = run("nl", &["-n", "xx"], b"");
    assert!(stderr(&format).contains("invalid line numbering format: 'xx'"));
}

#[test]
fn nl_missing_file_fails_but_continues() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"a\n");
    let out = run_in(sb.path(), "nl", &["nope.txt", "a.txt"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(stdout(&out), "     1\ta\n");
}

#[test]
fn nl_help_and_version() {
    assert_eq!(code(&run("nl", &["--help"], b"")), 0);
    assert_eq!(code(&run("nl", &["--version"], b"")), 0);
}

// ---------------------------------------------------------------- tac

#[test]
fn tac_reverses_lines() {
    let out = run("tac", &[], b"1\n2\n3\n");
    assert_eq!(out.stdout, b"3\n2\n1\n");
}

#[test]
fn tac_reverses_each_file_separately() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"1\n2\n");
    sb.write("b.txt", b"3\n");
    let out = run_in(sb.path(), "tac", &["a.txt", "b.txt"], b"");
    assert_eq!(out.stdout, b"2\n1\n3\n");
}

#[test]
fn tac_keeps_unterminated_last_record_in_place() {
    let out = run("tac", &[], b"l1\nl2");
    assert_eq!(out.stdout, b"l2l1\n");
}

#[test]
fn tac_custom_and_before_separators() {
    assert_eq!(run("tac", &["-s", "X"], b"aXbXc").stdout, b"cbXaX");
    assert_eq!(run("tac", &["-b"], b"1\n2\n").stdout, b"\n\n21");
}

#[test]
fn tac_regex_separator() {
    assert_eq!(
        run("tac", &["-r", "-s", "[0-9]"], b"a1b22c\n").stdout,
        b"c\n2b2a1"
    );
}

#[test]
fn tac_missing_file_reports_and_continues() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"1\n");
    let out = run_in(sb.path(), "tac", &["nope.txt", "a.txt"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("tac: failed to open 'nope.txt' for reading"));
    assert_eq!(out.stdout, b"1\n");
}

#[test]
fn tac_help_and_version() {
    assert_eq!(code(&run("tac", &["--help"], b"")), 0);
    assert_eq!(code(&run("tac", &["--version"], b"")), 0);
}

// ---------------------------------------------------------------- usage errors and extra paths

#[test]
fn every_command_rejects_unknown_options_with_exit_one() {
    for util in [
        "cut", "tr", "paste", "fold", "expand", "unexpand", "nl", "tac",
    ] {
        let out = run(util, &["--no-such-option"], b"");
        assert_eq!(code(&out), 1, "{util}");
        assert!(
            stderr(&out).contains(&format!("{util}: unrecognized option '--no-such-option'")),
            "{util}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn expand_legacy_numeric_and_missing_file() {
    assert_eq!(run("expand", &["-4"], b"a\tb\n").stdout, b"a   b\n");
    let sb = Sandbox::new();
    sb.write("a.txt", b"a\tb\n");
    let out = run_in(sb.path(), "expand", &["nope.txt", "a.txt"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(out.stdout, b"a       b\n");
    assert!(stderr(&out).contains("expand: nope.txt: No such file or directory"));
}

#[test]
fn unexpand_legacy_numeric_and_missing_file() {
    assert_eq!(run("unexpand", &["-4"], b"        a\n").stdout, b"\t\ta\n");
    let sb = Sandbox::new();
    sb.write("a.txt", b"        a\n");
    let out = run_in(sb.path(), "unexpand", &["nope.txt", "a.txt"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(out.stdout, b"\ta\n");
}

#[test]
fn unexpand_tabs_with_first_only_converts_leading_blanks() {
    let out = run(
        "unexpand",
        &["--tabs=8", "--first-only"],
        b"        a        b\n",
    );
    assert_eq!(out.stdout, b"\ta        b\n");
}

#[test]
fn nl_numbers_files_as_one_stream() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"a\n");
    sb.write("b.txt", b"b\n");
    let out = run_in(sb.path(), "nl", &["a.txt", "b.txt"], b"");
    assert_eq!(stdout(&out), "     1\ta\n     2\tb\n");
    let stdin = run("nl", &["-ba"], b"x\n");
    assert_eq!(stdout(&stdin), "     1\tx\n");
}

#[test]
fn nl_join_blank_lines_numbers_every_nth_blank() {
    let out = run("nl", &["-ba", "-l", "2"], b"a\n\n\n\nb\n");
    assert_eq!(
        stdout(&out),
        "     1\ta\n       \n     2\t\n       \n     3\tb\n"
    );
}

#[test]
fn tac_reads_standard_input_and_rejects_bad_regex() {
    assert_eq!(run("tac", &[], b"x\ny\n").stdout, b"y\nx\n");
    let out = run("tac", &["-r", "-s", "\\("], b"a\n");
    assert_eq!(code(&out), 1);
}

#[test]
fn broken_pipe_on_stdout_exits_zero_silently() {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .args(["tr", "a-z", "A-Z"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn agentcmd");
    let mut stdin = child.stdin.take().expect("child stdin");
    let writer = std::thread::spawn(move || {
        let chunk = vec![b'a'; 1 << 16];
        for _ in 0..512 {
            if stdin.write_all(&chunk).is_err() {
                break;
            }
        }
    });
    let mut stdout = child.stdout.take().expect("child stdout");
    let mut head = [0u8; 16];
    stdout.read_exact(&mut head).expect("first output bytes");
    drop(stdout);
    let status = child.wait().expect("wait for agentcmd");
    let mut err = String::new();
    child
        .stderr
        .take()
        .expect("child stderr")
        .read_to_string(&mut err)
        .expect("read stderr");
    writer.join().expect("writer thread");
    assert_eq!(status.code(), Some(0));
    assert!(err.is_empty(), "stderr: {err}");
}
