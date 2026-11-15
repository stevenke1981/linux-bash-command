mod common;
use common::Sandbox;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
fn executable(name: &str) -> &'static str {
    match name {
        "kill" => env!("CARGO_BIN_EXE_kill"),
        "timeout" => env!("CARGO_BIN_EXE_timeout"),
        "pgrep" => env!("CARGO_BIN_EXE_pgrep"),
        "pkill" => env!("CARGO_BIN_EXE_pkill"),
        _ => panic!("unknown command"),
    }
}
fn run(s: &Sandbox, name: &str, args: &[&str]) -> Output {
    let mut c = Command::new(executable(name));
    c.args(args).current_dir(&s.path);
    common::execute(c, b"")
}
fn fixture_path(s: &Sandbox) -> PathBuf {
    let name = format!(
        "p4{:x}{:x}{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        std::env::consts::EXE_SUFFIX
    );
    let path = s.path.join(name);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/process_child.rs");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let compiled = Command::new(rustc)
        .args(["--edition=2021", "-Cdebuginfo=0", "-Copt-level=1"])
        .arg(source)
        .arg("-o")
        .arg(&path)
        .current_dir(&s.path)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "fixture compilation: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    path
}
fn fixture_command(path: &Path, mode: &str, ready: &Path) -> Command {
    let mut c = Command::new(path);
    c.args(["--exact", "process_child_helper", "--nocapture"])
        .env("AGENTCMD_P4_MODE", mode)
        .env("AGENTCMD_P4_READY", ready)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn_child(path: &Path, mode: &str, ready: &Path) -> OwnedChild {
    let child = OwnedChild(fixture_command(path, mode, ready).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "fixture did not start");
    child
}
fn wait_exit(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
fn ok(out: &Output) {
    assert!(out.status.success(), "{out:?}");
}
fn regex_path(path: &Path) -> String {
    regex::escape(&path.to_string_lossy())
}

#[test]
fn process_child_helper() {
    let Ok(mode) = std::env::var("AGENTCMD_P4_MODE") else {
        return;
    };
    let ready = PathBuf::from(std::env::var_os("AGENTCMD_P4_READY").unwrap());
    if mode == "exit7" {
        std::process::exit(7);
    }
    let mut descendant = None;
    if mode == "tree" {
        let marker = ready.with_extension("leaf");
        let mut leaf = Command::new(std::env::current_exe().unwrap());
        descendant = Some(
            leaf.args(["--exact", "process_child_helper", "--nocapture"])
                .env("AGENTCMD_P4_MODE", "leaf")
                .env("AGENTCMD_P4_READY", &marker)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    if mode == "leaf" {
        thread::sleep(Duration::from_secs(2));
        fs::write(ready, b"survived").unwrap();
        return;
    }
    #[cfg(unix)]
    if mode == "ignore" {
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
        }
    }
    fs::write(ready, b"ready").unwrap();
    thread::sleep(if mode == "ignore" {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(10)
    });
    if let Some(mut child) = descendant {
        let _ = child.wait();
    }
}

#[test]
fn help_version_and_usage_errors() {
    let s = Sandbox::new();
    for name in ["kill", "timeout", "pgrep", "pkill"] {
        for option in ["--help", "--version"] {
            let out = run(&s, name, &[option]);
            ok(&out);
            assert!(!out.stdout.is_empty());
        }
        let out = run(&s, name, &["--unknown-agentcmd-option"]);
        assert!(!out.status.success(), "{name}");
    }
    assert_eq!(run(&s, "timeout", &[]).status.code(), Some(125));
    assert_eq!(run(&s, "pgrep", &[]).status.code(), Some(2));
    assert_eq!(run(&s, "pkill", &[]).status.code(), Some(2));
    assert_eq!(run(&s, "kill", &[]).status.code(), Some(1));
}

#[test]
fn kill_signal_lists_and_errors() {
    let s = Sandbox::new();
    for (input, output) in [
        ("TERM", "15\n"),
        ("SIGTERM", "15\n"),
        ("15", "TERM\n"),
        ("9", "KILL\n"),
        ("0", "0\n"),
        ("143", "TERM\n"),
    ] {
        let out = run(&s, "kill", &["-l", input]);
        ok(&out);
        assert_eq!(out.stdout, output.as_bytes());
    }
    for args in [
        vec!["-s", "invalid", "1"],
        vec!["-n", "999", "1"],
        vec!["not-a-pid"],
    ] {
        assert_eq!(run(&s, "kill", &args).status.code(), Some(1));
    }
    let out = run(&s, "kill", &["-L"]);
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("TERM"));
}

#[test]
fn kill_signal_zero_and_owned_child() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let ready = s.path.join("ready");
    let mut child = spawn_child(&path, "stay", &ready);
    let pid = child.0.id().to_string();
    for args in [
        vec!["-0", pid.as_str()],
        vec!["--signal=0", pid.as_str()],
        vec!["-s", "0", pid.as_str()],
        vec!["-n", "0", pid.as_str()],
    ] {
        ok(&run(&s, "kill", &args));
        assert!(child.0.try_wait().unwrap().is_none());
    }
    ok(&run(&s, "kill", &["-TERM", &pid]));
    assert!(wait_exit(&mut child.0));
}

#[test]
fn timeout_success_failure_and_options() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let path = path.to_str().unwrap();
    for option in ["0", "1s", "0.5m", "1h", "1d"] {
        let out = run(&s, "timeout", &[option, executable("kill"), "-l", "15"]);
        ok(&out);
        assert_eq!(out.stdout, b"TERM\n");
    }
    let mut cmd = Command::new(executable("timeout"));
    cmd.args(["2", path, "--exact", "process_child_helper"])
        .current_dir(&s.path)
        .env("AGENTCMD_P4_MODE", "exit7")
        .env("AGENTCMD_P4_READY", s.path.join("unused"));
    assert_eq!(common::execute(cmd, b"").status.code(), Some(7));
    for args in [
        vec!["bad", path],
        vec!["-s", "BAD", "1", path],
        vec!["-k", "bad", "1", path],
    ] {
        assert_eq!(run(&s, "timeout", &args).status.code(), Some(125));
    }
    assert_eq!(
        run(&s, "timeout", &["1", "agentcmd-no-such-executable-2026"])
            .status
            .code(),
        Some(127)
    );
}

#[test]
fn timeout_terminates_owned_child() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let p = path.to_str().unwrap();
    for args in [
        vec!["0.1", p, "--exact", "process_child_helper"],
        vec!["--foreground", "0.1", p, "--exact", "process_child_helper"],
        vec![
            "--signal=TERM",
            "--verbose",
            "0.1",
            p,
            "--exact",
            "process_child_helper",
        ],
    ] {
        let mut c = Command::new(executable("timeout"));
        c.args(&args)
            .current_dir(&s.path)
            .env("AGENTCMD_P4_MODE", "stay")
            .env("AGENTCMD_P4_READY", s.path.join("ready"));
        let out = common::execute(c, b"");
        assert_eq!(out.status.code(), Some(124), "{args:?}: {out:?}");
    }
    let mut c = Command::new(executable("timeout"));
    c.args([
        "--preserve-status",
        "0.1",
        p,
        "--exact",
        "process_child_helper",
    ])
    .current_dir(&s.path)
    .env("AGENTCMD_P4_MODE", "stay")
    .env("AGENTCMD_P4_READY", s.path.join("ready"));
    let out = common::execute(c, b"");
    assert_eq!(
        out.status.code(),
        Some(if cfg!(windows) { 1 } else { 143 }),
        "{out:?}"
    );
}

#[test]
fn timeout_terminates_owned_process_tree() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let ready = s.path.join("ready");
    let mut c = Command::new(executable("timeout"));
    c.args([
        "1",
        path.to_str().unwrap(),
        "--exact",
        "process_child_helper",
    ])
    .current_dir(&s.path)
    .env("AGENTCMD_P4_MODE", "tree")
    .env("AGENTCMD_P4_READY", &ready);
    let out = common::execute(c, b"");
    assert_eq!(out.status.code(), Some(124), "{out:?}");
    assert!(ready.exists(), "fixture never started a descendant");
    thread::sleep(Duration::from_millis(2200));
    assert!(
        !ready.with_extension("leaf").exists(),
        "descendant survived the timeout"
    );
}

#[cfg(unix)]
#[test]
fn timeout_kill_after_escalates() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let mut c = Command::new(executable("timeout"));
    c.args([
        "--kill-after=0.2",
        "0.5",
        path.to_str().unwrap(),
        "--exact",
        "process_child_helper",
    ])
    .current_dir(&s.path)
    .env("AGENTCMD_P4_MODE", "ignore")
    .env("AGENTCMD_P4_READY", s.path.join("ready"));
    let out = common::execute(c, b"");
    assert_eq!(out.status.code(), Some(137), "{out:?}");
}

#[test]
fn pgrep_finds_only_owned_fixture() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let ready = s.path.join("ready");
    let child = spawn_child(&path, "stay", &ready);
    let pid = child.0.id().to_string();
    let parent = std::process::id().to_string();
    let pattern = regex_path(&path);
    for args in [
        vec!["-f", "-P", &parent, &pattern],
        vec!["--full", "--parent", &parent, &pattern],
        vec!["--full", "--ignore-case", "--parent", &parent, &pattern],
        vec!["--full", "--newest", "--parent", &parent, &pattern],
        vec!["--full", "--oldest", "--parent", &parent, &pattern],
    ] {
        let out = run(&s, "pgrep", &args);
        ok(&out);
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), pid);
    }
    for args in [
        vec!["--full", "--list-name", "--parent", &parent, &pattern],
        vec!["--full", "--list-full", "--parent", &parent, &pattern],
    ] {
        let out = run(&s, "pgrep", &args);
        ok(&out);
        assert!(String::from_utf8_lossy(&out.stdout).starts_with(&pid));
    }
    let out = run(
        &s,
        "pgrep",
        &["--full", "--count", "--parent", &parent, &pattern],
    );
    ok(&out);
    assert_eq!(out.stdout, b"1\n");
    let name = path.file_stem().unwrap().to_str().unwrap();
    let out = run(&s, "pgrep", &["--exact", "--parent", &parent, name]);
    ok(&out);
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), pid);
    let out = run(
        &s,
        "pgrep",
        &[
            "--inverse",
            "--parent",
            &parent,
            "agentcmd-never-match-7c2208",
        ],
    );
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line == pid));
}

#[test]
fn pgrep_delimiter_with_two_owned_children() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let first = spawn_child(&path, "stay", &s.path.join("ready1"));
    let second = spawn_child(&path, "stay", &s.path.join("ready2"));
    let parent = std::process::id().to_string();
    let pattern = regex_path(&path);
    let out = run(
        &s,
        "pgrep",
        &["--full", "--delimiter=,", "--parent", &parent, &pattern],
    );
    ok(&out);
    let mut expected = [first.0.id(), second.0.id()];
    expected.sort_unstable();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{},{}\n", expected[0], expected[1])
    );
}

#[cfg(unix)]
#[test]
fn process_user_filters_match_the_owned_child() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let child = spawn_child(&path, "stay", &s.path.join("ready"));
    let parent = std::process::id().to_string();
    let pattern = regex_path(&path);
    let euid = unsafe { libc::geteuid() }.to_string();
    let uid = unsafe { libc::getuid() }.to_string();
    for (name, flag, user) in [
        ("pgrep", "--euid", &euid),
        ("pgrep", "--uid", &uid),
        ("pkill", "--euid", &euid),
        ("pkill", "--uid", &uid),
    ] {
        let mut args = vec!["--full", "--parent", parent.as_str(), flag, user.as_str()];
        if name == "pkill" {
            args.push("--signal=0");
        }
        args.push(&pattern);
        let out = run(&s, name, &args);
        ok(&out);
        if name == "pgrep" {
            assert_eq!(
                String::from_utf8(out.stdout).unwrap().trim(),
                child.0.id().to_string()
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn pgrep_preserves_non_utf8_process_names() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let s = Sandbox::new();
    let prefix = format!("p4r{:x}", NEXT.fetch_add(1, Ordering::Relaxed));
    let mut raw = prefix.as_bytes().to_vec();
    raw.push(0xff);
    let path = s.path.join(OsStr::from_bytes(&raw));
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/process_child.rs");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let compiled = Command::new(rustc)
        .args(["--edition=2021", "-Cdebuginfo=0", "-Copt-level=1"])
        .arg(source)
        .arg("-o")
        .arg(&path)
        .current_dir(&s.path)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "fixture compilation: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let child = spawn_child(&path, "stay", &s.path.join("ready"));
    let parent = std::process::id().to_string();
    let pattern = format!("{}.", regex::escape(&prefix));
    let out = run(
        &s,
        "pgrep",
        &["--list-name", "--exact", "--parent", &parent, &pattern],
    );
    ok(&out);
    let mut expected = child.0.id().to_string().into_bytes();
    expected.push(b' ');
    expected.extend_from_slice(&raw);
    expected.push(b'\n');
    assert_eq!(out.stdout, expected);
}

#[test]
fn pgrep_no_match_and_syntax() {
    let s = Sandbox::new();
    let parent = std::process::id().to_string();
    let out = run(
        &s,
        "pgrep",
        &[
            "--count",
            "--parent",
            &parent,
            "agentcmd-never-match-7c2208",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"0\n");
    for args in [
        vec!["["],
        vec!["-n", "-o", "x"],
        vec!["-v", "-n", "x"],
        vec!["--terminal", "tty", "x"],
        vec!["-P", "bad", "x"],
    ] {
        assert_eq!(run(&s, "pgrep", &args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn pkill_only_signals_owned_fixture() {
    let s = Sandbox::new();
    let path = fixture_path(&s);
    let ready = s.path.join("ready");
    let mut child = spawn_child(&path, "stay", &ready);
    let parent = std::process::id().to_string();
    let pattern = regex_path(&path);
    let out = run(
        &s,
        "pkill",
        &[
            "--signal=0",
            "--full",
            "--count",
            "--parent",
            &parent,
            &pattern,
        ],
    );
    ok(&out);
    assert_eq!(out.stdout, b"1\n");
    assert!(child.0.try_wait().unwrap().is_none());
    for selector in ["--ignore-case", "--newest", "--oldest"] {
        let out = run(
            &s,
            "pkill",
            &[
                "--signal=0",
                "--full",
                selector,
                "--parent",
                &parent,
                &pattern,
            ],
        );
        ok(&out);
        assert!(child.0.try_wait().unwrap().is_none());
    }
    let name = path.file_stem().unwrap().to_str().unwrap();
    let out = run(
        &s,
        "pkill",
        &["--signal=0", "--exact", "--parent", &parent, name],
    );
    ok(&out);
    let out = run(
        &s,
        "pkill",
        &["-TERM", "--full", "--echo", "--parent", &parent, &pattern],
    );
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains(&child.0.id().to_string()));
    assert!(wait_exit(&mut child.0));
}

#[test]
fn pkill_no_match_and_syntax() {
    let s = Sandbox::new();
    let parent = std::process::id().to_string();
    let out = run(
        &s,
        "pkill",
        &[
            "--signal=0",
            "--count",
            "--parent",
            &parent,
            "agentcmd-never-match-7c2208",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"0\n");
    for args in [
        vec!["["],
        vec!["--signal=BAD", "x"],
        vec!["-n", "-o", "x"],
        vec!["--terminal", "tty", "x"],
    ] {
        assert_eq!(run(&s, "pkill", &args).status.code(), Some(2), "{args:?}");
    }
}
