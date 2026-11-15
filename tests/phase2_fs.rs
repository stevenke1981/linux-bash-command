mod common;
use common::Sandbox;
use std::{
    fs,
    process::{Command, Output},
};

fn run(s: &Sandbox, command: &str, args: &[&str], input: &[u8]) -> Output {
    let bin = match command {
        "mkdir" => env!("CARGO_BIN_EXE_mkdir"),
        "rmdir" => env!("CARGO_BIN_EXE_rmdir"),
        "rm" => env!("CARGO_BIN_EXE_rm"),
        "cp" => env!("CARGO_BIN_EXE_cp"),
        "mv" => env!("CARGO_BIN_EXE_mv"),
        "ln" => env!("CARGO_BIN_EXE_ln"),
        "link" => env!("CARGO_BIN_EXE_link"),
        "unlink" => env!("CARGO_BIN_EXE_unlink"),
        "touch" => env!("CARGO_BIN_EXE_touch"),
        "truncate" => env!("CARGO_BIN_EXE_truncate"),
        "mktemp" => env!("CARGO_BIN_EXE_mktemp"),
        "date" => env!("CARGO_BIN_EXE_date"),
        _ => panic!("unknown command {command}"),
    };
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(&s.path)
        .env("TMPDIR", &s.path)
        .env("TEMP", &s.path)
        .env("TMP", &s.path)
        .env_remove("VERSION_CONTROL")
        .env_remove("SIMPLE_BACKUP_SUFFIX");
    common::execute(cmd, input)
}
fn ok(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}
fn fail(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn help_version_and_unknown_options() {
    let s = Sandbox::new();
    for name in [
        "mkdir", "rmdir", "rm", "cp", "mv", "ln", "link", "unlink", "touch", "truncate", "mktemp",
        "date",
    ] {
        for flag in ["--help", "--version"] {
            let out = run(&s, name, &[flag], b"");
            ok(&out);
            assert!(!out.stdout.is_empty(), "{name} {flag}");
        }
        let out = run(&s, name, &["--unknown-agentcmd-option"], b"");
        fail(&out);
    }
}
#[test]
fn mkdir_nested_and_existing() {
    let s = Sandbox::new();
    ok(&run(&s, "mkdir", &["-p", "one/two"], b""));
    assert!(s.path.join("one/two").is_dir());
    ok(&run(&s, "mkdir", &["-p", "one/two"], b""));
    fail(&run(&s, "mkdir", &["one"], b""));
}
#[test]
fn mkdir_verbose_and_multiple() {
    let s = Sandbox::new();
    let o = run(&s, "mkdir", &["-v", "a", "b"], b"");
    ok(&o);
    assert_eq!(
        o.stdout,
        b"mkdir: created directory 'a'\nmkdir: created directory 'b'\n"
    );
}
#[test]
fn mkdir_mode_and_failures() {
    let s = Sandbox::new();
    ok(&run(&s, "mkdir", &["-m", "u=rwx,g=rx,o=", "a"], b""));
    fail(&run(&s, "mkdir", &["-m", "oops", "b"], b""));
    fail(&run(&s, "mkdir", &[], b""));
}
#[test]
fn rmdir_parents_and_verbose() {
    let s = Sandbox::new();
    fs::create_dir_all(s.path.join("a/b")).unwrap();
    let o = run(&s, "rmdir", &["-pv", "a/b"], b"");
    ok(&o);
    assert!(!s.path.join("a").exists());
    assert!(o.stdout.starts_with(b"rmdir: removing directory, 'a/b'"));
}
#[test]
fn rmdir_nonempty_and_missing() {
    let s = Sandbox::new();
    s.write("a/f", b"x");
    fail(&run(&s, "rmdir", &["a"], b""));
    ok(&run(&s, "rmdir", &["--ignore-fail-on-non-empty", "a"], b""));
    fail(&run(&s, "rmdir", &["missing"], b""));
}
#[test]
fn rm_files_recursive_and_force() {
    let s = Sandbox::new();
    s.write("a/f", b"x");
    s.write("file", b"x");
    fail(&run(&s, "rm", &["a"], b""));
    ok(&run(&s, "rm", &["-r", "a", "file"], b""));
    assert!(!s.path.join("a").exists());
    ok(&run(&s, "rm", &["-f", "missing"], b""));
}
#[test]
fn rm_interactive_decline_and_accept() {
    let s = Sandbox::new();
    s.write("a", b"x");
    ok(&run(&s, "rm", &["-i", "a"], b"n\n"));
    assert!(s.path.join("a").exists());
    ok(&run(&s, "rm", &["-i", "a"], b"y\n"));
    assert!(!s.path.join("a").exists());
}
#[test]
fn rm_empty_directory() {
    let s = Sandbox::new();
    fs::create_dir(s.path.join("a")).unwrap();
    ok(&run(&s, "rm", &["-d", "a"], b""));
    assert!(!s.path.join("a").exists());
}
#[test]
fn link_and_unlink_files() {
    let s = Sandbox::new();
    s.write("a", b"abc");
    ok(&run(&s, "link", &["a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b")).unwrap(), b"abc");
    ok(&run(&s, "unlink", &["b"], b""));
    assert!(s.path.join("a").exists());
    fail(&run(&s, "unlink", &["b"], b""));
}
#[test]
fn link_and_unlink_arity_and_directories() {
    let s = Sandbox::new();
    fs::create_dir(s.path.join("d")).unwrap();
    fail(&run(&s, "link", &[], b""));
    fail(&run(&s, "link", &["a"], b""));
    fail(&run(&s, "link", &["a", "b", "c"], b""));
    fail(&run(&s, "unlink", &["d"], b""));
    fail(&run(&s, "unlink", &["a", "b"], b""));
}
#[test]
fn ln_hard_and_force() {
    let s = Sandbox::new();
    s.write("a", b"a");
    s.write("b", b"b");
    fail(&run(&s, "ln", &["a", "b"], b""));
    ok(&run(&s, "ln", &["-f", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b")).unwrap(), b"a");
}
#[test]
fn cp_file_multiple_and_parents() {
    let s = Sandbox::new();
    s.write("a/f", b"a");
    s.write("b", b"b");
    fs::create_dir(s.path.join("out")).unwrap();
    ok(&run(&s, "cp", &["a/f", "b", "out"], b""));
    assert_eq!(fs::read(s.path.join("out/f")).unwrap(), b"a");
    ok(&run(&s, "cp", &["--parents", "a/f", "out"], b""));
    assert_eq!(fs::read(s.path.join("out/a/f")).unwrap(), b"a");
}
#[test]
fn cp_recursive_and_same_file() {
    let s = Sandbox::new();
    s.write("a/f", b"data");
    fail(&run(&s, "cp", &["a", "b"], b""));
    ok(&run(&s, "cp", &["-R", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b/f")).unwrap(), b"data");
    fail(&run(&s, "cp", &["a/f", "a/f"], b""));
}
#[test]
fn cp_no_clobber_and_backup() {
    let s = Sandbox::new();
    s.write("a", b"a");
    s.write("b", b"b");
    ok(&run(&s, "cp", &["-n", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b")).unwrap(), b"b");
    ok(&run(&s, "cp", &["-b", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b~")).unwrap(), b"b");
}
#[test]
fn mv_rename_and_no_clobber() {
    let s = Sandbox::new();
    s.write("a", b"a");
    s.write("b", b"b");
    ok(&run(&s, "mv", &["-n", "a", "b"], b""));
    assert!(s.path.join("a").exists());
    ok(&run(&s, "mv", &["-f", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b")).unwrap(), b"a");
}
#[test]
fn mv_multiple_into_directory() {
    let s = Sandbox::new();
    s.write("a", b"a");
    s.write("b", b"b");
    fs::create_dir(s.path.join("out")).unwrap();
    ok(&run(&s, "mv", &["a", "b", "out"], b""));
    assert!(s.path.join("out/a").exists());
    assert!(s.path.join("out/b").exists());
}
#[test]
fn touch_create_and_no_create() {
    let s = Sandbox::new();
    ok(&run(&s, "touch", &["a", "b"], b""));
    assert!(s.path.join("a").exists());
    ok(&run(&s, "touch", &["-c", "missing"], b""));
    assert!(!s.path.join("missing").exists());
}
#[test]
fn touch_times_from_reference() {
    let s = Sandbox::new();
    ok(&run(&s, "touch", &["-t", "202001020304.05", "ref"], b""));
    ok(&run(&s, "touch", &["-r", "ref", "copy"], b""));
    assert_eq!(
        fs::metadata(s.path.join("ref"))
            .unwrap()
            .modified()
            .unwrap(),
        fs::metadata(s.path.join("copy"))
            .unwrap()
            .modified()
            .unwrap()
    );
}
#[test]
fn truncate_extend_reduce_and_relative() {
    let s = Sandbox::new();
    s.write("a", b"1234");
    ok(&run(&s, "truncate", &["-s", "8", "a"], b""));
    assert_eq!(fs::metadata(s.path.join("a")).unwrap().len(), 8);
    ok(&run(&s, "truncate", &["-s", "-3", "a"], b""));
    assert_eq!(fs::metadata(s.path.join("a")).unwrap().len(), 5);
}
#[test]
fn truncate_reference_and_no_create() {
    let s = Sandbox::new();
    s.write("a", b"123");
    ok(&run(&s, "truncate", &["-r", "a", "b"], b""));
    assert_eq!(fs::metadata(s.path.join("b")).unwrap().len(), 3);
    ok(&run(&s, "truncate", &["-c", "-s", "1", "missing"], b""));
    assert!(!s.path.join("missing").exists());
}
#[test]
fn mktemp_in_sandbox_and_dry_run() {
    let s = Sandbox::new();
    let o = run(&s, "mktemp", &["-p", ".", "file.XXXXXX"], b"");
    ok(&o);
    let name = String::from_utf8(o.stdout).unwrap();
    assert!(s.path.join(name.trim()).is_file());
    let o = run(&s, "mktemp", &["-u", "dry.XXXXXX"], b"");
    ok(&o);
    let name = String::from_utf8(o.stdout).unwrap();
    assert!(!s.path.join(name.trim()).exists());
}
#[test]
fn mktemp_directory_and_invalid() {
    let s = Sandbox::new();
    let o = run(&s, "mktemp", &["-d", "dir.XXXXXX"], b"");
    ok(&o);
    let name = String::from_utf8(o.stdout).unwrap();
    assert!(s.path.join(name.trim()).is_dir());
    fail(&run(&s, "mktemp", &["badXX"], b""));
}
#[test]
fn date_iso_and_epoch() {
    let s = Sandbox::new();
    let o = run(&s, "date", &["-u", "-d", "@0", "+%F %T %z"], b"");
    ok(&o);
    assert_eq!(o.stdout, b"1970-01-01 00:00:00 +0000\n");
    let o = run(
        &s,
        "date",
        &["-u", "-d", "2026-01-02 03:04:05", "-Iseconds"],
        b"",
    );
    ok(&o);
    assert_eq!(o.stdout, b"2026-01-02T03:04:05+00:00\n");
}
#[test]
fn date_file_continues_after_invalid_input() {
    let s = Sandbox::new();
    s.write("dates", b"@0\ninvalid\n@1\n");
    let o = run(&s, "date", &["-u", "-f", "dates", "+%s"], b"");
    fail(&o);
    assert_eq!(o.stdout, b"0\n1\n");
}

#[test]
fn mktemp_template_variants_are_created_in_the_sandbox() {
    let s = Sandbox::new();
    for args in [
        vec!["a.XXX"],
        vec!["b.XXXX"],
        vec!["c.XXXXXX"],
        vec!["d.XXXXXXXXXX"],
        vec!["suffix.XXXX.tmp"],
        vec!["--suffix=.txt", "explicit.XXXX"],
        vec!["-p", ".", "parent.XXXX"],
        vec!["--tmpdir=.", "long.XXXX"],
        vec!["-t", "temp.XXXX"],
        vec!["-d", "dir.XXXX"],
        vec!["--directory", "dirlong.XXXX"],
        vec!["--dry-run", "dry.XXXX"],
        vec!["--quiet", "quiet.XXXX"],
        vec!["-u", "uncreated.XXXX"],
        vec![],
    ] {
        let out = run(&s, "mktemp", &args, b"");
        ok(&out);
        let text = String::from_utf8(out.stdout).unwrap();
        let path = s.path.join(text.trim());
        assert!(path.starts_with(&s.path), "{args:?}: {text:?}");
        if args.contains(&"-u") || args.contains(&"--dry-run") {
            assert!(!path.exists());
        } else if args.contains(&"-d") || args.contains(&"--directory") {
            assert!(path.is_dir());
        } else {
            assert!(path.is_file());
        }
    }
}

#[test]
fn mktemp_rejects_bad_templates_without_creating_files() {
    let s = Sandbox::new();
    for args in [
        vec!["a"],
        vec!["xXX"],
        vec!["--suffix=/bad", "xXXX"],
        vec!["xXXX", "yXXX"],
        vec!["-p", "missing", "xXXX"],
    ] {
        fail(&run(&s, "mktemp", &args, b""));
    }
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 0);
}

#[test]
fn ln_force_never_deletes_its_source() {
    let s = Sandbox::new();
    s.write("a", b"contents");
    fail(&run(&s, "ln", &["-f", "a", "a"], b""));
    assert_eq!(fs::read(s.path.join("a")).unwrap(), b"contents");
}

#[test]
fn rm_recursive_interactive_can_decline_descent() {
    let s = Sandbox::new();
    s.write("d/a", b"contents");
    ok(&run(&s, "rm", &["-ri", "d"], b"n\n"));
    assert_eq!(fs::read(s.path.join("d/a")).unwrap(), b"contents");
}

#[test]
fn touch_relative_date_uses_reference_times() {
    use std::time::Duration;
    let s = Sandbox::new();
    ok(&run(&s, "touch", &["-t", "202001020304.05", "ref"], b""));
    ok(&run(
        &s,
        "touch",
        &["-r", "ref", "-d", "+1 day", "copy"],
        b"",
    ));
    let reference = fs::metadata(s.path.join("ref"))
        .unwrap()
        .modified()
        .unwrap();
    let copy = fs::metadata(s.path.join("copy"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        copy.duration_since(reference).unwrap(),
        Duration::from_secs(86400)
    );
}

#[test]
fn touch_dash_updates_stdout_file() {
    use std::process::Stdio;
    let s = Sandbox::new();
    let output = fs::File::create(s.path.join("out")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_touch"))
        .current_dir(&s.path)
        .env("TZ", "UTC")
        .args(["-d", "@0", "-"])
        .stdout(Stdio::from(output))
        .output()
        .unwrap();
    ok(&result);
    let modified = fs::metadata(s.path.join("out"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        0
    );
    assert!(!s.path.join("-").exists());
}

#[cfg(windows)]
#[test]
fn windows_mkdir_mode_is_validated_but_ignored() {
    let s = Sandbox::new();
    ok(&run(&s, "mkdir", &["-m", "000", "d"], b""));
    assert!(!fs::metadata(s.path.join("d"))
        .unwrap()
        .permissions()
        .readonly());
}

#[cfg(windows)]
#[test]
fn windows_rm_clears_readonly_attribute() {
    let s = Sandbox::new();
    s.write("readonly", b"content");
    let path = s.path.join("readonly");
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    ok(&run(&s, "rm", &["readonly"], b""));
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn recursive_rm_does_not_follow_symlinks() {
    use std::os::unix::fs::symlink;
    let s = Sandbox::new();
    s.write("outside/keep", b"content");
    fs::create_dir(s.path.join("inside")).unwrap();
    symlink("../outside", s.path.join("inside/link")).unwrap();
    ok(&run(&s, "rm", &["-r", "inside"], b""));
    assert_eq!(fs::read(s.path.join("outside/keep")).unwrap(), b"content");
}

#[cfg(unix)]
#[test]
fn mktemp_creates_private_files_and_directories() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    for (args, mode) in [(vec!["f.XXXXXX"], 0o600), (vec!["-d", "d.XXXXXX"], 0o700)] {
        let out = run(&s, "mktemp", &args, b"");
        ok(&out);
        let name = String::from_utf8(out.stdout).unwrap();
        assert_eq!(
            fs::metadata(s.path.join(name.trim()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            mode
        );
    }
}

#[test]
fn mv_backup_and_declined_overwrite() {
    let s = Sandbox::new();
    s.write("a", b"a");
    s.write("b", b"b");
    ok(&run(&s, "mv", &["-i", "a", "b"], b"n\n"));
    assert!(s.path.join("a").exists());
    ok(&run(&s, "mv", &["-b", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b~")).unwrap(), b"b");
    fail(&run(&s, "mv", &["missing", "dest"], b""));
}

#[test]
fn ln_backup_and_target_directory() {
    let s = Sandbox::new();
    s.write("a", b"a");
    s.write("b", b"b");
    ok(&run(&s, "ln", &["-b", "a", "b"], b""));
    assert_eq!(fs::read(s.path.join("b~")).unwrap(), b"b");
    fs::create_dir(s.path.join("out")).unwrap();
    ok(&run(&s, "ln", &["-t", "out", "a"], b""));
    assert_eq!(fs::read(s.path.join("out/a")).unwrap(), b"a");
}

#[test]
fn unlink_dash_operand_and_missing() {
    let s = Sandbox::new();
    s.write("-name", b"x");
    ok(&run(&s, "unlink", &["--", "-name"], b""));
    fail(&run(&s, "unlink", &["--", "-name"], b""));
}

#[test]
fn date_reads_standard_input_and_rejects_invalid_date() {
    let s = Sandbox::new();
    let result = run(&s, "date", &["-u", "-f", "-", "+%s"], b"@0\n@1\n");
    ok(&result);
    assert_eq!(result.stdout, b"0\n1\n");
    fail(&run(&s, "date", &["-d", "not a date"], b""));
}

#[cfg(unix)]
#[test]
fn cp_preserves_existing_destination_mode_without_preserve_option() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    s.write("source", b"a");
    s.write("dest", b"b");
    fs::set_permissions(s.path.join("source"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(s.path.join("dest"), fs::Permissions::from_mode(0o640)).unwrap();
    ok(&run(&s, "cp", &["source", "dest"], b""));
    assert_eq!(
        fs::metadata(s.path.join("dest"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
}

#[test]
fn explicit_target_directory_must_exist() {
    let s = Sandbox::new();
    s.write("source", b"x");
    for command in ["cp", "mv", "ln"] {
        fail(&run(&s, command, &["-t", "missing", "source"], b""));
        assert!(s.path.join("source").exists(), "{command}");
        assert!(!s.path.join("missing").exists(), "{command}");
    }
}

#[test]
fn cp_and_mv_update_none_fail_create_when_destination_is_absent() {
    for command in ["cp", "mv"] {
        let s = Sandbox::new();
        s.write("source", b"source");
        let out = run(&s, command, &["--update=none-fail", "source", "dest"], b"");
        ok(&out);
        assert_eq!(fs::read(s.path.join("dest")).unwrap(), b"source");
        assert_eq!(s.path.join("source").exists(), command == "cp");
    }
}

#[test]
fn cp_and_mv_update_none_fail_preserve_existing_files_and_fail() {
    for command in ["cp", "mv"] {
        let s = Sandbox::new();
        s.write("source", b"source");
        s.write("dest", b"dest");
        let out = run(&s, command, &["--update=none-fail", "source", "dest"], b"");
        fail(&out);
        assert!(!out.stderr.is_empty());
        assert_eq!(fs::read(s.path.join("source")).unwrap(), b"source");
        assert_eq!(fs::read(s.path.join("dest")).unwrap(), b"dest");
    }
}

#[test]
fn cp_and_mv_last_update_option_controls_replacement() {
    for command in ["cp", "mv"] {
        let s = Sandbox::new();
        s.write("source", b"source");
        s.write("dest", b"dest");
        let out = run(
            &s,
            command,
            &["--update=all", "--update=none-fail", "source", "dest"],
            b"",
        );
        fail(&out);
        assert_eq!(fs::read(s.path.join("source")).unwrap(), b"source");
        assert_eq!(fs::read(s.path.join("dest")).unwrap(), b"dest");
        let out = run(
            &s,
            command,
            &["--update=none-fail", "--update=all", "source", "dest"],
            b"",
        );
        ok(&out);
        assert_eq!(fs::read(s.path.join("dest")).unwrap(), b"source");
    }
}

#[test]
fn link_missing_source_never_creates_destination() {
    let s = Sandbox::new();
    fail(&run(&s, "link", &["missing", "dest"], b""));
    assert!(!s.path.join("dest").exists());
}

#[test]
fn rmdir_parents_stops_at_nonempty_ancestor() {
    let s = Sandbox::new();
    s.write("parent/keep", b"x");
    fs::create_dir(s.path.join("parent/child")).unwrap();
    fail(&run(&s, "rmdir", &["--parents", "parent/child"], b""));
    assert!(!s.path.join("parent/child").exists());
    assert!(s.path.join("parent/keep").exists());
}

#[test]
fn truncate_invalid_size_does_not_create_file() {
    let s = Sandbox::new();
    fail(&run(&s, "truncate", &["--size=oops", "new"], b""));
    assert!(!s.path.join("new").exists());
}

#[test]
fn mktemp_legacy_template_rejects_a_directory_component() {
    let s = Sandbox::new();
    fs::create_dir(s.path.join("sub")).unwrap();
    fail(&run(&s, "mktemp", &["-t", "sub/file.XXXX"], b""));
    assert_eq!(fs::read_dir(s.path.join("sub")).unwrap().count(), 0);
}

#[test]
fn link_and_unlink_quote_file_names_in_diagnostics() {
    let s = Sandbox::new();
    let link = run(&s, "link", &["missing", "dest"], b"");
    fail(&link);
    assert_eq!(
        link.stderr,
        b"link: cannot create link 'dest' to 'missing': No such file or directory\n"
    );
    let unlink = run(&s, "unlink", &["missing"], b"");
    fail(&unlink);
    assert_eq!(
        unlink.stderr,
        b"unlink: cannot unlink 'missing': No such file or directory\n"
    );
}

#[test]
fn backup_environment_defaults_and_explicit_overrides() {
    for command in ["cp", "mv", "ln"] {
        let bin = match command {
            "cp" => env!("CARGO_BIN_EXE_cp"),
            "mv" => env!("CARGO_BIN_EXE_mv"),
            _ => env!("CARGO_BIN_EXE_ln"),
        };
        for (control, suffix, extra, backup, expected) in [
            ("numbered", ".bak", Vec::<&str>::new(), "dest.~1~", 0),
            ("simple", ".bak", Vec::<&str>::new(), "dest.bak", 0),
            (
                "numbered",
                ".bak",
                vec!["--backup=simple", "--suffix=.old"],
                "dest.old",
                0,
            ),
            ("invalid", ".bak", Vec::<&str>::new(), "", 1),
        ] {
            let s = Sandbox::new();
            s.write("source", b"new");
            s.write("dest", b"old");
            let mut cmd = Command::new(bin);
            cmd.current_dir(&s.path)
                .env("VERSION_CONTROL", control)
                .env("SIMPLE_BACKUP_SUFFIX", suffix)
                .arg("-b")
                .args(extra)
                .args(["source", "dest"]);
            let result = common::execute(cmd, b"");
            assert_eq!(
                result.status.code(),
                Some(expected),
                "{command} {control}: {result:?}"
            );
            if expected == 0 {
                assert_eq!(fs::read(s.path.join(backup)).unwrap(), b"old");
                assert_eq!(fs::read(s.path.join("dest")).unwrap(), b"new");
            } else {
                assert_eq!(fs::read(s.path.join("source")).unwrap(), b"new");
                assert_eq!(fs::read(s.path.join("dest")).unwrap(), b"old");
            }
        }
    }
}

#[test]
fn recognized_but_unsupported_gnu_options_report_specific_error() {
    let s = Sandbox::new();
    for (command, options) in [
        (
            "cp",
            vec![
                "--attributes-only",
                "--context",
                "--copy-contents",
                "--debug",
                "--one-file-system",
                "--strip-trailing-slashes",
                "--reflink=always",
            ],
        ),
        ("mv", vec!["--context", "--debug"]),
        ("ln", vec!["--directory"]),
        ("mkdir", vec!["--context"]),
        ("date", vec!["--debug"]),
    ] {
        for option in options {
            let output = run(&s, command, &[option], b"");
            fail(&output);
            let text = String::from_utf8(output.stderr).unwrap();
            assert!(
                text.contains(&format!("option '{option}' is not supported by agentcmd")),
                "{command} {option}: {text}"
            );
        }
    }
    let output = run(&s, "date", &["--set=2020-01-01"], b"");
    fail(&output);
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("cannot set date: Operation not permitted"));
}

#[test]
fn backup_target_must_not_overwrite_source() {
    for command in ["cp", "mv", "ln"] {
        let s = Sandbox::new();
        s.write("x", b"old");
        s.write("x~", b"source");
        let result = run(&s, command, &["-b", "x~", "x"], b"");
        fail(&result);
        assert_eq!(fs::read(s.path.join("x")).unwrap(), b"old", "{command}");
        assert_eq!(fs::read(s.path.join("x~")).unwrap(), b"source", "{command}");
    }
}

#[test]
fn backup_hardlink_alias_must_not_overwrite_source() {
    for command in ["cp", "mv", "ln"] {
        let s = Sandbox::new();
        s.write("x", b"old");
        s.write("source", b"source");
        fs::hard_link(s.path.join("source"), s.path.join("x~")).unwrap();
        let result = run(&s, command, &["-b", "source", "x"], b"");
        fail(&result);
        assert_eq!(fs::read(s.path.join("x")).unwrap(), b"old", "{command}");
        assert_eq!(
            fs::read(s.path.join("source")).unwrap(),
            b"source",
            "{command}"
        );
        assert_eq!(fs::read(s.path.join("x~")).unwrap(), b"source", "{command}");
    }
}

#[cfg(unix)]
#[test]
fn rm_trailing_separator_does_not_follow_directory_symlink() {
    use std::os::unix::fs::symlink;
    let s = Sandbox::new();
    s.write("target/keep", b"important");
    symlink("target", s.path.join("link")).unwrap();
    fail(&run(&s, "rm", &["-r", "link/"], b""));
    assert_eq!(fs::read(s.path.join("target/keep")).unwrap(), b"important");
    assert!(fs::symlink_metadata(s.path.join("link"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(windows)]
#[test]
fn rm_trailing_separator_does_not_follow_directory_symlink() {
    use std::os::windows::fs::symlink_dir;
    let s = Sandbox::new();
    s.write("target/keep", b"important");
    match symlink_dir(s.path.join("target"), s.path.join("link")) {
        Ok(()) => {}
        Err(e)
            if e.kind() == std::io::ErrorKind::PermissionDenied
                || e.raw_os_error() == Some(1314) =>
        {
            return
        }
        Err(e) => panic!("failed to create test symlink: {e}"),
    }
    fail(&run(&s, "rm", &["-r", "link/"], b""));
    assert_eq!(fs::read(s.path.join("target/keep")).unwrap(), b"important");
    assert!(fs::symlink_metadata(s.path.join("link"))
        .unwrap()
        .file_type()
        .is_symlink());
}
