use crate::common::{run, Sandbox};

#[test]
fn tee_stdin() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &[], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
}
#[test]
fn tee_file() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["out"], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
    assert_eq!(
        std::fs::read(sandbox.path.join("out")).unwrap(),
        &[97, 10, 98, 10]
    );
}
#[test]
fn tee_multiple() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["a", "b"], &[116, 101, 115, 116]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[116, 101, 115, 116]);
    assert_eq!(
        std::fs::read(sandbox.path.join("a")).unwrap(),
        &[116, 101, 115, 116]
    );
    assert_eq!(
        std::fs::read(sandbox.path.join("b")).unwrap(),
        &[116, 101, 115, 116]
    );
}
#[test]
fn tee_append() {
    let sandbox = Sandbox::new();
    sandbox.write("out", [111, 108, 100]);
    let out = sandbox.run("tee", &["--append", "out"], &[110, 101, 119]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[110, 101, 119]);
    assert_eq!(
        std::fs::read(sandbox.path.join("out")).unwrap(),
        &[111, 108, 100, 110, 101, 119]
    );
}
#[test]
fn tee_truncate() {
    let sandbox = Sandbox::new();
    sandbox.write("out", [111, 108, 100]);
    let out = sandbox.run("tee", &["out"], &[110, 101, 119]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[110, 101, 119]);
    assert_eq!(
        std::fs::read(sandbox.path.join("out")).unwrap(),
        &[110, 101, 119]
    );
}
#[test]
fn tee_ignore() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--ignore-interrupts", "out"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("out")).unwrap(), &[97]);
}
#[test]
fn tee_p() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["-p", "out"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("out")).unwrap(), &[97]);
}
#[test]
fn tee_output_error_warn() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--output-error=warn", "out"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("out")).unwrap(), &[97]);
}
#[test]
fn tee_output_error_warn_nopipe() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--output-error=warn-nopipe", "out"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("out")).unwrap(), &[97]);
}
#[test]
fn tee_output_error_exit() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--output-error=exit", "out"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("out")).unwrap(), &[97]);
}
#[test]
fn tee_output_error_exit_nopipe() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--output-error=exit-nopipe", "out"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("out")).unwrap(), &[97]);
}
#[test]
fn tee_output_error_default() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--output-error", "out"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("out")).unwrap(), &[97]);
}
#[test]
fn tee_missing() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["missing/out"], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 98, 10]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tee_open_failure_continue() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["missing/out", "good"], &[97]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert!(!out.stderr.is_empty());
    assert_eq!(std::fs::read(sandbox.path.join("good")).unwrap(), &[97]);
}
#[test]
fn tee_bad_option() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--oops"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tee_bad_mode() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["--output-error=oops"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tee_empty() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
}
#[test]
fn tee_dash_is_filename() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["-"], &[97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
    assert_eq!(std::fs::read(sandbox.path.join("-")).unwrap(), &[97]);
}
#[test]
fn tee_long_binary() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tee", &["out"], &[97, 98, 99, 0, 120, 121, 122, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 99, 0, 120, 121, 122, 10]);
    assert_eq!(
        std::fs::read(sandbox.path.join("out")).unwrap(),
        &[97, 98, 99, 0, 120, 121, 122, 10]
    );
}
#[test]
fn tee_help() {
    let out = run("tee", &["--help"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
#[test]
fn tee_version() {
    let out = run("tee", &["--version"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
