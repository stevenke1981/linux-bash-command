use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
static N: AtomicUsize = AtomicUsize::new(0);
struct BoxDir(PathBuf);
impl BoxDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "p4-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn run(&self, bin: &str, args: &[&str], input: &[u8]) -> Output {
        let exe = match bin {
            "which" => env!("CARGO_BIN_EXE_which"),
            "uname" => env!("CARGO_BIN_EXE_uname"),
            "whoami" => env!("CARGO_BIN_EXE_whoami"),
            "free" => env!("CARGO_BIN_EXE_free"),
            "cmp" => env!("CARGO_BIN_EXE_cmp"),
            "rev" => env!("CARGO_BIN_EXE_rev"),
            "dos2unix" => env!("CARGO_BIN_EXE_dos2unix"),
            _ => env!("CARGO_BIN_EXE_unix2dos"),
        };
        let mut c = Command::new(exe)
            .current_dir(&self.0)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(e) = c.stdin.take().unwrap().write_all(input) {
            assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe)
        }
        c.wait_with_output().unwrap()
    }
    fn put(&self, n: &str, b: &[u8]) {
        fs::write(self.0.join(n), b).unwrap()
    }
}
impl Drop for BoxDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn which_cli() {
    let d = BoxDir::new();
    assert_eq!(d.run("which", &["--help"], b"").status.code(), Some(0));
    assert_eq!(d.run("which", &["--version"], b"").status.code(), Some(0));
    assert_eq!(
        d.run("which", &["-s", "missing"], b"").status.code(),
        Some(1)
    );
    assert!(d.run("which", &["-s", "missing"], b"").stdout.is_empty());
    assert_eq!(
        d.run("which", &["-a", "missing"], b"").status.code(),
        Some(1)
    );
    assert_eq!(d.run("which", &["--bad"], b"").status.code(), Some(2));
    assert_eq!(d.run("which", &["-a"], b"").status.code(), Some(2));
    assert_eq!(
        d.run("which", &["missing", "missing"], b"").status.code(),
        Some(1)
    );
}
#[test]
fn uname_cli() {
    let d = BoxDir::new();
    for a in [
        &[][..],
        &["-a"],
        &["-s"],
        &["-n"],
        &["-r"],
        &["-v"],
        &["-m"],
        &["-p"],
        &["-i"],
        &["-o"],
        &["--help"],
        &["--version"],
    ] {
        let o = d.run("uname", a, b"");
        assert_eq!(o.status.code(), Some(0));
    }
}
#[test]
fn whoami_cli() {
    let d = BoxDir::new();
    assert_eq!(d.run("whoami", &[], b"").status.code(), Some(0));
    assert!(!d.run("whoami", &[], b"").stdout.is_empty());
    assert_eq!(d.run("whoami", &["--help"], b"").status.code(), Some(0));
    assert_eq!(d.run("whoami", &["--version"], b"").status.code(), Some(0));
    assert_eq!(d.run("whoami", &["x"], b"").status.code(), Some(1));
    assert_eq!(d.run("whoami", &["--bad"], b"").status.code(), Some(1));
}
#[test]
fn free_cli() {
    let d = BoxDir::new();
    for a in [
        &[][..],
        &["-b"],
        &["-k"],
        &["-m"],
        &["-g"],
        &["-h"],
        &["-t"],
        &["-w"],
        &["-s", "0.001", "-c", "1"],
        &["--help"],
        &["--version"],
    ] {
        let out = d.run("free", a, b"");
        if !a.contains(&"--help") && !a.contains(&"--version") {
            assert_eq!(out.status.code(), Some(0));
        }
    }
    let output = d.run("free", &["--total", "--bytes"], b"");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Mem:"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Swap:"));
}
#[test]
fn cmp_cli() {
    let d = BoxDir::new();
    d.put("a", b"abc\n");
    d.put("b", b"axc\n");
    assert_eq!(d.run("cmp", &["a", "b"], b"").status.code(), Some(1));
    for a in [
        &["-b", "a", "b"][..],
        &["-l", "a", "b"],
        &["-i", "1", "a", "b"],
        &["-n", "1", "a", "b"],
        &["-s", "a", "b"],
        &["--print-bytes", "a", "b"],
        &["--help"],
        &["--version"],
    ] {
        let _ = d.run("cmp", a, b"");
    }
}
#[test]
fn rev_cli() {
    let d = BoxDir::new();
    for (input, expected) in [
        (b"abc\n".as_slice(), b"cba\n".as_slice()),
        ("λ😊\n".as_bytes(), "😊λ\n".as_bytes()),
        (b"a\xff\n", b"\xffa\n"),
        (b"", b""),
        (b"a\r\n", b"\ra\n"),
    ] {
        assert_eq!(d.run("rev", &[], input).stdout, expected);
    }
    for a in [&["--help"][..], &["--version"], &["--bad"]] {
        let _ = d.run("rev", a, b"");
    }
}
#[test]
fn dos2unix_cli() {
    let d = BoxDir::new();
    d.put("in", b"a\r\nb");
    for a in [
        &["--help"][..],
        &["--version"],
        &["-q", "in"],
        &["-k", "in"],
        &["-f", "in"],
        &["-b", "in"],
        &["-r", "in"],
        &["-m", "in"],
        &["-e", "in"],
        &["-c", "ascii", "in"],
        &["--newfile", "in", "out"],
    ] {
        d.put("in", b"a\r\nb");
        let out = d.run("dos2unix", a, b"");
        if a == ["-q", "in"] {
            assert_eq!(out.status.code(), Some(0));
            assert_eq!(fs::read(d.0.join("in")).unwrap(), b"a\nb");
        }
        if a == ["--newfile", "in", "out"] {
            assert_eq!(out.status.code(), Some(0));
            assert_eq!(fs::read(d.0.join("out")).unwrap(), b"a\nb");
        }
    }
}
#[test]
fn unix2dos_cli() {
    let d = BoxDir::new();
    d.put("in", b"a\nb");
    for a in [
        &["--help"][..],
        &["--version"],
        &["-q", "in"],
        &["-k", "in"],
        &["-f", "in"],
        &["-b", "in"],
        &["-r", "in"],
        &["-m", "in"],
        &["-e", "in"],
        &["-c", "ascii", "in"],
        &["--newfile", "in", "out"],
    ] {
        d.put("in", b"a\nb");
        let out = d.run("unix2dos", a, b"");
        if a == ["-q", "in"] {
            assert_eq!(out.status.code(), Some(0));
            assert_eq!(fs::read(d.0.join("in")).unwrap(), b"a\r\nb");
        }
        if a == ["--newfile", "in", "out"] {
            assert_eq!(out.status.code(), Some(0));
            assert_eq!(fs::read(d.0.join("out")).unwrap(), b"a\r\nb");
        }
    }
}
