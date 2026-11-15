mod common;
use common::Sandbox;
use std::{
    fs,
    process::{Command, Output},
};

fn run(s: &Sandbox, name: &str, args: &[&str], input: &[u8]) -> Output {
    let executable = match name {
        "cp" => env!("CARGO_BIN_EXE_cp"),
        "mv" => env!("CARGO_BIN_EXE_mv"),
        "ln" => env!("CARGO_BIN_EXE_ln"),
        "rm" => env!("CARGO_BIN_EXE_rm"),
        "touch" => env!("CARGO_BIN_EXE_touch"),
        "date" => env!("CARGO_BIN_EXE_date"),
        "truncate" => env!("CARGO_BIN_EXE_truncate"),
        _ => panic!("unknown command"),
    };
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(&s.path)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env_remove("VERSION_CONTROL")
        .env_remove("SIMPLE_BACKUP_SUFFIX");
    common::execute(command, input)
}
fn ok(out: &Output) {
    assert!(out.status.success(), "{out:?}");
}
fn fail(out: &Output) {
    assert!(!out.status.success(), "{out:?}");
}

#[test]
fn copy_attributes_links_and_directory_options() {
    let s = Sandbox::new();
    s.write("source/file", b"contents");
    fs::hard_link(s.path.join("source/file"), s.path.join("source/second")).unwrap();
    ok(&run(&s, "cp", &["-av", "source", "archive"], b""));
    assert_eq!(fs::read(s.path.join("archive/file")).unwrap(), b"contents");
    assert!(agentcmd::common::meta::same_file(
        &s.path.join("archive/file"),
        &s.path.join("archive/second")
    )
    .unwrap());
    ok(&run(&s, "cp", &["-p", "source/file", "preserved"], b""));
    ok(&run(
        &s,
        "cp",
        &[
            "--preserve=mode,timestamps,links",
            "--no-preserve=mode,links",
            "source/file",
            "selected",
        ],
        b"",
    ));
    ok(&run(&s, "cp", &["-l", "source/file", "hard"], b""));
    assert!(
        agentcmd::common::meta::same_file(&s.path.join("source/file"), &s.path.join("hard"))
            .unwrap()
    );
    fs::create_dir(s.path.join("out")).unwrap();
    ok(&run(&s, "cp", &["--parents", "source/file", "out"], b""));
    assert_eq!(
        fs::read(s.path.join("out/source/file")).unwrap(),
        b"contents"
    );
    ok(&run(
        &s,
        "cp",
        &["--remove-destination", "source/file", "selected"],
        b"",
    ));
    ok(&run(
        &s,
        "cp",
        &[
            "--reflink=never",
            "--sparse=never",
            "source/file",
            "ordinary",
        ],
        b"",
    ));
}

#[cfg(unix)]
#[test]
fn copy_symlinks_follow_modes_and_relative_links() {
    use std::os::unix::fs::symlink;
    let s = Sandbox::new();
    s.write("source", b"contents");
    symlink("source", s.path.join("link")).unwrap();
    for (flag, dest) in [
        ("-P", "physical"),
        ("-d", "preserve_link"),
        ("-L", "followed"),
        ("-H", "argument_followed"),
    ] {
        ok(&run(&s, "cp", &[flag, "link", dest], b""));
        let link = fs::symlink_metadata(s.path.join(dest))
            .unwrap()
            .file_type()
            .is_symlink();
        assert_eq!(link, flag == "-P" || flag == "-d");
        assert_eq!(fs::read(s.path.join(dest)).unwrap(), b"contents");
    }
    ok(&run(&s, "cp", &["-s", "source", "symbolic"], b""));
    assert_eq!(
        fs::read_link(s.path.join("symbolic")).unwrap().to_str(),
        Some("source")
    );
    ok(&run(&s, "ln", &["-sr", "source", "relative"], b""));
    assert_eq!(fs::read(s.path.join("relative")).unwrap(), b"contents");
}

#[test]
fn copy_invalid_options_and_destinations() {
    let s = Sandbox::new();
    s.write("source", b"x");
    fs::create_dir(s.path.join("dir")).unwrap();
    for args in [
        vec!["--preserve=bad", "source", "dest"],
        vec!["--no-preserve=bad", "source", "dest"],
        vec!["--reflink=always", "source", "dest"],
        vec!["--reflink=bad", "source", "dest"],
        vec!["--sparse=bad", "source", "dest"],
        vec!["--update=bad", "source", "dest"],
        vec!["-ls", "source", "dest"],
        vec!["-t", "missing", "source"],
        vec!["-T", "source", "dir"],
        vec!["missing", "dest"],
        vec!["source"],
    ] {
        fail(&run(&s, "cp", &args, b""));
    }
    assert_eq!(fs::read(s.path.join("source")).unwrap(), b"x");
}

#[test]
fn move_backup_updates_and_errors() {
    let s = Sandbox::new();
    s.write("a", b"a");
    s.write("b", b"b");
    ok(&run(&s, "mv", &["--update=none", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("a")).unwrap(), b"a");
    fail(&run(&s, "mv", &["--update=none-fail", "a", "b"], b""));
    ok(&run(&s, "mv", &["-v", "-S", ".bak", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b.bak")).unwrap(), b"b");
    assert_eq!(fs::read(s.path.join("b")).unwrap(), b"a");
    for args in [
        vec!["--backup=bad", "b", "c"],
        vec!["--update=bad", "b", "c"],
        vec!["--no-copy", "missing", "c"],
        vec!["-t", "missing", "b"],
    ] {
        fail(&run(&s, "mv", &args, b""));
    }
}

#[test]
fn remove_prompt_modes_and_directory_errors() {
    let s = Sandbox::new();
    for name in ["a", "b", "c", "d"] {
        s.write(name, b"x");
    }
    ok(&run(&s, "rm", &["-I", "a", "b", "c", "d"], b"n\n"));
    assert!(s.path.join("a").exists());
    ok(&run(
        &s,
        "rm",
        &["--interactive=never", "-v", "a", "b"],
        b"",
    ));
    assert!(!s.path.join("a").exists());
    ok(&run(&s, "rm", &["--interactive=once", "c", "d"], b""));
    fs::create_dir(s.path.join("dir")).unwrap();
    s.write("dir/f", b"x");
    fail(&run(&s, "rm", &["-d", "dir"], b""));
    ok(&run(
        &s,
        "rm",
        &["-rv", "--one-file-system", "--preserve-root=all", "dir"],
        b"",
    ));
    fail(&run(&s, "rm", &["--interactive=invalid", "missing"], b""));
    fail(&run(&s, "rm", &["--preserve-root=invalid", "missing"], b""));
    ok(&run(&s, "rm", &["-f"], b""));
}

#[test]
fn date_formats_file_reference_and_errors() {
    let s = Sandbox::new();
    s.write("dates", b"2020-01-02\n2021-02-03\n");
    s.write("ref", b"");
    ok(&run(
        &s,
        "touch",
        &["-d", "2020-01-02 03:04:05 UTC", "ref"],
        b"",
    ));
    let out = run(&s, "date", &["-u", "-f", "dates", "+%F"], b"");
    ok(&out);
    assert_eq!(out.stdout, b"2020-01-02\n2021-02-03\n");
    let out = run(&s, "date", &["-u", "-r", "ref", "+%F"], b"");
    ok(&out);
    assert_eq!(out.stdout, b"2020-01-02\n");
    for option in [
        "--iso-8601=hours",
        "--iso-8601=minutes",
        "--iso-8601=seconds",
        "--rfc-3339=date",
        "--rfc-3339=seconds",
        "--rfc-3339=ns",
        "-R",
    ] {
        ok(&run(
            &s,
            "date",
            &["-u", "-d", "2020-01-02 03:04:05 UTC", option],
            b"",
        ));
    }
    for args in [
        vec!["--iso-8601=bad"],
        vec!["--rfc-3339=bad"],
        vec!["-r", "missing"],
        vec!["-f", "missing"],
        vec!["-d", "bad date"],
        vec!["+%F", "extra"],
    ] {
        fail(&run(&s, "date", &args, b""));
    }
}

#[test]
fn touch_time_selectors_and_invalid_inputs() {
    let s = Sandbox::new();
    s.write("ref", b"r");
    s.write("file", b"x");
    for args in [
        vec!["-a", "-d", "2020-01-02", "file"],
        vec!["-m", "-r", "ref", "file"],
        vec!["--time=atime", "file"],
        vec!["--time=mtime", "file"],
        vec!["-t", "202001020304.05", "file"],
    ] {
        ok(&run(&s, "touch", &args, b""));
    }
    for args in [
        vec!["--time=bad", "file"],
        vec!["-t", "not-a-date", "file"],
        vec!["-r", "missing", "file"],
        vec!["-d", "bad date", "file"],
    ] {
        fail(&run(&s, "touch", &args, b""));
    }
}

#[test]
fn truncate_prefixes_units_and_failures() {
    let s = Sandbox::new();
    s.write("file", b"123456789");
    s.write("ref", b"12345");
    for (size, expected) in [
        ("<8", 8),
        ("+3", 11),
        (">20", 20),
        ("/6", 18),
        ("%8", 24),
        ("-50", 0),
        ("3w", 6),
        ("1b", 512),
        ("1KB", 1000),
    ] {
        ok(&run(&s, "truncate", &["-s", size, "file"], b""));
        assert_eq!(
            fs::metadata(s.path.join("file")).unwrap().len(),
            expected,
            "{size}"
        );
    }
    for unit in ["M", "G", "T", "P", "E", "MB", "GB", "TB", "PB", "EB"] {
        ok(&run(
            &s,
            "truncate",
            &["-s", &format!("0{unit}"), "file"],
            b"",
        ));
    }
    ok(&run(&s, "truncate", &["-r", "ref", "file"], b""));
    assert_eq!(fs::metadata(s.path.join("file")).unwrap().len(), 5);
    ok(&run(&s, "truncate", &["-o", "-s", "0", "file"], b""));
    for args in [
        vec!["-s", "/0", "file"],
        vec!["-s", "%0", "file"],
        vec!["-s", "2bad", "file"],
        vec!["-r", "missing", "file"],
        vec!["-s", "2", "missing/child"],
        vec!["file"],
        vec!["-s", "1"],
    ] {
        fail(&run(&s, "truncate", &args, b""));
    }
}
