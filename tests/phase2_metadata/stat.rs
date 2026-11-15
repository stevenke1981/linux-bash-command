use super::*;

#[test]
fn info() {
    let s = Sandbox::new();
    for flag in ["--help", "--version"] {
        success(&run(&s, "stat", &[flag]));
    }
}
#[test]
fn file_fields_are_real_metadata() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let metadata = std::fs::metadata(s.path.join("sample")).unwrap();
    let id = agentcmd::common::meta::file_identity(&s.path.join("sample"), &metadata);
    let out = run(&s, "stat", &["-c", "%s %i %h %n", "sample"]);
    success(&out);
    assert_eq!(text(&out), format!("3 {} {} sample\n", id.inode, id.nlink));
}
#[test]
fn printf_escapes_and_no_added_newline() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let out = run(&s, "stat", &["--printf=%n\\t%s\\0", "sample"]);
    success(&out);
    assert_eq!(out.stdout, b"sample\t3\0");
    let out = run(&s, "stat", &["--printf=\\xff%n\\0", "sample"]);
    success(&out);
    assert_eq!(out.stdout, b"\xffsample\0");
}
#[test]
fn file_modes_and_type_formats() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let out = run(&s, "stat", &["--format=%a|%A|%f|%F|%C|%N", "sample"]);
    assert_eq!(out.status.code(), Some(1));
    let text = text(&out);
    assert!(
        text.contains("|regular file|?|\u{27}sample\u{27}"),
        "{text}"
    );
    assert!(!text.contains('%'));
}
#[test]
fn default_layout_has_birth_and_timestamps() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let out = run(&s, "stat", &["sample"]);
    success(&out);
    let text = text(&out);
    for label in [
        "File:", "Size:", "Blocks:", "Device:", "Inode:", "Access:", "Modify:", "Change:", "Birth:",
    ] {
        assert!(text.contains(label), "{text}");
    }
}
#[test]
fn terse_layout_is_single_record() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let out = run(&s, "stat", &["--terse", "sample"]);
    success(&out);
    assert_eq!(text(&out).lines().count(), 1);
    assert!(text(&out).starts_with("sample 3 "));
}
#[test]
fn filesystem_fields_use_native_values() {
    let s = Sandbox::new();
    let out = run(
        &s,
        "stat",
        &["--file-system", "-c", "%n|%s|%S|%l|%T|%b|%f|%a", "."],
    );
    success(&out);
    let text = text(&out);
    let fields: Vec<_> = text.trim().split('|').collect();
    assert_eq!(fields.len(), 8);
    assert!(fields[1].parse::<u64>().unwrap() > 0);
    assert!(fields[2].parse::<u64>().unwrap() > 0);
    assert!(fields[5].parse::<u64>().unwrap() >= fields[6].parse::<u64>().unwrap());
}
#[test]
fn cached_values_and_usage_errors() {
    let s = Sandbox::new();
    s.write("sample", b"");
    for value in ["always", "never", "default"] {
        success(&run(&s, "stat", &[&format!("--cached={value}"), "sample"]));
    }
    for args in [
        vec![],
        vec!["--bogus"],
        vec!["--cached=nope", "sample"],
        vec!["--cached=nope", "--help"],
    ] {
        assert_eq!(run(&s, "stat", &args).status.code(), Some(1));
    }
}
#[test]
fn multiple_files_continue_after_error() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let out = run(&s, "stat", &["-c", "%n", "missing", "sample"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"sample\n");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("os error"));
}
#[test]
fn width_flags_and_all_time_formats() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let out = run(
        &s,
        "stat",
        &["-c", "[%05s][%-5n] %x|%X|%y|%Y|%z|%Z|%w|%W", "sample"],
    );
    success(&out);
    assert!(text(&out).starts_with("[00003][sample] "));
    assert!(!text(&out).contains('%'));
}
#[cfg(unix)]
#[test]
fn dereference_changes_link_metadata_to_target() {
    let s = Sandbox::new();
    s.write("sample", b"abcdef");
    std::os::unix::fs::symlink("sample", s.path.join("link")).unwrap();
    assert_eq!(
        run(&s, "stat", &["-c", "%F", "link"]).stdout,
        b"symbolic link\n"
    );
    assert_eq!(
        run(&s, "stat", &["--dereference", "-c", "%F %s", "link"]).stdout,
        b"regular file 6\n"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn gnu_oracle_default_terse_and_filesystem_layouts_on_the_same_file() {
    if std::env::var("AGENTCMD_GNU_DIFF").as_deref() != Ok("1") {
        eprintln!("stat layout oracle: NOT_RUN (set AGENTCMD_GNU_DIFF=1)");
        return;
    }
    let sandbox = Sandbox::new();
    sandbox.write("sample", b"oracle content\n");
    for args in [
        vec!["sample"],
        vec!["--terse", "sample"],
        vec!["--file-system", "sample"],
    ] {
        let expected = Command::new("/usr/bin/stat")
            .args(&args)
            .current_dir(&sandbox.path)
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env("COLUMNS", "80")
            .env_remove("POSIXLY_CORRECT")
            .output()
            .unwrap();
        let actual = run(&sandbox, "stat", &args);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{args:?}: {actual:?} / {expected:?}"
        );
        if args.contains(&"--file-system") {
            // Other tests and background services allocate/free blocks between
            // the two stat calls. Compare every static field and the layout.
            assert_eq!(
                stable_filesystem_layout(&actual.stdout),
                stable_filesystem_layout(&expected.stdout),
                "same-file stat layout {args:?}"
            );
        } else {
            assert_eq!(
                actual.stdout, expected.stdout,
                "same-file stat layout {args:?}"
            );
        }
        assert_eq!(
            actual.stderr, expected.stderr,
            "same-file stat diagnostics {args:?}"
        );
    }
}

fn stable_filesystem_layout(bytes: &[u8]) -> String {
    let live_counts = regex::Regex::new(r"(Free|Available):[ \t]*[0-9]+[ \t]*").unwrap();
    live_counts
        .replace_all(&String::from_utf8_lossy(bytes), "$1: <live> ")
        .into_owned()
}

#[test]
fn filesystem_oracle_normalization_preserves_static_fields() {
    let a = b"Blocks: Total: 12 Free: 8   Available: 7\nInodes: Total: 20 Free: 3\n";
    let b = b"Blocks: Total: 12 Free: 6   Available: 5\nInodes: Total: 20 Free: 1\n";
    assert_eq!(stable_filesystem_layout(a), stable_filesystem_layout(b));
    assert_ne!(
        stable_filesystem_layout(a),
        stable_filesystem_layout(
            b"Blocks: Total: 99 Free: 8 Available: 7\nInodes: Total: 20 Free: 3\n"
        )
    );
}
