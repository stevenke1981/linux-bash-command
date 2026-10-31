//! `grep --color` output: escape layout, GREP_COLORS overrides, and the
//! environment rules shared with every other command (AGENTCMD_COLOR, NO_COLOR).

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A unique temporary directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "agentcmd-color-grep-{}-{nanos}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs grep with the color environment cleared, plus `envs`, feeding `stdin`.
fn run(args: &[&str], envs: &[(&str, &str)], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_grep"))
        .args(args)
        .env_remove("AGENTCMD_COLOR")
        .env_remove("NO_COLOR")
        .env_remove("GREP_COLORS")
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// One colored span in GNU layout.
fn painted(sgr: &str, text: &str) -> String {
    format!("\x1b[{sgr}m\x1b[K{text}\x1b[m\x1b[K")
}

const MATCH: &str = "01;31";

#[test]
fn piped_default_output_has_no_escapes() {
    let out = run(&["-n", "bar"], &[], b"foo bar\nbaz\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "1:foo bar\n");
    assert!(!text(&out).contains('\x1b'));
}

#[test]
fn always_highlights_the_match() {
    let out = run(&["--color=always", "bar"], &[], b"foo bar\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "foo \x1b[01;31m\x1b[Kbar\x1b[m\x1b[K\n");
}

#[test]
fn names_numbers_and_separators_get_their_colors() {
    let out = run(&["-n", "-H", "--color=always", "bar"], &[], b"foo bar\n");
    let expected = format!(
        "{}{}{}{}foo {}\n",
        painted("35", "(standard input)"),
        painted("36", ":"),
        painted("32", "1"),
        painted("36", ":"),
        painted(MATCH, "bar"),
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), expected);
}

#[test]
fn only_matching_prints_each_match_colored() {
    let out = run(&["-o", "--color=always", "bar"], &[], b"bar x bar\n");
    assert_eq!(out.status.code(), Some(0));
    let one = painted(MATCH, "bar") + "\n";
    assert_eq!(text(&out), one.repeat(2));
}

#[test]
fn grep_colors_overrides_match_color() {
    let out = run(
        &["--color=always", "bar"],
        &[("GREP_COLORS", "ms=01;32")],
        b"foo bar\n",
    );
    assert_eq!(text(&out), "foo \x1b[01;32m\x1b[Kbar\x1b[m\x1b[K\n");
}

#[test]
fn grep_colors_overrides_file_and_line_number_colors() {
    let out = run(
        &["-n", "-H", "--color=always", "bar"],
        &[("GREP_COLORS", "fn=34:ln=33")],
        b"bar\n",
    );
    let expected = format!(
        "{}{}{}{}{}",
        painted("34", "(standard input)"),
        painted("36", ":"),
        painted("33", "1"),
        painted("36", ":"),
        painted(MATCH, "bar") + "\n",
    );
    assert_eq!(text(&out), expected);
}

#[test]
fn no_color_variable_disables_auto_color() {
    let out = run(&["bar"], &[("NO_COLOR", "1")], b"foo bar\n");
    assert_eq!(text(&out), "foo bar\n");
    assert!(!text(&out).contains('\x1b'));
}

#[test]
fn explicit_always_wins_over_no_color() {
    let out = run(
        &["--color=always", "bar"],
        &[("NO_COLOR", "1")],
        b"foo bar\n",
    );
    assert_eq!(text(&out), "foo \x1b[01;31m\x1b[Kbar\x1b[m\x1b[K\n");
}

#[test]
fn invalid_when_is_a_usage_error() {
    let out = run(&["--color=bogus", "bar"], &[], b"bar\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn file_names_in_list_mode_are_colored() {
    let dir = TempDir::new();
    let file = dir.0.join("hit.txt");
    fs::write(&file, b"bar\n").unwrap();
    let name = file.to_string_lossy().into_owned();
    // grep prints names with forward slashes on Windows.
    let shown = if cfg!(windows) {
        name.replace('\\', "/")
    } else {
        name.clone()
    };
    let out = run(&["-l", "--color=always", "bar", &name], &[], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), format!("{}\n", painted("35", &shown)));
}

#[test]
fn count_prefix_is_colored() {
    let out = run(&["-c", "-H", "--color=always", "bar"], &[], b"bar\nbar\n");
    let expected = format!(
        "{}{}2\n",
        painted("35", "(standard input)"),
        painted("36", ":")
    );
    assert_eq!(text(&out), expected);
}

#[test]
fn null_after_name_is_not_colored() {
    let out = run(&["-Z", "-H", "--color=always", "-l", "bar"], &[], b"bar\n");
    assert_eq!(
        text(&out),
        format!("{}\0", painted("35", "(standard input)"))
    );
}

#[test]
fn inverted_selection_has_no_match_highlight() {
    let out = run(&["-v", "--color=always", "bar"], &[], b"bar\nbaz\n");
    assert_eq!(text(&out), "baz\n");
}

#[test]
fn quiet_prints_nothing_even_when_colored() {
    let out = run(&["-q", "--color=always", "bar"], &[], b"bar\n");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn group_separator_is_colored_and_context_is_plain() {
    let out = run(
        &["-A1", "--color=always", "bar"],
        &[],
        b"bar\nx\ny\nz\nbar\n",
    );
    let one = painted(MATCH, "bar") + "\n";
    let expected = format!("{one}x\n{}\n{one}", painted("36", "--"));
    assert_eq!(text(&out), expected);
}

#[test]
fn context_lines_with_matches_are_not_highlighted() {
    // Under -v the matching line is printed as context and must stay plain.
    let out = run(&["-v", "-A1", "--color=always", "bar"], &[], b"x\nbar\n");
    assert_eq!(text(&out), "x\nbar\n");
}
