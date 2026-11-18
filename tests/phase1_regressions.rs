mod common;
#[test]
fn printf_quotes_mixed_shell_characters() {
    let out = common::run("printf", &["%q", "a'b$c"], b"");
    assert_eq!(out.stdout, b"'a'\\''b$c'");
    assert!(out.status.success());
}
#[test]
fn expand_initial_stops_after_backspace() {
    let out = common::run("expand", &["-i"], b"\x08\tX\n");
    assert_eq!(out.stdout, b"\x08\tX\n");
    assert!(out.status.success());
}
#[test]
fn legacy_width_does_not_rewrite_filename_after_double_dash() {
    let sb = common::Sandbox::new();
    sb.write("-4", b"\tX\n");
    assert_eq!(sb.run("expand", &["--", "-4"], b"").stdout, b"        X\n");
    assert_eq!(sb.run("fold", &["--", "-4"], b"").stdout, b"\tX\n");
}
#[test]
fn required_suffix_owns_help_token() {
    assert_eq!(
        common::run("basename", &["-s", "--help", "file--help"], b"").stdout,
        b"file\n"
    );
}
#[test]
fn tac_seek_blocks_preserve_crossing_separator() {
    let sb = common::Sandbox::new();
    let mut data = vec![b'a'; 65535];
    data.extend_from_slice(b"::middle::end");
    sb.write("data", &data);
    let mut expected = b"endmiddle::".to_vec();
    expected.extend_from_slice(&vec![b'a'; 65535]);
    expected.extend_from_slice(b"::");
    let out = sb.run("tac", &["-s", "::", "data"], b"");
    assert!(out.status.success());
    assert_eq!(out.stdout, expected);
}
#[test]
fn tac_overlapping_literal_separators() {
    let sb = common::Sandbox::new();
    sb.write("data", b"XaaaY");
    for (args, expected) in [
        (&["-s", "aa"][..], &b"YXaaa"[..]),
        (&["-b", "-s", "aa"][..], &b"aaYXa"[..]),
    ] {
        let input = sb.run("tac", args, b"XaaaY");
        assert!(input.status.success(), "{input:?}");
        assert_eq!(input.stdout, expected);
        let file_args = [args, &["data"]].concat();
        let file = sb.run("tac", &file_args, b"");
        assert!(file.status.success(), "{file:?}");
        assert_eq!(file.stdout, expected);
    }

    sb.write("data", b"aaa");
    let file = sb.run(
        "timeout",
        &["5", common::binary("tac"), "-b", "-s", "aa", "data"],
        b"",
    );
    assert!(file.status.success(), "{file:?}");
    assert_eq!(file.stdout, b"aaa");
}

#[test]
fn yes_preserves_second_terminator_operand() {
    let (out, status) = common::limited("yes", &["--", "--"], 6);
    assert_eq!(out, b"--\n--\n");
    assert_eq!(status, 0);
}
