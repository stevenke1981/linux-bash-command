use crate::common::{run, Sandbox};

#[test]
fn paste_stdin() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &[], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn paste_serial() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["--serial"], &[97, 10, 98, 10, 99]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 98, 9, 99, 10]);
}
#[test]
fn paste_two_files() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97, 10, 98, 10]);
    sandbox.write("b", [49, 10, 50, 10]);
    let out = sandbox.run("paste", &["a", "b"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 49, 10, 98, 9, 50, 10]);
}
#[test]
fn paste_unequal() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97, 10, 98, 10]);
    sandbox.write("b", [49, 10]);
    let out = sandbox.run("paste", &["a", "b"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 49, 10, 98, 9, 10]);
}
#[test]
fn paste_serial_files() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97, 10, 98, 10]);
    sandbox.write("b", [49, 10, 50, 10]);
    let out = sandbox.run("paste", &["-s", "a", "b"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 98, 10, 49, 9, 50, 10]);
}
#[test]
fn paste_cycle() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97, 10]);
    sandbox.write("b", [98, 10]);
    sandbox.write("c", [99, 10]);
    sandbox.write("d", [100, 10]);
    let out = sandbox.run("paste", &["--delimiters=,;", "a", "b", "c", "d"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 44, 98, 59, 99, 44, 100, 10]);
}
#[test]
fn paste_serial_cycle() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["-sd,;"], &[97, 10, 98, 10, 99, 10, 100, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 44, 98, 59, 99, 44, 100, 10]);
}
#[test]
fn paste_empty_delim() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["-sd", ""], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 10]);
}
#[test]
fn paste_nul_delim() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["-sd", "\\0"], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 10]);
}
#[test]
fn paste_newline_delim() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["-sd", "\\n"], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn paste_slash_delim() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["-sd", "\\\\"], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 92, 98, 10]);
}
#[test]
fn paste_zero() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["--zero-terminated", "-s"], &[97, 0, 98, 0]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 98, 0]);
}
#[test]
fn paste_repeated_stdin() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "paste",
        &["-", "-"],
        &[97, 10, 98, 10, 99, 10, 100, 10, 101, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 98, 10, 99, 9, 100, 10, 101, 9, 10]);
}
#[test]
fn paste_empty() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn paste_serial_empty() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["-s"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[10]);
}
#[test]
fn paste_missing() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["missing"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn paste_bad_option() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("paste", &["--oops"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn paste_partial() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97]);
    sandbox.write("b", [98]);
    let out = sandbox.run("paste", &["a", "b"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 98, 10]);
}
#[test]
fn paste_three_stdin() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "paste",
        &["-d,;", "-", "-", "-"],
        &[49, 10, 50, 10, 51, 10, 52, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 44, 50, 59, 51, 10, 52, 44, 59, 10]);
}
#[test]
fn paste_help() {
    let out = run("paste", &["--help"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
#[test]
fn paste_version() {
    let out = run("paste", &["--version"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
