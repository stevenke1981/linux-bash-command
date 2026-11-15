use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-sed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).unwrap()
    }
    fn run(&self, args: &[&str], input: &[u8]) -> Output {
        run_at(&self.0, args, input)
    }
    fn clean_temps(&self) {
        assert!(fs::read_dir(&self.0).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".agentcmd-sed-")));
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sed"));
    command
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env_remove("POSIXLY_CORRECT");
    command
}
fn run_at(path: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command()
        .args(args)
        .current_dir(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    output
}
fn success(output: Output, bytes: &[u8]) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, bytes);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn failure(output: Output, code: i32, text: &str) {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(text),
        "{output:?}"
    );
}

#[test]
fn help_and_version() {
    let s = Sandbox::new();
    let help = s.run(&["--help"], b"");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("CR bytes are ordinary data"));
    let version = format!("sed (agentcmd) {}\n", env!("CARGO_PKG_VERSION"));
    success(s.run(&["--version"], b""), version.as_bytes());
}
#[test]
fn usage_and_unsupported_options() {
    let s = Sandbox::new();
    failure(s.run(&["--unknown"], b""), 1, "unrecognized option");
    failure(
        s.run(&["--debug", "p"], b"a\n"),
        1,
        "not supported by agentcmd",
    );
    failure(s.run(&[], b""), 1, "no script specified");
    failure(s.run(&["-e"], b""), 1, "requires an argument");
}
#[test]
fn syntax_error_location() {
    let s = Sandbox::new();
    failure(s.run(&["s/a"], b"a\n"), 1, "-e expression #1, char");
    failure(s.run(&["b nowhere"], b"a\n"), 1, "can't find label");
}
#[test]
fn unsupported_execution_and_pattern_references() {
    let s = Sandbox::new();
    for script in ["e echo bad", "s/a/b/e", r"s/\(a\)\1/b/"] {
        failure(
            s.run(&[script], b"aa\n"),
            1,
            if script.contains("\\1") {
                "unsupported backreference in regex"
            } else {
                "not supported by agentcmd"
            },
        );
    }
}
#[test]
fn stdin_binary_and_missing_newline() {
    let s = Sandbox::new();
    success(s.run(&["s/a/A/g"], b"a\xff\na"), b"A\xff\nA");
    success(s.run(&["-n", "p;p"], b"a"), b"a\na");
}
#[test]
fn cr_is_not_converted() {
    let s = Sandbox::new();
    success(s.run(&["s/a$/X/"], b"a\r\na\n"), b"a\r\nX\n");
}
#[test]
fn multiple_files_and_missing_input() {
    let s = Sandbox::new();
    s.file("one", b"a\n");
    s.file("two", b"b");
    success(s.run(&["=", "one", "two"], b""), b"1\na\n2\nb");
    let missing = s.run(&["p", "missing", "one"], b"");
    failure(missing.clone(), 2, "No such file or directory");
    assert_eq!(missing.stdout, b"a\na\n");
}
#[test]
fn separate_resets_number_and_hold() {
    let s = Sandbox::new();
    s.file("a", b"x\ny\n");
    s.file("b", b"z\n");
    success(
        s.run(&["--separate", "-n", "=;1g;p", "a", "b"], b""),
        b"1\n\n2\ny\n1\n\n",
    );
}
#[test]
fn multiple_scripts_and_script_file() {
    let s = Sandbox::new();
    s.file("script", b"s/a/A/\n");
    success(
        s.run(&["--file=script", "--expression=s/b/B/"], b"ab\n"),
        b"AB\n",
    );
}
#[test]
fn source_location_for_script_file() {
    let s = Sandbox::new();
    s.file("script", b"p\ns/x\n");
    failure(s.run(&["-f", "script"], b""), 1, "file script line 2");
    failure(s.run(&["-f", "missing"], b""), 4, "couldn't open file");
}
#[test]
fn addresses_and_blocks() {
    let s = Sandbox::new();
    success(
        s.run(&["-n", "0,/b/p;3,~2{=;p}"], b"a\nb\nc\nd\ne\n"),
        b"a\nb\n3\nc\n4\nd\n",
    );
}
#[test]
fn regex_reuse_and_extended_groups() {
    let s = Sandbox::new();
    success(s.run(&["-E", "s/(a|ab)/[\\1]/;s//X/"], b"ab\n"), b"[X]\n");
    failure(
        s.run(&["-n", "/a/s//\\1/p"], b"a\n"),
        1,
        "invalid reference",
    );
}
#[test]
fn substitution_occurrence_and_case() {
    let s = Sandbox::new();
    success(s.run(&["s/a/\\u&/2g"], b"a a a\n"), b"a A A\n");
    success(s.run(&["s/[a-z]*/X/g"], b"abc!"), b"X!X");
}
#[test]
fn hold_space_and_restart() {
    let s = Sandbox::new();
    success(s.run(&["1h;2{H;g}"], b"a\nb\n"), b"a\na\nb\n");
    success(s.run(&["N;P;D"], b"a\nb\nc\n"), b"a\nb\nc\n");
}
#[test]
fn branches_and_quit_codes() {
    let s = Sandbox::new();
    success(s.run(&[":x;s/aa/a/;tx"], b"aaaa\n"), b"a\n");
    let o = s.run(&["1Q7"], b"a\nb\n");
    assert_eq!(o.status.code(), Some(7));
    assert!(o.stdout.is_empty());
}
#[test]
fn append_insert_change_and_deferred_order() {
    let s = Sandbox::new();
    success(s.run(&["1a x\nN;d"], b"a\nb\n"), b"x\n");
    success(
        s.run(&["2,3c changed"], b"a\nb\nc\nd\n"),
        b"a\nchanged\nd\n",
    );
}
#[test]
fn null_records_and_list_delimiter() {
    let s = Sandbox::new();
    success(s.run(&["--null-data", "=;l"], b"a\0"), b"1\0a$\0a\0");
    success(s.run(&["-z", "a hi"], b"a\0"), b"a\0hi\n");
}
#[test]
fn list_wrap_and_transliteration() {
    let s = Sandbox::new();
    success(
        s.run(&["-n", "--line-length=4", "l"], b"abcde\n"),
        b"abc\\\nde$\n",
    );
    success(s.run(&["y/ab/BA/"], b"abba\n"), b"BAAB\n");
}
#[test]
fn read_and_write_destinations() {
    let s = Sandbox::new();
    s.file("extra", b"one\ntwo\n");
    success(s.run(&["R extra\nw out"], b"a\nb\n"), b"a\none\nb\ntwo\n");
    assert_eq!(s.read("out"), b"a\nb\n");
    success(s.run(&["-n", "w /dev/stdout"], b"a\n"), b"a\n");
}
#[test]
fn repeated_write_destination() {
    let s = Sandbox::new();
    success(s.run(&["w out\nw out"], b"a\n"), b"a\n");
    assert_eq!(s.read("out"), b"a\na\n");
}
#[test]
fn io_failures_have_status_four() {
    let s = Sandbox::new();
    fs::create_dir(s.0.join("dir")).unwrap();
    failure(s.run(&["w dir"], b"a\n"), 4, "");
    failure(s.run(&["R dir"], b"a\n"), 4, "");
}
#[test]
fn in_place_backup_and_no_stdout() {
    let s = Sandbox::new();
    s.file("file", b"a\na");
    success(s.run(&["-i.bak", "s/a/A/g", "file"], b""), b"");
    assert_eq!(s.read("file"), b"A\nA");
    assert_eq!(s.read("file.bak"), b"a\na");
    s.clean_temps();
}
#[test]
fn in_place_wildcard_and_independent_ranges() {
    let s = Sandbox::new();
    s.file("one", b"a\na\n");
    s.file("two", b"a\na\n");
    success(
        s.run(&["--in-place=old-*", "1s/a/A/", "one", "two"], b""),
        b"",
    );
    for f in ["one", "two"] {
        assert_eq!(s.read(f), b"A\na\n");
        assert_eq!(s.read(&format!("old-{f}")), b"a\na\n");
    }
}
#[test]
fn in_place_failure_preserves_original() {
    let s = Sandbox::new();
    s.file("file", b"original\n");
    fs::create_dir(s.0.join("file.bak")).unwrap();
    failure(
        s.run(&["-i.bak", "s/original/changed/", "file"], b""),
        4,
        "",
    );
    assert_eq!(s.read("file"), b"original\n");
    s.clean_temps();
    failure(s.run(&["-i", "s/bad", "file"], b""), 1, "unterminated");
    assert_eq!(s.read("file"), b"original\n");
}
#[test]
fn in_place_quit_does_not_edit_later_files() {
    let s = Sandbox::new();
    s.file("a", b"one\ntwo\n");
    s.file("b", b"three\n");
    success(s.run(&["-i", "q", "a", "b"], b""), b"");
    assert_eq!(s.read("a"), b"one\n");
    assert_eq!(s.read("b"), b"three\n");
}
#[test]
fn explicit_quit_two_commits_in_place_edits() {
    let s = Sandbox::new();
    for (script, expected) in [
        ("s/a/X/;q2", b"X\n".as_slice()),
        ("s/a/X/;Q2", b"".as_slice()),
    ] {
        s.file("first", b"a\nb\n");
        s.file("later", b"a\n");
        let out = s.run(&["-i.bak", script, "first", "later"], b"");
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stderr.is_empty());
        assert!(out.stdout.is_empty());
        assert_eq!(s.read("first"), expected);
        assert_eq!(s.read("first.bak"), b"a\nb\n");
        assert_eq!(s.read("later"), b"a\n");
        s.clean_temps();
    }
}
#[test]
fn large_alternative_subject_does_not_expand_compiled_regex() {
    let s = Sandbox::new();
    let mut input = vec![b'a'];
    input.extend(std::iter::repeat_n(b'b', 100_000));
    input.push(b'\n');
    let mut expected = input.clone();
    expected[0] = b'X';
    success(s.run(&["-E", "s/a|b/X/"], &input), &expected);
}
#[test]
fn quiet_posix_sandbox_and_abbreviation() {
    let s = Sandbox::new();
    for flag in ["--quiet", "--silent", "--qui"] {
        success(s.run(&[flag, "p"], b"a\n"), b"a\n");
    }
    success(s.run(&["--posix", "--sandbox", "p"], b"a\n"), b"a\na\n");
}
#[test]
fn large_input_is_processed_as_records() {
    let s = Sandbox::new();
    let input = b"abcdefghij\n".repeat(100_000);
    let expected = b"Abcdefghij\n".repeat(100_000);
    success(s.run(&["s/a/A/"], &input), &expected);
}
#[test]
fn closed_stdout_is_quiet_success() {
    let s = Sandbox::new();
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let output = command()
        .arg("p")
        .current_dir(&s.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = output;
    child.stdin.take().unwrap().write_all(b"a\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
}
#[test]
fn unbuffered_output_does_not_wait_for_next_record() {
    let s = Sandbox::new();
    let mut child = command()
        .args(["--unbuffered", "-n", "p"])
        .current_dir(&s.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"first\n").unwrap();
    stdin.flush().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = [0; 6];
        let result = stdout.read_exact(&mut bytes).map(|()| bytes);
        let _ = sender.send(result);
    });
    let result = receiver.recv_timeout(std::time::Duration::from_secs(3));
    drop(stdin);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert_eq!(result.unwrap().unwrap(), *b"first\n");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn script_filenames_preserve_non_utf8_bytes() {
    use std::os::unix::ffi::OsStrExt;
    let s = Sandbox::new();
    let name = std::ffi::OsStr::from_bytes(b"raw-\xff");
    fs::write(s.0.join(name), b"extra\n").unwrap();
    s.file("script", b"r raw-\xff\nw output-\xfe\n");
    success(s.run(&["-f", "script"], b"input\n"), b"input\nextra\n");
    assert_eq!(
        fs::read(s.0.join(std::ffi::OsStr::from_bytes(b"output-\xfe"))).unwrap(),
        b"input\n"
    );
}

#[cfg(unix)]
#[test]
fn in_place_symlinks_and_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let s = Sandbox::new();
    s.file("target", b"a\n");
    fs::set_permissions(s.0.join("target"), fs::Permissions::from_mode(0o751)).unwrap();
    symlink("target", s.0.join("link")).unwrap();
    success(s.run(&["-i", "s/a/A/", "link"], b""), b"");
    assert!(!fs::symlink_metadata(s.0.join("link"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(s.read("target"), b"a\n");
    symlink("target", s.0.join("follow")).unwrap();
    success(
        s.run(&["-i", "--follow-symlinks", "s/a/A/", "follow"], b""),
        b"",
    );
    assert!(fs::symlink_metadata(s.0.join("follow"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(s.read("target"), b"A\n");
    assert_eq!(
        fs::metadata(s.0.join("target"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
}
