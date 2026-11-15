mod common;

use common::{run, Sandbox};
use std::process::Command;

#[test]
fn legacy_commands_report_their_version() {
    for name in "search diff list read rg ls pwd cd cat less head tail grep find ps df curl"
        .split_whitespace()
    {
        let output = run(name, &["--version"], b"");
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("{name} (agentcmd) {}\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
    }
}

#[test]
fn file_helpers_accept_unique_long_prefixes_and_attached_values() {
    let sandbox = Sandbox::new();
    sandbox.write("sample", b"one\ntwo\nthree\n");
    let output = sandbox.run("read", &["--sta=2", "--lim", "1", "sample"], b"");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"     2\ttwo\n");
    let output = sandbox.run("list", &["--max-d=0"], b"");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
    for name in ["read", "list", "diff", "rg", "search"] {
        let output = sandbox.run(name, &["--vers"], b"");
        assert!(output.status.success(), "{name}: {output:?}");
    }
}

#[test]
fn dash_dash_and_option_values_do_not_trigger_version() {
    let sandbox = Sandbox::new();
    sandbox.write("--version", b"version is a file\n");
    let output = sandbox.run("read", &["--", "--version"], b"");
    assert_eq!(output.stdout, b"     1\tversion is a file\n");
    assert!(output.status.success());
    let output = sandbox.run("rg", &["-e", "--version", "--version"], b"");
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("(agentcmd)"));
    let output = sandbox.run("rg", &["-e", "--version", "--", "--version"], b"");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn posix_stops_legacy_option_parsing_at_the_first_operand() {
    let sandbox = Sandbox::new();
    sandbox.write("sample", b"line\n");
    for (name, args) in [
        ("read", vec!["sample", "--version"]),
        ("list", vec![".", "--version"]),
        ("diff", vec!["sample", "sample", "--version"]),
        ("rg", vec!["line", "--version"]),
        ("search", vec!["line", "--version"]),
    ] {
        let output = Command::new(common::binary(name))
            .args(args)
            .env("POSIXLY_CORRECT", "1")
            .env("AGENTCMD_COLOR", "never")
            .current_dir(&sandbox.path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{name}: {output:?}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("(agentcmd)"));
    }
}

#[test]
fn missing_file_messages_do_not_expose_rust_os_codes() {
    for (name, args) in [
        ("list", vec!["missing"]),
        ("read", vec!["missing"]),
        ("diff", vec!["missing", "other"]),
        ("rg", vec!["x", "missing"]),
        ("rg", vec!["-f", "missing"]),
        ("search", vec!["-f", "missing"]),
    ] {
        let output = run(name, &args, b"");
        assert_eq!(output.status.code(), Some(2), "{name}: {output:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("No such file or directory"),
            "{name}: {error}"
        );
        assert!(!error.contains("os error"), "{name}: {error}");
    }
}

#[test]
fn earlier_invalid_values_and_extra_operands_take_precedence_over_help() {
    for (name, args) in [
        ("read", vec!["--start", "nope", "--help"]),
        ("list", vec!["--max-depth", "nope", "--help"]),
        ("read", vec!["a", "b", "--help"]),
    ] {
        let output = run(name, &args, b"");
        assert_eq!(output.status.code(), Some(2), "{name}: {output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn cat_missing_operand_uses_gnu_error_format() {
    let output = run("cat", &["missing"], b"");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stderr, b"cat: missing: No such file or directory\n");
}
