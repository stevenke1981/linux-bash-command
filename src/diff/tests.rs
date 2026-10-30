//! Unit tests for the `diff` command's formats and option parsing.

use super::{parse_args, report_pair, Options, Style};

fn names() -> (String, String) {
    ("a".to_string(), "b".to_string())
}

fn times() -> (String, String) {
    ("T1".to_string(), "T2".to_string())
}

fn text_options() -> Options {
    Options::default()
}

fn unified_options() -> Options {
    Options {
        style: Style::Unified,
        style_set: true,
        ..Options::default()
    }
}

#[test]
fn unchanged_bytes_produce_no_output() {
    let (status, text) = report_pair(b"x\n", b"x\n", &names(), &times(), &text_options());
    assert_eq!(status, 0);
    assert!(text.is_empty());
}

#[test]
fn default_output_is_normal_format() {
    let (status, text) = report_pair(b"a\n", b"b\n", &names(), &times(), &text_options());
    assert_eq!(status, 1);
    assert_eq!(text, b"1c1\n< a\n---\n> b\n");
}

#[test]
fn whitespace_only_change_is_equal_under_w() {
    let options = Options {
        ignore_all_space: true,
        ..text_options()
    };
    let (status, _) = report_pair(b"a  b\n", b"a b\n", &names(), &times(), &options);
    assert_eq!(status, 0);
}

#[test]
fn strip_trailing_cr_treats_crlf_as_lf() {
    let options = Options {
        strip_trailing_cr: true,
        ..text_options()
    };
    let (status, _) = report_pair(b"x\r\n", b"x\n", &names(), &times(), &options);
    assert_eq!(status, 0);
    let (status, _) = report_pair(b"x\r\n", b"x\n", &names(), &times(), &text_options());
    assert_eq!(status, 1);
}

#[test]
fn unified_style_keeps_headers_and_context() {
    let (_, text) = report_pair(b"a\nb\n", b"a\nc\n", &names(), &times(), &unified_options());
    assert_eq!(
        String::from_utf8(text).unwrap(),
        "--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n"
    );
}

#[test]
fn context_style_uses_the_given_times() {
    let options = Options {
        style: Style::Context,
        style_set: true,
        context: 0,
        ..Options::default()
    };
    let (_, text) = report_pair(b"a\n", b"b\n", &names(), &times(), &options);
    assert_eq!(
        String::from_utf8(text).unwrap(),
        "*** a\tT1\n--- b\tT2\n***************\n*** 1 ****\n! a\n--- 1 ----\n! b\n"
    );
}

#[test]
fn conflicting_styles_are_rejected() {
    let args: Vec<String> = ["-u", "-c", "a", "b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(parse_args(&args).unwrap_err(), 2);
    let same: Vec<String> = ["-c", "-C", "2", "a", "b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (options, paths) = parse_args(&same).unwrap();
    assert_eq!(options.style, Style::Context);
    assert_eq!(options.context, 2);
    assert_eq!(paths, ["a", "b"]);
}

#[test]
fn width_must_be_positive() {
    let args: Vec<String> = ["-W", "0", "a", "b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(parse_args(&args).unwrap_err(), 2);
    let ok: Vec<String> = ["--width=40", "a", "b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(parse_args(&ok).unwrap().0.width, 40);
}
