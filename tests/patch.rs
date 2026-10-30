//! CLI tests for `patch`: options, exit codes, messages, rejects, backups and the
//! path-safety rule. Every test works in its own temporary sandbox.

mod common;

use common::{run, run_in, Sandbox};

const SIMPLE: &str = "--- t.txt\n+++ t.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn code(out: &std::process::Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

#[test]
fn help_and_version_print_to_stdout() {
    let out = run("patch", &["--help"], b"");
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).starts_with("Usage: patch"));
    let out = run("patch", &["--version"], b"");
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).starts_with("patch (agentcmd) "));
}

#[test]
fn unknown_option_and_bad_strip_are_serious_errors() {
    let out = run("patch", &["--bogus"], b"");
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("unrecognized option '--bogus'"));
    let out = run("patch", &["-p", "x"], b"");
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("strip count 'x' is not a number"));
}

#[test]
fn applies_a_unified_diff_read_with_dash_i() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo\nthree\n");
    sb.write("p.diff", SIMPLE.as_bytes());
    let out = run_in(sb.path(), "patch", &["-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "patching file t.txt\n");
    assert_eq!(sb.read("t.txt"), b"one\nTWO\nthree\n");
    assert!(!sb.path().join("t.txt.orig").exists());
}

#[test]
fn patch_file_can_be_read_from_stdin() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo\nthree\n");
    let out = run_in(sb.path(), "patch", &["t.txt"], SIMPLE.as_bytes());
    assert_eq!(code(&out), 0);
    assert_eq!(sb.read("t.txt"), b"one\nTWO\nthree\n");
}

#[test]
fn missing_patch_file_and_garbage_input_exit_two() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "patch", &["-i", "nosuch.diff"], b"");
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("Can't open patch file nosuch.diff"));
    let out = run_in(sb.path(), "patch", &[], b"this is not a patch\n");
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("Only garbage was found in the patch input."));
}

#[test]
fn failed_hunks_are_rejected_and_exit_one() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo\nthree\n");
    sb.write(
        "p.diff",
        b"--- t.txt\n+++ t.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n@@ -3,2 +3,2 @@\n nope\n-zzz\n+ZZZ\n",
    );
    let out = run_in(sb.path(), "patch", &["-i", "p.diff"], b"");
    assert_eq!(code(&out), 1);
    let text = stdout(&out);
    assert!(text.contains("Hunk #2 FAILED at 3."), "{text}");
    assert!(
        text.contains("1 out of 2 hunks FAILED -- saving rejects to file t.txt.rej"),
        "{text}"
    );
    assert_eq!(
        sb.read("t.txt"),
        b"one\nTWO\nthree\n",
        "the good hunk is still applied"
    );
    let rej = String::from_utf8(sb.read("t.txt.rej")).unwrap();
    assert!(
        rej.starts_with("--- t.txt\n+++ t.txt\n@@ -3,2 +3,2 @@\n"),
        "{rej}"
    );
}

#[test]
fn offset_and_fuzz_are_reported_and_backed_up() {
    let sb = Sandbox::new();
    sb.write("g.txt", b"pre1\npre2\npre3\na\nb\nc\nd\ne\n");
    sb.write(
        "p.diff",
        b"--- g.txt\n+++ g.txt\n@@ -2,4 +2,4 @@\n b\n-c\n+C\n d\n e\n",
    );
    let out = run_in(sb.path(), "patch", &["-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("Hunk #1 succeeded at 5 (offset 3 lines)."));
    assert_eq!(sb.read("g.txt.orig"), b"pre1\npre2\npre3\na\nb\nc\nd\ne\n");
}

#[test]
fn no_backup_if_mismatch_suppresses_the_backup() {
    let sb = Sandbox::new();
    sb.write("g.txt", b"pre\npre\na\nb\nc\nd\n");
    sb.write(
        "p.diff",
        b"--- g.txt\n+++ g.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n",
    );
    let out = run_in(
        sb.path(),
        "patch",
        &["--no-backup-if-mismatch", "-i", "p.diff"],
        b"",
    );
    assert_eq!(code(&out), 0);
    assert!(!sb.path().join("g.txt.orig").exists());
}

#[test]
fn dry_run_and_output_option_leave_the_target_alone() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo\nthree\n");
    sb.write("p.diff", SIMPLE.as_bytes());
    let out = run_in(sb.path(), "patch", &["--dry-run", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "checking file t.txt\n");
    assert_eq!(sb.read("t.txt"), b"one\ntwo\nthree\n");
    let out = run_in(sb.path(), "patch", &["-o", "out.txt", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).starts_with("patching file out.txt (read from t.txt)"));
    assert_eq!(sb.read("out.txt"), b"one\nTWO\nthree\n");
    assert_eq!(sb.read("t.txt"), b"one\ntwo\nthree\n");
}

#[test]
fn already_applied_patch_is_skipped_unless_assumed_reversed() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\nTWO\nthree\n");
    sb.write("p.diff", SIMPLE.as_bytes());
    let out = run_in(sb.path(), "patch", &["-i", "p.diff"], b"");
    assert_eq!(code(&out), 1);
    assert!(
        stdout(&out).contains("Reversed (or previously applied) patch detected!  Assume -R? [n]")
    );
    let out = run_in(sb.path(), "patch", &["-N", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 1);
    assert!(stdout(&out).contains("Skipping patch."));
    let out = run_in(sb.path(), "patch", &["-t", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("Assuming -R."));
    assert_eq!(sb.read("t.txt"), b"one\ntwo\nthree\n");
}

#[test]
fn force_quiet_binary_and_unified_options_are_accepted() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\nTWO\nthree\n");
    sb.write("p.diff", SIMPLE.as_bytes());
    // -f skips the reversed-patch check, so the hunk fails; --quiet keeps only the summary.
    let out = run_in(
        sb.path(),
        "patch",
        &["-f", "--quiet", "--binary", "-u", "-i", "p.diff"],
        b"",
    );
    assert_eq!(code(&out), 1);
    assert_eq!(
        stdout(&out),
        "1 out of 1 hunk FAILED -- saving rejects to file t.txt.rej\n"
    );
    assert_eq!(sb.read("t.txt"), b"one\nTWO\nthree\n");
}

#[test]
fn reverse_option_undoes_a_patch() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\nTWO\nthree\n");
    sb.write("p.diff", SIMPLE.as_bytes());
    let out = run_in(sb.path(), "patch", &["-R", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(sb.read("t.txt"), b"one\ntwo\nthree\n");
}

#[test]
fn git_headers_create_and_delete_files() {
    let sb = Sandbox::new();
    sb.write("old.txt", b"hello\nworld\n");
    sb.write(
        "p.diff",
        b"diff --git a/new.txt b/new.txt\nnew file mode 100644\nindex 0000000..ce01362\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+hello\n+world\ndiff --git a/old.txt b/old.txt\ndeleted file mode 100644\nindex ce01362..0000000\n--- a/old.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-hello\n-world\n",
    );
    let out = run_in(sb.path(), "patch", &["-p1", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(sb.read("new.txt"), b"hello\nworld\n");
    assert!(!sb.path().join("old.txt").exists());
}

#[test]
fn directory_option_changes_the_base_directory() {
    let sb = Sandbox::new();
    sb.mkdir("sub");
    sb.write("sub/t.txt", b"one\ntwo\nthree\n");
    sb.write("p.diff", SIMPLE.as_bytes());
    let out = run_in(sb.path(), "patch", &["-d", "sub", "-i", "../p.diff"], b"");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(sb.read("sub/t.txt"), b"one\nTWO\nthree\n");
}

#[test]
fn remove_empty_files_option_deletes_emptied_targets() {
    let sb = Sandbox::new();
    sb.write("g", b"x\n");
    sb.write("p.diff", b"--- g\n+++ g\n@@ -1 +0,0 @@\n-x\n");
    let out = run_in(sb.path(), "patch", &["-E", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert!(!sb.path().join("g").exists());
}

#[test]
fn parent_directory_and_absolute_names_are_refused() {
    let sb = Sandbox::new();
    sb.mkdir("work");
    sb.write(
        "p.diff",
        b"--- ../outside.txt\n+++ ../outside.txt\n@@ -1 +1 @@\n-a\n+b\n",
    );
    sb.write("work/outside.txt", b"a\n");
    let out = run_in(
        &sb.path().join("work"),
        "patch",
        &["-p0", "-i", "../p.diff"],
        b"",
    );
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("refusing to patch '../outside.txt'"));
    assert_eq!(sb.read("work/outside.txt"), b"a\n");

    let abs = "/agentcmd-never-created-target.txt";
    let text = format!("--- {abs}\n+++ {abs}\n@@ -1 +1 @@\n-a\n+b\n");
    sb.write("abs.diff", text.as_bytes());
    let out = run_in(sb.path(), "patch", &["-p0", "-i", "abs.diff"], b"");
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("refusing to patch"));
    assert!(!std::path::Path::new(abs).exists());
}

#[test]
fn context_diffs_are_reported_as_unsupported() {
    let sb = Sandbox::new();
    sb.write(
        "p.diff",
        b"*** t.txt\n--- t.txt\n***************\n*** 1 ****\n! one\n--- 1 ----\n! ONE\n",
    );
    let out = run_in(sb.path(), "patch", &["-i", "p.diff"], b"");
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("context diffs are not supported by agentcmd"));
}

#[test]
fn silent_prints_only_summary_lines() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo\nthree\n");
    sb.write(
        "p.diff",
        b"--- t.txt\n+++ t.txt\n@@ -1 +1 @@\n-nope\n+yes\n",
    );
    let out = run_in(sb.path(), "patch", &["-s", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(
        stdout(&out),
        "1 out of 1 hunk FAILED -- saving rejects to file t.txt.rej\n"
    );
}

#[test]
fn whitespace_mode_matches_runs_of_blanks() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo   words\nthree\n");
    sb.write(
        "p.diff",
        b"--- t.txt\n+++ t.txt\n@@ -1,3 +1,3 @@\n one\n-two words\n+TWO\n three\n",
    );
    let out = run_in(sb.path(), "patch", &["-l", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(sb.read("t.txt"), b"one\nTWO\nthree\n");
}

#[test]
fn backup_suffix_option_names_the_copy() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo\nthree\n");
    sb.write("p.diff", SIMPLE.as_bytes());
    let out = run_in(
        sb.path(),
        "patch",
        &["-b", "-z", ".bk", "-i", "p.diff"],
        b"",
    );
    assert_eq!(code(&out), 0);
    assert_eq!(sb.read("t.txt.bk"), b"one\ntwo\nthree\n");
}

#[test]
fn reject_file_option_collects_rejects_in_one_file() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"one\ntwo\nthree\n");
    sb.write("p.diff", b"--- t.txt\n+++ t.txt\n@@ -1 +1 @@\n-x\n+y\n");
    let out = run_in(sb.path(), "patch", &["-r", "all.rej", "-i", "p.diff"], b"");
    assert_eq!(code(&out), 1);
    assert!(stdout(&out).contains("saving rejects to file all.rej"));
    assert!(sb
        .read("all.rej")
        .starts_with(b"--- t.txt\n+++ t.txt\n@@ -1 +1 @@\n-x\n+y\n"));
}
