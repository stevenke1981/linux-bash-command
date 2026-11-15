mod common;
use std::process::Output;

fn run(name: &str, args: &[&str], input: &[u8], files: &[(&str, &[u8])]) -> Output {
    let sandbox = common::Sandbox::new();
    for (path, bytes) in files {
        sandbox.write(path, bytes);
    }
    sandbox.run(name, args, input)
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}
fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}
fn digest(name: &str) -> &'static str {
    match name {
        "md5sum" => "900150983cd24fb0d6963f7d28e17f72",
        "sha1sum" => "a9993e364706816aba3e25717850c26c9cd0d89d",
        "sha224sum" => "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
        "sha256sum" => "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha384sum" => "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
        "sha512sum" => "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
        "b2sum" => "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923",
        _ => panic!("no digest vector for {name}"),
    }
}

const HASHES: &[&str] = &[
    "md5sum",
    "sha1sum",
    "sha224sum",
    "sha256sum",
    "sha384sum",
    "sha512sum",
    "b2sum",
];

#[test]
fn standard_hashes_cover_stdin_files_modes_and_check_formats() {
    for name in HASHES {
        let d = digest(name);
        let o = run(name, &[], b"abc", &[]);
        assert_eq!(code(&o), 0, "{name} stdin");
        assert!(out(&o).starts_with(d), "{name}: {}", out(&o));
        assert!(out(&o).ends_with("  -\n"), "{name}: {}", out(&o));

        let o = run(name, &["one"], b"", &[("one", b"abc")]);
        assert_eq!(code(&o), 0, "{name} file");
        assert_eq!(out(&o), format!("{d}  one\n"));

        let o = run(
            name,
            &["one", "two"],
            b"",
            &[("one", b"abc"), ("two", b"abc")],
        );
        assert_eq!(code(&o), 0, "{name} multiple files");
        assert_eq!(out(&o), format!("{d}  one\n{d}  two\n"));

        for flag in ["--binary", "--text"] {
            let o = run(name, &[flag], b"abc", &[]);
            assert_eq!(code(&o), 0, "{name} {flag}");
            assert!(out(&o).starts_with(d));
        }

        let o = run(name, &["--tag"], b"abc", &[]);
        assert_eq!(code(&o), 0, "{name} --tag");
        assert!(out(&o).contains(" (-) = "));
        assert!(out(&o).contains(d));

        let o = run(name, &["--zero"], b"abc", &[]);
        assert_eq!(code(&o), 0, "{name} --zero");
        assert!(o.stdout.ends_with(b"  -\0"));

        let check = format!("{d}  data\n");
        let o = run(
            name,
            &["--check", "SUMS"],
            b"",
            &[("data", b"abc"), ("SUMS", check.as_bytes())],
        );
        assert_eq!(code(&o), 0, "{name} GNU check");
        assert!(out(&o).contains("data: OK"));

        let tagged = out(&run(name, &["--tag"], b"abc", &[])).replace("(-)", "(data)");
        let o = run(
            name,
            &["--check", "SUMS"],
            b"",
            &[("data", b"abc"), ("SUMS", tagged.as_bytes())],
        );
        assert_eq!(code(&o), 0, "{name} BSD check");
        assert!(out(&o).contains("data: OK"));

        let bad = run(
            name,
            &["--check", "SUMS"],
            b"",
            &[("SUMS", b"not a checksum line\n")],
        );
        assert_ne!(code(&bad), 0, "{name} malformed check");
        assert_ne!(code(&run(name, &["--definitely-invalid"], b"", &[])), 0);
        assert_eq!(code(&run(name, &["--help"], b"", &[])), 0);
        assert_eq!(code(&run(name, &["--version"], b"", &[])), 0);
    }
}

#[test]
fn hash_check_controls_missing_mismatch_and_strict_modes() {
    for name in HASHES {
        let d = digest(name);
        let mismatch = format!("{}  -\n", "0".repeat(d.len()));
        assert_ne!(
            code(&run(
                name,
                &["-c", "SUMS"],
                b"",
                &[
                    ("data", b"abc"),
                    ("SUMS", format!("{mismatch}\n").as_bytes())
                ]
            )),
            0,
            "{name}"
        );
        let line = format!("{d}  data\n");
        assert_eq!(
            code(&run(
                name,
                &["--check", "--quiet", "SUMS"],
                b"",
                &[("data", b"abc"), ("SUMS", line.as_bytes())]
            )),
            0
        );
        let o = run(
            name,
            &["--check", "--status", "SUMS"],
            b"",
            &[("data", b"abc"), ("SUMS", line.as_bytes())],
        );
        assert_eq!(code(&o), 0);
        assert!(o.stdout.is_empty() && o.stderr.is_empty());
        let missing = format!("{d}  absent\n{d}  data\n");
        let o = run(
            name,
            &["--check", "--ignore-missing", "SUMS"],
            b"",
            &[("data", b"abc"), ("SUMS", missing.as_bytes())],
        );
        assert_eq!(code(&o), 0, "{name} ignore missing");
        let malformed = format!("{d}  data\nmalformed\n");
        assert_ne!(
            code(&run(
                name,
                &["--check", "--strict", "SUMS"],
                b"",
                &[("data", b"abc"), ("SUMS", malformed.as_bytes())]
            )),
            0
        );
        let o = run(
            name,
            &["--check", "--warn", "SUMS"],
            b"",
            &[("data", b"abc"), ("SUMS", malformed.as_bytes())],
        );
        assert_eq!(code(&o), 0);
        assert!(!o.stderr.is_empty());
    }
}

#[test]
fn b2sum_lengths_are_parameterized_and_validated() {
    for bits in ["0", "8", "128", "256", "512"] {
        let o = run("b2sum", &["--length", bits], b"abc", &[]);
        assert_eq!(code(&o), 0, "length {bits}");
        let expected = if bits == "0" {
            128
        } else {
            bits.parse::<usize>().unwrap() / 4
        };
        assert_eq!(out(&o).split_whitespace().next().unwrap().len(), expected);
    }
    for bits in ["7", "513", "abc"] {
        assert_ne!(
            code(&run("b2sum", &["--length", bits], b"abc", &[])),
            0,
            "length {bits}"
        );
    }
}

#[test]
fn cksum_algorithms_and_output_modes() {
    for (args, files) in [
        (vec!["data"], vec![("data", b"abc".as_slice())]),
        (
            vec!["one", "two"],
            vec![("one", b"abc".as_slice()), ("two", b"def".as_slice())],
        ),
    ] {
        let result = run("cksum", &args, b"", &files);
        assert_eq!(code(&result), 0, "cksum {args:?}");
        assert_eq!(out(&result).lines().count(), files.len());
    }
    for args in [
        vec![],
        vec!["--algorithm=crc"],
        vec!["--algorithm=crc32b"],
        vec!["--algorithm=bsd"],
        vec!["--algorithm=sysv"],
        vec!["--algorithm=md5"],
        vec!["--algorithm=sha1"],
        vec!["--algorithm=sha224"],
        vec!["--algorithm=sha256"],
        vec!["--algorithm=sha384"],
        vec!["--algorithm=sha512"],
        vec!["--algorithm=blake2b"],
    ] {
        let o = run("cksum", &args, b"123456789", &[]);
        assert_eq!(code(&o), 0, "{args:?}");
        assert!(!o.stdout.is_empty());
    }
    let raw = run("cksum", &["--algorithm=sha256", "--raw"], b"abc", &[]);
    assert_eq!(code(&raw), 0);
    assert_eq!(raw.stdout.len(), 32);
    let tagged = run("cksum", &["--algorithm=sha256", "--tag"], b"abc", &[]);
    assert_eq!(code(&tagged), 0);
    assert!(out(&tagged).contains("SHA256 (-) = "));
    let untagged = run("cksum", &["--algorithm=sha256", "--untagged"], b"abc", &[]);
    assert_eq!(code(&untagged), 0);
    assert!(!out(&untagged).contains(" = "));
    let b64 = run("cksum", &["--algorithm=sha256", "--base64"], b"abc", &[]);
    assert_eq!(code(&b64), 0);
    assert_eq!(
        out(&b64),
        "SHA256 (-) = ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=\n"
    );
    assert_ne!(code(&run("cksum", &["--length", "7"], b"abc", &[])), 0);
    assert_ne!(
        code(&run(
            "cksum",
            &["--raw", "a", "b"],
            b"",
            &[("a", b"x"), ("b", b"y")]
        )),
        0
    );
    let zero = run("cksum", &["--zero"], b"abc", &[]);
    assert_eq!(code(&zero), 0);
    assert!(zero.stdout.ends_with(b"\0"));
    assert_eq!(code(&run("cksum", &["--help"], b"", &[])), 0);
    assert_eq!(code(&run("cksum", &["--version"], b"", &[])), 0);
    assert_ne!(code(&run("cksum", &["--not-a-real-option"], b"", &[])), 0);
}

#[test]
fn cksum_check_long_options_are_supported() {
    let tagged =
        out(&run("cksum", &["--algorithm=sha256", "--tag"], b"abc", &[])).replace("(-)", "(data)");
    let files = [("data", b"abc".as_slice()), ("SUMS", tagged.as_bytes())];
    for flag in ["--check", "--quiet", "--status"] {
        let output = if flag == "--check" {
            run("cksum", &["--algorithm=sha256", flag, "SUMS"], b"", &files)
        } else {
            run(
                "cksum",
                &["--algorithm=sha256", flag, "--check", "SUMS"],
                b"",
                &files,
            )
        };
        assert_eq!(code(&output), 0, "{flag}");
        if flag == "--status" {
            assert!(output.stdout.is_empty() && output.stderr.is_empty());
        }
    }
    let missing_then_valid = tagged.replace("data)", "absent)") + &tagged;
    let ignored = run(
        "cksum",
        &["--algorithm=sha256", "--ignore-missing", "--check", "SUMS"],
        b"",
        &[("data", b"abc"), ("SUMS", missing_then_valid.as_bytes())],
    );
    assert_eq!(code(&ignored), 0);
    let malformed = tagged.clone() + "malformed\n";
    assert_ne!(
        code(&run(
            "cksum",
            &["--algorithm=sha256", "--strict", "--check", "SUMS"],
            b"",
            &[("data", b"abc"), ("SUMS", malformed.as_bytes())]
        )),
        0
    );
    assert_eq!(
        code(&run(
            "cksum",
            &["--algorithm=sha256", "--warn", "--check", "SUMS"],
            b"",
            &[("data", b"abc"), ("SUMS", malformed.as_bytes())]
        )),
        0
    );
}

#[test]
fn sum_supports_bsd_and_sysv_and_operand_errors() {
    let bsd = run("sum", &["-r"], b"", &[("x", b"abc")]);
    let sysv = run("sum", &["-s"], b"", &[("x", b"abc")]);
    assert_eq!(code(&bsd), 0);
    assert_eq!(code(&sysv), 0);
    assert_ne!(out(&bsd), out(&sysv));
    assert_eq!(code(&run("sum", &["--sysv"], b"abc", &[])), 0);
    assert_eq!(code(&run("sum", &["--help"], b"", &[])), 0);
    assert_eq!(code(&run("sum", &["--version"], b"", &[])), 0);
    assert_ne!(code(&run("sum", &["missing-file"], b"", &[])), 0);
    assert_ne!(code(&run("sum", &["--unknown"], b"", &[])), 0);
}

#[test]
fn missing_operand_and_binary_nul_inputs_are_handled() {
    for name in HASHES {
        let o = run(name, &[], b"a\0b\n", &[]);
        assert_eq!(code(&o), 0, "{name} binary stdin");
        assert!(!o.stdout.is_empty());
        let m = run(name, &["definitely-absent"], b"", &[]);
        assert_ne!(code(&m), 0, "{name} missing file must report an error");
    }
    assert_ne!(code(&run("cksum", &["definitely-absent"], b"", &[])), 0);
}

#[test]
fn manifests_accept_horizontal_whitespace_and_tag_like_names() {
    let hash = digest("md5sum");
    for line in [
        format!(" {hash}  a (b) = c\n"),
        format!("\t{hash}\ta (b) = c\n"),
    ] {
        let output = run(
            "md5sum",
            &["-c", "list"],
            b"",
            &[("list", line.as_bytes()), ("a (b) = c", b"abc")],
        );
        assert_eq!(code(&output), 0, "{output:?}");
        assert_eq!(output.stdout, b"a (b) = c: OK\n");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn sysv_large_stream_matches_ubuntu_gnu() {
    if std::env::var("AGENTCMD_GNU_DIFF").as_deref() != Ok("1") {
        return;
    }
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let data = vec![255; 16_843_010];
    let mut child = Command::new("/usr/bin/sum")
        .arg("-s")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&data).unwrap();
    let expected = child.wait_with_output().unwrap();
    let actual = run("sum", &["-s"], &data, &[]);
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
}

#[test]
fn stdin_manifests_cannot_verify_the_same_stdin_as_a_file() {
    for name in HASHES {
        let empty = run(name, &[], b"", &[]);
        let output = run(name, &["-c"], &empty.stdout, &[]);
        assert_eq!(code(&output), 1, "{name}: {output:?}");
        assert!(output.stdout.is_empty());
        for mode in ["--binary", "--text"] {
            let line = format!("{}  data\n", digest(name));
            let output = run(
                name,
                &[mode, "-c", "list"],
                b"",
                &[("list", line.as_bytes()), ("data", b"abc")],
            );
            assert_eq!(code(&output), 1, "{name} {mode}: {output:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("meaningless"));
        }
    }
}
#[test]
fn bsd_sum_prints_filenames_only_with_multiple_operands() {
    let output = run("sum", &["data"], b"", &[("data", b"abc")]);
    assert_eq!(output.stdout, b"16556     1\n");
    let output = run("sum", &["-"], b"abc", &[]);
    assert_eq!(output.stdout, b"16556     1\n");
    let output = run("sum", &["-", "data"], b"abc", &[("data", b"abc")]);
    assert_eq!(output.stdout, b"16556     1 -\n16556     1 data\n");
    let output = run("sum", &["-s", "data"], b"", &[("data", b"abc")]);
    assert_eq!(output.stdout, b"294 1 data\n");
}
