mod common;

use common::{run, Sandbox};
use std::fs;
use std::time::{Duration, SystemTime};

fn assert_test(sandbox: &Sandbox, args: &[&str], expected: i32) {
    let out = sandbox.run("test", args, &[]);
    assert_eq!(out.status.code(), Some(expected), "{args:?}: {out:?}");
    assert!(out.stdout.is_empty(), "{args:?}: {out:?}");
    if expected == 2 {
        assert!(out.stderr.starts_with(b"test: "), "{args:?}: {out:?}");
    } else {
        assert!(out.stderr.is_empty(), "{args:?}: {out:?}");
    }
}

#[test]
fn sort_named_algorithms_order_their_respective_keys() {
    for (mode, input, expected) in [
        ("general-numeric", "1e2\n2\n", "2\n1e2\n"),
        ("human-numeric", "1K\n900\n", "900\n1K\n"),
        ("month", "Dec\nJan\n", "Jan\nDec\n"),
        ("numeric", "10\n2\n", "2\n10\n"),
        ("version", "v10\nv2\n", "v2\nv10\n"),
    ] {
        let out = run("sort", &[&format!("--sort={mode}")], input.as_bytes());
        assert_eq!(out.status.code(), Some(0), "{mode}: {out:?}");
        assert_eq!(out.stdout, expected.as_bytes(), "{mode}");
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn sort_random_source_is_repeatable_and_groups_equal_keys() {
    let sandbox = Sandbox::new();
    sandbox.write("seed", [42; 64]);
    let args = ["--sort=random", "--random-source=seed"];
    let first = sandbox.run("sort", &args, b"pear\napple\npear\nbanana\n");
    let second = sandbox.run("sort", &args, b"pear\napple\npear\nbanana\n");
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    assert_eq!(second.status.code(), Some(0), "{second:?}");
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());
    let text = String::from_utf8(first.stdout).unwrap();
    assert!(text.contains("pear\npear\n"));
    let mut lines: Vec<_> = text.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["apple", "banana", "pear", "pear"]);
}

#[test]
fn sort_merge_writes_unique_numeric_records_to_output() {
    let sandbox = Sandbox::new();
    sandbox.write("odd", b"1\n3\n5\n");
    sandbox.write("even", b"2\n3\n4\n");
    sandbox.write("empty", b"");
    let out = sandbox.run(
        "sort",
        &[
            "--merge",
            "--unique",
            "--sort=numeric",
            "--output=merged",
            "odd",
            "empty",
            "even",
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
    assert_eq!(
        fs::read(sandbox.path.join("merged")).unwrap(),
        b"1\n2\n3\n4\n5\n"
    );
}

#[test]
fn sort_files0_lists_and_nul_field_separator() {
    let sandbox = Sandbox::new();
    sandbox.write("first", b"b\n");
    sandbox.write("second", b"a\n");
    sandbox.write("names", b"first\0second\0");
    let out = sandbox.run("sort", &["--files0-from=names"], &[]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"a\nb\n");
    assert!(out.stderr.is_empty());
    let out = sandbox.run(
        "sort",
        &["--field-separator=\\0", "--key=2,2n"],
        b"left\x002\nright\x001\n",
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"right\x001\nleft\x002\n");
}

#[test]
fn sort_quiet_checks_report_only_the_exit_status() {
    for option in ["-C", "--check=quiet", "--check=silent"] {
        for (input, status) in [(b"b\na\n".as_slice(), 1), (b"a\nb\n", 0)] {
            let out = run("sort", &[option], input);
            assert_eq!(out.status.code(), Some(status), "{option}: {out:?}");
            assert!(out.stdout.is_empty());
            assert!(out.stderr.is_empty());
        }
    }
}

#[test]
fn sort_resource_options_accept_valid_values() {
    let out = run(
        "sort",
        &[
            "--parallel=2",
            "--batch-size=2",
            "--buffer-size=8K",
            "--temporary-directory=.",
        ],
        b"b\na\n",
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"a\nb\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn sort_reverse_also_reverses_the_whole_line_tiebreaker() {
    let out = run("sort", &["-fnr"], b"A\na\n");
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"a\nA\n");
}

#[test]
fn sort_rejects_invalid_or_conflicting_options() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["-n", "-g"],
        vec!["--sort=month", "--sort=version"],
        vec!["--sort=unknown"],
        vec!["--field-separator=ab"],
        vec!["--check=unknown"],
        vec!["--check", "--merge"],
        vec!["--check", "--output=unused"],
        vec!["--check", "one", "two"],
        vec!["--parallel=0"],
        vec!["--parallel=invalid"],
        vec!["--batch-size=1"],
        vec!["--buffer-size=invalid"],
        vec!["--buffer-size=1?"],
        vec!["--key=0"],
    ] {
        let out = sandbox.run("sort", &args, b"a\n");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}: {out:?}");
        assert!(out.stderr.starts_with(b"sort: "), "{args:?}: {out:?}");
    }
    assert!(!sandbox.path.join("unused").exists());
}

#[test]
fn sort_reports_files0_and_random_source_errors() {
    let sandbox = Sandbox::new();
    sandbox.write("empty", b"");
    sandbox.write("bad-names", b"\0");
    sandbox.write("short-seed", b"short");
    for (args, diagnostic) in [
        (vec!["--files0-from=empty", "operand"], "extra operand"),
        (vec!["--files0-from=missing-names"], "missing-names"),
        (vec!["--files0-from=empty"], "no input"),
        (vec!["--files0-from=bad-names"], "zero-length"),
        (vec!["--random-source=missing-seed"], "missing-seed"),
        (vec!["--random-source=short-seed"], "short-seed"),
    ] {
        let out = sandbox.run("sort", &args, b"b\na\n");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains(diagnostic));
    }
}

#[test]
fn sort_reports_unreadable_input_and_unwritable_output() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path.join("directory")).unwrap();
    for args in [vec!["directory"], vec!["--output=directory"]] {
        let out = sandbox.run("sort", &args, b"b\na\n");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("directory"));
    }
}

#[test]
fn test_integer_operators_support_signs_and_large_values() {
    let sandbox = Sandbox::new();
    for (args, expected) in [
        (["-1", "-lt", "0"], 0),
        (["0", "-gt", "-1"], 0),
        (["-10", "-le", "-2"], 0),
        (["+0002", "-ge", "2"], 0),
        (["-0", "-ne", "+0"], 1),
        (["9999999999999999999999999999999999999999", "-ne", "1"], 0),
        (["1", "-lt", "invalid"], 2),
    ] {
        assert_test(&sandbox, &args, expected);
    }
}

#[test]
fn test_terminal_descriptors_and_nonempty_strings() {
    let sandbox = Sandbox::new();
    assert_test(&sandbox, &["-n", "word"], 0);
    assert_test(&sandbox, &["-n", ""], 1);
    for descriptor in ["0", "1", "2", "2147483648", "-1"] {
        assert_test(&sandbox, &["-t", descriptor], 1);
    }
    assert_test(&sandbox, &["-t", "invalid"], 2);
}

#[test]
fn test_nested_expressions_and_missing_operands() {
    let sandbox = Sandbox::new();
    assert_test(&sandbox, &["!", "1", "-eq", "2"], 0);
    assert_test(&sandbox, &["(", "-n", "word", ")"], 0);
    assert_test(&sandbox, &["-n", "word", "-a", "3", "-ge", "2"], 0);
    assert_test(
        &sandbox,
        &[
            "(", "1", "-lt", "2", "-o", "3", "-ne", "3", ")", "-a", "-n", "word",
        ],
        0,
    );
    assert_test(&sandbox, &["yes", "-a", "value", "-eq"], 2);
    assert_test(&sandbox, &["yes", "-a", "-n"], 2);
    assert_test(&sandbox, &["(", "yes", "-a", "yes"], 2);
}

#[test]
fn test_file_timestamps_include_missing_operand_semantics() {
    let sandbox = Sandbox::new();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    let new = old + Duration::from_secs(3600);
    for (name, modified, accessed) in [("old", old, new), ("new", new, old)] {
        let file = fs::File::create(sandbox.path.join(name)).unwrap();
        file.set_times(
            fs::FileTimes::new()
                .set_modified(modified)
                .set_accessed(accessed),
        )
        .unwrap();
    }
    assert_test(&sandbox, &["-N", "new"], 0);
    assert_test(&sandbox, &["-N", "old"], 1);
    for (args, expected) in [
        (["new", "-nt", "old"], 0),
        (["old", "-ot", "new"], 0),
        (["old", "-nt", "new"], 1),
        (["new", "-ot", "old"], 1),
        (["new", "-nt", "missing"], 0),
        (["missing", "-ot", "new"], 0),
        (["missing", "-nt", "new"], 1),
        (["new", "-ot", "missing"], 1),
        (["missing", "-nt", "absent"], 1),
    ] {
        assert_test(&sandbox, &args, expected);
    }
}

#[test]
fn test_regular_file_access_owner_and_special_file_predicates() {
    let sandbox = Sandbox::new();
    sandbox.write("regular", b"data");
    fs::create_dir(sandbox.path.join("directory")).unwrap();
    for flag in ["-r", "-w", "-O", "-G"] {
        assert_test(&sandbox, &[flag, "regular"], 0);
    }
    assert_test(&sandbox, &["-x", "directory"], 0);
    for flag in ["-b", "-c", "-p", "-S", "-h", "-L"] {
        assert_test(&sandbox, &[flag, "regular"], 1);
    }
}

#[cfg(unix)]
#[test]
fn test_unix_symlinks_fifo_socket_and_mode_bits() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::os::unix::net::UnixListener;

    let sandbox = Sandbox::new();
    sandbox.write("executable", b"data");
    fs::set_permissions(
        sandbox.path.join("executable"),
        fs::Permissions::from_mode(0o7700),
    )
    .unwrap();
    for flag in ["-x", "-u", "-g", "-k"] {
        assert_test(&sandbox, &[flag, "executable"], 0);
    }
    symlink("missing", sandbox.path.join("dangling")).unwrap();
    assert_test(&sandbox, &["-h", "dangling"], 0);
    assert_test(&sandbox, &["-L", "dangling"], 0);
    assert_test(&sandbox, &["-e", "dangling"], 1);
    let listener = UnixListener::bind(sandbox.path.join("socket")).unwrap();
    assert_test(&sandbox, &["-S", "socket"], 0);
    let fifo_status = std::process::Command::new("mkfifo")
        .arg(sandbox.path.join("fifo"))
        .status()
        .unwrap();
    assert!(fifo_status.success());
    assert_test(&sandbox, &["-p", "fifo"], 0);
    drop(listener);
}
