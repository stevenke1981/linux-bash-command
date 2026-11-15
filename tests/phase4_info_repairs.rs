use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "p4-info-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn spawn(&self, command: &str, args: &[&str]) -> Child {
        Command::new(binary(command))
            .current_dir(&self.0)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn run_env(&self, command: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::new(binary(command))
            .current_dir(&self.0)
            .args(args)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn binary(name: &str) -> &'static str {
    match name {
        "free" => env!("CARGO_BIN_EXE_free"),
        "which" => env!("CARGO_BIN_EXE_which"),
        "uname" => env!("CARGO_BIN_EXE_uname"),
        _ => unreachable!(),
    }
}

fn count_headers(output: &[u8]) -> usize {
    String::from_utf8_lossy(output)
        .lines()
        .filter(|line| line.contains("total") && line.contains("used") && line.contains("free"))
        .count()
}

fn wait_bounded(mut child: Child, timeout: Duration) -> (Output, bool) {
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    (child.wait_with_output().unwrap(), timed_out)
}

#[test]
fn free_rejects_bad_interval_and_zero_count_with_exact_status_and_diagnostic() {
    let sandbox = Sandbox::new();
    for (args, message) in [
        (&["-s", "bad"][..], "invalid interval"),
        (&["-sbad"][..], "invalid interval"),
        (&["-s", "NaN"][..], "invalid interval"),
        (&["--seconds=0"][..], "invalid interval"),
        (&["-c", "0"][..], "invalid count"),
        (&["-c0"][..], "invalid count"),
        (&["--count=0"][..], "invalid count"),
    ] {
        let output = sandbox.run_env("free", args, &[]);
        assert_eq!(output.status.code(), Some(1), "args={args:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("free: {message}\nTry 'free --help' for more information.\n")
        );
    }
}

#[test]
fn free_interval_without_count_keeps_emitting_and_flushes_until_interrupted() {
    let sandbox = Sandbox::new();
    let mut child = sandbox.spawn("free", &["--seconds=0.01"]);
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(Ok(line)) = lines.next() {
            if line.contains("total")
                && line.contains("used")
                && line.contains("free")
                && sender.send(()).is_err()
            {
                break;
            }
        }
    });
    let first = receiver.recv_timeout(Duration::from_secs(2));
    let second = receiver.recv_timeout(Duration::from_secs(2));
    let kill = child.kill();
    let _ = child.wait();
    reader.join().unwrap();
    assert!(
        first.is_ok(),
        "first interval header was not flushed: {first:?}"
    );
    assert!(
        second.is_ok(),
        "second interval header was not flushed: {second:?}"
    );
    assert!(kill.is_ok(), "free exited instead of repeating");
}

#[test]
fn free_count_with_interval_finishes_with_exact_number_of_snapshots() {
    let sandbox = Sandbox::new();
    let child = sandbox.spawn("free", &["-s", "0.001", "-c", "3"]);
    let (output, timed_out) = wait_bounded(child, Duration::from_secs(3));
    assert!(!timed_out, "finite free count did not terminate");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(count_headers(&output.stdout), 3);
}

#[cfg(windows)]
#[test]
fn free_lohi_is_accepted_but_omitted_on_windows() {
    let output = Sandbox::new().run_env("free", &["-l", "-c", "1"], &[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Low:"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("High:"));
}

#[test]
fn which_does_not_search_the_current_directory_implicitly() {
    let sandbox = Sandbox::new();
    sandbox.write("local-tool", b"plain file");
    let output = sandbox.run_env("which", &["local-tool"], &[("PATH", "")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[cfg(windows)]
#[test]
fn which_windows_requires_a_pathext_executable_and_prefers_it_to_plain_text() {
    let sandbox = Sandbox::new();
    sandbox.write("tool", b"extensionless text");
    let executable = sandbox.write("tool.exe", b"not executed by which");
    sandbox.write("note.txt", b"not executable by PATHEXT");
    let path = sandbox.0.to_string_lossy().into_owned();
    let output = sandbox.run_env(
        "which",
        &["tool"],
        &[("PATH", &path), ("PATHEXT", ".EXE;.CMD")],
    );
    assert_eq!(output.status.code(), Some(0));
    let found = sandbox.0.join("tool.EXE");
    let raw = found.to_string_lossy();
    let without_verbatim_prefix = raw.strip_prefix(r"\\?\").unwrap_or(raw.as_ref());
    let expected = format!("{}\n", without_verbatim_prefix.replace('\\', "/"));
    assert_eq!(output.stdout, expected.as_bytes());

    let output = sandbox.run_env(
        "which",
        &["note.txt"],
        &[("PATH", &path), ("PATHEXT", ".EXE;.CMD")],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(executable.is_file());
}

#[cfg(windows)]
#[test]
fn uname_kernel_release_contains_the_native_windows_build() {
    let output = Sandbox::new().run_env("uname", &["-r"], &[]);
    assert_eq!(output.status.code(), Some(0));
    let release = String::from_utf8(output.stdout).unwrap();
    let build = release.trim().strip_prefix("10.0.").unwrap();
    assert!(build.parse::<u32>().is_ok_and(|number| number > 0));
}
