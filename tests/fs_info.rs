//! CLI tests for `date`, `stat`, `du`, `mktemp`, `realpath` and `readlink`. Every file
//! operation happens inside a `Sandbox` temporary directory.

mod common;

use std::fs;
use std::path::Path;

use common::{run, run_in, Sandbox};

fn text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &std::process::Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Canonical form of `path` as the commands print it (`\\?\` removed on Windows).
fn canonical_text(path: &Path) -> String {
    let real = fs::canonicalize(path).expect("canonicalize");
    let stripped = agentcmd::common::path::strip_verbatim(&real);
    agentcmd::common::path::display_path(&stripped)
}

// ---------------------------------------------------------------- date

#[test]
fn date_help_and_version() {
    let help = run("date", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: date"));
    let version = run("date", &["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(text(&version).starts_with("date (agentcmd) "));
}

#[test]
fn date_unknown_option_is_usage_error() {
    let out = run("date", &["--bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("unrecognized option '--bogus'"));
}

#[test]
fn date_formats_fixed_instant_in_utc() {
    let out = run(
        "date",
        &["-u", "-d", "2024-01-02 10:00 +0800", "+%F %T %Z"],
        b"",
    );
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out), "2024-01-02 02:00:00 UTC\n");
}

#[test]
fn date_default_format_in_utc() {
    let out = run("date", &["-u", "-d", "2024-01-02 10:00 UTC"], b"");
    assert_eq!(text(&out), "Tue Jan  2 10:00:00 UTC 2024\n");
}

#[test]
fn date_iso_and_rfc_output() {
    let iso = run("date", &["-u", "-d", "2024-01-02", "-I"], b"");
    assert_eq!(text(&iso), "2024-01-02\n");
    let seconds = run(
        "date",
        &["-u", "-d", "2024-01-02 10:00:00", "-Iseconds"],
        b"",
    );
    assert_eq!(text(&seconds), "2024-01-02T10:00:00+00:00\n");
    let rfc = run("date", &["-u", "-d", "2024-01-02 10:00:00", "-R"], b"");
    assert_eq!(text(&rfc), "Tue, 02 Jan 2024 10:00:00 +0000\n");
    let rfc3339 = run(
        "date",
        &["-u", "-d", "2024-01-02 10:00:00", "--rfc-3339=date"],
        b"",
    );
    assert_eq!(text(&rfc3339), "2024-01-02\n");
}

#[test]
fn date_invalid_string_reports_and_fails() {
    let out = run("date", &["-d", "bogus"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(err_text(&out), "date: invalid date 'bogus'\n");
}

#[test]
fn date_reads_dates_from_stdin_file() {
    let out = run("date", &["-u", "-f", "-", "+%F"], b"2024-01-02\n@0\n");
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out), "2024-01-02\n1970-01-01\n");
}

#[test]
fn date_bad_line_in_file_fails_but_continues() {
    let out = run("date", &["-u", "-f", "-", "+%F"], b"2024-01-02\nnope\n@0\n");
    assert_eq!(code(&out), 1);
    assert_eq!(text(&out), "2024-01-02\n1970-01-01\n");
    assert!(err_text(&out).contains("invalid date 'nope'"));
}

#[test]
fn date_reference_file_mtime_is_printed() {
    let sandbox = Sandbox::new();
    sandbox.write("ref.txt", b"x");
    let out = run_in(sandbox.path(), "date", &["-u", "-r", "ref.txt", "+%Y"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).trim().parse::<u32>().is_ok());
}

#[test]
fn date_refuses_to_set_the_clock() {
    let out = run("date", &["-s", "2024-01-02"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(
        err_text(&out),
        "date: cannot set date: Operation not permitted\n"
    );
}

#[test]
fn date_conflicting_output_formats_fail() {
    let out = run("date", &["-I", "-R"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("multiple output formats specified"));
}

#[test]
fn date_extra_operand_fails() {
    let out = run("date", &["+%Y", "+%m"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("extra operand '+%m'"));
}

// ---------------------------------------------------------------- stat

#[test]
fn stat_help_and_version() {
    let help = run("stat", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: stat"));
    let version = run("stat", &["--version"], b"");
    assert!(text(&version).starts_with("stat (agentcmd) "));
}

#[test]
fn stat_unknown_option_is_usage_error() {
    let out = run("stat", &["--bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("unrecognized option"));
}

#[test]
fn stat_without_operands_is_an_error() {
    let out = run("stat", &[], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).starts_with("stat: missing operand"));
}

#[test]
fn stat_missing_file_is_reported() {
    let sandbox = Sandbox::new();
    let out = run_in(sandbox.path(), "stat", &["-c", "%n", "nope"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("stat: cannot statx 'nope': No such file or directory"));
}

#[test]
fn stat_format_prints_size_and_name_per_file() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"hello\n");
    sandbox.write("b", b"xyz");
    let out = run_in(sandbox.path(), "stat", &["-c", "%s %n", "a.txt", "b"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out), "6 a.txt\n3 b\n");
}

#[test]
fn stat_printf_interprets_escapes_without_newline() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"hello\n");
    let out = run_in(sandbox.path(), "stat", &["--printf=%s\\t%n", "a.txt"], b"");
    assert_eq!(text(&out), "6\ta.txt");
}

#[test]
fn stat_terse_has_sixteen_fields() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"hello\n");
    let out = run_in(sandbox.path(), "stat", &["-t", "a.txt"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out).split_whitespace().count(), 16);
    assert!(text(&out).starts_with("a.txt 6 "));
}

#[test]
fn stat_default_layout_starts_with_file_line() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"hello\n");
    let out = run_in(sandbox.path(), "stat", &["a.txt"], b"");
    let shown = text(&out);
    assert!(shown.starts_with("  File: a.txt\n  Size: 6 "));
    assert!(shown.contains("regular file"));
    assert!(shown.contains(" Birth: "));
}

#[test]
fn stat_unknown_directive_prints_question_mark() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    let out = run_in(sandbox.path(), "stat", &["-c", "%q%%", "a.txt"], b"");
    assert_eq!(text(&out), "?%\n");
}

#[test]
fn stat_filesystem_format_has_name_and_block_size() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    let out = run_in(sandbox.path(), "stat", &["-f", "-c", "%n %l", "a.txt"], b"");
    if cfg!(unix) {
        assert_eq!(code(&out), 0);
        assert!(text(&out).starts_with("a.txt "));
    } else {
        assert_eq!(code(&out), 1);
        assert!(err_text(&out).contains("option '-f' is not supported"));
    }
}

#[cfg(unix)]
#[test]
fn stat_symlink_is_not_followed_unless_asked() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"hello\n");
    if sandbox.symlink("a.txt", "link").is_none() {
        return;
    }
    let plain = run_in(sandbox.path(), "stat", &["-c", "%F|%N", "link"], b"");
    assert_eq!(text(&plain), "symbolic link|'link' -> 'a.txt'\n");
    let followed = run_in(sandbox.path(), "stat", &["-L", "-c", "%F", "link"], b"");
    assert_eq!(text(&followed), "regular file\n");
}

// ---------------------------------------------------------------- du

#[test]
fn du_help_and_version() {
    let help = run("du", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: du"));
    let version = run("du", &["--version"], b"");
    assert!(text(&version).starts_with("du (agentcmd) "));
}

#[test]
fn du_unknown_option_is_usage_error() {
    let out = run("du", &["--bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("unrecognized option"));
}

#[test]
fn du_apparent_bytes_of_a_tree() {
    let sandbox = Sandbox::new();
    sandbox.write("a/f1", b"0123456789");
    sandbox.write("a/sub/f2", b"abc");
    let out = run_in(sandbox.path(), "du", &["-b", "a"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out), "3\ta/sub\n13\ta\n");
}

#[test]
fn du_all_lists_files_too() {
    let sandbox = Sandbox::new();
    sandbox.write("a/f1", b"0123456789");
    let out = run_in(sandbox.path(), "du", &["-ab", "a"], b"");
    assert_eq!(text(&out), "10\ta/f1\n10\ta\n");
}

#[test]
fn du_summarize_prints_only_operands() {
    let sandbox = Sandbox::new();
    sandbox.write("a/f1", b"0123456789");
    sandbox.write("a/sub/f2", b"abc");
    let out = run_in(sandbox.path(), "du", &["-sb", "a"], b"");
    assert_eq!(text(&out), "13\ta\n");
}

#[test]
fn du_total_line() {
    let sandbox = Sandbox::new();
    sandbox.write("a/f1", b"0123456789");
    let out = run_in(sandbox.path(), "du", &["-cb", "a"], b"");
    assert_eq!(text(&out), "10\ta\n10\ttotal\n");
}

#[test]
fn du_missing_operand_reported_and_others_counted() {
    let sandbox = Sandbox::new();
    sandbox.write("a/f1", b"0123456789");
    let out = run_in(sandbox.path(), "du", &["-sb", "nope", "a"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(text(&out), "10\ta\n");
    assert!(err_text(&out).contains("du: cannot access 'nope': No such file or directory"));
}

#[test]
fn du_rejects_bad_max_depth_and_block_size() {
    assert_eq!(code(&run("du", &["-d", "x", "."], b"")), 1);
    assert_eq!(code(&run("du", &["-B", "3q", "."], b"")), 1);
    assert_eq!(code(&run("du", &["--time=mtime", "."], b"")), 1);
}

#[test]
fn du_exclude_pattern_skips_matches() {
    let sandbox = Sandbox::new();
    sandbox.write("a/keep", b"abcdef");
    sandbox.write("a/skip.bin", b"abcdefghijklmnop");
    let out = run_in(sandbox.path(), "du", &["-ab", "--exclude=*.bin", "a"], b"");
    assert_eq!(text(&out), "6\ta/keep\n6\ta\n");
}

#[test]
fn du_nul_terminated_output() {
    let sandbox = Sandbox::new();
    sandbox.write("f", b"0123");
    let out = run_in(sandbox.path(), "du", &["-0", "-b", "f"], b"");
    assert_eq!(out.stdout, b"4\tf\0".to_vec());
}

#[test]
fn du_human_readable_apparent_size() {
    let sandbox = Sandbox::new();
    sandbox.write("big", &vec![b'x'; 1500]);
    let out = run_in(sandbox.path(), "du", &["-h", "--apparent-size", "big"], b"");
    assert_eq!(text(&out), "1.5K\tbig\n");
}

#[test]
fn du_default_block_mode_runs_on_a_directory() {
    let sandbox = Sandbox::new();
    sandbox.write("a/f", b"x");
    let out = run_in(sandbox.path(), "du", &["a"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).ends_with("\ta\n"));
}

// ---------------------------------------------------------------- mktemp

#[test]
fn mktemp_help_and_version() {
    let help = run("mktemp", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: mktemp"));
    let version = run("mktemp", &["--version"], b"");
    assert!(text(&version).starts_with("mktemp (agentcmd) "));
}

#[test]
fn mktemp_unknown_option_is_usage_error() {
    let out = run("mktemp", &["--bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("unrecognized option"));
}

#[test]
fn mktemp_creates_file_in_directory_template() {
    let sandbox = Sandbox::new();
    let template = sandbox.path().join("job.XXXXXX");
    let out = run("mktemp", &[template.to_str().unwrap()], b"");
    assert_eq!(code(&out), 0);
    let created = text(&out).trim_end().to_string();
    assert!(created.contains("job."));
    assert!(Path::new(&created).is_file());
}

#[test]
fn mktemp_directory_option_creates_directory() {
    let sandbox = Sandbox::new();
    let template = sandbox.path().join("d.XXXX");
    let out = run("mktemp", &["-d", template.to_str().unwrap()], b"");
    let created = text(&out).trim_end().to_string();
    assert!(Path::new(&created).is_dir());
}

#[test]
fn mktemp_dry_run_creates_nothing() {
    let sandbox = Sandbox::new();
    let template = sandbox.path().join("n.XXXX");
    let out = run("mktemp", &["-u", template.to_str().unwrap()], b"");
    assert_eq!(code(&out), 0);
    let name = text(&out).trim_end().to_string();
    assert!(!Path::new(&name).exists());
    assert_eq!(fs::read_dir(sandbox.path()).unwrap().count(), 0);
}

#[test]
fn mktemp_suffix_is_appended() {
    let sandbox = Sandbox::new();
    let template = sandbox.path().join("t.XXXX");
    let out = run(
        "mktemp",
        &["--suffix=.log", template.to_str().unwrap()],
        b"",
    );
    assert!(text(&out).trim_end().ends_with(".log"));
}

#[test]
fn mktemp_too_few_x_is_an_error() {
    let out = run("mktemp", &["-u", "XX"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(err_text(&out), "mktemp: too few X's in template 'XX'\n");
}

#[test]
fn mktemp_quiet_suppresses_creation_error() {
    let out = run("mktemp", &["-q", "/no-such-dir-agentcmd/x.XXX"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).is_empty());
}

#[test]
fn mktemp_missing_directory_is_reported() {
    let out = run("mktemp", &["/no-such-dir-agentcmd/x.XXX"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("failed to create file via template"));
}

#[test]
fn mktemp_p_option_uses_given_directory() {
    let sandbox = Sandbox::new();
    let dir = sandbox.path().to_str().unwrap().to_string();
    let out = run("mktemp", &["-p", &dir, "-d", "pXXXX"], b"");
    assert_eq!(code(&out), 0);
    let created = text(&out).trim_end().to_string();
    assert!(Path::new(&created).starts_with(sandbox.path()));
    assert!(Path::new(&created).is_dir());
}

#[test]
fn mktemp_rejects_two_templates() {
    let out = run("mktemp", &["-u", "a.XXX", "b.XXX"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("too many templates"));
}

#[test]
fn mktemp_single_component_rejects_directory_part() {
    let out = run("mktemp", &["-t", "sub/aXXX"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("contains directory separator"));
}

// ---------------------------------------------------------------- realpath

#[test]
fn realpath_help_and_version() {
    let help = run("realpath", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: realpath"));
    let version = run("realpath", &["--version"], b"");
    assert!(text(&version).starts_with("realpath (agentcmd) "));
}

#[test]
fn realpath_unknown_option_is_usage_error() {
    let out = run("realpath", &["--bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("unrecognized option"));
}

#[test]
fn realpath_prints_absolute_canonical_name() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    let out = run_in(sandbox.path(), "realpath", &["a.txt"], b"");
    assert_eq!(code(&out), 0);
    let expected = canonical_text(&sandbox.path().join("a.txt"));
    assert_eq!(text(&out), format!("{expected}\n"));
}

#[test]
fn realpath_relative_to_current_directory() {
    let sandbox = Sandbox::new();
    sandbox.write("sub/a.txt", b"x");
    let out = run_in(
        sandbox.path(),
        "realpath",
        &["--relative-to=.", "sub/a.txt"],
        b"",
    );
    assert_eq!(text(&out), "sub/a.txt\n");
}

#[test]
fn realpath_exists_mode_rejects_missing_name() {
    let sandbox = Sandbox::new();
    let out = run_in(sandbox.path(), "realpath", &["-e", "missing"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("realpath: missing: No such file or directory"));
}

#[test]
fn realpath_missing_mode_accepts_any_name() {
    let sandbox = Sandbox::new();
    let out = run_in(sandbox.path(), "realpath", &["-m", "x/y/z"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).trim_end().ends_with("x/y/z") || text(&out).trim_end().ends_with("x\\y\\z"));
}

#[test]
fn realpath_zero_terminates_lines() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    let out = run_in(
        sandbox.path(),
        "realpath",
        &["-z", "--relative-to=.", "a.txt"],
        b"",
    );
    assert_eq!(out.stdout, b"a.txt\0".to_vec());
}

#[test]
fn realpath_quiet_hides_message() {
    let sandbox = Sandbox::new();
    let out = run_in(sandbox.path(), "realpath", &["-q", "-e", "nope"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).is_empty());
}

#[test]
fn realpath_requires_an_operand() {
    let out = run("realpath", &[], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).starts_with("realpath: missing operand"));
}

#[cfg(unix)]
#[test]
fn realpath_follows_symlinks_unless_stripped() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    if sandbox.symlink("a.txt", "link").is_none() {
        return;
    }
    let resolved = run_in(
        sandbox.path(),
        "realpath",
        &["--relative-to=.", "link"],
        b"",
    );
    assert_eq!(text(&resolved), "a.txt\n");
    let kept = run_in(
        sandbox.path(),
        "realpath",
        &["-s", "--relative-to=.", "link"],
        b"",
    );
    assert_eq!(text(&kept), "link\n");
}

// ---------------------------------------------------------------- readlink

#[test]
fn readlink_help_and_version() {
    let help = run("readlink", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: readlink"));
    let version = run("readlink", &["--version"], b"");
    assert!(text(&version).starts_with("readlink (agentcmd) "));
}

#[test]
fn readlink_unknown_option_is_usage_error() {
    let out = run("readlink", &["--bogus"], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).contains("unrecognized option"));
}

#[cfg(unix)]
#[test]
fn readlink_prints_target_and_suffix_options() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    if sandbox.symlink("a.txt", "link").is_none() {
        return;
    }
    let plain = run_in(sandbox.path(), "readlink", &["link"], b"");
    assert_eq!(text(&plain), "a.txt\n");
    let no_newline = run_in(sandbox.path(), "readlink", &["-n", "link"], b"");
    assert_eq!(text(&no_newline), "a.txt");
    let zero = run_in(sandbox.path(), "readlink", &["-z", "link"], b"");
    assert_eq!(zero.stdout, b"a.txt\0".to_vec());
}

#[test]
fn readlink_regular_file_fails_silently_unless_verbose() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    let quiet = run_in(sandbox.path(), "readlink", &["a.txt"], b"");
    assert_eq!(code(&quiet), 1);
    assert!(err_text(&quiet).is_empty());
    let verbose = run_in(sandbox.path(), "readlink", &["-v", "a.txt"], b"");
    assert_eq!(code(&verbose), 1);
    assert!(err_text(&verbose).contains("a.txt: Invalid argument"));
}

#[test]
fn readlink_canonicalize_missing_prints_name() {
    let sandbox = Sandbox::new();
    let out = run_in(sandbox.path(), "readlink", &["-m", "x/y"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).trim_end().ends_with("y"));
}

#[test]
fn readlink_canonicalize_existing_fails_for_missing() {
    let sandbox = Sandbox::new();
    let out = run_in(sandbox.path(), "readlink", &["-e", "missing"], b"");
    assert_eq!(code(&out), 1);
    assert!(text(&out).is_empty());
}

#[test]
fn readlink_canonicalize_existing_file_matches_realpath() {
    let sandbox = Sandbox::new();
    sandbox.write("a.txt", b"x");
    let out = run_in(sandbox.path(), "readlink", &["-f", "a.txt"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(
        text(&out),
        format!("{}\n", canonical_text(&sandbox.path().join("a.txt")))
    );
}

#[test]
fn readlink_requires_an_operand() {
    let out = run("readlink", &[], b"");
    assert_eq!(code(&out), 1);
    assert!(err_text(&out).starts_with("readlink: missing operand"));
}
