use super::*;

fn shown(path: &std::path::Path) -> String {
    agentcmd::display_path(path)
}

#[test]
fn realpath_info() {
    let s = Sandbox::new();
    for flag in ["--help", "--version"] {
        let out = run(&s, "realpath", &[flag]);
        success(&out);
        assert!(!out.stdout.is_empty());
    }
}
#[test]
fn realpath_usage() {
    let s = Sandbox::new();
    for args in [vec![], vec!["--bogus"]] {
        assert_eq!(run(&s, "realpath", &args).status.code(), Some(1));
    }
}
#[test]
fn realpath_parent_may_end_in_a_missing_name() {
    let s = Sandbox::new();
    let out = run(&s, "realpath", &["missing"]);
    success(&out);
    assert_eq!(text(&out), format!("{}\n", shown(&s.path.join("missing"))));
}
#[test]
fn realpath_existing_and_missing_policies() {
    let s = Sandbox::new();
    assert_eq!(
        run(&s, "realpath", &["--canonicalize-existing", "missing"])
            .status
            .code(),
        Some(1)
    );
    let out = run(&s, "realpath", &["--canonicalize-missing", "a/b/../c"]);
    success(&out);
    assert_eq!(text(&out), format!("{}\n", shown(&s.path.join("a/c"))));
}
#[test]
fn realpath_strip_normalizes_components() {
    let s = Sandbox::new();
    s.write("a/file", b"");
    let out = run(
        &s,
        "realpath",
        &["--strip", "--no-symlinks", "--logical", "a/../a/file"],
    );
    success(&out);
    assert_eq!(text(&out), format!("{}\n", shown(&s.path.join("a/file"))));
}
#[test]
fn realpath_relative_to_and_base() {
    let s = Sandbox::new();
    s.write("a/file", b"");
    let out = run(
        &s,
        "realpath",
        &["--relative-to=a", "--relative-base=.", "a/file"],
    );
    success(&out);
    assert_eq!(out.stdout, b"file\n");
}
#[test]
fn realpath_relative_base_keeps_outside_absolute() {
    let s = Sandbox::new();
    s.write("a/file", b"");
    s.write("b/file", b"");
    let out = run(
        &s,
        "realpath",
        &["--relative-to=a", "--relative-base=a", "b/file"],
    );
    success(&out);
    assert_eq!(text(&out), format!("{}\n", shown(&s.path.join("b/file"))));
}
#[test]
fn realpath_zero_and_multiple_operands() {
    let s = Sandbox::new();
    s.write("one", b"");
    s.write("two", b"");
    let out = run(&s, "realpath", &["-z", "--relative-to=.", "one", "two"]);
    success(&out);
    assert_eq!(out.stdout, b"one\0two\0");
}
#[test]
fn realpath_quiet_continues_after_failure() {
    let s = Sandbox::new();
    s.write("good", b"");
    let out = run(
        &s,
        "realpath",
        &["-eq", "--relative-to=.", "missing", "good"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"good\n");
    assert!(out.stderr.is_empty());
}
#[cfg(unix)]
#[test]
fn realpath_logical_and_physical_differ_at_symlink_parent() {
    let s = Sandbox::new();
    s.write("dir/target/file", b"");
    std::os::unix::fs::symlink("dir/target", s.path.join("link")).unwrap();
    let physical = run(&s, "realpath", &["-P", "--relative-to=.", "link/.."]);
    let logical = run(&s, "realpath", &["-L", "--relative-to=.", "link/.."]);
    assert_eq!(physical.stdout, b"dir\n");
    assert_eq!(logical.stdout, b".\n");
    let strip = run(&s, "realpath", &["-s", "--relative-to=.", "link"]);
    assert_eq!(strip.stdout, b"link\n");
}

#[test]
fn readlink_info() {
    let s = Sandbox::new();
    for flag in ["--help", "--version"] {
        success(&run(&s, "readlink", &[flag]));
    }
}
#[test]
fn readlink_regular_file_is_quiet_failure() {
    let s = Sandbox::new();
    s.write("plain", b"");
    let out = run(&s, "readlink", &["plain"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}
#[test]
fn readlink_verbose_reports_missing_file() {
    let s = Sandbox::new();
    let out = run(&s, "readlink", &["-v", "missing"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No such file or directory"));
}
#[test]
fn readlink_quiet_aliases_suppress_errors() {
    let s = Sandbox::new();
    for flag in ["--quiet", "--silent"] {
        let out = run(&s, "readlink", &["-v", flag, "missing"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stderr.is_empty());
    }
}
#[test]
fn readlink_canonicalize_accepts_missing_last_component() {
    let s = Sandbox::new();
    let out = run(&s, "readlink", &["-f", "missing"]);
    success(&out);
    assert_eq!(text(&out), format!("{}\n", shown(&s.path.join("missing"))));
}
#[test]
fn readlink_existing_requires_all_components() {
    let s = Sandbox::new();
    assert_eq!(
        run(&s, "readlink", &["-e", "missing"]).status.code(),
        Some(1)
    );
}
#[test]
fn readlink_missing_and_no_newline() {
    let s = Sandbox::new();
    let out = run(
        &s,
        "readlink",
        &["--canonicalize-missing", "--no-newline", "one/two"],
    );
    success(&out);
    assert_eq!(text(&out), shown(&s.path.join("one/two")));
}
#[test]
fn readlink_zero_delimiter() {
    let s = Sandbox::new();
    let out = run(&s, "readlink", &["--canonicalize", "--zero", "missing"]);
    success(&out);
    assert_eq!(out.stdout.last(), Some(&0));
}
#[test]
fn readlink_invalid_options_and_missing_operand() {
    let s = Sandbox::new();
    assert_eq!(run(&s, "readlink", &[]).status.code(), Some(1));
    assert_eq!(run(&s, "readlink", &["--bogus"]).status.code(), Some(1));
}
#[cfg(unix)]
#[test]
fn readlink_prints_raw_targets_in_operand_order() {
    let s = Sandbox::new();
    std::os::unix::fs::symlink("../first", s.path.join("one")).unwrap();
    std::os::unix::fs::symlink("second", s.path.join("two")).unwrap();
    assert_eq!(
        run(&s, "readlink", &["one", "two"]).stdout,
        b"../first\nsecond\n"
    );
}
