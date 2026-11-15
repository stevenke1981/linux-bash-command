mod common;
use common::Sandbox;
use std::process::{Command, Output};
const CHANGE: &[u8] = b"--- a\n+++ a\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
fn run(s: &Sandbox, args: &[&str], input: &[u8]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_patch"));
    c.args(args).current_dir(&s.path);
    common::execute(c, input)
}
fn good(out: &Output) {
    assert!(out.status.success(), "{out:?}");
}
#[test]
fn information_and_usage() {
    let s = Sandbox::new();
    for flag in ["--help", "--version"] {
        good(&run(&s, &[flag], b""));
    }
    assert_eq!(run(&s, &["--unknown"], b"").status.code(), Some(2));
    assert_eq!(run(&s, &[], b"not a patch").status.code(), Some(2));
}
#[test]
fn applies_unified_input_and_explicit_input_file() {
    let s = Sandbox::new();
    s.write("a", b"one\ntwo\nthree\n");
    let out = run(&s, &[], CHANGE);
    good(&out);
    assert_eq!(out.stdout, b"patching file a\n");
    assert_eq!(
        std::fs::read(s.path.join("a")).unwrap(),
        b"one\nTWO\nthree\n"
    );
    s.write("script", CHANGE);
    good(&run(&s, &["-R", "-i", "script"], b""));
    assert_eq!(
        std::fs::read(s.path.join("a")).unwrap(),
        b"one\ntwo\nthree\n"
    );
}
#[test]
fn dry_run_does_not_write_backups_or_rejects() {
    let s = Sandbox::new();
    s.write("a", b"one\ntwo\nthree\n");
    good(&run(&s, &["--dry-run", "--backup"], CHANGE));
    assert_eq!(
        std::fs::read(s.path.join("a")).unwrap(),
        b"one\ntwo\nthree\n"
    );
    assert!(!s.path.join("a.orig").exists());
    assert_eq!(std::fs::read_dir(&s.path).unwrap().count(), 1);
}
#[test]
fn backups_output_and_no_backup_if_mismatch() {
    let s = Sandbox::new();
    s.write("a", b"one\ntwo\nthree\n");
    good(&run(&s, &["-b"], CHANGE));
    assert_eq!(
        std::fs::read(s.path.join("a.orig")).unwrap(),
        b"one\ntwo\nthree\n"
    );
    good(&run(&s, &["-R", "-o", "out"], CHANGE));
    assert_eq!(
        std::fs::read(s.path.join("out")).unwrap(),
        b"one\ntwo\nthree\n"
    );
    assert_eq!(
        std::fs::read(s.path.join("a")).unwrap(),
        b"one\nTWO\nthree\n"
    );
}
#[test]
fn offset_and_fuzz_find_original_context() {
    let s = Sandbox::new();
    s.write("a", b"prefix\none\ntwo\nthree\n");
    let out = run(&s, &["--no-backup-if-mismatch"], CHANGE);
    good(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("offset 1 line"));
    assert_eq!(
        std::fs::read(s.path.join("a")).unwrap(),
        b"prefix\none\nTWO\nthree\n"
    );
    assert!(!s.path.join("a.orig").exists());
    s.write("a", b"changed\ntwo\nchanged\n");
    good(&run(&s, &["-F1"], CHANGE));
    assert_eq!(
        std::fs::read(s.path.join("a")).unwrap(),
        b"changed\nTWO\nchanged\n"
    );
}
#[test]
fn rejected_hunks_keep_other_lines_and_create_reject_file() {
    let s = Sandbox::new();
    s.write("a", b"different\n");
    let out = run(&s, &["--reject-file=rejected"], CHANGE);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(s.path.join("a")).unwrap(), b"different\n");
    let rejected = std::fs::read(s.path.join("rejected")).unwrap();
    assert!(rejected.starts_with(b"--- a\n+++ a\n"));
    assert!(!std::fs::read_dir(&s.path).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".agentcmd-")));
}
#[test]
fn strip_directory_create_delete_and_empty_file() {
    let s = Sandbox::new();
    s.write("dir/a", b"one\ntwo\nthree\n");
    let script = String::from_utf8(CHANGE.to_vec())
        .unwrap()
        .replace("--- a", "--- old/a")
        .replace("+++ a", "+++ new/a");
    good(&run(&s, &["-p1", "-d", "dir"], script.as_bytes()));
    assert_eq!(
        std::fs::read(s.path.join("dir/a")).unwrap(),
        b"one\nTWO\nthree\n"
    );
    let create = b"--- /dev/null\n+++ created\n@@ -0,0 +1 @@\n+hello\n";
    good(&run(&s, &[], create));
    assert_eq!(std::fs::read(s.path.join("created")).unwrap(), b"hello\n");
    good(&run(&s, &["-R"], create));
    assert!(!s.path.join("created").exists());
    s.write("x", b"x\n");
    good(&run(&s, &["-E"], b"--- x\n+++ x\n@@ -1 +0,0 @@\n-x\n"));
    assert!(!s.path.join("x").exists());
}
#[test]
fn crlf_whitespace_and_missing_newlines() {
    let s = Sandbox::new();
    s.write("a", b"one\r\ntwo\r\nthree\r\n");
    good(&run(&s, &[], CHANGE));
    assert_eq!(
        std::fs::read(s.path.join("a")).unwrap(),
        b"one\r\nTWO\r\nthree\r\n"
    );
    s.write("a", b"one  two\n");
    good(&run(
        &s,
        &["-l"],
        b"--- a\n+++ a\n@@ -1 +1 @@\n-one two\n+new line\n",
    ));
    assert_eq!(std::fs::read(s.path.join("a")).unwrap(), b"new line\n");
    s.write("a", b"old");
    good(&run(&s,&[],b"--- a\n+++ a\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n"));
    assert_eq!(std::fs::read(s.path.join("a")).unwrap(), b"new");
}
#[test]
fn safety_and_unsupported_formats_preserve_input() {
    let s = Sandbox::new();
    s.write("a", b"original\n");
    for patch in [
        b"*** a\n--- a\n".as_slice(),
        b"GIT binary patch\n",
        b"diff --git a/a b/b\nrename from a\nrename to b\n",
        b"--- ../a\n+++ ../a\n@@ -1 +1 @@\n-original\n+new\n",
    ] {
        assert_eq!(run(&s, &[], patch).status.code(), Some(2));
        assert_eq!(std::fs::read(s.path.join("a")).unwrap(), b"original\n");
    }
}
#[test]
fn separate_file_patches_concatenate_explicit_output() {
    let s = Sandbox::new();
    s.write("a", b"a\n");
    s.write("b", b"b\n");
    let patch = b"--- a\n+++ a\n@@ -1 +1 @@\n-a\n+A\n--- b\n+++ b\n@@ -1 +1 @@\n-b\n+B\n";
    good(&run(&s, &["-o", "out"], patch));
    assert_eq!(std::fs::read(s.path.join("out")).unwrap(), b"A\nB\n");
    assert_eq!(std::fs::read(s.path.join("a")).unwrap(), b"a\n");
}

#[test]
fn chooses_existing_new_header_and_explicit_original() {
    let s = Sandbox::new();
    s.write("new", b"old\n");
    let diff = b"--- a/missing\n+++ b/new\n@@ -1 +1 @@\n-old\n+new\n";
    let out = run(&s, &["-p1"], diff);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"patching file new\n");
    assert_eq!(std::fs::read(s.path.join("new")).unwrap(), b"new\n");
    s.write("explicit", b"old\n");
    good(&run(
        &s,
        &["-p1", "explicit"],
        b"--- absent\n+++ absent\n@@ -1 +1 @@\n-old\n+new\n",
    ));
    assert_eq!(std::fs::read(s.path.join("explicit")).unwrap(), b"new\n");
}
#[test]
fn creation_collision_does_not_modify_existing_or_backup() {
    let s = Sandbox::new();
    s.write("f", b"valuable\n");
    s.write("f.orig", b"backup\n");
    let out = run(
        &s,
        &["--batch"],
        b"--- /dev/null\n+++ f\n@@ -0,0 +1 @@\n+new\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"valuable\n");
    assert_eq!(std::fs::read(s.path.join("f.orig")).unwrap(), b"backup\n");
}
#[test]
fn reject_names_follow_directory_and_explicit_output() {
    let s = Sandbox::new();
    s.write("dir/f", b"different\n");
    let diff = b"--- f\n+++ f\n@@ -1 +1 @@\n-old\n+new\n";
    let out = run(&s, &["-d", "dir", "-r", "rejected"], diff);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(s.path.join("dir/rejected")).unwrap(), diff);
    assert!(!s.path.join("rejected").exists());
    let out = run(&s, &["-d", "dir", "-o", "out"], diff);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(s.path.join("dir/out.rej")).unwrap(), diff);
    assert!(!s.path.join("dir/f.rej").exists());
}
#[test]
fn ignored_patches_preserve_original_and_existing_backup() {
    let diff = b"--- f\n+++ f\n@@ -1 +1 @@\n-old\n+new\n";
    for flag in ["-N", "-t"] {
        let s = Sandbox::new();
        s.write("f", b"new\n");
        s.write("f.orig", b"valuable backup");
        let out = run(&s, &[flag], diff);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stdout).contains("hunk ignored"));
        assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"new\n");
        assert_eq!(
            std::fs::read(s.path.join("f.orig")).unwrap(),
            b"valuable backup"
        );
        assert_eq!(std::fs::read(s.path.join("f.rej")).unwrap(), diff);
    }
}
#[test]
fn force_disables_reverse_detection() {
    let s = Sandbox::new();
    s.write("f", b"new\n");
    let out = run(
        &s,
        &["-f", "-N"],
        b"--- f\n+++ f\n@@ -1 +1 @@\n-old\n+new\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Hunk #1 FAILED"));
    assert_eq!(std::fs::read(s.path.join("f.orig")).unwrap(), b"new\n");
}
#[test]
fn discard_rejects_and_preserve_all_multiple_rejects() {
    let s = Sandbox::new();
    s.write("a", b"a\n");
    s.write("b", b"b\n");
    let diff = b"--- a\n+++ a\n@@ -1 +1 @@\n-x\n+X\n--- b\n+++ b\n@@ -1 +1 @@\n-y\n+Y\n";
    assert_eq!(run(&s, &["-o", "out"], diff).status.code(), Some(1));
    assert_eq!(std::fs::read(s.path.join("out.rej")).unwrap(), diff);
    assert_eq!(run(&s, &["-r", "-"], diff).status.code(), Some(1));
    assert!(!s.path.join("-").exists());
}
#[test]
fn reject_modes_cover_long_options_and_dry_run() {
    let s = Sandbox::new();
    s.write("f", b"bad\n");
    let diff = b"--- f\n+++ f\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(
        run(&s, &["--no-backup-if-mismatch", "--reject-file=rej"], diff)
            .status
            .code(),
        Some(1)
    );
    assert!(!s.path.join("f.orig").exists());
    assert_eq!(std::fs::read(s.path.join("rej")).unwrap(), diff);
    assert_eq!(
        run(&s, &["--dry-run", "--binary", "-u"], diff)
            .status
            .code(),
        Some(1)
    );
}
#[cfg(unix)]
#[test]
fn quoted_non_utf8_paths_preserve_raw_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let s = Sandbox::new();
    let name = std::ffi::OsString::from_vec(b"f\xff".to_vec());
    if let Err(error) = std::fs::write(s.path.join(&name), b"old\n") {
        if error.raw_os_error() == Some(libc::EILSEQ) {
            eprintln!("filesystem does not support non-UTF-8 filenames: {error}");
            return;
        }
        panic!("write raw patch fixture: {error}");
    }
    let diff = b"--- \"a/f\\377\"\n+++ \"b/f\\377\"\n@@ -1 +1 @@\n-old\n+new\n";
    good(&run(&s, &["-p1"], diff));
    assert_eq!(std::fs::read(s.path.join(&name)).unwrap(), b"new\n");
    let out = run(&s, &["-f", "-p1"], diff);
    assert_eq!(out.status.code(), Some(1));
    let mut reject = name;
    reject.push(".rej");
    assert!(s.path.join(reject).exists());
}

#[test]
fn force_creation_handles_empty_and_nonempty_existing_files() {
    let s = Sandbox::new();
    let diff = b"--- /dev/null\n+++ f\n@@ -0,0 +1 @@\n+NEW\n";
    s.write("f", b"");
    good(&run(&s, &["-f"], diff));
    assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"NEW\n");
    s.write("f", b"");
    good(&run(&s, &["-f", "-b"], diff));
    assert_eq!(std::fs::read(s.path.join("f.orig")).unwrap(), b"");
    s.write("f", b"old\n");
    s.write("f.orig", b"valuable");
    assert_eq!(run(&s, &["-f"], diff).status.code(), Some(1));
    assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"old\n");
    assert_eq!(std::fs::read(s.path.join("f.orig")).unwrap(), b"old\n");
    assert_eq!(std::fs::read(s.path.join("f.rej")).unwrap(), diff);
}
#[test]
fn first_reversed_hunk_skips_every_hunk() {
    let diff = b"--- f\n+++ f\n@@ -1 +1 @@\n-a\n+A\n@@ -4 +4 @@\n-d\n+D\n";
    for flag in ["-N", "-t"] {
        let s = Sandbox::new();
        s.write("f", b"A\nb\nc\nd\n");
        s.write("f.orig", b"precious");
        let out = run(&s, &[flag], diff);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stdout).contains("2 out of 2 hunks ignored"));
        assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"A\nb\nc\nd\n");
        assert_eq!(std::fs::read(s.path.join("f.orig")).unwrap(), b"precious");
        assert_eq!(std::fs::read(s.path.join("f.rej")).unwrap(), diff);
    }
}
#[test]
fn git_empty_creation_deletion_and_reverse() {
    let s = Sandbox::new();
    let create = b"diff --git a/f b/f\nnew file mode 100755\nindex 0000000..e69de29\n";
    let delete = b"diff --git a/f b/f\ndeleted file mode 100755\nindex e69de29..0000000\n";
    good(&run(&s, &["-p1"], create));
    assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"");
    good(&run(&s, &["-p1"], delete));
    assert!(!s.path.join("f").exists());
    good(&run(&s, &["-p1", "-R"], delete));
    assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"");
}
#[test]
fn no_newline_marker_preserves_literal_carriage_return() {
    let s = Sandbox::new();
    s.write("f", b"old\n");
    let diff = b"--- f\n+++ f\n@@ -1 +1 @@\n-old\n+new\r\n\\ No newline at end of file\n";
    good(&run(&s, &[], diff));
    assert_eq!(std::fs::read(s.path.join("f")).unwrap(), b"new\r");
}
#[cfg(unix)]
#[test]
fn git_new_file_mode_sets_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    let diff = b"diff --git a/f b/f\nnew file mode 100755\nindex 0000000..1234567\n--- /dev/null\n+++ b/f\n@@ -0,0 +1 @@\n+#!/bin/sh\n";
    good(&run(&s, &["-p1"], diff));
    let mode = std::fs::metadata(s.path.join("f"))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o100, 0);
}

#[test]
fn git_empty_file_with_spaces_in_header() {
    let s = Sandbox::new();
    let diff = b"diff --git a/x y b/x y\nnew file mode 100644\nindex 0000000..e69de29\n";
    good(&run(&s, &["-p1"], diff));
    assert_eq!(std::fs::read(s.path.join("x y")).unwrap(), b"");
}

#[cfg(unix)]
#[test]
fn git_empty_mode_only_creates_executable_file() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    let diff = b"diff --git a/f b/f\nnew file mode 100755\nindex 0000000..e69de29\n";
    good(&run(&s, &["-p1"], diff));
    let metadata = std::fs::metadata(s.path.join("f")).unwrap();
    assert_eq!(metadata.len(), 0);
    assert_ne!(metadata.permissions().mode() & 0o100, 0);
}
