mod common;
#[path = "phase1_stream/cut.rs"]
mod cut;
#[path = "phase1_stream/paste.rs"]
mod paste;
#[path = "phase1_stream/tee.rs"]
mod tee;

fn closed_pipe(util: &str, args: &[&str]) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(common::binary(util))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        if let Err(e) = stdin.write_all(&b"a b c\n".repeat(65536)) {
            assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
        }
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert_eq!(out.status.code(), Some(0), "{util}: {:?}", out.stderr);
    assert!(out.stderr.is_empty(), "{util}: {:?}", out.stderr);
}

#[test]
fn wc_broken_pipe() {
    closed_pipe("wc", &[]);
}
#[test]
fn cut_broken_pipe() {
    closed_pipe("cut", &["-b1"]);
}
#[test]
fn tr_broken_pipe() {
    closed_pipe("tr", &["a-z", "A-Z"]);
}
#[test]
fn paste_broken_pipe() {
    closed_pipe("paste", &[]);
}
#[test]
fn tee_broken_pipe() {
    closed_pipe("tee", &[]);
}

#[test]
fn help_is_an_option_value() {
    let out = common::run("cut", &["-d", "--help", "-f1"], b"x\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("delimiter must be a single character"));
    let out = common::run("paste", &["-d", "--help", "-s"], b"a\nb\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"a-b\n");
}

#[test]
fn help_stops_before_later_errors() {
    for cmd in ["wc", "cut", "tr", "paste", "tee"] {
        let out = common::run(cmd, &["--help", "--unknown"], b"");
        assert_eq!(out.status.code(), Some(0));
        assert!(out.stdout.starts_with(b"Usage:"));
        let out = common::run(cmd, &["--unknown", "--help"], b"");
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
    }
}
#[path = "phase1_stream/tr.rs"]
mod tr;
#[path = "phase1_stream/wc.rs"]
mod wc;
