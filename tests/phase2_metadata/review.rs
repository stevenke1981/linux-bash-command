use super::*;

#[test]
fn du_deduplicates_regular_files_directories_and_descendants() {
    let s = Sandbox::new();
    s.write("dir/file", b"abc");
    let directory = 0;
    for args in [vec!["-bc", "dir", "dir"], vec!["-bc", "dir", "dir/file"]] {
        let out = run(&s, "du", &args);
        success(&out);
        assert_eq!(
            text(&out),
            format!("{}\tdir\n{}\ttotal\n", directory + 3, directory + 3)
        );
    }
    assert_eq!(
        run(&s, "du", &["-bc", "dir/file", "dir/file"]).stdout,
        b"3\tdir/file\n3\ttotal\n"
    );
    let out = run(&s, "du", &["-blc", "dir", "dir"]);
    success(&out);
    assert_eq!(
        text(&out),
        format!(
            "{0}\tdir\n{0}\tdir\n{1}\ttotal\n",
            directory + 3,
            2 * (directory + 3)
        )
    );
    assert_eq!(
        run(&s, "du", &["-blc", "dir/file", "dir/file"]).stdout,
        b"3\tdir/file\n3\tdir/file\n6\ttotal\n"
    );
}

#[test]
fn du_reads_exclusion_patterns_from_stdin() {
    let s = Sandbox::new();
    s.write("dir/keep", b"abc");
    s.write("dir/skip", b"abcd");
    let mut command = Command::new(env!("CARGO_BIN_EXE_du"));
    command.current_dir(&s.path).args(["-ab", "-X", "-", "dir"]);
    let out = common::execute(command, b"skip\n");
    success(&out);
    assert!(text(&out).contains("\tdir/keep\n"));
    assert!(!text(&out).contains("skip"));
}

#[cfg(unix)]
#[test]
fn du_separate_dirs_uses_dereferenced_child_kind() {
    let s = Sandbox::new();
    s.write("outside/file", b"abcdefgh");
    std::fs::create_dir(s.path.join("dir")).unwrap();
    std::os::unix::fs::symlink("../outside", s.path.join("dir/link")).unwrap();
    let out = run(&s, "du", &["-bLS", "dir"]);
    success(&out);
    let own = 0;
    assert!(text(&out).ends_with(&format!("{own}\tdir\n")), "{out:?}");
    assert!(text(&out).contains("\tdir/link\n"));
}

#[test]
fn canonical_paths_require_a_directory_after_a_separator() {
    let s = Sandbox::new();
    s.write("file", b"x");
    for (command, flags) in [
        ("realpath", vec![]),
        ("realpath", vec!["-e"]),
        ("readlink", vec!["-e"]),
        ("readlink", vec!["-f"]),
    ] {
        for file in ["file/", "file/."] {
            let mut args = flags.clone();
            args.push(file);
            let out = run(&s, command, &args);
            assert_eq!(out.status.code(), Some(1), "{command} {args:?}: {out:?}");
            assert!(out.stdout.is_empty());
        }
    }
    for command in ["realpath", "readlink"] {
        success(&run(&s, command, &["-m", "file/"]));
    }
    success(&run(&s, "realpath", &["missing/"]));
    success(&run(&s, "readlink", &["-f", "missing/"]));
    assert_eq!(run(&s, "realpath", &["missing/."]).status.code(), Some(1));
    assert_eq!(
        run(&s, "readlink", &["-f", "missing/."]).status.code(),
        Some(1)
    );
}

#[test]
fn stat_filesystem_hex_fields_honor_numeric_flags_and_unknown_directives() {
    let s = Sandbox::new();
    s.write("file", b"x");
    let out = run(
        &s,
        "stat",
        &["-f", "--printf=%i|%t|%020i|%016t|%#i|%#t|%Q", "file"],
    );
    success(&out);
    let text = text(&out);
    let parts: Vec<_> = text.split('|').collect();
    assert_eq!(parts.len(), 7);
    for (plain, padded, alternate, width) in [
        (parts[0], parts[2], parts[4], 20),
        (parts[1], parts[3], parts[5], 16),
    ] {
        if let Ok(number) = u64::from_str_radix(plain, 16) {
            assert_eq!(padded, format!("{number:0width$x}"));
            assert_eq!(
                alternate,
                if number == 0 {
                    "0".into()
                } else {
                    format!("0x{number:x}")
                }
            );
        } else {
            assert_eq!(padded.trim(), plain);
        }
    }
    assert_eq!(parts[6], "?");
    assert_eq!(run(&s, "stat", &["--printf=%Q%k", "file"]).stdout, b"??");
}

#[test]
fn df_custom_fields_preserve_operand_order_and_duplicates() {
    let s = Sandbox::new();
    s.write("a", b"x");
    s.write("b", b"x");
    let out = run(&s, "df", &["--output=file,target", "b", "a", "a"]);
    success(&out);
    let text = text(&out);
    let rows: Vec<_> = text
        .lines()
        .skip(1)
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .collect();
    assert_eq!(rows.len(), 3, "{text}");
    assert_eq!([rows[0][0], rows[1][0], rows[2][0]], ["b", "a", "a"]);
    assert_eq!(rows[0][1], rows[1][1]);
    assert_eq!(rows[1][1], rows[2][1]);
    let out = run(&s, "df", &["--output=source,source", "a"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("field 'source' used more than once"));
}

#[test]
fn ls_extension_sort_includes_dotfiles() {
    let s = Sandbox::new();
    for name in ["plain", "x.a", ".bashrc", ".z"] {
        s.write(name, b"");
    }
    assert_eq!(
        run(&s, "ls", &["-A1X"]).stdout,
        b"plain\nx.a\n.bashrc\n.z\n"
    );
    #[cfg(unix)]
    {
        s.write("a.", b"");
        assert_eq!(
            run(&s, "ls", &["-A1X"]).stdout,
            b"a.\nplain\nx.a\n.bashrc\n.z\n"
        );
    }
}

#[test]
fn ls_time_styles_choose_old_format_and_respect_last_option() {
    let s = Sandbox::new();
    s.write("old", b"");
    s.write("recent", b"");
    let old = filetime::FileTime::from_unix_time(946684800, 0);
    filetime::set_file_times(s.path.join("old"), old, old).unwrap();
    for (style, expected) in [("iso", "2000-01-01  old"), ("locale", "Jan  1  2000 old")] {
        let out = run(&s, "ls", &["-l", &format!("--time-style={style}"), "old"]);
        success(&out);
        assert!(text(&out).ends_with(&format!("{expected}\n")), "{out:?}");
        let out = run(
            &s,
            "ls",
            &["-l", &format!("--time-style={style}"), "recent"],
        );
        success(&out);
        assert!(text(&out).contains(':'), "{out:?}");
    }
    let full = run(&s, "ls", &["--time-style=+%Y", "--full-time", "old"]);
    assert!(text(&full).contains("2000-01-01 00:00:00.000000000 +0000 old"));
    let year = run(&s, "ls", &["--full-time", "--time-style=+%Y", "old"]);
    assert!(text(&year).ends_with(" 2000 old\n"));
}

#[test]
fn find_file_actions_share_repeated_and_equivalent_destinations() {
    let s = Sandbox::new();
    let out = run(
        &s,
        "find",
        &[
            ".",
            "-maxdepth",
            "0",
            "-fprint",
            "out",
            "-fprint",
            "./out",
            "-fprintf",
            "out",
            "%p!",
        ],
    );
    success(&out);
    assert!(out.stdout.is_empty());
    assert_eq!(std::fs::read(s.path.join("out")).unwrap(), b".\n.\n.!");
}

#[test]
fn stat_context_placeholder_reports_unavailable_context() {
    let s = Sandbox::new();
    s.write("file", b"x");
    let out = run(&s, "stat", &["--printf=%C %%C", "file"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"? %C");
    assert!(String::from_utf8_lossy(&out.stderr).contains("security context"));
    success(&run(&s, "stat", &["--printf=%%C", "file"]));
}

#[test]
fn stat_default_names_use_shell_quoting_and_custom_names_always_quote() {
    let s = Sandbox::new();
    s.write("sample", b"x");
    s.write("has space", b"x");
    let out = run(&s, "stat", &["sample"]);
    success(&out);
    assert!(out.stdout.starts_with(b"  File: sample\n"));
    let out = run(&s, "stat", &["has space"]);
    success(&out);
    assert!(out.stdout.starts_with(b"  File: 'has space'\n"));
    assert_eq!(
        run(&s, "stat", &["--format=%N", "sample"]).stdout,
        b"'sample'\n"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("sample", s.path.join("link")).unwrap();
        let out = run(&s, "stat", &["link"]);
        success(&out);
        assert!(out.stdout.starts_with(b"  File: link -> sample\n"));
        assert_eq!(
            run(&s, "stat", &["--format=%N", "link"]).stdout,
            b"'link' -> 'sample'\n"
        );
    }
}

#[test]
fn du_modification_time_uses_bare_time_option() {
    let s = Sandbox::new();
    s.write("file", b"x");
    success(&run(&s, "du", &["--time", "file"]));
    for word in ["mtime", "modification", "modify"] {
        assert_eq!(
            run(&s, "du", &[&format!("--time={word}"), "file"])
                .status
                .code(),
            Some(1)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn gnu_oracle_du_on_the_same_directory() {
    if std::env::var("AGENTCMD_GNU_DIFF").as_deref() != Ok("1") {
        eprintln!("du same-file oracle: NOT_RUN (set AGENTCMD_GNU_DIFF=1)");
        return;
    }
    let s = Sandbox::new();
    s.write("dir/leaf", b"hello");
    s.write("dir/sub/file", b"abc");
    for args in [
        vec!["-ab", "dir"],
        vec!["-bS", "dir"],
        vec!["-bs", "dir"],
        vec!["-bc", "dir", "dir"],
        vec!["-ab", "--max-depth=1", "dir"],
        vec!["-b", "--exclude=leaf", "dir"],
        vec!["-s", "dir"],
    ] {
        let expected = Command::new("/usr/bin/du")
            .args(&args)
            .current_dir(&s.path)
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .output()
            .unwrap();
        let actual = run(&s, "du", &args);
        let sorted = |bytes: &[u8]| {
            let mut rows = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
            rows.sort();
            rows.into_iter().map(<[u8]>::to_vec).collect::<Vec<_>>()
        };
        assert_eq!(actual.status.code(), expected.status.code(), "{args:?}");
        assert_eq!(sorted(&actual.stdout), sorted(&expected.stdout), "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn find_newer_reference_symlinks_follow_the_selected_policy() {
    let s = Sandbox::new();
    s.write("target", b"");
    s.write("candidate", b"");
    std::os::unix::fs::symlink("target", s.path.join("reference")).unwrap();
    std::os::unix::fs::symlink("missing", s.path.join("dangling")).unwrap();
    let stamp = |seconds| filetime::FileTime::from_unix_time(seconds, 0);
    filetime::set_file_times(s.path.join("target"), stamp(300), stamp(300)).unwrap();
    filetime::set_file_times(s.path.join("candidate"), stamp(200), stamp(200)).unwrap();
    for path in ["reference", "dangling"] {
        filetime::set_symlink_file_times(s.path.join(path), stamp(100), stamp(100)).unwrap();
        for predicate in ["-newermm", "-newer"] {
            assert_eq!(
                run(&s, "find", &["-P", "candidate", predicate, path]).stdout,
                b"candidate\n"
            );
        }
    }
    for follow in ["-H", "-L"] {
        let out = run(&s, "find", &[follow, "candidate", "-newermm", "reference"]);
        success(&out);
        assert!(out.stdout.is_empty());
        assert_eq!(
            run(&s, "find", &[follow, "candidate", "-newermm", "dangling"])
                .status
                .code(),
            Some(1)
        );
    }
}
