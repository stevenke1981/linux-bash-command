mod common;

use common::run;

#[test]
fn true_00() {
    let out = run("true", &[], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn true_01() {
    let out = run("true", &["--help"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn true_02() {
    let out = run("true", &["--version"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn true_03() {
    let out = run("true", &["--unknown"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn true_04() {
    let out = run("true", &["missing"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn true_05() {
    let out = run("true", &["-"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn true_06() {
    let out = run("true", &["a", "b"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn true_07() {
    let out = run("true", &["--help", "x"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn false_00() {
    let out = run("false", &[], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn false_01() {
    let out = run("false", &["--help"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn false_02() {
    let out = run("false", &["--version"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn false_03() {
    let out = run("false", &["--unknown"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn false_04() {
    let out = run("false", &["missing"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn false_05() {
    let out = run("false", &["-"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn false_06() {
    let out = run("false", &["a", "b"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn false_07() {
    let out = run("false", &["--help", "x"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn echo_00() {
    let out = run("echo", &[], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[10]);
}

#[test]
fn echo_01() {
    let out = run("echo", &["--help"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn echo_02() {
    let out = run("echo", &["--version"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn echo_03() {
    let out = run("echo", &["-nx", "x"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[45, 110, 120, 32, 120, 10]);
}

#[test]
fn echo_04() {
    let out = run("echo", &["-ne", "a\\tb"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 98]);
}

#[test]
fn echo_05() {
    let out = run("echo", &["-e", "a\\cb"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
}

#[test]
fn echo_06() {
    let out = run("echo", &["a", "-n"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 32, 45, 110, 10]);
}

#[test]
fn echo_07() {
    let out = run("echo", &["-E", "\\n"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[92, 110, 10]);
}

#[test]
fn sleep_00() {
    let out = run("sleep", &["0"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn sleep_01() {
    let out = run("sleep", &["--help"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn sleep_02() {
    let out = run("sleep", &["--version"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn sleep_03() {
    let out = run("sleep", &["--unknown"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn sleep_04() {
    let out = run("sleep", &[], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn sleep_05() {
    let out = run("sleep", &["0s", "0m"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn sleep_06() {
    let out = run("sleep", &["NaN"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn sleep_07() {
    let out = run("sleep", &["-1"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn basename_00() {
    let out = run("basename", &["a/b"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 10]);
}

#[test]
fn basename_01() {
    let out = run("basename", &["--help"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn basename_02() {
    let out = run("basename", &["--version"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn basename_03() {
    let out = run("basename", &["--unknown"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn basename_04() {
    let out = run("basename", &[], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn basename_05() {
    let out = run("basename", &["-a", "a/b", "c/d"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 10, 100, 10]);
}

#[test]
fn basename_06() {
    let out = run("basename", &["--suffix=.txt", "a.txt"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10]);
}

#[test]
fn basename_07() {
    let out = run("basename", &["-z", "a"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 0]);
}

#[test]
fn dirname_00() {
    let out = run("dirname", &["a/b"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10]);
}

#[test]
fn dirname_01() {
    let out = run("dirname", &["--help"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn dirname_02() {
    let out = run("dirname", &["--version"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert!(!out.stdout.is_empty());
}

#[test]
fn dirname_03() {
    let out = run("dirname", &["--unknown"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn dirname_04() {
    let out = run("dirname", &[], b"ignored stdin");
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}

#[test]
fn dirname_05() {
    let out = run("dirname", &["a/b", "c/d"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 99, 10]);
}

#[test]
fn dirname_06() {
    let out = run("dirname", &["-z", "a"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[46, 0]);
}

#[test]
fn dirname_07() {
    let out = run("dirname", &["///"], b"ignored stdin");
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[47, 10]);
}

#[test]
fn yes_help() {
    assert!(run("yes", &["--help"], b"").status.success());
}

#[test]
fn yes_version() {
    assert!(run("yes", &["--version"], b"").status.success());
}

#[test]
fn yes_unknown() {
    assert_eq!(run("yes", &["--unknown"], b"").status.code(), Some(1));
}

#[test]
fn yes_pipe_0() {
    let (bytes, status) = common::limited("yes", &[], 8);
    assert_eq!(bytes, &[121, 10, 121, 10, 121, 10, 121, 10]);
    assert_eq!(status, 0);
}

#[test]
fn yes_pipe_1() {
    let (bytes, status) = common::limited("yes", &["x"], 8);
    assert_eq!(bytes, &[120, 10, 120, 10, 120, 10, 120, 10]);
    assert_eq!(status, 0);
}

#[test]
fn yes_pipe_2() {
    let (bytes, status) = common::limited("yes", &["a", "b"], 16);
    assert_eq!(
        bytes,
        &[97, 32, 98, 10, 97, 32, 98, 10, 97, 32, 98, 10, 97, 32, 98, 10]
    );
    assert_eq!(status, 0);
}

#[test]
fn yes_pipe_3() {
    let (bytes, status) = common::limited("yes", &[""], 4);
    assert_eq!(bytes, &[10, 10, 10, 10]);
    assert_eq!(status, 0);
}

#[test]
fn yes_pipe_4() {
    let (bytes, status) = common::limited("yes", &["--", "-n"], 12);
    assert_eq!(bytes, &[45, 110, 10, 45, 110, 10, 45, 110, 10, 45, 110, 10]);
    assert_eq!(status, 0);
}
