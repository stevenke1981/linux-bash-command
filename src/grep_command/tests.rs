use super::files::{offset_width, read_glob_file};
use super::parse::parse;
use super::pattern::{build_regex, glob_to_regex};
use super::*;
use crate::common::regex::bre_to_regex;

fn bre(pattern: &str) -> ByteRegex {
    let translated = bre_to_regex(pattern).unwrap();
    ByteRegex::new(&translated).unwrap()
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|item| item.to_string()).collect()
}

fn options(list: &[&str]) -> Options {
    parse(&args(list)).unwrap().unwrap()
}

#[test]
fn bre_treats_bare_metacharacters_as_literals() {
    assert!(bre("a+b").is_match(b"a+b"));
    assert!(!bre("a+b").is_match(b"aab"));
    assert!(bre("f(x)|y").is_match(b"f(x)|y"));
    assert!(!bre("f(x)|y").is_match(b"y"));
}

#[test]
fn bre_escaped_operators_are_special() {
    assert!(bre(r"a\|b").is_match(b"b"));
    assert!(bre(r"\(ab\)\{2\}").is_match(b"abab"));
    assert!(bre(r"ab\+").is_match(b"abb"));
    assert!(bre(r"ab\?c").is_match(b"ac"));
}

#[test]
fn bre_anchors_and_leading_star_are_positional() {
    assert!(bre("*a").is_match(b"*a"));
    assert!(bre("a^b").is_match(b"a^b"));
    assert!(bre("a$b").is_match(b"a$b"));
    assert!(bre("^x$").is_match(b"x"));
    assert!(!bre("^x$").is_match(b"yx"));
}

#[test]
fn bre_bracket_backslash_is_literal() {
    assert!(bre(r"[\]").is_match(b"\\"));
    assert!(bre("[[:space:]]").is_match(b" "));
}

#[test]
fn bre_rejects_unsupported_constructs() {
    assert!(bre_to_regex(r"\(a\)\1").is_err());
    assert!(bre_to_regex("[a").is_err());
    assert!(bre_to_regex(r"a\<(b").is_err());
    assert!(bre_to_regex(r"\<word\>").is_ok());
}

#[test]
fn glob_supports_braces_classes_and_wildcards() {
    let rust = glob_to_regex("*.{rs,toml}").unwrap();
    assert!(rust.is_match("main.rs"));
    assert!(rust.is_match("Cargo.toml"));
    assert!(!rust.is_match("main.rsx"));
    let class = glob_to_regex("file[!0-9].txt").unwrap();
    assert!(class.is_match("fileA.txt"));
    assert!(!class.is_match("file1.txt"));
    assert!(glob_to_regex("a?c").unwrap().is_match("abc"));
}

#[test]
fn spans_skip_empty_matches() {
    let regex = ByteRegex::new("(a*)").unwrap();
    assert_eq!(spans(&regex, b"baab"), vec![(1, 3)]);
}

#[test]
fn null_data_lets_dot_match_newline_inside_a_record() {
    let plain = options(&["a.b"]);
    assert!(!build_regex(&plain).unwrap().is_match(b"a\nb"));
    let nul = options(&["-z", "a.b"]);
    assert!(build_regex(&nul).unwrap().is_match(b"a\nb"));
}

#[test]
fn parse_sets_record_and_column_flags() {
    let parsed = options(&["-zZbT", "--label=X", "pat", "f"]);
    assert!(parsed.zero);
    assert!(parsed.null);
    assert!(parsed.byte_offset);
    assert!(parsed.initial_tab);
    assert_eq!(parsed.label.as_deref(), Some("X"));
    assert_eq!(parsed.files, vec!["f".to_string()]);
    let long = options(&[
        "--null-data",
        "--null",
        "--byte-offset",
        "--initial-tab",
        "p",
    ]);
    assert!(long.zero && long.null && long.byte_offset && long.initial_tab);
}

#[test]
fn parse_directories_action_is_last_wins() {
    let skip_then_recurse = options(&["-d", "skip", "-r", "p"]);
    assert_eq!(skip_then_recurse.recurse, Recurse::Command);
    let recurse_then_skip = options(&["-r", "-d", "skip", "p"]);
    assert_eq!(recurse_then_skip.recurse, Recurse::Off);
    assert!(recurse_then_skip.skip_dirs);
    let read = options(&["-r", "-d", "read", "p"]);
    assert_eq!(read.recurse, Recurse::Off);
    assert!(!read.skip_dirs);
    let dereference = options(&["-R", "-d", "recurse", "p"]);
    assert_eq!(dereference.recurse, Recurse::All);
}

#[test]
fn parse_devices_values_and_errors() {
    assert_eq!(options(&["-D", "skip", "p"]).devices, Devices::Skip);
    assert_eq!(options(&["--devices=read", "p"]).devices, Devices::Read);
    assert_eq!(options(&["p"]).devices, Devices::Default);
    let error = parse(&args(&["-D", "bogus", "p"])).err().unwrap();
    assert_eq!(error, "unknown devices method");
}

#[test]
fn parse_directories_rejects_unknown_action() {
    let error = parse(&args(&["-d", "bogus", "p"])).err().unwrap();
    assert!(error.starts_with("invalid argument 'bogus' for '--directories'"));
}

#[test]
fn parse_attached_values_for_d_and_capital_d() {
    let parsed = options(&["-dskip", "-D", "read", "p"]);
    assert_eq!(parsed.recurse, Recurse::Off);
    assert!(parsed.skip_dirs);
    assert_eq!(parsed.devices, Devices::Read);
}

#[test]
fn offset_width_counts_digits_and_defaults_for_pipes() {
    assert_eq!(offset_width(Some(0)), 1);
    assert_eq!(offset_width(Some(2)), 1);
    assert_eq!(offset_width(Some(18)), 2);
    assert_eq!(offset_width(Some(1007)), 4);
    assert_eq!(offset_width(None), 18);
}

#[test]
fn read_glob_file_splits_lines_and_ignores_final_newline() {
    let path = std::env::temp_dir().join(format!("agentcmd-grep-globs-{}", std::process::id()));
    std::fs::write(&path, "*.txt\nb.log").unwrap();
    let globs = read_glob_file(path.to_str().unwrap());
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        globs.unwrap(),
        vec!["*.txt".to_string(), "b.log".to_string()]
    );
}

#[test]
fn read_glob_file_reports_missing_file() {
    let error = read_glob_file("no-such-agentcmd-globs.lst").err().unwrap();
    assert!(error.starts_with("no-such-agentcmd-globs.lst: "));
}
