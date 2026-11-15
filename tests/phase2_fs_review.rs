mod common;
use common::Sandbox;
use std::{fs, process::Command};

fn run(s: &Sandbox, name: &str, args: &[&str], stdin: &[u8]) -> std::process::Output {
    let path = match name {
        "cp" => env!("CARGO_BIN_EXE_cp"),
        "mv" => env!("CARGO_BIN_EXE_mv"),
        "ln" => env!("CARGO_BIN_EXE_ln"),
        "date" => env!("CARGO_BIN_EXE_date"),
        _ => panic!("unknown command"),
    };
    let mut cmd = Command::new(path);
    cmd.args(args)
        .current_dir(&s.path)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env_remove("VERSION_CONTROL")
        .env_remove("SIMPLE_BACKUP_SUFFIX");
    common::execute(cmd, stdin)
}

#[test]
fn declining_overwrite_sets_failure_and_preserves_files() {
    for name in ["cp", "mv", "ln"] {
        let s = Sandbox::new();
        s.write("a", b"source");
        s.write("b", b"destination");
        let out = run(&s, name, &["-i", "a", "b"], b"n\n");
        assert_eq!(out.status.code(), Some(1), "{name}: {out:?}");
        assert_eq!(fs::read(s.path.join("a")).unwrap(), b"source");
        assert_eq!(fs::read(s.path.join("b")).unwrap(), b"destination");
    }
}

#[test]
fn move_no_clobber_reports_failure() {
    let s = Sandbox::new();
    s.write("a", b"source");
    s.write("b", b"destination");
    let out = run(&s, "mv", &["-n", "a", "b"], b"");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    assert_eq!(fs::read(s.path.join("a")).unwrap(), b"source");
    assert_eq!(fs::read(s.path.join("b")).unwrap(), b"destination");
}

#[test]
fn link_backup_over_source_preserves_both_contents() {
    let s = Sandbox::new();
    s.write("x", b"old");
    s.write("x~", b"new");
    let out = run(&s, "ln", &["-b", "x~", "x"], b"");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(s.path.join("x")).unwrap(), b"new");
    assert_eq!(fs::read(s.path.join("x~")).unwrap(), b"old");
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 2);
}

#[test]
fn link_backup_over_hardlink_alias_preserves_source() {
    let s = Sandbox::new();
    s.write("x", b"old");
    s.write("source", b"new");
    fs::hard_link(s.path.join("source"), s.path.join("x~")).unwrap();
    let out = run(&s, "ln", &["-b", "source", "x"], b"");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(s.path.join("x")).unwrap(), b"new");
    assert_eq!(fs::read(s.path.join("source")).unwrap(), b"new");
    assert_eq!(fs::read(s.path.join("x~")).unwrap(), b"old");
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 3);
}

#[test]
fn verbose_hard_link_uses_gnu_arrow() {
    let s = Sandbox::new();
    s.write("a", b"source");
    let out = run(&s, "ln", &["-v", "a", "b"], b"");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"'b' => 'a'\n");
}

#[test]
fn negative_fractional_timestamp_uses_floor_seconds() {
    let s = Sandbox::new();
    for (value, expected) in [
        ("@-0.123456789123", "-1.876543210\n"),
        ("@-1", "-1.000000000\n"),
        ("@0.5", "0.500000000\n"),
    ] {
        let out = run(&s, "date", &["-u", "-d", value, "+%s.%N"], b"");
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, expected.as_bytes());
    }
}
