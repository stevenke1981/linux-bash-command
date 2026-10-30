//! Compatibility tests for the `rg` subset (ripgrep flags used by coding agents).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-rg-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn rg(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rg"))
            .current_dir(&self.0)
            .env("NO_PROXY", "127.0.0.1,localhost")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.rg(args);
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project() -> Fixture {
    let f = Fixture::new();
    f.write("a.txt", "hello world\nhello again\n");
    f.write("b.txt", "nothing here\n");
    f.write("src/main.rs", "fn main() {\n    println!(\"hello\");\n}\n");
    f.write("notes.md", "hello from markdown\n");
    f
}

#[test]
fn files_with_matches_lists_each_matching_file_once() {
    let f = project();
    let out = f.stdout(&["-l", "hello", ".", "--sort=path"]);
    assert_eq!(out, "./a.txt\n./notes.md\n./src/main.rs\n");
}

#[test]
fn files_without_match_lists_files_that_do_not_match() {
    let f = project();
    let out = f.stdout(&["--files-without-match", "hello", ".", "--sort=path"]);
    assert_eq!(out, "./b.txt\n");
}

#[test]
fn count_reports_per_file_and_skips_files_without_matches() {
    let f = project();
    let out = f.stdout(&["-c", "hello", "a.txt", "b.txt"]);
    assert_eq!(out, "a.txt:2\n");
    let out = f.stdout(&["--count-matches", "l", "a.txt"]);
    assert_eq!(out, "5\n");
}

#[test]
fn quiet_exits_zero_without_output_and_one_without_match() {
    let f = project();
    let hit = f.rg(&["-q", "hello", "a.txt"]);
    assert_eq!(hit.status.code(), Some(0));
    assert!(hit.stdout.is_empty());
    assert_eq!(f.rg(&["-q", "absent", "a.txt"]).status.code(), Some(1));
}

#[test]
fn context_uses_dash_separators_and_group_breaks() {
    let f = Fixture::new();
    f.write("f.txt", "1\nfind\n3\n4\n5\nfind\n7\n");
    let out = f.stdout(&["-C1", "find", "f.txt"]);
    assert_eq!(out, "1-1\n2:find\n3-3\n--\n5-5\n6:find\n7-7\n");
    let after = f.stdout(&["-A1", "find", "f.txt"]);
    assert_eq!(after, "2:find\n3-3\n--\n6:find\n7-7\n");
}

#[test]
fn type_filter_selects_and_excludes_by_file_type() {
    let f = project();
    let out = f.stdout(&["-t", "rust", "hello", "."]);
    assert_eq!(out, "./src/main.rs:2:    println!(\"hello\");\n");
    let out = f.stdout(&["-T", "rust", "-l", "hello", ".", "--sort=path"]);
    assert_eq!(out, "./a.txt\n./notes.md\n");
    let out = f.rg(&["-t", "nosuchtype", "hello", "."]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nosuchtype"));
}

#[test]
fn type_list_describes_built_in_types() {
    let f = project();
    let out = f.stdout(&["--type-list"]);
    assert!(out.lines().any(|line| line == "rust: *.rs"), "{out}");
    assert!(out.lines().any(|line| line.starts_with("py: ")), "{out}");
}

#[test]
fn glob_include_and_negated_glob_filter_paths() {
    let f = project();
    let out = f.stdout(&["-g", "*.rs", "-l", "hello", ".", "--sort=path"]);
    assert_eq!(out, "./src/main.rs\n");
    let out = f.stdout(&["-g", "!*.md", "-l", "hello", ".", "--sort=path"]);
    assert_eq!(out, "./a.txt\n./src/main.rs\n");
}

#[test]
fn files_sort_by_path_and_strip_dot_slash_without_path_argument() {
    let f = project();
    let out = f.stdout(&["--files", "--sort=path"]);
    assert_eq!(out, "a.txt\nb.txt\nnotes.md\nsrc/main.rs\n");
    let out = f.stdout(&["--files", "--sort=path", "-g", "*.rs"]);
    assert_eq!(out, "src/main.rs\n");
}

#[test]
fn multiline_pattern_matches_across_lines_only_with_u_flag() {
    let f = Fixture::new();
    f.write("m.txt", "foo\nbar\nbaz\n");
    let out = f.stdout(&["-U", r"foo\nbar", "m.txt"]);
    assert_eq!(out, "1:foo\n2:bar\n");
    let out = f.stdout(&["-U", "-o", r"o\nb", "m.txt"]);
    assert_eq!(out, "1:o\nb\n");
    let out = f.rg(&[r"foo\nbar", "m.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("-U"));
}

#[test]
fn word_option_requires_whole_word_matches() {
    let f = Fixture::new();
    f.write("w.txt", "cat\ncatalog\nthe cat sat\n");
    assert_eq!(f.stdout(&["-w", "cat", "w.txt"]), "1:cat\n3:the cat sat\n");
}

#[test]
fn only_matching_prints_each_match_with_its_line_number() {
    let f = Fixture::new();
    f.write("o.txt", "a1 b22\nno digits\n3\n");
    assert_eq!(f.stdout(&["-o", "[0-9]+", "o.txt"]), "1:1\n1:22\n3:3\n");
}

#[test]
fn smart_case_ignores_case_unless_pattern_has_uppercase() {
    let f = Fixture::new();
    f.write("s.txt", "Hello\nhello\nHELLO\n");
    assert_eq!(f.stdout(&["-S", "hello", "s.txt"]).lines().count(), 3);
    assert_eq!(f.stdout(&["-S", "Hello", "s.txt"]), "1:Hello\n");
    assert_eq!(f.stdout(&["-s", "-i", "hello", "s.txt"]).lines().count(), 3);
}

#[test]
fn max_columns_replaces_long_lines_with_a_notice() {
    let f = Fixture::new();
    let long = format!("{} needle", "x".repeat(50));
    f.write("long.txt", &format!("short needle\n{long}\n"));
    let out = f.stdout(&["--max-columns", "20", "needle", "long.txt"]);
    assert_eq!(
        out,
        "1:short needle\n2:[Omitted long line with 1 matches]\n"
    );
}

#[test]
fn output_paths_use_forward_slashes() {
    let f = Fixture::new();
    f.write("a/b/c.txt", "hit\n");
    let out = f.stdout(&["hit", "."]);
    assert_eq!(out, "./a/b/c.txt:1:hit\n");
    assert!(!out.contains('\\'));
    let out = f.stdout(&["hit", "a/b"]);
    assert_eq!(out, "a/b/c.txt:1:hit\n");
}

#[test]
fn bundled_short_flags_and_attached_values_work() {
    let f = project();
    assert_eq!(f.stdout(&["-inl", "HELLO", "a.txt"]), "a.txt\n");
    assert_eq!(f.stdout(&["-m1", "hello", "a.txt"]), "1:hello world\n");
    assert_eq!(f.stdout(&["-uc", "hello", "a.txt"]), "2\n");
}

#[test]
fn column_pattern_file_and_multiple_patterns() {
    let f = Fixture::new();
    f.write("p.txt", "alpha\nbeta\ngamma\n");
    f.write("pats.txt", "alpha\ngamma\n");
    assert_eq!(
        f.stdout(&["-f", "pats.txt", "--column", "p.txt"]),
        "1:1:alpha\n3:1:gamma\n"
    );
    assert_eq!(
        f.stdout(&["-e", "beta", "-e", "alpha", "-n", "p.txt"]),
        "1:alpha\n2:beta\n"
    );
}

#[test]
fn invert_and_max_count_select_expected_lines() {
    let f = Fixture::new();
    f.write("i.txt", "keep\ndrop\nkeep\n");
    assert_eq!(f.stdout(&["-v", "drop", "i.txt"]), "1:keep\n3:keep\n");
    assert_eq!(f.stdout(&["-m", "1", "keep", "i.txt"]), "1:keep\n");
}

#[test]
fn null_separator_and_heading_layouts() {
    let f = project();
    let out = f.stdout(&["-0", "-l", "hello", "a.txt"]);
    assert_eq!(out, "a.txt\0");
    let out = f.stdout(&["-H", "--heading", "-n", "hello", "a.txt"]);
    assert_eq!(out, "a.txt\n1:hello world\n2:hello again\n");
}

#[test]
fn invalid_pattern_and_missing_path_exit_two() {
    let f = project();
    assert_eq!(f.rg(&["[", "a.txt"]).status.code(), Some(2));
    assert_eq!(f.rg(&["hello", "absent"]).status.code(), Some(2));
    assert_eq!(f.rg(&["hello", "b.txt"]).status.code(), Some(1));
}

#[test]
fn unsupported_flags_fail_with_clear_errors() {
    let f = project();
    let out = f.rg(&["-z", "hello", "a.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not supported"));
    let out = f.rg(&["--json", "hello", "a.txt"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn ignore_flags_control_hidden_and_ignored_files() {
    let f = Fixture::new();
    f.write(".hidden.txt", "needle\n");
    f.write("visible.txt", "needle\n");
    assert_eq!(f.stdout(&["-l", "needle", "."]), "./visible.txt\n");
    assert_eq!(
        f.stdout(&["-l", "--hidden", "needle", ".", "--sort=path"]),
        "./.hidden.txt\n./visible.txt\n"
    );
    assert_eq!(
        f.stdout(&["-l", "-uu", "needle", ".", "--sort=path"]),
        "./.hidden.txt\n./visible.txt\n"
    );
}
