use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-text-compat-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &[u8]) {
        fs::write(self.0.join(name), contents).unwrap();
    }

    fn run(&self, name: &str, args: &[&str], stdin: &[u8]) -> Output {
        let mut child = Command::new(binary(name))
            .current_dir(&self.0)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(stdin) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn binary(name: &str) -> &'static str {
    match name {
        "cat" => env!("CARGO_BIN_EXE_cat"),
        "head" => env!("CARGO_BIN_EXE_head"),
        "tail" => env!("CARGO_BIN_EXE_tail"),
        "less" => env!("CARGO_BIN_EXE_less"),
        _ => panic!("unknown binary {name}"),
    }
}

#[test]
fn text_commands_expand_unique_long_options() {
    let fixture = Fixture::new();
    let cat = fixture.run("cat", &["--number-nonb"], b"one\n\ntwo\n");
    assert_eq!(cat.status.code(), Some(0));
    assert_eq!(cat.stdout, b"     1\tone\n\n     2\ttwo\n");
    let head = fixture.run("head", &["--lin=1"], b"one\ntwo\n");
    assert_eq!(head.status.code(), Some(0));
    assert_eq!(head.stdout, b"one\n");
    let ambiguous = fixture.run("cat", &["--number-n"], b"");
    assert_eq!(ambiguous.status.code(), Some(0));
    let ambiguous = fixture.run("cat", &["--show-"], b"");
    assert_eq!(ambiguous.status.code(), Some(1));
}

#[test]
fn cat_stops_parsing_options_after_an_operand_in_posix_mode() {
    let fixture = Fixture::new();
    fixture.write("input", b"first\n");
    fixture.write("-n", b"second\n");
    let output = Command::new(binary("cat"))
        .current_dir(&fixture.0)
        .env("POSIXLY_CORRECT", "1")
        .args(["input", "-n"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"first\nsecond\n");
}

/// Kills the child on drop so a failing assertion never leaks a process or a file handle.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Reads child stdout on a thread and forwards chunks through a channel.
fn stdout_chunks(child: &mut Child) -> Receiver<Vec<u8>> {
    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0u8; 512];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if sender.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    receiver
}

/// Waits until `seen` contains `needle`, failing with the observed output after a timeout.
fn wait_for_output(receiver: &Receiver<Vec<u8>>, seen: &mut Vec<u8>, needle: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !seen.windows(needle.len()).any(|window| window == needle) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(chunk) => seen.extend_from_slice(&chunk),
            Err(error) => panic!(
                "expected {:?} in tail output ({error}); saw {:?}",
                String::from_utf8_lossy(needle),
                String::from_utf8_lossy(seen)
            ),
        }
    }
}

#[test]
fn cat_numbers_lines_and_squeezes_blank_runs() {
    let f = Fixture::new();
    let text = b"a\n\nb\n";
    assert_eq!(
        f.run("cat", &["-n"], text).stdout,
        b"     1\ta\n     2\t\n     3\tb\n"
    );
    assert_eq!(
        f.run("cat", &["-b"], text).stdout,
        b"     1\ta\n\n     2\tb\n"
    );
    assert_eq!(
        f.run("cat", &["--squeeze-blank"], b"a\n\n\n\nb\n").stdout,
        b"a\n\nb\n"
    );
    assert_eq!(
        f.run("cat", &["-ns"], b"a\n\n\nb\n").stdout,
        b"     1\ta\n     2\t\n     3\tb\n"
    );
}

#[test]
fn cat_show_all_marks_tabs_ends_and_nonprinting_bytes() {
    let f = Fixture::new();
    let out = f.run("cat", &["-A"], b"a\tb\x01\xff\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"a^Ib^AM-^?$\n");
    assert_eq!(f.run("cat", &["-E"], b"x\n").stdout, b"x$\n");
    assert_eq!(f.run("cat", &["-T"], b"x\ty\n").stdout, b"x^Iy\n");
    assert_eq!(f.run("cat", &["-v"], b"x\ty\n").stdout, b"x\ty\n");
}

#[test]
fn cat_keeps_non_utf8_bytes_when_plain() {
    let f = Fixture::new();
    let bytes = b"\xff\x00line\r\n";
    assert_eq!(f.run("cat", &[], bytes).stdout, bytes);
    assert_eq!(
        f.run("cat", &["-n"], bytes).stdout,
        b"     1\t\xff\x00line\r\n"
    );
}

#[test]
fn head_counts_bytes_and_lines_with_negative_and_suffix_forms() {
    let f = Fixture::new();
    assert_eq!(f.run("head", &["-c", "3"], b"abcdef").stdout, b"abc");
    assert_eq!(f.run("head", &["-c", "-2"], b"abcdef").stdout, b"abcd");
    assert_eq!(
        f.run("head", &["-n", "-2"], b"1\n2\n3\n4\n").stdout,
        b"1\n2\n"
    );
    assert_eq!(
        f.run("head", &["-n3"], b"1\n2\n3\n4\n").stdout,
        b"1\n2\n3\n"
    );
    assert_eq!(f.run("head", &["--lines=1"], b"1\n2\n").stdout, b"1\n");
    assert_eq!(f.run("head", &["-n", "0"], b"1\n").stdout, b"");

    let big = vec![b'x'; 2000];
    assert_eq!(f.run("head", &["-c", "1K"], &big).stdout.len(), 1024);
    assert_eq!(f.run("head", &["-c", "1b"], &big).stdout.len(), 512);
}

#[test]
fn tail_applies_from_and_last_forms_for_lines_and_bytes() {
    let f = Fixture::new();
    assert_eq!(f.run("tail", &["-n", "+2"], b"1\n2\n3\n").stdout, b"2\n3\n");
    assert_eq!(f.run("tail", &["-n", "+1"], b"1\n2\n").stdout, b"1\n2\n");
    assert_eq!(f.run("tail", &["-c", "+3"], b"abcdef").stdout, b"cdef");
    assert_eq!(f.run("tail", &["-c", "3"], b"abcdef").stdout, b"def");
    assert_eq!(f.run("tail", &["-n", "2"], b"1\n2\n3\n").stdout, b"2\n3\n");
    assert_eq!(f.run("tail", &["-n", "-2"], b"1\n2\n3\n").stdout, b"2\n3\n");
    assert_eq!(f.run("tail", &["-2"], b"1\n2\n3\n").stdout, b"2\n3\n");
    assert!(f.run("tail", &["-n", "0"], b"1\n2\n").stdout.is_empty());
}

#[test]
fn head_quiet_and_verbose_control_headers_for_multiple_files() {
    let f = Fixture::new();
    f.write("a", b"A1\nA2\n");
    f.write("b", b"B1\n");
    assert_eq!(
        f.run("head", &["-q", "-n", "1", "a", "b"], b"").stdout,
        b"A1\nB1\n"
    );
    assert_eq!(
        f.run("head", &["-n", "1", "a", "b"], b"").stdout,
        b"==> a <==\nA1\n\n==> b <==\nB1\n"
    );
    assert_eq!(
        f.run("tail", &["-v", "-n", "1", "b"], b"").stdout,
        b"==> b <==\nB1\n"
    );
    assert_eq!(
        f.run("tail", &["-q", "-n", "1", "a", "b"], b"").stdout,
        b"A2\nB1\n"
    );
}

#[test]
fn head_and_tail_reject_unsupported_options_with_exit_one() {
    let f = Fixture::new();
    assert_eq!(f.run("head", &["--bogus"], b"x").status.code(), Some(1));
    assert_eq!(f.run("head", &["-c", "x"], b"x").status.code(), Some(1));
    assert_eq!(
        f.run("tail", &["-s", "x", "-f"], b"x").status.code(),
        Some(1)
    );
    assert_eq!(f.run("head", &["-f"], b"x").status.code(), Some(1));
}

#[test]
fn tail_follow_prints_appended_bytes_until_killed() {
    let f = Fixture::new();
    f.write("grow.txt", b"first\n");
    let child = Command::new(binary("tail"))
        .current_dir(&f.0)
        .args(["-n", "1", "-f", "grow.txt"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = KillOnDrop(child);
    let receiver = stdout_chunks(&mut child.0);

    let mut seen = Vec::new();
    wait_for_output(&receiver, &mut seen, b"first\n");

    let mut file = OpenOptions::new()
        .append(true)
        .open(f.0.join("grow.txt"))
        .unwrap();
    file.write_all(b"second\n").unwrap();
    file.flush().unwrap();
    drop(file);

    wait_for_output(&receiver, &mut seen, b"second\n");
    assert_eq!(
        child.0.try_wait().unwrap(),
        None,
        "tail should keep following"
    );
}

#[test]
fn tail_pid_stops_following_when_process_exits() {
    let f = Fixture::new();
    f.write("log.txt", b"line\n");
    let mut watched = Command::new(binary("cat"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = watched.id();
    let child = Command::new(binary("tail"))
        .current_dir(&f.0)
        .args(["-f", "--pid", &pid.to_string(), "-s", "0.05", "log.txt"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut tail = KillOnDrop(child);
    let receiver = stdout_chunks(&mut tail.0);
    let mut seen = Vec::new();
    wait_for_output(&receiver, &mut seen, b"line\n");

    drop(watched.stdin.take());
    watched.wait().unwrap();

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = tail.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(0));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tail did not stop after the watched process exited"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn less_raw_flags_pass_file_through_when_piped() {
    let f = Fixture::new();
    let contents = b"\x1b[31mred\x1b[0m\n\xffraw\n";
    f.write("file.txt", contents);
    let out = f.run("less", &["-RFX", "file.txt"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, contents);
    let plus = f.run("less", &["-R", "+G", "+/raw", "file.txt"], b"");
    assert_eq!(plus.status.code(), Some(0));
    assert_eq!(plus.stdout, contents);
    assert_eq!(
        f.run("less", &["-RX", "--chop-long-lines"], contents)
            .stdout,
        contents
    );
    assert_eq!(
        f.run("less", &["-QK", "file.txt"], b"").status.code(),
        Some(0)
    );
    assert_eq!(
        f.run("less", &["-z", "file.txt"], b"").status.code(),
        Some(1)
    );
}
