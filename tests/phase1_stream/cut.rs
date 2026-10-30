use crate::common::{run, Sandbox};

#[test]
fn cut_bytes() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-b", "2-4"],
        &[97, 98, 99, 100, 101, 102, 10, 49, 50, 51, 52, 53, 54, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 99, 100, 10, 50, 51, 52, 10]);
}
#[test]
fn cut_characters() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["--characters=2,4"], &[97, 98, 99, 100, 101, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 100, 10]);
}
#[test]
fn cut_long_bytes() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["--bytes=-2,5-"], &[97, 98, 99, 100, 101, 102, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 101, 102, 10]);
}
#[test]
fn cut_merge() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-b", "5-7,1-3,3-6"],
        &[97, 98, 99, 100, 101, 102, 103, 104, 105, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 99, 100, 101, 102, 103, 10]);
}
#[test]
fn cut_complement() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-b", "2-3", "--complement"],
        &[97, 98, 99, 100, 101, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 100, 101, 10]);
}
#[test]
fn cut_byte_output_delimiter() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-b", "1,3,5", "--output-delimiter=:"],
        &[97, 98, 99, 100, 101, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 58, 99, 58, 101, 10]);
}
#[test]
fn cut_fields() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-f2"],
        &[97, 9, 98, 9, 99, 10, 112, 108, 97, 105, 110, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 10, 112, 108, 97, 105, 110, 10]);
}
#[test]
fn cut_custom_fields() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["--fields=1,3", "--delimiter=:"],
        &[97, 58, 98, 58, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 58, 99, 10]);
}
#[test]
fn cut_only_delimited() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-sf2", "-d:"],
        &[112, 108, 97, 105, 110, 10, 97, 58, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 10]);
}
#[test]
fn cut_long_only() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["--only-delimited", "-f1"],
        &[112, 108, 97, 105, 110, 10, 97, 9, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10]);
}
#[test]
fn cut_fields_output_delimiter() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-f1,3", "-d:", "--output-delimiter=--"],
        &[97, 58, 98, 58, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 45, 45, 99, 10]);
}
#[test]
fn cut_empty_output_delim() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["-f1,3", "-d:", "--output-delimiter="],
        &[97, 58, 98, 58, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 0, 99, 10]);
}
#[test]
fn cut_zero() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "cut",
        &["--zero-terminated", "-b2"],
        &[97, 98, 99, 0, 100, 101, 102, 0],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 0, 101, 0]);
}
#[test]
fn cut_n_ignored() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-nb1"], &[97, 98, 99, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10]);
}
#[test]
fn cut_multiple_files() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97, 98, 99, 10]);
    sandbox.write("b", [120, 121, 122, 10]);
    let out = sandbox.run("cut", &["-b1", "a", "b"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 120, 10]);
}
#[test]
fn cut_missing() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-b1", "missing"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn cut_bad_option() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["--oops"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn cut_bad_range() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-b3-1"], &[97, 98, 99, 10]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn cut_two_lists() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-b1", "-c2"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn cut_multibyte_delim() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-f1", "-dxx"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn cut_partial() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-b2-"], &[97, 98, 99]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 99, 10]);
}
#[test]
fn cut_nul_input_delim() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-f2", "-d", ""], &[97, 0, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[98, 10]);
}
#[test]
fn cut_empty_fields() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-f2-", "-d:"], &[97, 58, 58, 99, 58, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[58, 99, 58, 10]);
}
#[test]
fn cut_empty() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("cut", &["-b1"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn cut_help() {
    let out = run("cut", &["--help"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
#[test]
fn cut_version() {
    let out = run("cut", &["--version"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
