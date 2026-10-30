mod common;
use common::{execute, Sandbox};
use std::process::{Command, Output};
fn run(util: &str, args: &[&str], input: &[u8], files: &[(&str, &[u8])]) -> Output {
    let sandbox = Sandbox::new();
    for (name, bytes) in files {
        sandbox.write(name, bytes);
    }
    let binary = match util {
        "printf" => env!("CARGO_BIN_EXE_printf"),
        "seq" => env!("CARGO_BIN_EXE_seq"),
        "test" | "[" => env!("CARGO_BIN_EXE_test"),
        "sort" => env!("CARGO_BIN_EXE_sort"),
        "uniq" => env!("CARGO_BIN_EXE_uniq"),
        "comm" => env!("CARGO_BIN_EXE_comm"),
        _ => unreachable!(),
    };
    let path = if util == "[" {
        let p = sandbox.path.join(if cfg!(windows) { "[.exe" } else { "[" });
        std::fs::copy(binary, &p).unwrap();
        p
    } else {
        std::path::PathBuf::from(binary)
    };
    let mut cmd = Command::new(path);
    cmd.args(args).current_dir(&sandbox.path);
    execute(cmd, input)
}
#[test]
fn printf_help() {
    let out = run("printf", &["--help"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
}
#[test]
fn printf_version() {
    let out = run("printf", &["--version"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("agentcmd"));
}
#[test]
fn seq_help() {
    let out = run("seq", &["--help"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
}
#[test]
fn seq_version() {
    let out = run("seq", &["--version"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("agentcmd"));
}
#[test]
fn sort_help() {
    let out = run("sort", &["--help"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
}
#[test]
fn sort_version() {
    let out = run("sort", &["--version"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("agentcmd"));
}
#[test]
fn uniq_help() {
    let out = run("uniq", &["--help"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
}
#[test]
fn uniq_version() {
    let out = run("uniq", &["--version"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("agentcmd"));
}
#[test]
fn comm_help() {
    let out = run("comm", &["--help"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
}
#[test]
fn comm_version() {
    let out = run("comm", &["--version"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("agentcmd"));
}
#[test]
fn test_help() {
    let out = run("test", &["--help"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}
#[test]
fn test_version() {
    let out = run("test", &["--version"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}
#[test]
fn bracket_help() {
    let out = run("[", &["--help"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
}
#[test]
fn bracket_version() {
    let out = run("[", &["--version"], &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("agentcmd"));
}
#[test]
fn printf_basic() {
    let out = run("printf", &["%s %d\\n", "hi", "3"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[104, 105, 32, 51, 10]);
}
#[test]
fn printf_repeat() {
    let out = run("printf", &["%s\\n", "a", "b"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn printf_numeric() {
    let out = run("printf", &["%#06x %o %d", "15", "8", "'A"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(
        out.stdout,
        &[48, 120, 48, 48, 48, 102, 32, 49, 48, 32, 54, 53]
    );
}
#[test]
fn printf_float() {
    let out = run("printf", &["%.2e %.3g %a", "12", "1.2", "1.5"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, b"1.20e+01 1.2 0xcp-3");
}
#[test]
fn printf_star() {
    let out = run("printf", &["%*.*f", "8", "2", "1.25"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[32, 32, 32, 32, 49, 46, 50, 53]);
}
#[test]
fn printf_escape() {
    let out = run("printf", &["%bEND", "a\\cignored"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97]);
}
#[test]
fn printf_quote() {
    let out = run("printf", &["%q", "a b"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[39, 97, 32, 98, 39]);
}
#[test]
fn printf_unknown() {
    let out = run("printf", &["--unknown"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[45, 45, 117, 110, 107, 110, 111, 119, 110]);
}
#[test]
fn printf_partial() {
    let out = run("printf", &["%d", "12junk"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[49, 50]);
}
#[test]
fn printf_format_error() {
    let out = run("printf", &["%Q", "x"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn printf_missing() {
    let out = run("printf", &[], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn printf_ucn_ascii() {
    let out = run("printf", &["\\u0043"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[67]);
}
#[test]
fn seq_basic() {
    let out = run("seq", &["3"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[49, 10, 50, 10, 51, 10]);
}
#[test]
fn seq_negative() {
    let out = run("seq", &["-3", "-1"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[45, 51, 10, 45, 50, 10, 45, 49, 10]);
}
#[test]
fn seq_fraction() {
    let out = run("seq", &["0.1", "0.1", "0.3"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(
        out.stdout,
        &[48, 46, 49, 10, 48, 46, 50, 10, 48, 46, 51, 10]
    );
}
#[test]
fn seq_format() {
    let out = run("seq", &["-f", "%04.1f", "1", "0.5", "2"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(
        out.stdout,
        &[48, 49, 46, 48, 10, 48, 49, 46, 53, 10, 48, 50, 46, 48, 10]
    );
}
#[test]
fn seq_width() {
    let out = run("seq", &["-w", "8", "10"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[48, 56, 10, 48, 57, 10, 49, 48, 10]);
}
#[test]
fn seq_separator() {
    let out = run("seq", &["-s,", "3"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[49, 44, 50, 44, 51, 10]);
}
#[test]
fn seq_zero() {
    let out = run("seq", &["1", "0", "2"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn seq_unknown() {
    let out = run("seq", &["--unknown"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn seq_separator_help_value() {
    let out = run("seq", &["-s", "--help", "2"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[49, 45, 45, 104, 101, 108, 112, 50, 10]);
}
#[test]
fn test_true() {
    let out = run("test", &["a"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_false() {
    let out = run("test", &[""], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_equal() {
    let out = run("test", &["x", "=", "x"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_integer() {
    let out = run("test", &[" 2 ", "-eq", "02"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_invalid_integer() {
    let out = run("test", &["x", "-eq", "2"], &[], &[]);
    assert_eq!(out.status.code(), Some(2), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_less() {
    let out = run("test", &["a", "<", "b"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_greater() {
    let out = run("test", &["b", ">", "a"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_precedence() {
    let out = run("test", &["", "-a", "x", "-o", "y"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_missing_file() {
    let out = run("test", &["-e", "missing"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_file() {
    let out = run("test", &["-f", "data"], &[], &[("data", &[97, 98, 99])]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_file_size() {
    let out = run("test", &["-s", "data"], &[], &[("data", &[97, 98, 99])]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_invalid() {
    let out = run("test", &["x", "bogus", "y"], &[], &[]);
    assert_eq!(out.status.code(), Some(2), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn test_no_permute() {
    let out = run("test", &["--unknown"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_true() {
    let out = run("[", &["a", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_false() {
    let out = run("[", &["", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_equal() {
    let out = run("[", &["x", "=", "x", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_integer() {
    let out = run("[", &[" 2 ", "-eq", "02", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_invalid_integer() {
    let out = run("[", &["x", "-eq", "2", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(2), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_less() {
    let out = run("[", &["a", "<", "b", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_greater() {
    let out = run("[", &["b", ">", "a", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_precedence() {
    let out = run("[", &["", "-a", "x", "-o", "y", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_missing_file() {
    let out = run("[", &["-e", "missing", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_file() {
    let out = run("[", &["-f", "data", "]"], &[], &[("data", &[97, 98, 99])]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_file_size() {
    let out = run("[", &["-s", "data", "]"], &[], &[("data", &[97, 98, 99])]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_invalid() {
    let out = run("[", &["x", "bogus", "y", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(2), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_no_permute() {
    let out = run("[", &["--unknown", "]"], &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn bracket_missing_close() {
    let out = run("[", &["x"], &[], &[]);
    assert_eq!(out.status.code(), Some(2), "{:?}", out);
    assert_eq!(out.stdout, &[]);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("missing ']'"),
        "{:?}",
        out
    );
}
#[test]
fn sort_stdin() {
    let out = run("sort", &[], &[98, 10, 97, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn sort_numeric() {
    let out = run("sort", &["-n"], &[49, 48, 10, 50, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[50, 10, 49, 48, 10]);
}
#[test]
fn sort_key() {
    let out = run(
        "sort",
        &["-t,", "-k2,2nr"],
        &[97, 44, 50, 10, 98, 44, 49, 48, 10],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[98, 44, 49, 48, 10, 97, 44, 50, 10]);
}
#[test]
fn sort_stable() {
    let out = run(
        "sort",
        &["-s", "-k1,1"],
        &[120, 32, 122, 10, 120, 32, 97, 10],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[120, 32, 122, 10, 120, 32, 97, 10]);
}
#[test]
fn sort_unique() {
    let out = run("sort", &["-u"], &[98, 10, 97, 10, 97, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn sort_missing() {
    let out = run("sort", &["missing"], &[], &[]);
    assert_eq!(out.status.code(), Some(2), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn sort_unknown() {
    let out = run("sort", &["--unknown"], &[], &[]);
    assert_eq!(out.status.code(), Some(2), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn sort_check() {
    let out = run("sort", &["-c"], &[98, 10, 97, 10], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn sort_files() {
    let out = run(
        "sort",
        &["a", "b"],
        &[],
        &[("a", &[98, 10]), ("b", &[97, 10])],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn sort_zero() {
    let out = run("sort", &["-z"], &[98, 0, 97, 0], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 0, 98, 0]);
}
#[test]
fn uniq_stdin() {
    let out = run("uniq", &[], &[97, 10, 97, 10, 98, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn uniq_count() {
    let out = run("uniq", &["-c"], &[97, 10, 97, 10, 98, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(
        out.stdout,
        &[32, 32, 32, 32, 32, 32, 50, 32, 97, 10, 32, 32, 32, 32, 32, 32, 49, 32, 98, 10]
    );
}
#[test]
fn uniq_repeated() {
    let out = run("uniq", &["-D"], &[97, 10, 97, 10, 98, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 97, 10]);
}
#[test]
fn uniq_unique() {
    let out = run("uniq", &["-u"], &[97, 10, 97, 10, 98, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[98, 10]);
}
#[test]
fn uniq_fields() {
    let out = run(
        "uniq",
        &["-f1"],
        &[120, 32, 97, 10, 121, 32, 97, 10, 122, 32, 98, 10],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[120, 32, 97, 10, 122, 32, 98, 10]);
}
#[test]
fn uniq_chars() {
    let out = run(
        "uniq",
        &["-s1", "-w1", "-i"],
        &[97, 88, 49, 10, 98, 120, 50, 10, 99, 89, 51, 10],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 88, 49, 10, 99, 89, 51, 10]);
}
#[test]
fn uniq_group() {
    let out = run("uniq", &["--group=both"], &[97, 10, 97, 10, 98, 10], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[10, 97, 10, 97, 10, 10, 98, 10, 10]);
}
#[test]
fn uniq_files() {
    let out = run(
        "uniq",
        &["data"],
        &[],
        &[("data", &[97, 10, 97, 10, 98, 10])],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn uniq_missing() {
    let out = run("uniq", &["missing"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn uniq_unknown() {
    let out = run("uniq", &["--unknown"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn uniq_conflict() {
    let out = run("uniq", &["-Dc"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn comm_files() {
    let out = run(
        "comm",
        &["a", "b"],
        &[],
        &[("a", &[97, 10, 98, 10]), ("b", &[98, 10, 99, 10])],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 9, 9, 98, 10, 9, 99, 10]);
}
#[test]
fn comm_stdin() {
    let out = run(
        "comm",
        &["-", "b"],
        &[97, 10, 98, 10],
        &[("b", &[98, 10, 99, 10])],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 9, 9, 98, 10, 9, 99, 10]);
}
#[test]
fn comm_suppress() {
    let out = run(
        "comm",
        &["-12", "a", "b"],
        &[],
        &[("a", &[97, 10, 98, 10]), ("b", &[98, 10, 99, 10])],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[98, 10]);
}
#[test]
fn comm_total() {
    let out = run(
        "comm",
        &["-123", "--total", "a", "b"],
        &[],
        &[("a", &[97, 10, 98, 10]), ("b", &[98, 10, 99, 10])],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(
        out.stdout,
        &[49, 9, 49, 9, 49, 9, 116, 111, 116, 97, 108, 10]
    );
}
#[test]
fn comm_separator() {
    let out = run(
        "comm",
        &["--output-delimiter=:", "a", "b"],
        &[],
        &[("a", &[97, 10, 98, 10]), ("b", &[98, 10, 99, 10])],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, &[97, 10, 58, 58, 98, 10, 58, 99, 10]);
}
#[test]
fn comm_disorder() {
    let out = run(
        "comm",
        &["--check-order", "a", "b"],
        &[],
        &[("a", &[98, 10, 97, 10]), ("b", &[99, 10])],
    );
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[98, 10]);
}
#[test]
fn comm_missing() {
    let out = run(
        "comm",
        &["missing", "b"],
        &[],
        &[("a", &[97, 10, 98, 10]), ("b", &[98, 10, 99, 10])],
    );
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn comm_unknown() {
    let out = run("comm", &["--unknown"], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn comm_arity() {
    let out = run("comm", &[], &[], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn seq_last_precision_spec() {
    let out = run("seq", &["1", "1", "3.0"], &[], &[]);
    assert_eq!(out.stdout, b"1.0\n2.0\n3.0\n");
    assert!(out.status.success());
}
#[test]
fn sort_bundle_check_stable() {
    let out = run("sort", &["-cs"], b"a\nb\n", &[]);
    assert!(out.status.success());
}
#[test]
fn test_hardlink_identity() {
    let s = Sandbox::new();
    s.write("a", b"x");
    std::fs::hard_link(s.path.join("a"), s.path.join("b")).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_test"));
    cmd.args(["a", "-ef", "b"]).current_dir(&s.path);
    assert!(execute(cmd, &[]).status.success());
}

#[test]
fn sort_general_stable_signed_zero() {
    let out = run("sort", &["-sg"], b"0\n-0\n+0\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"0\n-0\n+0\n");
}
#[test]
fn sort_general_unique_signed_zero() {
    let out = run("sort", &["-ug"], b"0\n-0\n+0\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"0\n");
}
#[test]
fn sort_general_nonnumeric_nan_numbers() {
    let out = run("sort", &["-g"], b"nan\nzebra\n1\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"zebra\nnan\n1\n");
}
#[test]
fn sort_general_stable_categories() {
    let out = run(
        "sort",
        &["-sg"],
        b"nan\nzebra\n1\nnan\napple\n+nan\n-inf\n",
        &[],
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"zebra\napple\nnan\nnan\n+nan\n-inf\n1\n");
}
#[test]
fn sort_general_unique_categories() {
    let out = run("sort", &["-ug"], b"zebra\napple\nnan\n+nan\n1\n1.0\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"zebra\nnan\n1\n");
}
#[test]
fn sort_general_ascii_whitespace_prefix() {
    let out = run("sort", &["-sg"], b"25\n\x0b\t 2.5e1suffix\n1\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"1\n25\n\x0b\t 2.5e1suffix\n");
}
#[test]
fn sort_general_reverse_categories() {
    let out = run("sort", &["-sgr"], b"nan\nzebra\n1\n0\n-0\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"1\n0\n-0\nnan\nzebra\n");
}

#[test]
fn printf_hex_gnu_leading_nibble() {
    let out = run("printf", &["%a|%A|%#.0a", "1.5", "2.5", "1"], &[], &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"0xcp-3|0XAP-2|0x8.p-3");
}
#[test]
fn printf_hex_precision_and_carry() {
    let out = run(
        "printf",
        &["%.0a|%.1a|%.2a", "1.99", "1.99609375", "1.125"],
        &[],
        &[],
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"0x1p+1|0x1.0p+1|0x9.00p-3");
}
#[test]
fn printf_hex_zeros_fraction_subnormal() {
    let out = run(
        "printf",
        &["%a|%a|%a|%a", "0", "-0", "0.5", "0x0.0000000000001p-1022"],
        &[],
        &[],
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"0x0p+0|-0x0p+0|0x8p-4|0x8p-1077");
}
#[test]
fn sort_files0_empty_is_error() {
    let out = run(
        "sort",
        &["--files0-from=names"],
        b"b\na\n",
        &[("names", b"")],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no input from"));
}
#[test]
fn sort_general_signed_nan_stable() {
    let out = run("sort", &["-sg"], b"nan\n-nan\n+nan\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"-nan\nnan\n+nan\n");
}
#[test]
fn sort_general_signed_nan_unique() {
    let out = run("sort", &["-ug"], b"nan\n-nan\n+nan\n", &[]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"-nan\nnan\n");
}
