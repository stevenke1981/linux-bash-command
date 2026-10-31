use crate::common::{run, Sandbox};

#[test]
fn wc_default() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "wc",
        &[],
        &[
            111, 110, 101, 32, 116, 119, 111, 10, 116, 104, 114, 101, 101, 10,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[
            32, 32, 32, 32, 32, 32, 50, 32, 32, 32, 32, 32, 32, 32, 51, 32, 32, 32, 32, 32, 32, 49,
            52, 10
        ]
    );
}
#[test]
fn wc_flag_c() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["-c"], &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 48, 10]);
}
#[test]
fn wc_flag_bytes() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "wc",
        &["--bytes"],
        &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 48, 10]);
}
#[test]
fn wc_flag_m() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["-m"], &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 48, 10]);
}
#[test]
fn wc_flag_chars() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "wc",
        &["--chars"],
        &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 48, 10]);
}
#[test]
fn wc_flag_l() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["-l"], &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 10]);
}
#[test]
fn wc_flag_lines() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "wc",
        &["--lines"],
        &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 10]);
}
#[test]
fn wc_flag_w() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["-w"], &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[52, 10]);
}
#[test]
fn wc_flag_words() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "wc",
        &["--words"],
        &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[52, 10]);
}
#[test]
fn wc_flag_max_short() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["-L"], &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 49, 10]);
}
#[test]
fn wc_flag_max_line_length() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "wc",
        &["--max-line-length"],
        &[97, 9, 98, 32, 99, 10, 108, 97, 115, 116],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 49, 10]);
}
#[test]
fn wc_multi() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97, 32, 98, 10]);
    sandbox.write("b", [99, 10]);
    let out = sandbox.run("wc", &["a", "b"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[
            49, 32, 50, 32, 52, 32, 97, 10, 49, 32, 49, 32, 50, 32, 98, 10, 50, 32, 51, 32, 54, 32,
            116, 111, 116, 97, 108, 10
        ]
    );
}
#[test]
fn wc_multi_bytes() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [49, 50, 51, 52, 53, 54, 55, 56, 57, 48]);
    sandbox.write("b", [49, 50]);
    let out = sandbox.run("wc", &["-c", "a", "b"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[49, 48, 32, 97, 10, 32, 50, 32, 98, 10, 49, 50, 32, 116, 111, 116, 97, 108, 10]
    );
}
#[test]
fn wc_partial() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &[], &[108, 97, 115, 116]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[
            32, 32, 32, 32, 32, 32, 48, 32, 32, 32, 32, 32, 32, 32, 49, 32, 32, 32, 32, 32, 32, 32,
            52, 10
        ]
    );
}
#[test]
fn wc_empty() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[
            32, 32, 32, 32, 32, 32, 48, 32, 32, 32, 32, 32, 32, 32, 48, 32, 32, 32, 32, 32, 32, 32,
            48, 10
        ]
    );
}
#[test]
fn wc_missing() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["missing"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn wc_bad_option() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["--oops"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn wc_controls() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "wc",
        &["-lwcL"],
        &[97, 13, 98, 11, 119, 111, 114, 100, 12, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[
            32, 32, 32, 32, 32, 32, 49, 32, 32, 32, 32, 32, 32, 32, 52, 32, 32, 32, 32, 32, 32, 49,
            49, 32, 32, 32, 32, 32, 32, 32, 53, 10
        ]
    );
}
#[test]
fn wc_files0() {
    let sandbox = Sandbox::new();
    sandbox.write("names", [97, 0, 98, 0]);
    sandbox.write("a", [97, 10]);
    sandbox.write("b", [98, 10, 10]);
    let out = sandbox.run("wc", &["--files0-from=names", "-l"], &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[49, 32, 97, 10, 50, 32, 98, 10, 51, 32, 116, 111, 116, 97, 108, 10]
    );
}
#[test]
fn wc_files0_stdin() {
    let sandbox = Sandbox::new();
    sandbox.write("a", [97, 98, 99]);
    let out = sandbox.run("wc", &["--files0-from=-", "-c"], &[97, 0]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[51, 32, 97, 10]);
}
#[test]
fn wc_stdin_twice() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("wc", &["-l", "-", "-"], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[
            32, 32, 32, 32, 32, 32, 50, 32, 45, 10, 32, 32, 32, 32, 32, 32, 48, 32, 45, 10, 32, 32,
            32, 32, 32, 32, 50, 32, 116, 111, 116, 97, 108, 10
        ]
    );
}
#[test]
fn wc_help() {
    let out = run("wc", &["--help"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
#[test]
fn wc_version() {
    let out = run("wc", &["--version"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
#[test]
fn wc_total_modes() {
    let s = Sandbox::new();
    s.write("a", b"a\n");
    s.write("b", b"b c\n");
    for (mode, expected) in [
        ("auto", b"1 1 2 a\n1 2 4 b\n2 3 6 total\n".as_slice()),
        ("always", b"1 1 2 a\n1 2 4 b\n2 3 6 total\n".as_slice()),
        ("only", b"2 3 6\n".as_slice()),
        ("never", b"1 1 2 a\n1 2 4 b\n".as_slice()),
    ] {
        let out = s.run("wc", &["--total", mode, "a", "b"], b"");
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, expected);
    }
}

#[test]
fn wc_empty_file_names_are_rejected() {
    for bytes in [b"\0".as_slice(), b"a\0\0"] {
        let sandbox = Sandbox::new();
        sandbox.write("names", bytes);
        let out = sandbox.run("wc", &["--files0-from=names"], b"");
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("zero-length file name"));
    }
}

#[test]
fn wc_utf8_and_invalid_bytes_follow_spec() {
    let out = run("wc", &["-m"], b"\xc3\xa9\xff\xf0");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"3\n");
}

#[test]
fn wc_files0_continues_after_empty_name() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"x");
    sandbox.write("b", b"y");
    sandbox.write("names", b"a\0\0b\0");
    let out = sandbox.run("wc", &["--files0-from=names", "-c"], b"");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"1 a\n1 b\n2 total\n");
}

#[test]
fn wc_files0_stdin_dash_is_invalid_and_still_counts_toward_total() {
    let sandbox = Sandbox::new();
    sandbox.write("a", b"abc");
    let out = sandbox.run("wc", &["--files0-from=-", "-c"], b"-\0a\0");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"3 a\n3 total\n");
}

// macOS APFS rejects non-UTF-8 file names with EILSEQ.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn wc_files0_preserves_non_utf8_name_for_open() {
    use std::os::unix::ffi::OsStringExt;
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.path.join(std::ffi::OsString::from_vec(vec![255])),
        b"abc",
    )
    .unwrap();
    sandbox.write("names", [255, 0]);
    let out = sandbox.run("wc", &["--files0-from=names", "-c"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"3 \xef\xbf\xbd\n");
}
