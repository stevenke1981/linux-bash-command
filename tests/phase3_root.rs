mod common;
use common::Sandbox;

#[test]
fn expr_closed_stdout_succeeds_for_false_and_true_values() {
    use std::process::{Command, Stdio};
    for value in ["0", "", "1"] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let result = Command::new(env!("CARGO_BIN_EXE_expr"))
            .arg(value)
            .stdin(Stdio::null())
            .stdout(writer)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(0), "{value}: {result:?}");
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn expr_arithmetic_precedence_comparisons_and_short_circuit() {
    for (args, expected, code) in [
        (vec!["2", "+", "3", "*", "4"], "14\n", 0),
        (vec!["(", "2", "+", "3", ")", "*", "4"], "20\n", 0),
        (vec!["5", "<", "12"], "1\n", 0),
        (vec!["a", ">", "b"], "0\n", 1),
        (vec!["yes", "|", "1", "/", "0"], "yes\n", 0),
        (vec!["0", "&", "1", "/", "0"], "0\n", 1),
        (vec![""], "\n", 1),
        (vec!["-00"], "-00\n", 1),
        (vec!["+0"], "+0\n", 0),
        (vec!["+", "length"], "length\n", 0),
        (vec!["--", "2", "+", "3"], "5\n", 0),
    ] {
        let out = common::run("expr", &args, b"");
        assert_eq!(out.status.code(), Some(code), "{args:?}: {out:?}");
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn expr_byte_functions_and_posix_bre_capture_semantics() {
    for (args, expected, code) in [
        (vec!["length", "\u{00e9}"], b"2\n".as_slice(), 0),
        (vec!["substr", "\u{00e9}", "1", "1"], b"\xc3\n", 0),
        (vec!["index", "abcdef", "ed"], b"4\n", 0),
        (vec!["substr", "abcdef", "2", "3"], b"bcd\n", 0),
        (vec!["substr", "abcdef", "0", "3"], b"\n", 1),
        (vec!["abc", ":", r"a\|ab"], b"2\n", 0),
        (vec!["abc", ":", r"\(a\|ab\).*"], b"a\n", 0),
        (vec!["abab", ":", r"\(ab\)\1"], b"ab\n", 0),
        (vec!["match", "abc", r"\(z\)"], b"\n", 1),
        (vec!["xxx", ":", r"\(\)*"], b"\n", 1),
    ] {
        let out = common::run("expr", &args, b"");
        assert_eq!(out.status.code(), Some(code), "{args:?}: {out:?}");
        assert_eq!(out.stdout, expected, "{args:?}");
    }
}

#[test]
fn expr_help_version_and_error_status_contracts() {
    for flag in ["--help", "--version"] {
        assert!(common::run("expr", &[flag], b"").status.success());
    }
    let maximum = i128::MAX.to_string();
    for (args, code, message) in [
        (vec!["1", "+"], 2, "syntax error"),
        (vec!["1", "--unknown"], 2, "syntax error"),
        (vec!["x", ":", "["], 2, "regular expression"),
        (vec!["1", "/", "0"], 3, "division by zero"),
        (vec!["a", "+", "1"], 3, "non-integer argument"),
        (vec![maximum.as_str(), "+", "1"], 3, "integer overflow"),
    ] {
        let out = common::run("expr", &args, b"");
        assert_eq!(out.status.code(), Some(code), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(message),
            "{args:?}: {out:?}"
        );
    }
}

#[test]
fn sort_external_spills_are_stable_unique_and_cleaned_up() {
    let sandbox = Sandbox::new();
    let input: String = (0..1000)
        .rev()
        .map(|n| format!("{} {n:04}\n", n % 5))
        .collect();
    sandbox.write("input", input.as_bytes());
    let options = ["-S1K", "-T", ".", "--batch-size=2", "-s", "-k1,1", "input"];
    let out = sandbox.run("sort", &options, b"");
    assert!(out.status.success(), "{out:?}");
    let expected: String = (0..5)
        .flat_map(|key| (0..1000).rev().filter(move |n| n % 5 == key))
        .map(|n| format!("{} {n:04}\n", n % 5))
        .collect();
    assert_eq!(out.stdout, expected.as_bytes());
    let out = sandbox.run("sort", &["-S1K", "-T", ".", "-u", "-k1,1", "input"], b"");
    let expected: String = (0..5)
        .map(|key| {
            let n = (0..1000).rev().find(|n| n % 5 == key).unwrap();
            format!("{key} {n:04}\n")
        })
        .collect();
    assert_eq!(out.stdout, expected.as_bytes());
    assert_eq!(std::fs::read_dir(&sandbox.path).unwrap().count(), 1);
}

#[test]
fn sort_merge_can_replace_an_input_and_nul_records_survive_spills() {
    let sandbox = Sandbox::new();
    sandbox.write("one", b"a\nc\n");
    sandbox.write("two", b"b\nd\n");
    let out = sandbox.run(
        "sort",
        &[
            "-m",
            "-S1b",
            "--batch-size=2",
            "-T",
            ".",
            "-o",
            "one",
            "one",
            "two",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read(sandbox.path.join("one")).unwrap(),
        b"a\nb\nc\nd\n"
    );
    let out = sandbox.run("sort", &["-z", "-S1b", "-T", "."], b"\xff\0a\0b\0a");
    assert_eq!(out.stdout, b"a\0a\0b\0\xff\0");
    assert_eq!(std::fs::read_dir(&sandbox.path).unwrap().count(), 2);
}

#[test]
fn sort_spill_errors_checks_and_output_failures_keep_their_statuses() {
    let sandbox = Sandbox::new();
    sandbox.write("file", b"unchanged");
    let out = sandbox.run("sort", &["-S1b", "-T", "file"], b"b\na\n");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        std::fs::read(sandbox.path.join("file")).unwrap(),
        b"unchanged"
    );
    let out = sandbox.run("sort", &["-C"], b"b\na\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    let out = sandbox.run("sort", &["-o", "absent/output"], b"a\n");
    assert_eq!(out.status.code(), Some(2));
}
