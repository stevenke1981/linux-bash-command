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
fn yes_preserves_second_terminator_operand() {
    let (out, status) = common::limited("yes", &["--", "--"], 6);
    assert_eq!(out, b"--\n--\n");
    assert_eq!(status, 0);
}
