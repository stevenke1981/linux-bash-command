//! Part of the fsops CLI tests; helpers come from the parent module (`tests/fsops.rs`).

use super::*;

// ---------------------------------------------------------------- ln

#[test]
fn ln_help_and_version() {
    assert_help_and_version("ln");
}

#[test]
fn ln_creates_a_hard_link() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    assert_eq!(code(&run_in(sb.path(), "ln", &["a", "b"], b"")), 0);
    assert_eq!(sb.read("b"), b"x");
}

#[test]
#[cfg(unix)]
fn ln_symbolic_link_stores_target_as_given() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    assert_eq!(code(&run_in(sb.path(), "ln", &["-s", "a", "l"], b"")), 0);
    assert_eq!(
        fs::read_link(sb.path().join("l")).unwrap(),
        std::path::Path::new("a")
    );
}

#[test]
#[cfg(unix)]
fn ln_relative_symbolic_link_from_nested_directory() {
    let sb = Sandbox::new();
    sb.write("d/t", b"x");
    sb.mkdir("e");
    assert_eq!(
        code(&run_in(sb.path(), "ln", &["-sr", "d/t", "e/l"], b"")),
        0
    );
    assert_eq!(
        fs::read_link(sb.path().join("e/l")).unwrap(),
        std::path::Path::new("../d/t")
    );
}

#[test]
fn ln_existing_name_needs_force() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    sb.write("b", b"y");
    let refused = run_in(sb.path(), "ln", &["a", "b"], b"");
    assert_eq!(code(&refused), 1);
    assert!(stderr(&refused).contains("ln: failed to create hard link 'b': File exists"));
    assert_eq!(code(&run_in(sb.path(), "ln", &["-f", "a", "b"], b"")), 0);
    assert_eq!(sb.read("b"), b"x");
}

#[test]
#[cfg(unix)]
fn ln_verbose_formats_for_both_kinds() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    let sym = run_in(sb.path(), "ln", &["-sv", "a", "l"], b"");
    assert_eq!(stdout(&sym), "'l' -> 'a'\n");
    let hard = run_in(sb.path(), "ln", &["-v", "a", "h"], b"");
    assert_eq!(stdout(&hard), "'h' => 'a'\n");
}

#[test]
#[cfg(unix)]
fn ln_into_directory_and_multiple_targets() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    sb.write("b", b"2");
    sb.mkdir("d");
    assert_eq!(
        code(&run_in(sb.path(), "ln", &["-s", "a", "b", "d"], b"")),
        0
    );
    assert!(fs::symlink_metadata(sb.path().join("d/a"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::symlink_metadata(sb.path().join("d/b"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn ln_missing_hard_link_target_is_reported() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "ln", &["nope", "x"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("ln: failed to access 'nope': No such file or directory"));
}

#[test]
fn ln_unsupported_directory_option_and_usage() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "ln", &["-d", "a", "b"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("option '-d' is not supported by agentcmd"));
    assert!(stderr(&run_in(sb.path(), "ln", &[], b"")).contains("ln: missing file operand"));
    assert_unknown_option("ln");
}

#[test]
fn ln_target_must_be_directory_for_several_sources() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    sb.write("b", b"2");
    let out = run_in(sb.path(), "ln", &["a", "b", "c"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("ln: target 'c': No such file or directory"));
}

// ---------------------------------------------------------------- link / unlink

#[test]
fn link_help_and_version() {
    assert_help_and_version("link");
}

#[test]
fn link_creates_a_hard_link() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    assert_eq!(code(&run_in(sb.path(), "link", &["a", "b"], b"")), 0);
    assert_eq!(sb.read("b"), b"x");
}

#[test]
fn link_usage_errors() {
    let sb = Sandbox::new();
    assert!(stderr(&run_in(sb.path(), "link", &[], b"")).contains("link: missing operand"));
    assert!(
        stderr(&run_in(sb.path(), "link", &["a"], b"")).contains("link: missing operand after 'a'")
    );
    assert!(stderr(&run_in(sb.path(), "link", &["a", "b", "c"], b""))
        .contains("link: extra operand 'c'"));
    assert_unknown_option("link");
}

#[test]
fn link_existing_destination_and_missing_source_fail() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    sb.write("b", b"y");
    let exists = run_in(sb.path(), "link", &["a", "b"], b"");
    assert_eq!(code(&exists), 1);
    assert!(stderr(&exists).contains("link: cannot create link 'b' to 'a': File exists"));
    let missing = run_in(sb.path(), "link", &["nope", "x"], b"");
    assert!(stderr(&missing).contains("link: cannot create link 'x' to 'nope'"));
}

#[test]
fn unlink_help_and_version() {
    assert_help_and_version("unlink");
}

#[test]
fn unlink_removes_a_file() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    assert_eq!(code(&run_in(sb.path(), "unlink", &["a"], b"")), 0);
    assert!(!sb.path().join("a").exists());
}

#[test]
#[cfg(unix)]
fn unlink_refuses_directories_and_bad_operand_counts() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let dir = run_in(sb.path(), "unlink", &["d"], b"");
    assert_eq!(code(&dir), 1);
    assert!(stderr(&dir).contains("unlink: cannot unlink 'd': Is a directory"));
    assert!(stderr(&run_in(sb.path(), "unlink", &[], b"")).contains("unlink: missing operand"));
    let extra = run_in(sb.path(), "unlink", &["a", "b"], b"");
    assert!(stderr(&extra).contains("unlink: extra operand 'b'"));
    assert_unknown_option("unlink");
}
