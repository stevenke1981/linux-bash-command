mod common;
use common::{run, Sandbox};
use std::process::Command;

fn check(util: &str, args: &[&str], input: &[u8], expected: &[u8], code: i32) {
    let out = run(util, args, input);
    assert_eq!(
        out.status.code(),
        Some(code),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected);
}
#[test]
fn env_help() {
    let out = run("env", &["--help"], b"");
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"Usage: env"));
}
#[test]
fn env_version() {
    let out = run("env", &["--version"], b"");
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"env (agentcmd)"));
}
#[test]
fn env_unknown() {
    assert_eq!(run("env", &["--bad-option"], b"").status.code(), Some(125));
}
#[test]
fn env_empty() {
    check("env", &["-i"], b"", b"", 0);
}
#[test]
fn env_assignments() {
    check("env", &["-i", "A=1", "A=2", "B=x"], b"", b"A=2\nB=x\n", 0);
}
#[test]
fn env_nul() {
    check(
        "env",
        &["--ignore-environment", "--null", "A=1"],
        b"",
        b"A=1\0",
        0,
    );
}
#[test]
fn env_unset() {
    let mut cmd = Command::new(common::binary("env"));
    cmd.args([
        "--unset=AGENTCMD_P1_TEST",
        common::binary("printenv"),
        "AGENTCMD_P1_TEST",
    ])
    .env("AGENTCMD_P1_TEST", "x");
    let out = common::execute(cmd, b"");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}
#[test]
fn env_child() {
    check(
        "env",
        &["-i", common::binary("echo"), "hello"],
        b"",
        b"hello\n",
        0,
    );
}
#[test]
fn env_child_stdin() {
    check("env", &[common::binary("cat")], b"input", b"input", 0);
}
#[test]
fn env_directory() {
    let s = Sandbox::new();
    s.write("dir/file", b"contents");
    let out = s.run("env", &["-Cdir", common::binary("cat"), "file"], b"");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"contents");
}
#[test]
fn env_missing_directory() {
    assert_eq!(
        run("env", &["-Cmissing", common::binary("true")], b"")
            .status
            .code(),
        Some(125)
    );
}
#[test]
fn env_missing_command() {
    assert_eq!(
        run("env", &["AGENTCMD_MISSING_PROGRAM_XXXX"], b"")
            .status
            .code(),
        Some(127)
    );
}
#[test]
fn env_split() {
    let split = format!("\"{}\" '%s'", common::binary("echo").replace('\\', "/"));
    check("env", &["-i", "-S", &split], b"", b"%s\n", 0);
}
#[test]
fn env_split_preserves_terminator() {
    check("env", &["-S", "", "--", "-i"], b"", b"", 127);
}
#[test]
fn env_split_inserts_command_boundary() {
    let split = format!("\"{}\"", common::binary("echo").replace('\\', "/"));
    check("env", &["-S", &split, "--unknown"], b"", b"--unknown\n", 0);
}
#[test]
fn env_empty_path_components() {
    let s = Sandbox::new();
    let name = if cfg!(windows) {
        "localtool.exe"
    } else {
        "localtool"
    };
    std::fs::copy(common::binary("echo"), s.path.join(name)).unwrap();
    let separator = if cfg!(windows) { ';' } else { ':' };
    for path in [
        String::new(),
        format!("{separator}missing"),
        format!("missing{separator}"),
    ] {
        let assignment = format!("PATH={path}");
        let out = s.run("env", &[&assignment, "localtool", "x"], b"");
        assert!(
            out.status.success(),
            "path={path:?}, stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"x\n");
    }
}
#[test]
fn env_empty_path_with_chdir() {
    let s = Sandbox::new();
    s.write("dir/file", b"");
    let name = if cfg!(windows) {
        "localtool.exe"
    } else {
        "localtool"
    };
    std::fs::copy(common::binary("echo"), s.path.join("dir").join(name)).unwrap();
    let out = s.run("env", &["-Cdir", "PATH=", "localtool", "x"], b"");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"x\n");
}
#[test]
fn env_debug() {
    let out = run("env", &["--debug", common::binary("true")], b"");
    assert!(out.status.success());
    assert!(!out.stderr.is_empty());
}

#[cfg(windows)]
#[test]
fn native_command_forward_slashes() {
    check(
        "env",
        &["C:/Windows/System32/cmd.exe", "/d", "/c", "exit", "255"],
        b"",
        b"",
        255,
    );
    check(
        "xargs",
        &[
            "-I{}",
            "C:/Windows/System32/cmd.exe",
            "/d",
            "/c",
            "exit",
            "255",
        ],
        b"a\n",
        b"",
        124,
    );
}

#[test]
fn printenv_help() {
    assert!(run("printenv", &["--help"], b"")
        .stdout
        .starts_with(b"Usage: printenv"));
}
#[test]
fn printenv_version() {
    assert!(run("printenv", &["--version"], b"")
        .stdout
        .starts_with(b"printenv (agentcmd)"));
}
#[test]
fn printenv_unknown() {
    assert_eq!(
        run("printenv", &["--bad-option"], b"").status.code(),
        Some(1)
    );
}
#[test]
fn printenv_invalid_option_precedes_help() {
    for option in ["--unknown", "--null=bad"] {
        check("printenv", &[option, "--help"], b"", b"", 1);
    }
    let out = run("printenv", &["--help", "--unknown"], b"");
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"Usage: printenv"));
}
#[test]
fn printenv_missing() {
    check("printenv", &["AGENTCMD_MISSING_VAR_XXXX"], b"", b"", 1);
}
#[test]
fn printenv_invalid() {
    check("printenv", &["A=B"], b"", b"", 1);
}
#[test]
fn printenv_values() {
    let mut cmd = Command::new(common::binary("printenv"));
    cmd.args(["AGENTCMD_A", "AGENTCMD_B"])
        .env("AGENTCMD_A", "one")
        .env("AGENTCMD_B", "two");
    let out = common::execute(cmd, b"");
    assert_eq!(out.stdout, b"one\ntwo\n");
    assert!(out.status.success());
}
#[test]
fn printenv_zero() {
    let mut cmd = Command::new(common::binary("printenv"));
    cmd.args(["--null", "AGENTCMD_A", "AGENTCMD_A"])
        .env("AGENTCMD_A", "x");
    let out = common::execute(cmd, b"");
    assert_eq!(out.stdout, b"x\0x\0");
    assert!(out.status.success());
}
#[test]
fn printenv_partial() {
    let mut cmd = Command::new(common::binary("printenv"));
    cmd.args(["AGENTCMD_A", "AGENTCMD_MISSING_VAR_XXXX"])
        .env("AGENTCMD_A", "");
    let out = common::execute(cmd, b"");
    assert_eq!(out.stdout, b"\n");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn printenv_permutation_spec() {
    let mut cmd = Command::new(common::binary("printenv"));
    cmd.args(["AGENTCMD_A", "-0"]).env("AGENTCMD_A", "x");
    let out = common::execute(cmd, b"");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"x\0");
}

#[test]
fn xargs_quoted_backslash() {
    check(
        "xargs",
        &[common::binary("echo")],
        br#"'a\b' "c\d""#,
        b"a\\b c\\d\n",
        0,
    );
}
#[test]
fn xargs_logical_lines() {
    check(
        "xargs",
        &["-L2", common::binary("echo")],
        b"a\n\n\nb\nc\n",
        b"a b\nc\n",
        0,
    );
    check(
        "xargs",
        &["-L1", common::binary("echo")],
        b"a \nb\nc\n",
        b"a b\nc\n",
        0,
    );
}

#[test]
fn xargs_abort_status() {
    let args = if cfg!(windows) {
        vec![
            "-I{}",
            r"C:\Windows\System32\cmd.exe",
            "/d",
            "/c",
            "exit",
            "255",
        ]
    } else {
        vec!["-I{}", "/bin/sh", "-c", "exit 255"]
    };
    let out = run("xargs", &args, b"a");
    assert_eq!(out.status.code(), Some(124));
}

#[test]
fn xargs_help() {
    assert!(run("xargs", &["--help"], b"")
        .stdout
        .starts_with(b"Usage: xargs"));
}
#[test]
fn xargs_version() {
    assert!(run("xargs", &["--version"], b"")
        .stdout
        .starts_with(b"xargs (agentcmd)"));
}
#[test]
fn xargs_unknown() {
    assert_eq!(run("xargs", &["--bad-option"], b"").status.code(), Some(1));
}
#[test]
fn xargs_stdin() {
    check(
        "xargs",
        &[common::binary("echo")],
        b"a b\nc\n",
        b"a b c\n",
        0,
    );
}
#[test]
fn xargs_quotes() {
    check(
        "xargs",
        &["-n1", common::binary("echo")],
        b"'a b' \"c d\" e\\ f",
        b"a b\nc d\ne f\n",
        0,
    );
}
#[test]
fn xargs_nul() {
    check(
        "xargs",
        &["--null", "-n1", common::binary("echo")],
        b"a b\0c\0",
        b"a b\nc\n",
        0,
    );
}
#[test]
fn xargs_delimiter() {
    check(
        "xargs",
        &["-d,", common::binary("echo")],
        b"a,b c,",
        b"a b c\n",
        0,
    );
}
#[test]
fn xargs_replace() {
    check(
        "xargs",
        &["-I{}", common::binary("echo"), "pre{}post"],
        b" a b\nc d\n",
        b"prea bpost\nprec dpost\n",
        0,
    );
}
#[test]
fn xargs_lines() {
    check(
        "xargs",
        &["-L1", common::binary("echo")],
        b"a b\nc d\n",
        b"a b\nc d\n",
        0,
    );
}
#[test]
fn xargs_eof() {
    check(
        "xargs",
        &["-ESTOP", common::binary("echo")],
        b"a STOP b",
        b"a\n",
        0,
    );
}
#[test]
fn xargs_empty() {
    check(
        "xargs",
        &["--no-run-if-empty", common::binary("echo")],
        b"",
        b"",
        0,
    );
}
#[test]
fn xargs_file() {
    let s = Sandbox::new();
    s.write("args", b"a b");
    let out = s.run("xargs", &["-aargs", common::binary("echo")], b"");
    assert_eq!(out.stdout, b"a b\n");
    assert!(out.status.success());
}
#[test]
fn xargs_missing_file() {
    assert_eq!(run("xargs", &["-amissing"], b"").status.code(), Some(1));
}
#[test]
fn xargs_missing_command() {
    assert_eq!(
        run("xargs", &["AGENTCMD_MISSING_PROGRAM_XXXX"], b"a")
            .status
            .code(),
        Some(127)
    );
}
#[test]
fn xargs_child_failure() {
    check("xargs", &[common::binary("false")], b"a", b"", 123);
}
#[test]
fn xargs_verbose() {
    let out = run("xargs", &["--verbose", common::binary("true")], b"a");
    assert!(out.status.success());
    assert!(!out.stderr.is_empty());
}
#[test]
fn xargs_slots() {
    check(
        "xargs",
        &[
            "-I{}",
            "--process-slot-var=SLOT",
            common::binary("printenv"),
            "SLOT",
        ],
        b"a\nb\n",
        b"0\n0\n",
        0,
    );
}
#[test]
fn xargs_size() {
    let out = run("xargs", &["-s1", common::binary("echo")], b"a");
    assert_eq!(out.status.code(), Some(1));
}
#[test]
fn xargs_unmatched() {
    let out = run("xargs", &[common::binary("echo")], b"'oops");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unmatched quote"));
}

macro_rules! alias_tests {
    ($module:ident,$util:literal) => {
        mod $module {
            use super::*;
            #[test]
            fn help() {
                assert!(run($util, &["--help"], b"")
                    .stdout
                    .starts_with(b"Usage: grep"));
            }
            #[test]
            fn version() {
                assert!(run($util, &["--version"], b"").status.success());
            }
            #[test]
            fn unknown() {
                assert_eq!(run($util, &["--bad-option"], b"").status.code(), Some(2));
            }
            #[test]
            fn stdin() {
                check($util, &["a"], b"a\nb\n", b"a\n", 0);
            }
            #[test]
            fn no_match() {
                check($util, &["x"], b"a\n", b"", 1);
            }
            #[test]
            fn file() {
                let s = Sandbox::new();
                s.write("one", b"a\nb\n");
                let out = s.run($util, &["a", "one"], b"");
                assert_eq!(out.stdout, b"a\n");
                assert!(out.status.success());
            }
            #[test]
            fn multi() {
                let s = Sandbox::new();
                s.write("one", b"a\n");
                s.write("two", b"a\n");
                let out = s.run($util, &["a", "one", "two"], b"");
                assert_eq!(out.stdout, b"one:a\ntwo:a\n");
                assert!(out.status.success());
            }
            #[test]
            fn missing() {
                assert_eq!(run($util, &["a", "missing"], b"").status.code(), Some(2));
            }
            #[test]
            fn syntax() {
                let (flag, expected): (&str, &[u8]) = if $util == "fgrep" {
                    ("-F", b"a|b\n")
                } else {
                    ("-E", b"a\nb\na|b\n")
                };
                check($util, &[flag, "a|b"], b"a\nb\na|b\nc\n", expected, 0);
            }
            #[test]
            fn conflicting_syntax() {
                for flag in [
                    "--basic-regexp",
                    "--perl-regexp",
                    if $util == "fgrep" {
                        "--extended-regexp"
                    } else {
                        "--fixed-strings"
                    },
                ] {
                    check($util, &[flag, "a"], b"a\n", b"", 2);
                }
            }
        }
    };
}
alias_tests!(egrep, "egrep");
alias_tests!(fgrep, "fgrep");

#[test]
fn grep_zero() {
    check("grep", &["-zn", "a"], b"b\0a\0", b"2:a\0", 0);
}
#[test]
fn grep_filename_zero() {
    let s = Sandbox::new();
    s.write("one", b"a\n");
    let out = s.run("grep", &["-ZHn", "a", "one"], b"");
    assert_eq!(out.stdout, b"one\x001:a\n");
}
#[test]
fn grep_offset() {
    check("grep", &["-b", "a"], b"b\naaa\n", b"2:aaa\n", 0);
}
#[test]
fn grep_only_offset() {
    check("grep", &["-bo", "a"], b"xaa\n", b"1:a\n2:a\n", 0);
}
#[test]
fn grep_label() {
    check(
        "grep",
        &["-H", "--label=label", "a"],
        b"a\n",
        b"label:a\n",
        0,
    );
}
#[test]
fn grep_tab() {
    let s = Sandbox::new();
    s.write("one", b"a\n");
    let out = s.run("grep", &["-nT", "a", "one"], b"");
    assert_eq!(out.stdout, b"1:\ta\n");
    for util in ["grep", "egrep", "fgrep"] {
        check(util, &["-nT", "a"], b"a\n", b"                  1:\ta\n", 0);
    }
}
#[test]
fn grep_files_without_match_preserves_baseline_status() {
    let s = Sandbox::new();
    s.write("one", b"a\n");
    for util in ["grep", "egrep", "fgrep"] {
        let out = s.run(util, &["--files-without-match", "z", "one"], b"");
        assert_eq!(out.stdout, b"one\n");
        assert!(out.status.success());
        let out = s.run(util, &["-L", "a", "one"], b"");
        assert!(out.stdout.is_empty());
        assert_eq!(out.status.code(), Some(1));
    }
}
#[test]
fn grep_matcher_precedence_preserves_baseline() {
    check("grep", &["-E", "-F", "a|b"], b"a\na|b\n", b"a|b\n", 0);
}
#[test]
fn grep_case_reset() {
    check(
        "grep",
        &["-i", "--no-ignore-case", "A"],
        b"a\nA\n",
        b"A\n",
        0,
    );
}
#[test]
fn grep_alias_y() {
    check("grep", &["-y", "A"], b"a\nA\n", b"a\nA\n", 0);
}
#[test]
fn grep_skip_directory() {
    let s = Sandbox::new();
    s.write("dir/one", b"a\n");
    assert_eq!(
        s.run("grep", &["-dskip", "a", "dir"], b"").status.code(),
        Some(1)
    );
}
#[test]
fn grep_invalid_directory_status() {
    for util in ["grep", "egrep", "fgrep"] {
        check(util, &["-dbad", "a"], b"", b"", 2);
    }
}
#[test]
fn grep_exclude_file() {
    let s = Sandbox::new();
    s.write("dir/one", b"a\n");
    s.write("dir/two", b"a\n");
    s.write("exclude", b"two\n");
    let out = s.run("grep", &["-r", "--exclude-from=exclude", "a", "dir"], b"");
    assert_eq!(out.stdout, b"dir/one:a\n");
}

#[test]
fn head_zero() {
    check("head", &["-zn1"], b"a\0b\0", b"a\0", 0);
}
#[test]
fn head_zero_negative() {
    check(
        "head",
        &["--zero-terminated", "-n-1"],
        b"a\0b\0c",
        b"a\0b\0",
        0,
    );
}
#[test]
fn tail_zero() {
    check("tail", &["-zn1"], b"a\0b\0", b"b\0", 0);
}
#[test]
fn tail_zero_from() {
    check(
        "tail",
        &["--zero-terminated", "-n+2"],
        b"a\0b\0c",
        b"b\0c",
        0,
    );
}
#[test]
fn cat_unbuffered() {
    check("cat", &["-u"], b"a\0b\n", b"a\0b\n", 0);
}
