use super::*;

#[test]
fn info() {
    let s = Sandbox::new();
    for flag in ["--help", "--version"] {
        success(&run(&s, "du", &[flag]));
    }
}
#[test]
fn bytes_and_apparent_size() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    assert_eq!(run(&s, "du", &["--bytes", "sample"]).stdout, b"3\tsample\n");
    assert_eq!(
        run(&s, "du", &["--apparent-size", "--block-size=2", "sample"]).stdout,
        b"2\tsample\n"
    );
}
#[test]
fn size_units_and_human_formats() {
    let s = Sandbox::new();
    s.write("sample", vec![0; 1536]);
    for (args, expected) in [
        (vec!["-b", "-h", "sample"], "1.5K\tsample\n"),
        (vec!["-b", "--si", "sample"], "1.6k\tsample\n"),
        (vec!["--apparent-size", "-k", "sample"], "2\tsample\n"),
        (vec!["--apparent-size", "-m", "sample"], "1\tsample\n"),
    ] {
        assert_eq!(text(&run(&s, "du", &args)), expected);
    }
}
#[test]
fn hard_links_count_once_unless_requested() {
    let s = Sandbox::new();
    s.write("one", b"abc");
    std::fs::hard_link(s.path.join("one"), s.path.join("two")).unwrap();
    assert_eq!(run(&s, "du", &["-b", "one", "two"]).stdout, b"3\tone\n");
    assert_eq!(
        run(&s, "du", &["-b", "--count-links", "one", "two"]).stdout,
        b"3\tone\n3\ttwo\n"
    );
}
#[test]
fn directory_depth_and_all_files() {
    let s = Sandbox::new();
    s.write("dir/sub/file", b"abc");
    let out = run(&s, "du", &["-ab", "--max-depth=1", "dir"]);
    success(&out);
    let listing = text(&out);
    assert!(listing.contains("\tdir/sub\n") && listing.contains("\tdir\n"));
    assert!(!listing.contains("file"));
    let summary = run(&s, "du", &["-b", "--summarize", "dir"]);
    assert_eq!(text(&summary).lines().count(), 1);
}
#[test]
fn excluded_files_and_pattern_file() {
    let s = Sandbox::new();
    s.write("dir/keep", b"abc");
    s.write("dir/drop.tmp", b"data");
    s.write("patterns", b"*.tmp\n");
    for args in [
        vec!["-ab", "--exclude=*.tmp", "dir"],
        vec!["-ab", "--exclude-from=patterns", "dir"],
    ] {
        let out = run(&s, "du", &args);
        success(&out);
        assert!(!text(&out).contains("drop.tmp"));
        assert!(text(&out).contains("keep"));
    }
}
#[test]
fn totals_and_null_delimiters() {
    let s = Sandbox::new();
    s.write("one", b"abc");
    s.write("two", b"12");
    let out = run(&s, "du", &["-b", "--total", "--null", "one", "two"]);
    success(&out);
    assert_eq!(out.stdout, b"3\tone\x002\ttwo\x005\ttotal\x00");
}
#[test]
fn thresholds_apply_before_scaling() {
    let s = Sandbox::new();
    s.write("one", b"abc");
    assert!(run(&s, "du", &["-b", "--threshold=4", "one"])
        .stdout
        .is_empty());
    assert_eq!(
        run(&s, "du", &["-b", "--threshold=-3", "one"]).stdout,
        b"3\tone\n"
    );
}
#[test]
fn times_use_selected_clock_and_style() {
    let s = Sandbox::new();
    s.write("one", b"abc");
    let file = std::fs::File::options()
        .write(true)
        .open(s.path.join("one"))
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1700000000))
        .unwrap();
    let out = run(&s, "du", &["-b", "--time", "--time-style=+%Y", "one"]);
    success(&out);
    assert_eq!(out.stdout, b"3\t2023\tone\n");
}
#[test]
fn unsupported_inodes_and_conflicting_depth_fail() {
    let s = Sandbox::new();
    for args in [
        vec!["--inodes"],
        vec!["-s", "--max-depth=2"],
        vec!["--block-size=0"],
        vec!["--block-size=0", "--help"],
        vec!["--bogus"],
    ] {
        assert_eq!(run(&s, "du", &args).status.code(), Some(1));
    }
}
#[test]
fn separate_dirs_and_one_filesystem_are_honored() {
    let s = Sandbox::new();
    s.write("dir/sub/file", b"abc");
    let out = run(
        &s,
        "du",
        &["-b", "--separate-dirs", "--one-file-system", "dir"],
    );
    success(&out);
    let dir_size = 0;
    assert!(text(&out).ends_with(&format!("{dir_size}\tdir\n")));
}
#[cfg(unix)]
#[test]
fn follow_modes_distinguish_commandline_and_nested_links() {
    let s = Sandbox::new();
    s.write("target", b"abcdefgh");
    std::os::unix::fs::symlink("target", s.path.join("link")).unwrap();
    assert_eq!(
        run(&s, "du", &["-b", "--dereference-args", "link"]).stdout,
        b"8\tlink\n"
    );
    assert_eq!(
        run(&s, "du", &["-b", "--dereference", "link"]).stdout,
        b"8\tlink\n"
    );
    assert_eq!(
        run(&s, "du", &["-b", "--no-dereference", "link"]).stdout,
        b"6\tlink\n"
    );
}
