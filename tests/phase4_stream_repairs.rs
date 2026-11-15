mod common;
use common::Sandbox;
use std::{
    fs,
    process::{Command, Output},
};

fn run(s: &Sandbox, name: &str, args: &[&str], input: &[u8]) -> Output {
    let exe = match name {
        "cmp" => env!("CARGO_BIN_EXE_cmp"),
        "dos2unix" => env!("CARGO_BIN_EXE_dos2unix"),
        "unix2dos" => env!("CARGO_BIN_EXE_unix2dos"),
        _ => unreachable!(),
    };
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(&s.path);
    common::execute(cmd, input)
}
#[test]
fn converters_stdin_bom_and_eol() {
    let s = Sandbox::new();
    for (name, args, input, expected) in [
        (
            "dos2unix",
            &[][..],
            b"a\r\nb\r\n".as_slice(),
            b"a\nb\n".as_slice(),
        ),
        ("unix2dos", &[][..], b"a\nb\r\n", b"a\r\nb\r\n"),
        (
            "dos2unix",
            &["-m"][..],
            b"\xef\xbb\xbfx\r\n",
            b"\xef\xbb\xbfx\n",
        ),
        ("dos2unix", &["-m"][..], b"x\r\n", b"\xef\xbb\xbfx\n"),
        ("unix2dos", &["-b"][..], b"x\n", b"x\r\n"),
        ("unix2dos", &["-r"][..], b"\xef\xbb\xbfx\n", b"x\r\n"),
        ("unix2dos", &["-e"][..], b"x", b"x\r\n"),
        ("dos2unix", &["-e"][..], b"", b""),
    ] {
        let out = run(&s, name, args, input);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(out.stdout, expected, "{name} {args:?}");
    }
}
#[test]
fn newfile_pairs_are_validated_before_changes() {
    let s = Sandbox::new();
    for name in ["dos2unix", "unix2dos"] {
        s.write("a", b"a\nb\n");
        s.write("b", b"c\nd\n");
        let out = run(&s, name, &["-q", "-n", "a", "x", "b", "y"], b"");
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(fs::read(s.path.join("a")).unwrap(), b"a\nb\n");
        assert_eq!(fs::read(s.path.join("b")).unwrap(), b"c\nd\n");
        let expected = if name == "unix2dos" {
            b"c\r\nd\r\n".as_slice()
        } else {
            b"c\nd\n".as_slice()
        };
        assert_eq!(fs::read(s.path.join("y")).unwrap(), expected);
        fs::remove_file(s.path.join("x")).unwrap();
        let out = run(&s, name, &["-n", "a", "x", "b"], b"");
        assert_eq!(out.status.code(), Some(1));
        assert!(!s.path.join("x").exists());
    }
}
#[test]
fn binary_input_rolls_back_destination() {
    let s = Sandbox::new();
    for name in ["dos2unix", "unix2dos"] {
        s.write("input", b"text\n\0binary");
        s.write("out", b"original");
        assert_eq!(
            run(&s, name, &["-q", "-n", "input", "out"], b"")
                .status
                .code(),
            Some(0)
        );
        assert_eq!(fs::read(s.path.join("out")).unwrap(), b"original");
        assert_eq!(fs::read(s.path.join("input")).unwrap(), b"text\n\0binary");
        assert_eq!(run(&s, name, &["-q"], b"text\n\0binary").stdout, b"");
        assert!(run(&s, name, &["-q", "-f"], b"text\n\0binary")
            .stdout
            .contains(&0));
        assert!(!fs::read_dir(&s.path).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
    }
}
#[test]
fn late_binary_detection_discards_buffered_output() {
    let s = Sandbox::new();
    let mut input = vec![b'x'; 131_072];
    input.extend_from_slice(b"\x1b\n");
    s.write("input", input);
    s.write("out", b"original");
    for name in ["dos2unix", "unix2dos"] {
        let out = run(&s, name, &["-q", "-n", "input", "out"], b"");
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(fs::read(s.path.join("out")).unwrap(), b"original");
    }
}
#[test]
fn dash_operands_use_standard_streams() {
    let s = Sandbox::new();
    let out = run(&s, "dos2unix", &["-q", "-n", "-", "out"], b"a\r\nb\r\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(fs::read(s.path.join("out")).unwrap(), b"a\nb\n");
    let out = run(&s, "unix2dos", &["-q", "-n", "out", "-"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"a\r\nb\r\n");
}
#[test]
fn comparison_output_and_eof() {
    let s = Sandbox::new();
    s.write("a", b"a\n");
    s.write("b", b"b\n");
    for (args, expected) in [
        (&["a", "b"][..], b"a b differ: byte 1, line 1\n".as_slice()),
        (
            &["-b", "a", "b"][..],
            b"a b differ: byte 1, line 1 is 141 a 142 b\n",
        ),
        (&["-l", "a", "b"][..], b"1 141 142\n"),
        (&["-lb", "a", "b"][..], b"1 141 a    142 b\n"),
    ] {
        let out = run(&s, "cmp", args, b"");
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(out.stdout, expected);
        assert!(out.stderr.is_empty());
    }
    s.write("a", b"");
    let out = run(&s, "cmp", &["a", "b"], b"");
    assert!(out.stdout.is_empty());
    assert_eq!(out.stderr, b"cmp: EOF on a which is empty\n");
}
#[test]
fn comparison_stdin_and_argument_limits() {
    let s = Sandbox::new();
    s.write("a", b"same\n");
    assert_eq!(run(&s, "cmp", &["a"], b"same\n").status.code(), Some(0));
    assert_eq!(run(&s, "cmp", &["a"], b"diff\n").status.code(), Some(1));
    assert_eq!(
        run(&s, "cmp", &["a", "a", "0", "0", "0"], b"")
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(&s, "cmp", &["-n", "2", "a"], b"sax").status.code(),
        Some(0)
    );
}
#[test]
fn verbose_offsets_are_padded_to_compared_length() {
    let s = Sandbox::new();
    s.write("a", b"aaaaaaaaaaaa");
    s.write("b", b"bbbbbbbbbbbb");
    let out = run(&s, "cmp", &["-l", "a", "b"], b"");
    assert!(out.stdout.starts_with(b" 1 141 142\n 2 141 142\n"));
    assert!(out.stdout.ends_with(b"12 141 142\n"));
    let out = run(&s, "cmp", &["-l", "-n", "2", "a", "b"], b"");
    assert_eq!(out.stdout, b"1 141 142\n2 141 142\n");
}
#[test]
fn large_input_crosses_stream_buffers() {
    let s = Sandbox::new();
    let mut input = vec![b'x'; 65535];
    input.extend_from_slice(b"\r\nlast\n");
    let mut expected = vec![b'x'; 65535];
    expected.extend_from_slice(b"\nlast\n");
    assert_eq!(run(&s, "dos2unix", &[], &input).stdout, expected);
}
#[cfg(unix)]
fn open_fifo_writer(path: &std::path::Path, child: &mut std::process::Child) -> fs::File {
    use std::{
        os::unix::fs::OpenOptionsExt,
        thread,
        time::{Duration, Instant},
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => return file,
            Err(e) if e.raw_os_error() == Some(libc::ENXIO) => {}
            Err(e) => panic!("opening FIFO failed: {e}"),
        }
        if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("converter did not open the FIFO");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(unix)]
#[test]
fn predictable_legacy_temp_hardlink_cannot_touch_victim() {
    use std::{ffi::CString, io::Write, os::unix::ffi::OsStrExt, process::Stdio};
    let s = Sandbox::new();
    s.write("victim", b"untouched");
    let fifo = s.path.join("fifo");
    let cpath = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    let mut child = Command::new(env!("CARGO_BIN_EXE_dos2unix"))
        .current_dir(&s.path)
        .args(["-q", "-n", "fifo", "out"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let legacy = s.path.join(format!("out.agentcmd-{}-tmp", child.id()));
    fs::hard_link(s.path.join("victim"), &legacy).unwrap();
    let mut writer = open_fifo_writer(&fifo, &mut child);
    writer.write_all(b"new\r\n").unwrap();
    drop(writer);
    assert!(child.wait_with_output().unwrap().status.success());
    assert_eq!(fs::read(s.path.join("victim")).unwrap(), b"untouched");
    assert_eq!(fs::read(s.path.join("out")).unwrap(), b"new\n");
}
