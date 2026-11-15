mod common;
use common::Sandbox;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn factor(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_factor"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn factor_cli_cases() {
    for (args, input, output) in [
        (vec!["0", "1", "2"], "", "0:\n1:\n2: 2\n"),
        (vec!["360"], "", "360: 2 2 2 3 3 5\n"),
        (vec!["-h", "360"], "", "360: 2^3 3^2 5\n"),
        (vec!["--exponents", "49"], "", "49: 7^2\n"),
        (vec![], "15\t17\n", "15: 3 5\n17: 17\n"),
        (vec!["+00012"], "", "12: 2 2 3\n"),
        (
            vec!["1000000016000000063"],
            "",
            "1000000016000000063: 1000000007 1000000009\n",
        ),
    ] {
        let result = factor(&args, input.as_bytes());
        assert!(result.status.success(), "{:?}", result.stderr);
        assert_eq!(result.stdout, output.as_bytes());
    }
    let result = factor(&["340282366920938463463374607431768211456"], b"");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("is too large"));
}

fn run(s: &Sandbox, name: &str, args: &[&str], input: &[u8]) -> std::process::Output {
    let path = match name {
        "od" => env!("CARGO_BIN_EXE_od"),
        "split" => env!("CARGO_BIN_EXE_split"),
        "csplit" => env!("CARGO_BIN_EXE_csplit"),
        _ => panic!("invalid command"),
    };
    let mut cmd = Command::new(path);
    cmd.args(args)
        .current_dir(&s.path)
        .env("TMPDIR", &s.path)
        .env("TEMP", &s.path)
        .env("TMP", &s.path)
        .env("LC_ALL", "C")
        .env("TZ", "UTC");
    common::execute(cmd, input)
}

#[test]
fn od_cli_cases() {
    let s = Sandbox::new();
    for (args, input, expected) in [
        (
            vec![],
            b"abc\n".as_slice(),
            "0000000 061141 005143\n0000004\n",
        ),
        (
            vec!["-Ax", "-t", "x1"],
            b"ab".as_slice(),
            "000000 61 62\n000002\n",
        ),
        (vec!["-An", "-t", "x1"], b"ab".as_slice(), " 61 62\n"),
        (vec!["-c"], b"a\n".as_slice(), "0000000   a  \\n\n0000002\n"),
        (
            vec!["--endian=big", "-t", "x2"],
            b"ab".as_slice(),
            "0000000 6162\n0000002\n",
        ),
        (
            vec!["-j1", "-N2", "-t", "x1"],
            b"abcd".as_slice(),
            "0000001 62 63\n0000003\n",
        ),
        (
            vec!["-w2", "-t", "x1"],
            b"abcd".as_slice(),
            "0000000 61 62\n0000002 63 64\n0000004\n",
        ),
        (
            vec!["-t", "x1", "-t", "c"],
            b"ab".as_slice(),
            "0000000  61  62\n          a   b\n0000002\n",
        ),
        (
            vec!["-f"],
            &[0, 0, 128, 63],
            "0000000               1\n0000004\n",
        ),
    ] {
        let out = run(&s, "od", &args, input);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
    let out = run(&s, "od", &["--endian=wrong"], b"");
    assert!(!out.status.success());
    let out = run(&s, "od", &["-w3", "-t", "x2"], b"abcd");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"0000000 6261\n0000002 6463\n0000004\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid width 3; using 2"));
    let out = run(&s, "od", &["-j", "0x2", "-An", "-tx1"], b"abcdef");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b" 63 64 65 66\n");
    let mut input = vec![b'a'; 20_000];
    let out = run(&s, "od", &["-An", "-S3"], &input);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    input.push(0);
    let out = run(&s, "od", &["-An", "-S3"], &input);
    assert!(out.status.success());
    assert_eq!(&out.stdout[..20_000], &input[..20_000]);
    assert_eq!(out.stdout.last(), Some(&b'\n'));
}

#[test]
fn split_cli_cases() {
    for (args, input, files) in [
        (vec!["-l1"], "a\nb\n", vec![("xaa", "a\n"), ("xab", "b\n")]),
        (
            vec!["-b2"],
            "abcde",
            vec![("xaa", "ab"), ("xab", "cd"), ("xac", "e")],
        ),
        (
            vec!["-C3"],
            "abc\nd\n",
            vec![("xaa", "abc"), ("xab", "\nd\n")],
        ),
        (vec!["-C3"], "a\nb", vec![("xaa", "a\n"), ("xab", "b")]),
        (
            vec!["-d", "-l1"],
            "a\nb\n",
            vec![("x00", "a\n"), ("x01", "b\n")],
        ),
        (
            vec!["-x", "-l1"],
            "a\nb\n",
            vec![("x00", "a\n"), ("x01", "b\n")],
        ),
        (
            vec!["-a1", "--additional-suffix=.txt", "-l1"],
            "a\nb\n",
            vec![("xa.txt", "a\n"), ("xb.txt", "b\n")],
        ),
        (
            vec!["-t:", "-l1"],
            "a:b:",
            vec![("xaa", "a:"), ("xab", "b:")],
        ),
        (
            vec!["-n", "r/2"],
            "a\nb\nc\n",
            vec![("xaa", "a\nc\n"), ("xab", "b\n")],
        ),
    ] {
        let s = Sandbox::new();
        let out = run(&s, "split", &args, input.as_bytes());
        assert!(out.status.success(), "{args:?} {out:?}");
        for (name, data) in files {
            assert_eq!(
                fs::read(s.path.join(name)).unwrap(),
                data.as_bytes(),
                "{args:?}"
            );
        }
    }
}

#[test]
fn split_chunk_selection_and_suffix_rollover() {
    let s = Sandbox::new();
    s.write("input", b"abc\nd\neee\nf\ng\n");
    for (args, expected) in [
        (vec!["-n", "2/3", "input"], b"d\nee".as_slice()),
        (vec!["-n", "l/2/3", "input"], b"d\neee\n".as_slice()),
        (vec!["-n", "r/2/3", "input"], b"d\ng\n".as_slice()),
    ] {
        let out = run(&s, "split", &args, b"");
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert_eq!(out.stdout, expected, "{args:?}");
    }
    let s = Sandbox::new();
    let input = b"x\n".repeat(92);
    let out = run(&s, "split", &["-d", "-l1"], &input);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(s.path.join("x89")).unwrap(), b"x\n");
    assert_eq!(fs::read(s.path.join("x9000")).unwrap(), b"x\n");
    assert_eq!(fs::read(s.path.join("x9001")).unwrap(), b"x\n");
    let s = Sandbox::new();
    s.write("input", b"ab");
    let out = run(&s, "split", &["-d", "-e", "-n", "101", "input"], b"");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(s.path.join("x000")).unwrap(), b"a");
    assert_eq!(fs::read(s.path.join("x001")).unwrap(), b"b");
    let out = run(&s, "split", &["-a1", "-e", "-n", "27", "input"], b"");
    assert!(!out.status.success());
    let s = Sandbox::new();
    s.write("input", b"ab");
    let out = run(
        &s,
        "split",
        &["--numeric-suffixes=50", "-e", "-n", "100", "input"],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(s.path.join("x050")).unwrap(), b"a");
}

#[test]
fn csplit_cli_cases() {
    for (args, counts, files) in [
        (
            vec!["-", "2"],
            "2\n4\n",
            vec![("xx00", "a\n"), ("xx01", "b\nc\n")],
        ),
        (
            vec!["-", "/b/"],
            "2\n4\n",
            vec![("xx00", "a\n"), ("xx01", "b\nc\n")],
        ),
        (
            vec!["-s", "-", "2"],
            "",
            vec![("xx00", "a\n"), ("xx01", "b\nc\n")],
        ),
        (
            vec!["-f", "part", "-n", "1", "-", "2"],
            "2\n4\n",
            vec![("part0", "a\n"), ("part1", "b\nc\n")],
        ),
        (
            vec!["-b", "%02x.dat", "-", "2"],
            "2\n4\n",
            vec![("xx00.dat", "a\n"), ("xx01.dat", "b\nc\n")],
        ),
        (
            vec!["--suppress-matched", "-", "/b/"],
            "2\n2\n",
            vec![("xx00", "a\n"), ("xx01", "c\n")],
        ),
        (vec!["-z", "-", "/a/"], "6\n", vec![("xx00", "a\nb\nc\n")]),
    ] {
        let s = Sandbox::new();
        let out = run(&s, "csplit", &args, b"a\nb\nc\n");
        assert!(out.status.success(), "{args:?} {out:?}");
        assert_eq!(out.stdout, counts.as_bytes());
        for (name, data) in files {
            assert_eq!(fs::read(s.path.join(name)).unwrap(), data.as_bytes());
        }
    }
    let s = Sandbox::new();
    let out = run(&s, "csplit", &["-", "/missing/"], b"a\n");
    assert!(!out.status.success());
    assert_eq!(out.stdout, b"2\n");
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 0);
    let out = run(&s, "csplit", &["-k", "-", "/missing/"], b"a\n");
    assert!(!out.status.success());
    assert_eq!(out.stdout, b"2\n");
    assert_eq!(fs::read(s.path.join("xx00")).unwrap(), b"a\n");
}

#[test]
fn csplit_suppresses_the_split_boundary_with_offsets_and_line_numbers() {
    for args in [
        vec!["--suppress-matched", "-", "2"],
        vec!["--suppress-matched", "-", "/a/+1"],
    ] {
        let s = Sandbox::new();
        let out = run(&s, "csplit", &args, b"a\nb\nc\n");
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert_eq!(fs::read(s.path.join("xx00")).unwrap(), b"a\n");
        assert_eq!(fs::read(s.path.join("xx01")).unwrap(), b"c\n");
    }
}

#[test]
fn csplit_repeats_line_patterns_and_cleans_up_on_failure() {
    let s = Sandbox::new();
    let out = run(&s, "csplit", &["-", "2", "{1}"], b"a\nb\nc\nd\n");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"2\n4\n2\n");
    assert_eq!(fs::read(s.path.join("xx00")).unwrap(), b"a\n");
    assert_eq!(fs::read(s.path.join("xx01")).unwrap(), b"b\nc\n");
    assert_eq!(fs::read(s.path.join("xx02")).unwrap(), b"d\n");
    let s = Sandbox::new();
    let out = run(&s, "csplit", &["-", "/b/", "{*}"], b"a\nb\nc\nb\nd\n");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"2\n4\n4\n");
    assert_eq!(fs::read(s.path.join("xx01")).unwrap(), b"b\nc\n");
    assert_eq!(fs::read(s.path.join("xx02")).unwrap(), b"b\nd\n");
    let s = Sandbox::new();
    let out = run(&s, "csplit", &["-", "/^b/", "{*}", "2"], b"a\nb\nc\nb\ne\n");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(s.path.join("xx02")).unwrap(), b"b\ne\n");
    let s = Sandbox::new();
    let out = run(&s, "csplit", &["-", "2", "{1}", "{1}"], b"a\nb\nc\n");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 0);
    let s = Sandbox::new();
    let out = run(&s, "csplit", &["-", "4"], b"a\nb\nc\n");
    assert!(!out.status.success());
    assert_eq!(out.stdout, b"6\n");
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 0);
    let out = run(&s, "csplit", &["-", "/a/-1"], b"a\nb\n");
    assert!(!out.status.success());
    assert_eq!(out.stdout, b"0\n");
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 0);
}

#[test]
fn csplit_basic_regex_backreference() {
    let s = Sandbox::new();
    let out = run(&s, "csplit", &["-", r"/\(ab\)\1/"], b"z\nabab\ny\n");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"2\n7\n");
    assert_eq!(fs::read(s.path.join("xx00")).unwrap(), b"z\n");
    assert_eq!(fs::read(s.path.join("xx01")).unwrap(), b"abab\ny\n");
}

#[test]
fn splitters_do_not_truncate_their_input() {
    let s = Sandbox::new();
    s.write("xaa", b"abc");
    let out = run(&s, "split", &["-b1", "xaa"], b"");
    assert!(!out.status.success());
    assert_eq!(fs::read(s.path.join("xaa")).unwrap(), b"abc");
    let s = Sandbox::new();
    s.write("xx00", b"a\nb\n");
    let out = run(&s, "csplit", &["xx00", "2"], b"");
    assert!(!out.status.success());
    assert_eq!(fs::read(s.path.join("xx00")).unwrap(), b"a\nb\n");
}

#[test]
fn split_verbose_unbuffered_and_unsupported_filter() {
    let s = Sandbox::new();
    let out = run(&s, "split", &["-u", "--verbose", "-l1"], b"a\n");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"creating file 'xaa'\n");
    assert_eq!(fs::read(s.path.join("xaa")).unwrap(), b"a\n");
    let out = run(&s, "split", &["--filter=cat"], b"a\n");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unsupported"));
}

#[test]
fn split_unbuffered_round_robin_emits_before_input_closes() {
    use std::{io::Read, sync::mpsc, time::Duration};
    let s = Sandbox::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_split"))
        .args(["--unbuffered", "-n", "r/1/2"])
        .current_dir(&s.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = [0u8; 2];
        let result = output.read_exact(&mut bytes).map(|()| bytes);
        let _ = send.send(result);
    });
    input.write_all(b"a\n").unwrap();
    input.flush().unwrap();
    let received = receive.recv_timeout(Duration::from_secs(5));
    if received.is_err() {
        let _ = child.kill();
    }
    drop(input);
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert_eq!(received.unwrap().unwrap(), *b"a\n");
    assert!(status.success());
}

#[test]
fn split_writes_completed_chunks_before_input_closes() {
    use std::time::{Duration, Instant};
    for (args, input, expected) in [
        (vec!["-b2"], b"abcd".as_slice(), b"ab".as_slice()),
        (vec!["-l1"], b"a\nb\n".as_slice(), b"a\n".as_slice()),
        (vec!["-C2"], b"abcd".as_slice(), b"ab".as_slice()),
    ] {
        let s = Sandbox::new();
        let mut child = Command::new(env!("CARGO_BIN_EXE_split"))
            .args(&args)
            .current_dir(&s.path)
            .env("TMPDIR", &s.path)
            .env("TEMP", &s.path)
            .env("TMP", &s.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pipe = child.stdin.take().unwrap();
        pipe.write_all(input).unwrap();
        pipe.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = None;
        while Instant::now() < deadline {
            if let Ok(bytes) = fs::read(s.path.join("xaa")) {
                if bytes.len() >= expected.len() {
                    observed = Some(bytes);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if observed.is_none() {
            let _ = child.kill();
        }
        drop(pipe);
        let result = child.wait_with_output().unwrap();
        assert_eq!(observed.as_deref(), Some(expected), "{args:?}: {result:?}");
        assert!(result.status.success(), "{args:?}: {result:?}");
    }
}

#[cfg(unix)]
#[test]
fn temporary_inputs_are_private() {
    use std::{
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    for (name, args, input, prefix) in [
        (
            "split",
            vec!["-n", "2"],
            b"a\n".as_slice(),
            "agentcmd-split-",
        ),
        (
            "split",
            vec!["-C", "2"],
            b"a\n".as_slice(),
            "agentcmd-split-",
        ),
        (
            "csplit",
            vec!["-", "/a/"],
            b"a\n".as_slice(),
            "agentcmd-csplit-",
        ),
        ("od", vec!["-S3"], b"abc\0".as_slice(), "agentcmd-od-"),
    ] {
        let s = Sandbox::new();
        let executable = match name {
            "split" => env!("CARGO_BIN_EXE_split"),
            "csplit" => env!("CARGO_BIN_EXE_csplit"),
            _ => env!("CARGO_BIN_EXE_od"),
        };
        let mut child = Command::new(executable)
            .args(&args)
            .current_dir(&s.path)
            .env("TMPDIR", &s.path)
            .env("TMP", &s.path)
            .env("TEMP", &s.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = None;
        while Instant::now() < deadline {
            observed = fs::read_dir(&s.path)
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
                .map(|entry| fs::metadata(entry.path()).unwrap().permissions().mode() & 0o777);
            if observed.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if observed.is_none() {
            let _ = child.kill();
        }
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(input);
        }
        let result = child.wait_with_output().unwrap();
        assert_eq!(observed, Some(0o600), "{name}: {result:?}");
        assert!(result.status.success(), "{name}: {result:?}");
    }
}
