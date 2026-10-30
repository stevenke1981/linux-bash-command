//! Part of the fsops CLI tests; helpers come from the parent module (`tests/fsops.rs`).

use super::*;

// ---------------------------------------------------------------- mv

#[test]
fn mv_help_and_version() {
    assert_help_and_version("mv");
}

#[test]
fn mv_renames_a_file() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    assert_eq!(code(&run_in(sb.path(), "mv", &["a", "b"], b"")), 0);
    assert!(!sb.path().join("a").exists());
    assert_eq!(sb.read("b"), b"x");
}

#[test]
fn mv_into_directory_and_target_option() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    sb.write("b", b"2");
    sb.mkdir("d");
    assert_eq!(code(&run_in(sb.path(), "mv", &["a", "b", "d"], b"")), 0);
    assert_eq!(sb.read("d/b"), b"2");
    sb.write("c", b"3");
    assert_eq!(code(&run_in(sb.path(), "mv", &["-t", "d", "c"], b"")), 0);
    assert_eq!(sb.read("d/c"), b"3");
}

#[test]
fn mv_verbose_reports_renames() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    let out = run_in(sb.path(), "mv", &["-v", "a", "b"], b"");
    assert_eq!(stdout(&out), "renamed 'a' -> 'b'\n");
}

#[test]
fn mv_no_clobber_reports_and_keeps_both() {
    let sb = Sandbox::new();
    sb.write("a", b"new");
    sb.write("b", b"old");
    let out = run_in(sb.path(), "mv", &["-n", "a", "b"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("mv: not replacing 'b'"));
    assert_eq!(sb.read("a"), b"new");
    assert_eq!(sb.read("b"), b"old");
}

#[test]
fn mv_non_empty_directory_destination_is_an_error() {
    let sb = Sandbox::new();
    sb.write("a/f", b"1");
    sb.write("b/g", b"2");
    let out = run_in(sb.path(), "mv", &["-T", "a", "b"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("mv: cannot move 'a' to 'b': Directory not empty"));
    let moved = run_in(sb.path(), "mv", &["a", "b"], b"");
    assert_eq!(code(&moved), 0);
    assert_eq!(sb.read("b/a/f"), b"1");
}

#[test]
fn mv_missing_source_and_operands() {
    let sb = Sandbox::new();
    let missing = run_in(sb.path(), "mv", &["nope", "x"], b"");
    assert_eq!(code(&missing), 1);
    assert!(stderr(&missing).contains("mv: cannot stat 'nope': No such file or directory"));
    assert!(stderr(&run_in(sb.path(), "mv", &[], b"")).contains("mv: missing file operand"));
}

#[test]
fn mv_interactive_answer_no_keeps_both() {
    let sb = Sandbox::new();
    sb.write("a", b"new");
    sb.write("b", b"old");
    assert_eq!(code(&run_in(sb.path(), "mv", &["-i", "a", "b"], b"n\n")), 1);
    assert_eq!(sb.read("a"), b"new");
    assert_eq!(sb.read("b"), b"old");
}

#[test]
fn mv_unknown_option_is_a_usage_error() {
    assert_unknown_option("mv");
}

// ---------------------------------------------------------------- touch

#[test]
fn touch_help_and_version() {
    assert_help_and_version("touch");
}

#[test]
fn touch_creates_an_empty_file() {
    let sb = Sandbox::new();
    assert_eq!(code(&run_in(sb.path(), "touch", &["new"], b"")), 0);
    assert_eq!(sb.read("new"), b"");
}

#[test]
fn touch_no_create_skips_missing_files() {
    let sb = Sandbox::new();
    assert_eq!(code(&run_in(sb.path(), "touch", &["-c", "nope"], b"")), 0);
    assert!(!sb.path().join("nope").exists());
}

#[test]
fn touch_sets_the_time_from_epoch_date() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    assert_eq!(
        code(&run_in(sb.path(), "touch", &["-d", "@1000000", "a"], b"")),
        0
    );
    let meta = fs::metadata(sb.path().join("a")).unwrap();
    let mtime = filetime::FileTime::from_last_modification_time(&meta);
    assert_eq!(mtime.unix_seconds(), 1_000_000);
}

#[test]
#[cfg(unix)]
fn touch_posix_stamp_is_read_in_utc() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .current_dir(sb.path())
        .env("TZ", "UTC")
        .args(["touch", "-t", "200102030405", "a"])
        .output()
        .expect("run touch");
    assert_eq!(code(&out), 0);
    let meta = fs::metadata(sb.path().join("a")).unwrap();
    let mtime = filetime::FileTime::from_last_modification_time(&meta);
    assert_eq!(mtime.unix_seconds(), 981_173_100);
}

#[test]
fn touch_reference_copies_times() {
    let sb = Sandbox::new();
    sb.write("ref", b"r");
    sb.write("a", b"x");
    let reference = fs::metadata(sb.path().join("ref")).unwrap();
    let want = filetime::FileTime::from_last_modification_time(&reference);
    assert_eq!(
        code(&run_in(sb.path(), "touch", &["-r", "ref", "a"], b"")),
        0
    );
    let got = filetime::FileTime::from_last_modification_time(
        &fs::metadata(sb.path().join("a")).unwrap(),
    );
    assert_eq!(got, want);
}

#[test]
fn touch_invalid_dates_and_sources_are_errors() {
    let sb = Sandbox::new();
    let bad = run_in(sb.path(), "touch", &["-d", "bogus", "a"], b"");
    assert_eq!(code(&bad), 1);
    assert!(stderr(&bad).contains("touch: invalid date format 'bogus'"));
    let two = run_in(
        sb.path(),
        "touch",
        &["-d", "@1", "-t", "200101010101", "a"],
        b"",
    );
    assert_eq!(code(&two), 1);
    assert!(stderr(&two).contains("cannot specify times from more than one source"));
    let missing_ref = run_in(sb.path(), "touch", &["-r", "nope", "a"], b"");
    assert!(stderr(&missing_ref).contains("failed to get attributes of 'nope'"));
}

#[test]
fn touch_missing_directory_and_bad_time_word() {
    let sb = Sandbox::new();
    let no_dir = run_in(sb.path(), "touch", &["nodir/x"], b"");
    assert_eq!(code(&no_dir), 1);
    assert!(stderr(&no_dir).contains("touch: cannot touch 'nodir/x': No such file or directory"));
    let bogus = run_in(sb.path(), "touch", &["--time=bogus", "a"], b"");
    assert!(stderr(&bogus).contains("invalid argument 'bogus' for '--time'"));
}

#[test]
fn touch_missing_operand_and_unknown_option() {
    let sb = Sandbox::new();
    assert!(stderr(&run_in(sb.path(), "touch", &[], b"")).contains("touch: missing file operand"));
    assert_unknown_option("touch");
}
