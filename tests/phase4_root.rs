mod common;
use common::Sandbox;

#[test]
fn closed_output_exits_quietly() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let s = Sandbox::new();
    s.write("file", b"data");
    for (name, args, input) in [
        ("column", vec!["-t"], b"a b\n".as_slice()),
        ("tree", vec!["."], b""),
        ("chmod", vec!["-v", "u+rw", "file"], b""),
    ] {
        let binary = match name {
            "column" => env!("CARGO_BIN_EXE_column"),
            "tree" => env!("CARGO_BIN_EXE_tree"),
            _ => env!("CARGO_BIN_EXE_chmod"),
        };
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let mut child = Command::new(binary)
            .args(args)
            .current_dir(&s.path)
            .stdin(Stdio::piped())
            .stdout(writer)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input);
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{name}: {out:?}");
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn all_tools_support_information_and_usage_errors() {
    for name in ["chmod", "column", "tree"] {
        for flag in ["--help", "--version"] {
            let out = common::run(name, &[flag], b"");
            assert!(out.status.success(), "{name} {flag}: {out:?}");
            assert!(!out.stdout.is_empty());
        }
        assert!(!common::run(name, &["--unknown"], b"").status.success());
    }
}
#[test]
fn chmod_handles_symbolic_octal_reference_and_multi_file_errors() {
    let s = Sandbox::new();
    s.write("one", b"one");
    s.write("two", b"two");
    for args in [
        vec!["0644", "one"],
        vec!["a-rwx,u+rw", "one"],
        vec!["--reference=one", "two"],
        vec!["u=rw,g=r,o=", "one", "two"],
        vec!["--verbose", "u+w", "one"],
        vec!["--changes", "a+X", "one"],
    ] {
        let out = s.run("chmod", &args, b"");
        assert!(out.status.success(), "{args:?}: {out:?}");
    }
    assert_eq!(s.run("chmod", &["u+z", "one"], b"").status.code(), Some(1));
    assert_eq!(
        s.run("chmod", &["--quiet", "u+w", "missing", "one"], b"")
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        s.run("chmod", &["--reference=missing", "one"], b"")
            .status
            .code(),
        Some(1)
    );
    assert_eq!(std::fs::read(s.path.join("one")).unwrap(), b"one");
}
#[test]
fn chmod_recurses_without_following_nested_links() {
    let s = Sandbox::new();
    s.write("nested/file", b"data");
    let out = s.run(
        "chmod",
        &[
            "-R",
            "--preserve-root",
            "--no-preserve-root",
            "u+rwX",
            "nested",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(s.path.join("nested"))
                .unwrap()
                .permissions()
                .mode()
                & 0o100,
            0
        );
    }
}

#[test]
fn chmod_preserves_literal_marker_like_filenames() {
    let s = Sandbox::new();
    s.write("victim", b"unrelated");
    s.write("MODE=victim", b"intended");
    let before = std::fs::metadata(s.path.join("victim"))
        .unwrap()
        .permissions();
    let target_before = std::fs::metadata(s.path.join("MODE=victim"))
        .unwrap()
        .permissions();
    let out = s.run("chmod", &["--", "a-w", "MODE=victim"], b"");
    let untouched = std::fs::metadata(s.path.join("victim"))
        .unwrap()
        .permissions()
        .readonly();
    let changed = std::fs::metadata(s.path.join("MODE=victim"))
        .unwrap()
        .permissions()
        .readonly();
    std::fs::set_permissions(s.path.join("victim"), before).unwrap();
    std::fs::set_permissions(s.path.join("MODE=victim"), target_before).unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(!untouched);
    assert!(changed);
}
#[test]
fn tree_default_ascii_and_json_describe_the_same_tree() {
    let s = Sandbox::new();
    s.write("alpha", b"x");
    s.write("dir/beta", b"y");
    s.write(".hidden", b"z");
    let out = s.run("tree", &[], b"");
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        ".\n├── alpha\n└── dir\n    └── beta\n\n1 directory, 2 files\n".as_bytes()
    );
    let out = s.run("tree", &["--charset=ascii", "--noreport", "-L1"], b"");
    assert_eq!(out.stdout, b".\n|-- alpha\n`-- dir\n");
    let out = s.run("tree", &["-J", "-a", "--noreport"], b"");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json[0]["contents"].as_array().unwrap().len(), 3);
    let out = s.run("tree", &["-d", "-i"], b"");
    assert_eq!(out.stdout, b".\ndir\n\n1 directory\n");
}
#[test]
fn tree_patterns_pruning_sorting_gitignore_and_file_limits() {
    let s = Sandbox::new();
    s.write("f10.txt", b"xx");
    s.write("f2.rs", b"x");
    s.write("dir/match.RS", b"y");
    s.write("empty/.hidden", b"z");
    s.write(".gitignore", b"f10.txt\n");
    let out = s.run(
        "tree",
        &["-P*.rs", "--ignore-case", "--prune", "--noreport", "-i"],
        b"",
    );
    assert_eq!(out.stdout, b".\ndir\nmatch.RS\nf2.rs\n");
    for args in [
        vec!["-v", "-r"],
        vec!["--dirsfirst", "--sort=size"],
        vec!["--filesfirst", "-t"],
        vec!["--sort=ctime"],
        vec!["-U"],
        vec!["-I*.txt|empty", "--matchdirs", "-Pdir"],
        vec!["--gitignore"],
        vec!["--filelimit=1"],
        vec!["-p", "-s", "-D", "--timefmt=%Y"],
        vec!["-h", "--si", "-f", "-F", "-n"],
    ] {
        let out = s.run("tree", &args, b"");
        assert!(out.status.success(), "{args:?}: {out:?}");
    }
    assert!(!s.run("tree", &["missing"], b"").status.success());
    assert_eq!(s.run("tree", &["-L0"], b"").status.code(), Some(1));
}
#[test]
fn column_table_splits_empty_fields_aligns_and_hides_columns() {
    for (args, input, expected) in [
        (vec!["-t"], "a bb\nlong c\n", "a     bb\nlong  c\n"),
        (vec!["-t", "-s,", "-o|"], "a,,c\nx,y,z\n", "a| |c\nx|y|z\n"),
        (vec!["-t", "-R2"], "a 2\nb 100\n", "a    2\nb  100\n"),
        (vec!["-t", "-H2"], "a b c\nx y z\n", "a  c\nx  z\n"),
        (vec!["-t", "-l2"], "a b  c\nx y\n", "a  b  c\nx  y\n"),
        (vec!["-t", "-L"], "a b\n\nx y\n", "a  b\n\nx  y\n"),
        (vec!["-t"], "界 x\na y\n", "界  x\na   y\n"),
    ] {
        let out = common::run("column", &args, input.as_bytes());
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}
#[test]
fn column_normal_fill_order_json_and_error_cases() {
    assert_eq!(
        common::run("column", &["-c16"], b"a\nb\nc\nd\n").stdout,
        b"a\tc\nb\td\n"
    );
    assert_eq!(
        common::run("column", &["-x", "-c16"], b"a\nb\nc\nd\n").stdout,
        b"a\tb\nc\td\n"
    );
    let out = common::run(
        "column",
        &["-J", "-NNAME,COUNT", "--table-name=items"],
        b"alpha 2\n",
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["items"][0]["name"], "alpha");
    assert_eq!(json["items"][0]["count"], "2");
    for args in [vec!["-c0"], vec!["-J"], vec!["-Runknown"], vec!["missing"]] {
        assert_eq!(
            common::run("column", &args, b"").status.code(),
            Some(1),
            "{args:?}"
        );
    }
    let s = Sandbox::new();
    s.write("a", b"one x\n");
    s.write("b", b"two y\n");
    assert_eq!(
        s.run("column", &["-t", "a", "b"], b"").stdout,
        b"one  x\ntwo  y\n"
    );
}
#[test]
fn diff_normal_context_and_side_formats_match_ascii_goldens() {
    let s = Sandbox::new();
    s.write("a", b"same\nold\nend\n");
    s.write("b", b"same\nnew\nend\n");
    assert_eq!(
        s.run("diff", &["a", "b"], b"").stdout,
        b"2c2\n< old\n---\n> new\n"
    );
    let out = s.run("diff", &["-c", "--label=A", "--label=B", "a", "b"], b"");
    assert_eq!(out.stdout,b"*** A\n--- B\n***************\n*** 1,3 ****\n  same\n! old\n  end\n--- 1,3 ----\n  same\n! new\n  end\n");
    assert_eq!(
        s.run("diff", &["-y", "-W20", "a", "b"], b"").stdout,
        b"same\tsame\nold   |\tnew\nend\tend\n"
    );
    assert_eq!(
        s.run("diff", &["-y", "--width=20", "a", "a"], b"").stdout,
        b"same\tsame\nold\told\nend\tend\n"
    );
    s.write("a", b"a");
    s.write("b", b"a\n");
    assert_eq!(
        s.run("diff", &["-y", "-W20", "a", "b"], b"").stdout,
        b"a     \\\ta\n"
    );
}
