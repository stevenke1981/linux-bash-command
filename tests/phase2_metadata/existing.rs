use super::*;

#[test]
fn ls_ignore_hide_backup_rules_and_extension_sort() {
    let s = Sandbox::new();
    for name in ["z", "one.b", "two.a", "backup~", "skip.txt", ".hidden"] {
        s.write(name, b"");
    }
    let out = run(
        &s,
        "ls",
        &[
            "--sort=extension",
            "--ignore=skip*",
            "--ignore-backups",
            "--hide=one*",
        ],
    );
    success(&out);
    assert_eq!(out.stdout, b"z\ntwo.a\n");
    let out = run(
        &s,
        "ls",
        &["-A", "--ignore=skip*", "-B", "--hide=one*", "-X"],
    );
    success(&out);
    assert!(text(&out).contains("one.b"));
    assert!(!text(&out).contains("backup~"));
}
#[test]
fn ls_inode_blocks_quoting_and_size_scaling() {
    let s = Sandbox::new();
    s.write("a name", vec![0; 1536]);
    let out = run(&s, "ls", &["-isdQ", "--block-size=1024", "a name"]);
    success(&out);
    let metadata = std::fs::metadata(s.path.join("a name")).unwrap();
    let id = agentcmd::common::meta::file_identity(&s.path.join("a name"), &metadata);
    assert!(text(&out).starts_with(&format!("{} ", id.inode)));
    assert!(text(&out).ends_with("\"a name\"\n"));
    let out = run(&s, "ls", &["-ld", "--si", "a name"]);
    success(&out);
    assert!(text(&out).contains(" 1.6k "));
    for (style, expected) in [
        ("literal", "a name\n"),
        ("shell", "'a name'\n"),
        ("shell-always", "'a name'\n"),
        ("shell-escape", "'a name'\n"),
        ("c", "\"a name\"\n"),
        ("escape", "a\\ name\n"),
    ] {
        let out = run(&s, "ls", &[&format!("--quoting-style={style}"), "a name"]);
        success(&out);
        assert_eq!(text(&out), expected);
    }
}
#[test]
fn ls_layout_and_indicators() {
    let s = Sandbox::new();
    s.write("one", b"");
    s.write("two", b"");
    std::fs::create_dir(s.path.join("dir")).unwrap();
    let out = run(&s, "ls", &["-m", "--width=80", "--file-type"]);
    success(&out);
    assert_eq!(out.stdout, b"dir/, one, two\n");
    for args in [
        vec!["-C", "-w", "10", "-T", "0"],
        vec!["-x", "--width=10"],
        vec!["--indicator-style=slash"],
        vec!["--indicator-style=none"],
        vec!["--indicator-style=classify"],
        vec!["--quote-name"],
        vec!["--literal"],
    ] {
        success(&run(&s, "ls", &args));
    }
    assert_eq!(run(&s, "ls", &["-D"]).status.code(), Some(2));
}
#[test]
fn ls_selected_time_and_full_resolution() {
    let s = Sandbox::new();
    s.write("sample", b"");
    filetime::set_file_times(
        s.path.join("sample"),
        filetime::FileTime::from_unix_time(946684800, 0),
        filetime::FileTime::from_unix_time(1700000000, 0),
    )
    .unwrap();
    let out = run(
        &s,
        "ls",
        &["-l", "--time=access", "--time-style=+%Y", "sample"],
    );
    success(&out);
    assert!(text(&out).contains(" 2000 sample"));
    let out = run(&s, "ls", &["--full-time", "sample"]);
    success(&out);
    assert!(text(&out).contains(".000000000 +0000 sample"));
    for flag in ["-c", "-u"] {
        success(&run(&s, "ls", &[flag, "sample"]));
    }
}
#[test]
fn find_ids_owners_links_and_samefile() {
    let s = Sandbox::new();
    s.write("one", b"abc");
    std::fs::hard_link(s.path.join("one"), s.path.join("two")).unwrap();
    let metadata = std::fs::metadata(s.path.join("one")).unwrap();
    let (uid, gid) = agentcmd::common::meta::owner_ids(&metadata);
    let id = agentcmd::common::meta::file_identity(&s.path.join("one"), &metadata);
    for args in [
        vec![".", "-samefile", "one"],
        vec![".", "-links", "2"],
        vec![".", "-inum", &id.inode.to_string()],
    ] {
        let out = run(&s, "find", &args);
        success(&out);
        assert!(text(&out).contains("./one\n") && text(&out).contains("./two\n"));
    }
    let uid = uid.to_string();
    let gid = gid.to_string();
    for predicate in ["-user", "-uid"] {
        assert_eq!(run(&s, "find", &["one", predicate, &uid]).stdout, b"one\n");
    }
    for predicate in ["-group", "-gid"] {
        assert_eq!(run(&s, "find", &["one", predicate, &gid]).stdout, b"one\n");
    }
    for predicate in ["-nouser", "-nogroup"] {
        assert!(run(&s, "find", &["one", predicate]).stdout.is_empty());
    }
}
#[test]
fn find_newer_xy_regex_and_file_outputs() {
    let s = Sandbox::new();
    s.write("sample", b"abc");
    let out = run(
        &s,
        "find",
        &[
            "sample",
            "-newermt",
            "@1",
            "-regextype",
            "posix-basic",
            "-regex",
            "\\(sample\\)",
            "-fprint",
            "paths",
            "-fprint0",
            "zero",
            "-fprintf",
            "formatted",
            "%f:%s\\n",
            "-fls",
            "listing",
        ],
    );
    success(&out);
    assert!(out.stdout.is_empty());
    assert_eq!(std::fs::read(s.path.join("paths")).unwrap(), b"sample\n");
    assert_eq!(std::fs::read(s.path.join("zero")).unwrap(), b"sample\0");
    assert_eq!(
        std::fs::read(s.path.join("formatted")).unwrap(),
        b"sample:3\n"
    );
    assert!(std::fs::read_to_string(s.path.join("listing"))
        .unwrap()
        .ends_with(" sample\n"));
    for kind in ["emacs", "posix-extended"] {
        success(&run(
            &s,
            "find",
            &["sample", "-regextype", kind, "-regex", "sample"],
        ));
    }
    success(&run(
        &s,
        "find",
        &[
            "sample",
            "-xdev",
            "-mount",
            "-ignore_readdir_race",
            "-noleaf",
            "-true",
            "!",
            "-false",
            "-xtype",
            "f",
        ],
    ));
}
#[test]
fn find_interactive_ok_reads_confirmation() {
    let s = Sandbox::new();
    s.write("sample", b"");
    for action in ["-ok", "-okdir"] {
        let mut command = Command::new(common::binary("find"));
        command.current_dir(&s.path).args([
            "sample",
            action,
            env!("CARGO_BIN_EXE_echo"),
            "{}",
            ";",
        ]);
        let out = common::execute(command, b"y\n");
        success(&out);
        assert!(text(&out).contains("sample"));
        assert!(String::from_utf8_lossy(&out.stderr).contains(" > ? "));
    }
}
#[test]
fn df_output_fields_total_and_rejections() {
    let s = Sandbox::new();
    let out = run(
        &s,
        "df",
        &[
            "--output=source,size,used,avail,pcent,target",
            "--total",
            ".",
        ],
    );
    success(&out);
    let listing = text(&out);
    assert!(
        listing.contains("Filesystem")
            && listing.contains("Mounted on")
            && listing.contains("total")
    );
    let out = run(&s, "df", &["--output", "."]);
    success(&out);
    assert!(text(&out).contains("Inodes"));
    for args in [
        vec!["--output=bogus"],
        vec!["--output="],
        vec!["--output=size", "-P"],
        vec!["--output=size", "-T"],
        vec!["-i"],
    ] {
        assert_eq!(run(&s, "df", &args).status.code(), Some(1));
    }
}
#[cfg(unix)]
#[test]
fn link_target_predicates_and_ls_dereference() {
    let s = Sandbox::new();
    s.write("target", b"abc");
    std::os::unix::fs::symlink("target", s.path.join("link")).unwrap();
    assert_eq!(
        run(&s, "find", &["link", "-lname", "target", "-xtype", "f"]).stdout,
        b"link\n"
    );
    assert_eq!(
        run(&s, "find", &["link", "-ilname", "TARGET"]).stdout,
        b"link\n"
    );
    for flag in ["--dereference", "--dereference-command-line"] {
        let out = run(&s, "ls", &["-ld", flag, "link"]);
        success(&out);
        assert!(!text(&out).contains(" -> "));
    }
}
