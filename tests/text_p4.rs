//! CLI tests for the Phase 4 text utilities: cmp, rev, column, dos2unix and unix2dos.
//! Files live in a `Sandbox`; the GNU differential cases are in tests/gnu_cases.

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

// ---------------------------------------------------------------- cmp

#[test]
fn cmp_identical_inputs_exit_zero_silently() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"same\n");
    sandbox.write("b", b"same\n");
    let output = run_in(sandbox.path(), "cmp", &["a", "b"], b"");
    assert_eq!(status(&output), 0);
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).is_empty());
}

#[test]
fn cmp_reports_first_difference_on_stdout() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"abc\ndef\nghi\n");
    sandbox.write("b", b"abc\nxef\nghi\n");
    let output = run_in(sandbox.path(), "cmp", &["a", "b"], b"");
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "a b differ: byte 5, line 2\n");
}

#[test]
fn cmp_verbose_lists_every_difference_in_octal() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"a\nc\n");
    sandbox.write("b", b"a\nx\n");
    let output = run_in(sandbox.path(), "cmp", &["-l", "a", "b"], b"");
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "3 143 170\n");
}

#[test]
fn cmp_print_bytes_shows_the_characters() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"\x01");
    sandbox.write("b", b"\x02");
    let output = run_in(sandbox.path(), "cmp", &["-b", "a", "b"], b"");
    assert_eq!(status(&output), 1);
    assert_eq!(
        stdout(&output),
        "a b differ: byte 1, line 1 is   1 ^A   2 ^B\n"
    );
}

#[test]
fn cmp_end_of_shorter_input_is_reported_on_stderr() {
    let sandbox = Sandbox::new();
    sandbox.write("long", b"abc\ndef\nghi\n");
    sandbox.write("short", b"abc\ndef\n");
    let output = run_in(sandbox.path(), "cmp", &["long", "short"], b"");
    assert_eq!(status(&output), 1);
    assert!(stdout(&output).is_empty());
    assert_eq!(stderr(&output), "cmp: EOF on short after byte 8, line 2\n");
}

#[test]
fn cmp_silent_option_prints_nothing_but_keeps_the_status() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"x\n");
    sandbox.write("b", b"y\n");
    let output = run_in(sandbox.path(), "cmp", &["-s", "a", "b"], b"");
    assert_eq!(status(&output), 1);
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).is_empty());
}

#[test]
fn cmp_skip_and_limit_restrict_the_compared_range() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"abc\ndef\n");
    sandbox.write("b", b"abc\nxef\n");
    assert_eq!(
        status(&run_in(sandbox.path(), "cmp", &["-n", "4", "a", "b"], b"")),
        0
    );
    let skipped = run_in(sandbox.path(), "cmp", &["-l", "-i", "1:1", "a", "b"], b"");
    assert_eq!(stdout(&skipped), "4 144 170\n");
    assert_eq!(status(&skipped), 1);
}

#[test]
fn cmp_reads_standard_input_for_dash() {
    let sandbox = Sandbox::new();
    sandbox.write("b", b"abc\nxef\n");
    let output = run_in(sandbox.path(), "cmp", &["-", "b"], b"abc\ndef\n");
    assert_eq!(stdout(&output), "- b differ: byte 5, line 2\n");
    assert_eq!(status(&output), 1);
}

#[test]
fn cmp_usage_and_trouble_exit_two() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"x\n");
    sandbox.mkdir("d");
    assert_eq!(status(&run_in(sandbox.path(), "cmp", &[], b"")), 2);
    assert_eq!(
        status(&run_in(
            sandbox.path(),
            "cmp",
            &["a", "a", "1", "2", "3"],
            b""
        )),
        2
    );
    assert_eq!(
        status(&run_in(sandbox.path(), "cmp", &["-n", "-1", "a", "a"], b"")),
        2
    );
    assert_eq!(
        status(&run_in(sandbox.path(), "cmp", &["a", "missing"], b"")),
        2
    );
    let directory = run_in(sandbox.path(), "cmp", &["a", "d"], b"");
    assert_eq!(status(&directory), 2);
    assert_eq!(stderr(&directory), "cmp: d: Is a directory\n");
}

#[test]
fn cmp_help_and_version() {
    let help = run("cmp", &["--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: cmp [OPTION]... FILE1 [FILE2 [SKIP1 [SKIP2]]]"));
    let version = run("cmp", &["--version"], b"");
    assert_eq!(stdout(&version), version_line("cmp"));
}

// ---------------------------------------------------------------- rev

#[test]
fn rev_reverses_each_line_and_keeps_newlines() {
    let output = run("rev", &[], b"abc\n\nxy");
    assert_eq!(stdout(&output), "cba\n\nyx");
    assert_eq!(status(&output), 0);
}

#[test]
fn rev_keeps_utf8_characters_whole() {
    let output = run("rev", &[], "h\u{e9}llo \u{65e5}\u{672c}\n".as_bytes());
    assert_eq!(stdout(&output), "\u{672c}\u{65e5} oll\u{e9}h\n");
}

#[test]
fn rev_reverses_lines_with_invalid_utf8_byte_by_byte() {
    let output = run("rev", &[], b"ab\xffcd\n");
    assert_eq!(output.stdout, b"dc\xffba\n");
}

#[test]
fn rev_treats_carriage_return_as_an_ordinary_character() {
    let output = run("rev", &[], b"abc\r\n");
    assert_eq!(output.stdout, b"\rcba\n");
}

#[test]
fn rev_reads_files_in_order_and_reports_missing_ones() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"ab\n");
    sandbox.write("b", b"cd");
    let output = run_in(sandbox.path(), "rev", &["a", "missing", "b"], b"");
    assert_eq!(stdout(&output), "ba\ndc");
    assert_eq!(
        stderr(&output),
        "rev: cannot open missing: No such file or directory\n"
    );
    assert_eq!(status(&output), 1);
}

#[test]
fn rev_double_dash_ends_options() {
    let sandbox = Sandbox::new();
    sandbox.write("-x", b"xy\n");
    let output = run_in(sandbox.path(), "rev", &["--", "-x"], b"");
    assert_eq!(stdout(&output), "yx\n");
}

#[test]
fn rev_unknown_option_is_a_usage_error() {
    let output = run("rev", &["-x"], b"");
    assert_eq!(status(&output), 1);
    assert_eq!(
        stderr(&output),
        "rev: invalid option -- 'x'\nTry 'rev --help' for more information.\n"
    );
}

#[test]
fn rev_help_and_version() {
    let help = run("rev", &["--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: rev [options] [file ...]"));
    let version = run("rev", &["--version"], b"");
    assert_eq!(stdout(&version), version_line("rev"));
}

// ---------------------------------------------------------------- column

#[test]
fn column_fills_items_down_columns_with_tab_stops() {
    let output = run("column", &[], b"a\nbb\nccc\ndddd\ne\nff\ng\n");
    assert_eq!(stdout(&output), "a\tbb\tccc\tdddd\te\tff\tg\n");
}

#[test]
fn column_width_limits_the_number_of_columns() {
    let items = b"a\nbb\nccc\ndddd\ne\nff\ng\n";
    assert_eq!(
        stdout(&run("column", &["-c", "20"], items)),
        "a\te\nbb\tff\nccc\tg\ndddd\n"
    );
    assert_eq!(
        stdout(&run("column", &["-x", "-c", "20"], items)),
        "a\tbb\nccc\tdddd\ne\tff\ng\n"
    );
}

#[test]
fn column_empty_lines_are_skipped_unless_kept() {
    let input = b"x y\n\n\nz\n";
    assert_eq!(stdout(&run("column", &[], input)), "x y\tz\n");
    assert_eq!(stdout(&run("column", &["-L"], input)), "x y\t\t\tz\n");
}

#[test]
fn column_table_aligns_fields_with_two_space_separator() {
    let output = run(
        "column",
        &["-t"],
        b"name age city\nalice 30 paris\nbob 4 rome\n",
    );
    assert_eq!(
        stdout(&output),
        "name   age  city\nalice  30   paris\nbob    4    rome\n"
    );
}

#[test]
fn column_table_right_alignment_and_hidden_columns() {
    let input = b"name age city\nalice 30 paris\n";
    assert_eq!(
        stdout(&run("column", &["-t", "-R", "2"], input)),
        "name   age  city\nalice   30  paris\n"
    );
    assert_eq!(
        stdout(&run("column", &["-t", "-N", "n,a,c", "-H", "a"], input)),
        "n      c\nname   city\nalice  paris\n"
    );
}

#[test]
fn column_table_separator_and_output_separator() {
    let output = run(
        "column",
        &["-t", "-s", ":", "-o", " | "],
        b"a:b:c\n1:22:333\n",
    );
    assert_eq!(stdout(&output), "a | b  | c\n1 | 22 | 333\n");
}

#[test]
fn column_table_json_uses_names_as_keys() {
    let output = run("column", &["-t", "-N", "n,a", "-J"], b"name age\nbob 4\n");
    assert_eq!(
        stdout(&output),
        "{\n   \"table\": [\n      {\n         \"n\": \"name\",\n         \"a\": \"age\"\n      },{\n         \"n\": \"bob\",\n         \"a\": \"4\"\n      }\n   ]\n}\n"
    );
}

#[test]
fn column_option_errors_exit_one() {
    assert_eq!(status(&run("column", &["-t", "-x"], b"a\n")), 1);
    assert_eq!(status(&run("column", &["-J"], b"a\n")), 1);
    assert_eq!(status(&run("column", &["-c", "x"], b"a\n")), 1);
    assert_eq!(status(&run("column", &["-Z"], b"")), 1);
    let unsupported = run("column", &["-T", "1"], b"a\n");
    assert_eq!(status(&unsupported), 1);
    assert!(stderr(&unsupported).contains("not supported by agentcmd"));
}

#[test]
fn column_help_and_version() {
    let help = run("column", &["--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: column [options] [file ...]"));
    let version = run("column", &["--version"], b"");
    assert_eq!(stdout(&version), version_line("column"));
}

// ---------------------------------------------------------------- dos2unix / unix2dos

#[test]
fn dos2unix_filters_standard_input_to_standard_output() {
    let output = run("dos2unix", &[], b"a\r\nb\rc\r\n");
    assert_eq!(output.stdout, b"a\nb\rc\n");
    assert_eq!(status(&output), 0);
}

#[test]
fn dos2unix_converts_in_place_through_a_temporary_file() {
    let sandbox = Sandbox::new();
    sandbox.write("f.txt", b"x\r\ny");
    let output = run_in(sandbox.path(), "dos2unix", &["f.txt"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(
        stderr(&output),
        "dos2unix: converting file f.txt to Unix format...\n"
    );
    assert_eq!(sandbox.read("f.txt"), b"x\ny");
    let leftovers: Vec<_> = std::fs::read_dir(sandbox.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
}

#[test]
fn dos2unix_skips_binary_files_unless_forced() {
    let sandbox = Sandbox::new();
    sandbox.write("bin", b"a\x1b\r\n");
    let skipped = run_in(sandbox.path(), "dos2unix", &["bin"], b"");
    assert_eq!(status(&skipped), 0);
    assert_eq!(stderr(&skipped), "dos2unix: Skipping binary file bin\n");
    assert_eq!(sandbox.read("bin"), b"a\x1b\r\n");
    let forced = run_in(sandbox.path(), "dos2unix", &["-f", "bin"], b"");
    assert_eq!(status(&forced), 0);
    assert_eq!(sandbox.read("bin"), b"a\x1b\n");
}

#[test]
fn dos2unix_new_file_pairs_leave_the_input_unchanged() {
    let sandbox = Sandbox::new();
    sandbox.write("in1", b"p\r\n");
    sandbox.write("in2", b"q\r\n");
    let output = run_in(
        sandbox.path(),
        "dos2unix",
        &["-n", "in1", "out1", "in2", "out2"],
        b"",
    );
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("out1"), b"p\n");
    assert_eq!(sandbox.read("out2"), b"q\n");
    assert_eq!(sandbox.read("in1"), b"p\r\n");
}

#[test]
fn dos2unix_removes_byte_order_mark_unless_kept() {
    let removed = run("dos2unix", &[], b"\xef\xbb\xbfa\r\n");
    assert_eq!(removed.stdout, b"a\n");
    let kept = run("dos2unix", &["-b"], b"\xef\xbb\xbfa\r\n");
    assert_eq!(kept.stdout, b"\xef\xbb\xbfa\n");
}

#[test]
fn dos2unix_quiet_suppresses_progress_messages() {
    let sandbox = Sandbox::new();
    sandbox.write("f", b"x\r\n");
    let output = run_in(sandbox.path(), "dos2unix", &["-q", "f"], b"");
    assert_eq!(status(&output), 0);
    assert!(stderr(&output).is_empty());
    assert_eq!(sandbox.read("f"), b"x\n");
}

#[test]
fn dos2unix_missing_file_exits_two_and_continues() {
    let sandbox = Sandbox::new();
    sandbox.write("f", b"x\r\n");
    let output = run_in(sandbox.path(), "dos2unix", &["missing", "f"], b"");
    assert_eq!(status(&output), 2);
    assert!(stderr(&output).contains("dos2unix: missing: No such file or directory"));
    assert_eq!(sandbox.read("f"), b"x\n");
}

#[test]
fn dos2unix_rejects_unsupported_conversion_mode() {
    let output = run("dos2unix", &["-c", "mac"], b"a\r\n");
    assert_eq!(status(&output), 1);
    assert!(stderr(&output).contains("invalid mac conversion mode specified"));
}

#[cfg(unix)]
#[test]
fn dos2unix_keeps_permissions_and_keepdate_keeps_mtime() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    sandbox.write("f", b"x\r\n");
    std::fs::set_permissions(
        sandbox.path().join("f"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    let mtime = filetime::FileTime::from_unix_time(1_600_000_000, 0);
    filetime::set_file_mtime(sandbox.path().join("f"), mtime).unwrap();
    let output = run_in(sandbox.path(), "dos2unix", &["-k", "f"], b"");
    assert_eq!(status(&output), 0);
    let meta = std::fs::metadata(sandbox.path().join("f")).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o640);
    assert_eq!(
        filetime::FileTime::from_last_modification_time(&meta),
        mtime
    );
}

#[cfg(unix)]
#[test]
fn dos2unix_skips_symbolic_links() {
    let sandbox = Sandbox::new();
    sandbox.write("target", b"x\r\n");
    if sandbox.symlink("target", "link").is_none() {
        return;
    }
    let output = run_in(sandbox.path(), "dos2unix", &["link"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stderr(&output), "dos2unix: Skipping symbolic link link.\n");
    assert_eq!(sandbox.read("target"), b"x\r\n");
}

#[test]
fn unix2dos_filters_and_adds_carriage_returns() {
    let output = run("unix2dos", &[], b"a\nb\r\nc\n");
    assert_eq!(output.stdout, b"a\r\nb\r\nc\r\n");
}

#[test]
fn unix2dos_add_bom_and_add_eol_options() {
    let bom = run("unix2dos", &["-m"], b"a\n");
    assert_eq!(bom.stdout, b"\xef\xbb\xbfa\r\n");
    let eol = run("unix2dos", &["-e"], b"a\nb");
    assert_eq!(eol.stdout, b"a\r\nb\r\n");
}

#[test]
fn unix2dos_keeps_a_byte_order_mark_by_default_and_removes_on_request() {
    let kept = run("unix2dos", &[], b"\xef\xbb\xbfa\n");
    assert_eq!(kept.stdout, b"\xef\xbb\xbfa\r\n");
    let removed = run("unix2dos", &["-r"], b"\xef\xbb\xbfa\n");
    assert_eq!(removed.stdout, b"a\r\n");
}

#[test]
fn unix2dos_converts_in_place_and_skips_binary_files() {
    let sandbox = Sandbox::new();
    sandbox.write("text", b"x\ny\n");
    sandbox.write("bin", b"a\x00\n");
    let output = run_in(sandbox.path(), "unix2dos", &["text", "bin"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(sandbox.read("text"), b"x\r\ny\r\n");
    assert_eq!(sandbox.read("bin"), b"a\x00\n");
    assert!(stderr(&output).contains("unix2dos: Skipping binary file bin"));
}

// ---------------------------------------------------------------- diff output formats

#[test]
fn diff_default_output_is_the_normal_format() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"a\nb\nc\n");
    sandbox.write("b", b"a\nx\nc\n");
    sandbox.write("c", b"a\nc\n");
    let change = run_in(sandbox.path(), "diff", &["a", "b"], b"");
    assert_eq!(status(&change), 1);
    assert_eq!(stdout(&change), "2c2\n< b\n---\n> x\n");
    assert_eq!(
        stdout(&run_in(sandbox.path(), "diff", &["c", "a"], b"")),
        "1a2\n> b\n"
    );
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "diff",
            &["--normal", "a", "b"],
            b""
        )),
        "2c2\n< b\n---\n> x\n"
    );
}

#[test]
fn diff_context_format_has_headers_and_bang_marks() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"one\ntwo\nthree\n");
    sandbox.write("b", b"one\nTWO\nthree\n");
    let out = stdout(&run_in(sandbox.path(), "diff", &["-C0", "a", "b"], b""));
    assert!(out.starts_with("*** a\t"), "{out}");
    assert!(out.contains("\n--- b\t"), "{out}");
    assert!(
        out.ends_with("***************\n*** 2 ****\n! two\n--- 2 ----\n! TWO\n"),
        "{out}"
    );
    let labelled = run_in(
        sandbox.path(),
        "diff",
        &["-c", "--label", "L", "--label", "M", "a", "b"],
        b"",
    );
    assert!(
        stdout(&labelled).starts_with("*** L\n--- M\n"),
        "{}",
        stdout(&labelled)
    );
}

#[test]
fn diff_side_by_side_uses_the_gnu_column_layout() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"b\n");
    sandbox.write("x", b"x\n");
    sandbox.write("same", b"a\n");
    assert_eq!(
        stdout(&run_in(sandbox.path(), "diff", &["-y", "a", "x"], b"")),
        "b\t\t\t\t\t\t\t      |\tx\n"
    );
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "diff",
            &["-y", "same", "same"],
            b""
        )),
        "a\t\t\t\t\t\t\t\ta\n"
    );
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "diff",
            &["-y", "-W", "40", "a", "x"],
            b""
        )),
        "b\t\t   |\tx\n"
    );
}

#[test]
fn diff_side_by_side_suppresses_or_marks_common_lines() {
    let sandbox = Sandbox::new();
    sandbox.write("left", b"a\nb\n");
    sandbox.write("right", b"a\nX\nb\n");
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "diff",
            &["-y", "--suppress-common-lines", "left", "right"],
            b""
        )),
        "\t\t\t\t\t\t\t      >\tX\n"
    );
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "diff",
            &["-y", "--left-column", "left", "right"],
            b""
        )),
        "a\t\t\t\t\t\t\t      (\n\t\t\t\t\t\t\t      >\tX\nb\t\t\t\t\t\t\t      (\n"
    );
    let brief = run_in(sandbox.path(), "diff", &["-y", "-q", "left", "right"], b"");
    assert_eq!(stdout(&brief), "Files left and right differ\n");
    assert_eq!(status(&brief), 1);
}

#[test]
fn diff_side_by_side_lists_identical_files_and_reports_with_s() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"x\ny\n");
    let listed = run_in(sandbox.path(), "diff", &["-y", "a", "a"], b"");
    assert_eq!(stdout(&listed), "x\t\t\t\t\t\t\t\tx\ny\t\t\t\t\t\t\t\ty\n");
    assert_eq!(status(&listed), 0);
    let reported = run_in(sandbox.path(), "diff", &["-y", "-s", "a", "a"], b"");
    assert!(stdout(&reported).ends_with("Files a and a are identical\n"));
}

#[test]
fn diff_style_and_width_option_errors_exit_two() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"x\n");
    sandbox.write("b", b"y\n");
    let conflict = run_in(sandbox.path(), "diff", &["-y", "-c", "a", "b"], b"");
    assert_eq!(status(&conflict), 2);
    assert_eq!(
        stderr(&conflict),
        "diff: conflicting output style options\nTry 'diff --help' for more information.\n"
    );
    assert_eq!(
        status(&run_in(
            sandbox.path(),
            "diff",
            &["--normal", "-u", "a", "b"],
            b""
        )),
        2
    );
    assert_eq!(
        status(&run_in(
            sandbox.path(),
            "diff",
            &["-y", "-W", "0", "a", "b"],
            b""
        )),
        2
    );
    assert_eq!(
        status(&run_in(
            sandbox.path(),
            "diff",
            &["-y", "--width=x", "a", "b"],
            b""
        )),
        2
    );
    let help = run("diff", &["--help"], b"");
    assert_eq!(status(&help), 0);
    for option in [
        "--normal",
        "--side-by-side",
        "--suppress-common-lines",
        "--left-column",
    ] {
        assert!(stdout(&help).contains(option), "missing {option}");
    }
}

#[test]
fn dos2unix_and_unix2dos_report_version_and_help() {
    assert_eq!(
        stdout(&run("dos2unix", &["--version"], b"")),
        version_line("dos2unix")
    );
    assert_eq!(
        stdout(&run("unix2dos", &["--version"], b"")),
        version_line("unix2dos")
    );
    let help = run("unix2dos", &["--help"], b"");
    assert_eq!(status(&help), 0);
    assert!(stdout(&help).starts_with("Usage: unix2dos [options]"));
}
