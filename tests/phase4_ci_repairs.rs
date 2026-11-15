mod common;

use common::Sandbox;
use std::{fs, process::Command};

fn run(sandbox: &Sandbox, name: &str, args: &[&str], input: &[u8]) -> std::process::Output {
    let exe = match name {
        "which" => env!("CARGO_BIN_EXE_which"),
        "whoami" => env!("CARGO_BIN_EXE_whoami"),
        "timeout" => env!("CARGO_BIN_EXE_timeout"),
        "sed" => env!("CARGO_BIN_EXE_sed"),
        "rev" => env!("CARGO_BIN_EXE_rev"),
        "dos2unix" => env!("CARGO_BIN_EXE_dos2unix"),
        "unix2dos" => env!("CARGO_BIN_EXE_unix2dos"),
        _ => unreachable!(),
    };
    let mut command = Command::new(exe);
    command
        .args(args)
        .current_dir(&sandbox.path)
        .env("LC_ALL", "C");
    common::execute(command, input)
}

#[test]
fn which_information_and_missing_operand_have_specified_statuses() {
    let s = Sandbox::new();
    let help = run(&s, "which", &["--help"], b"");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stdout.starts_with(b"Usage: which "));
    let version = run(&s, "which", &["--version"], b"");
    assert_eq!(version.status.code(), Some(0));
    assert_eq!(
        version.stdout,
        format!("which (agentcmd) {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
    assert_eq!(run(&s, "which", &[], b"").status.code(), Some(1));
    assert_eq!(run(&s, "which", &["--bad"], b"").status.code(), Some(2));
}

#[test]
fn whoami_accepts_unambiguous_information_prefixes() {
    let s = Sandbox::new();
    for flag in ["--help", "--hel"] {
        let output = run(&s, "whoami", &[flag], b"");
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.starts_with(b"Usage: whoami "));
    }
    for flag in ["--version", "--ver"] {
        let output = run(&s, "whoami", &[flag], b"");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            output.stdout,
            format!("whoami (agentcmd) {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
        );
    }
    assert_eq!(run(&s, "whoami", &["--bad"], b"").status.code(), Some(1));
    assert_eq!(
        run(&s, "whoami", &["--", "--hel"], b"").status.code(),
        Some(1)
    );
}

#[test]
fn timeout_information_has_local_help_and_version() {
    let s = Sandbox::new();
    let output = run(&s, "timeout", &["--help"], b"");
    assert_eq!(output.status.code(), Some(0));
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.starts_with("Usage: timeout "));
    for flag in [
        "--foreground",
        "--kill-after",
        "--preserve-status",
        "--signal",
        "--verbose",
    ] {
        assert!(help.contains(flag), "missing {flag}");
    }
    let output = run(&s, "timeout", &["--version"], b"");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        format!("timeout (agentcmd) {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
}

#[test]
fn sed_missing_label_is_script_syntax_error() {
    let s = Sandbox::new();
    let output = run(&s, "sed", &["b nowhere"], b"abc\n");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("can't find label"));
}

#[test]
fn rev_decodes_utf8_and_reverses_invalid_bytes_even_in_c_locale() {
    let s = Sandbox::new();
    let output = run(&s, "rev", &[], "a弇b\n".as_bytes());
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, "b弇a\n".as_bytes());
    let output = run(&s, "rev", &[], b"a\xffb\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"b\xffa\n");
}

#[test]
fn converters_report_missing_file_as_ordinary_error() {
    let s = Sandbox::new();
    for tool in ["dos2unix", "unix2dos"] {
        let output = run(&s, tool, &["missing"], b"");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("No such file or directory"));
    }
}

#[test]
fn converter_long_options_operate_on_real_files() {
    let s = Sandbox::new();
    for (tool, expected) in [
        ("dos2unix", b"a\nb\n".as_slice()),
        ("unix2dos", b"a\r\nb\r\n".as_slice()),
    ] {
        for option in [
            "--oldfile",
            "--keep-date",
            "--quiet",
            "--force",
            "--keep-bom",
            "--remove-bom",
            "--add-eol",
            "--safe",
            "--convmode=ascii",
        ] {
            fs::write(s.path.join("f"), b"a\r\nb\n").unwrap();
            let output = run(&s, tool, &[option, "f"], b"");
            assert_eq!(
                output.status.code(),
                Some(0),
                "{tool} {option}: {:?}",
                output
            );
            assert_eq!(
                fs::read(s.path.join("f")).unwrap(),
                expected,
                "{tool} {option}"
            );
        }
    }
}
