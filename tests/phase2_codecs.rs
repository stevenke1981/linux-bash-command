mod common;
use std::process::Output;

fn run(name: &str, args: &[&str], input: &[u8], files: &[(&str, &[u8])]) -> Output {
    let sandbox = common::Sandbox::new();
    for (path, bytes) in files {
        sandbox.write(path, bytes);
    }
    sandbox.run(name, args, input)
}

fn status(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn assert_help_version_errors(name: &str, selector: &[&str]) {
    assert_eq!(status(&run(name, &["--help"], b"", &[])), 0, "{name} help");
    assert_eq!(
        status(&run(name, &["--version"], b"", &[])),
        0,
        "{name} version"
    );
    let mut args = selector.to_vec();
    args.push("--unknown-option");
    assert_ne!(
        status(&run(name, &args, b"abc", &[])),
        0,
        "{name} unknown option"
    );
    let mut missing = selector.to_vec();
    missing.push("definitely-absent");
    assert_ne!(status(&run(name, &missing, b"", &[])), 0);
}

#[test]
fn base64_encode_decode_wrap_and_error_paths() {
    let encoded = run("base64", &[], b"foobar", &[]);
    assert_eq!(status(&encoded), 0);
    assert_eq!(encoded.stdout, b"Zm9vYmFy\n");

    let empty = run("base64", &[], b"", &[]);
    assert_eq!(status(&empty), 0);
    assert!(empty.stdout.is_empty());

    let file = run("base64", &["input"], b"", &[("input", b"foobar")]);
    assert_eq!(file.stdout, b"Zm9vYmFy\n");

    for flag in ["-d", "--decode"] {
        let decoded = run("base64", &[flag], b"Zm9vYmFy\n", &[]);
        assert_eq!(status(&decoded), 0, "{flag}");
        assert_eq!(decoded.stdout, b"foobar");
    }
    let file_decode = run(
        "base64",
        &["--decode", "input"],
        b"",
        &[("input", b"Zm9vYmFy\n")],
    );
    assert_eq!(file_decode.stdout, b"foobar");

    let wrap = run("base64", &["--wrap=4"], b"foobar", &[]);
    assert_eq!(wrap.stdout, b"Zm9v\nYmFy\n");
    let unwrapped = run("base64", &["--wrap=0"], b"foobar", &[]);
    assert_eq!(unwrapped.stdout, b"Zm9vYmFy");

    let garbage = run(
        "base64",
        &["--decode", "--ignore-garbage"],
        b"Z!m9vYmFy\n",
        &[],
    );
    assert_eq!(status(&garbage), 0);
    assert_eq!(garbage.stdout, b"foobar");

    for invalid in [b"Zm9v=".as_slice(), b"Zm9v!".as_slice()] {
        assert_ne!(status(&run("base64", &["--decode"], invalid, &[])), 0);
    }
    assert_ne!(status(&run("base64", &["--wrap=-1"], b"abc", &[])), 0);
    assert_ne!(
        status(&run(
            "base64",
            &["one", "two"],
            b"",
            &[("one", b"a"), ("two", b"b")]
        )),
        0
    );
    assert_help_version_errors("base64", &[]);
}

#[test]
fn base32_encode_decode_wrap_and_error_paths() {
    let encoded = run("base32", &[], b"foobar", &[]);
    assert_eq!(status(&encoded), 0);
    assert_eq!(encoded.stdout, b"MZXW6YTBOI======\n");

    let empty = run("base32", &[], b"", &[]);
    assert_eq!(status(&empty), 0);
    assert!(empty.stdout.is_empty());

    let file = run("base32", &["input"], b"", &[("input", b"foobar")]);
    assert_eq!(file.stdout, b"MZXW6YTBOI======\n");

    for flag in ["-d", "--decode"] {
        let decoded = run("base32", &[flag], b"MZXW6YTBOI======\n", &[]);
        assert_eq!(status(&decoded), 0, "{flag}");
        assert_eq!(decoded.stdout, b"foobar");
    }
    let file_decode = run(
        "base32",
        &["--decode", "input"],
        b"",
        &[("input", b"MZXW6YTBOI======\n")],
    );
    assert_eq!(file_decode.stdout, b"foobar");

    let wrap = run("base32", &["--wrap=4"], b"foobar", &[]);
    assert_eq!(wrap.stdout, b"MZXW\n6YTB\nOI==\n====\n");
    let unwrapped = run("base32", &["--wrap=0"], b"foobar", &[]);
    assert_eq!(unwrapped.stdout, b"MZXW6YTBOI======");

    let garbage = run(
        "base32",
        &["--decode", "--ignore-garbage"],
        b"MZ!XW6YTBOI======\n",
        &[],
    );
    assert_eq!(status(&garbage), 0);
    assert_eq!(garbage.stdout, b"foobar");

    for invalid in [
        b"MZXW6YTBOI=====".as_slice(),
        b"MZXW6YTBOI======!".as_slice(),
    ] {
        assert_ne!(status(&run("base32", &["--decode"], invalid, &[])), 0);
    }
    assert_ne!(status(&run("base32", &["--wrap=-1"], b"abc", &[])), 0);
    assert_ne!(
        status(&run(
            "base32",
            &["one", "two"],
            b"",
            &[("one", b"a"), ("two", b"b")]
        )),
        0
    );
    assert_help_version_errors("base32", &[]);
}

#[test]
fn basenc_all_eight_formats_round_trip() {
    let z85_raw = [0x86, 0x4f, 0xd2, 0x6f, 0xb5, 0x59, 0xf7, 0x5b];
    let samples: [(&str, &[u8], &[u8]); 8] = [
        ("--base64", b"foobar", b"Zm9vYmFy"),
        ("--base64url", b"foobar", b"Zm9vYmFy"),
        ("--base32", b"foobar", b"MZXW6YTBOI======"),
        ("--base32hex", b"foobar", b"CPNMUOJ1E8======"),
        ("--base16", b"foobar", b"666F6F626172"),
        ("--base2msbf", b"A", b"01000001"),
        ("--base2lsbf", b"A", b"10000010"),
        ("--z85", &z85_raw, b"HelloWorld"),
    ];
    for (selector, bytes, expected) in samples {
        let encoded = run("basenc", &[selector], bytes, &[]);
        assert_eq!(status(&encoded), 0, "{selector} encode");
        let mut expected_line = expected.to_vec();
        expected_line.push(b'\n');
        assert_eq!(encoded.stdout, expected_line, "{selector} encoding");

        let decoded = run("basenc", &[selector, "--decode"], expected, &[]);
        assert_eq!(status(&decoded), 0, "{selector} decode");
        assert_eq!(decoded.stdout, bytes, "{selector} round trip");
    }

    let invalid_length = run("basenc", &["--z85"], b"abc", &[]);
    assert_ne!(status(&invalid_length), 0);
    let file_encoded = run(
        "basenc",
        &["--base64", "input"],
        b"",
        &[("input", b"foobar")],
    );
    assert_eq!(status(&file_encoded), 0);
    assert_eq!(file_encoded.stdout, b"Zm9vYmFy\n");
    let file_decoded = run(
        "basenc",
        &["--base64", "--decode", "input"],
        b"",
        &[("input", b"Zm9vYmFy\n")],
    );
    assert_eq!(status(&file_decoded), 0);
    assert_eq!(file_decoded.stdout, b"foobar");
    let garbage = run(
        "basenc",
        &["--base64", "--decode", "--ignore-garbage"],
        b"Zm!9vYmFy\n",
        &[],
    );
    assert_eq!(status(&garbage), 0);
    assert_eq!(garbage.stdout, b"foobar");
    assert_ne!(
        status(&run("basenc", &["--base64", "--decode"], b"Zm9v=", &[])),
        0
    );
    assert_eq!(
        run("basenc", &["--base64", "--wrap=0"], b"foobar", &[]).stdout,
        b"Zm9vYmFy"
    );
    assert_eq!(
        run("basenc", &["--base64", "--wrap=4"], b"foobar", &[]).stdout,
        b"Zm9v\nYmFy\n"
    );
    assert_ne!(
        status(&run("basenc", &["--base64", "missing"], b"", &[])),
        0
    );
    assert_ne!(
        status(&run(
            "basenc",
            &["--base64", "one", "two"],
            b"",
            &[("one", b"a"), ("two", b"b")]
        )),
        0
    );
    assert_help_version_errors("basenc", &[]);
}

#[test]
fn base32_rejects_incomplete_groups_without_emitting_their_bytes() {
    for (input, expected) in [
        ("MY", ""),
        ("MZXQ=", ""),
        ("MZXW6YTBMY", "fooba"),
        ("MY======MY", "f"),
    ] {
        let output = run("base32", &["-d"], input.as_bytes(), &[]);
        assert_eq!(status(&output), 1);
        assert_eq!(output.stdout, expected.as_bytes());
    }
}
#[test]
fn basenc_uses_the_last_encoding_selector() {
    let output = run("basenc", &["--base32", "--base64", "--wrap=0"], b"f", &[]);
    assert_eq!(status(&output), 0);
    assert_eq!(output.stdout, b"Zg==");
}

#[test]
fn padding_remains_invalid_for_unpadded_encodings_with_ignore_garbage() {
    for (encoding, input, expected) in [
        ("--base16", "FF=", b"\xff".as_slice()),
        ("--base2msbf", "00000000=", b"\0"),
        ("--base2lsbf", "00000000=", b"\0"),
    ] {
        let output = run("basenc", &[encoding, "-di"], input.as_bytes(), &[]);
        assert_eq!(status(&output), 1);
        assert_eq!(output.stdout, expected);
    }
}
