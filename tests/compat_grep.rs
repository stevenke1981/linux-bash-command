//! GNU grep compatibility checks for the `grep` binary.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-grep-compat-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &[u8]) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn grep(&self, args: &[&str]) -> Output {
        self.grep_stdin(args, b"")
    }

    fn grep_stdin(&self, args: &[&str], stdin: &[u8]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_grep"))
            .current_dir(&self.0)
            .args(args)
            .stdin(if stdin.is_empty() {
                Stdio::null()
            } else {
                Stdio::piped()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if !stdin.is_empty() {
            if let Err(error) = child.stdin.take().unwrap().write_all(stdin) {
                assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
            }
        }
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

#[test]
fn color_options_are_accepted_without_color_output() {
    let f = Fixture::new();
    f.write("a.txt", b"alpha\nbeta\n");
    for flag in [
        "--color=auto",
        "--color",
        "--colour=never",
        "--color=always",
    ] {
        let out = f.grep(&[flag, "beta", "a.txt"]);
        assert_eq!(out.status.code(), Some(0), "{flag}");
        assert_eq!(text(&out), "beta\n", "{flag}");
    }
    let out = f.grep(&["--color=sometimes", "beta", "a.txt"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn recursive_search_prints_forward_slash_paths() {
    let f = Fixture::new();
    f.write("top.txt", b"hit\n");
    f.write("sub/a.txt", b"x\nhit\n");
    let out = f.grep(&["-rn", "hit", "."]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "./sub/a.txt:2:hit\n./top.txt:1:hit\n");
}

#[test]
fn recursive_search_without_operand_omits_dot_prefix() {
    let f = Fixture::new();
    f.write("top.txt", b"hit\n");
    f.write("sub/a.txt", b"hit\n");
    let out = f.grep(&["-r", "hit"]);
    assert_eq!(text(&out), "sub/a.txt:hit\ntop.txt:hit\n");
}

#[test]
fn include_glob_limits_recursive_files() {
    let f = Fixture::new();
    f.write("lib.rs", b"hit\n");
    f.write("Cargo.toml", b"hit\n");
    f.write("sub/mod.rs", b"hit\n");
    let out = f.grep(&["-r", "--include=*.rs", "hit", "."]);
    assert_eq!(text(&out), "./lib.rs:hit\n./sub/mod.rs:hit\n");
    let out = f.grep(&["-r", "--include=*.{rs,toml}", "hit"]);
    assert_eq!(text(&out), "Cargo.toml:hit\nlib.rs:hit\nsub/mod.rs:hit\n");
}

#[test]
fn exclude_dir_skips_named_directories() {
    let f = Fixture::new();
    f.write("target/x.rs", b"hit\n");
    f.write("src/y.rs", b"hit\n");
    let out = f.grep(&["-r", "--exclude-dir=target", "hit"]);
    assert_eq!(text(&out), "src/y.rs:hit\n");
    let out = f.grep(&["-r", "--exclude-dir", "target", "hit"]);
    assert_eq!(text(&out), "src/y.rs:hit\n");
}

#[test]
fn exclude_glob_skips_matching_files() {
    let f = Fixture::new();
    f.write("keep.log", b"hit\n");
    f.write("skip.tmp", b"hit\n");
    let out = f.grep(&["-r", "--exclude=*.tmp", "hit", "."]);
    assert_eq!(text(&out), "./keep.log:hit\n");
}

#[test]
fn basic_regex_alternation_and_literal_bars() {
    let f = Fixture::new();
    f.write("a.txt", b"alpha\nbeta\ngamma\n");
    let out = f.grep(&[r"alpha\|gamma", "a.txt"]);
    assert_eq!(text(&out), "alpha\ngamma\n");
    f.write("bar.txt", b"a|b\nb\n");
    let out = f.grep(&["a|b", "bar.txt"]);
    assert_eq!(text(&out), "a|b\n", "bare | is literal in BRE");
}

#[test]
fn extended_regex_alternation() {
    let f = Fixture::new();
    f.write("a.txt", b"alpha\nbeta\ngamma\n");
    let out = f.grep(&["-E", "alpha|gamma", "a.txt"]);
    assert_eq!(text(&out), "alpha\ngamma\n");
    let out = f.grep(&["-iE", "ALPHA|GAMMA", "a.txt"]);
    assert_eq!(text(&out), "alpha\ngamma\n");
    // Bundled -n is line numbers, so -inE prefixes them.
    let out = f.grep(&["-inE", "ALPHA|GAMMA", "a.txt"]);
    assert_eq!(text(&out), "1:alpha\n3:gamma\n");
}

#[test]
fn word_match_requires_word_boundaries() {
    let f = Fixture::new();
    f.write("w.txt", b"hit\nhits\nxhit hit\n");
    let out = f.grep(&["-w", "hit", "w.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "hit\nxhit hit\n");
}

#[test]
fn only_matching_prints_each_match_on_its_own_line() {
    let f = Fixture::new();
    f.write("o.txt", b"a1 a22 b\nzz\n");
    let out = f.grep(&["-o", "a[0-9]*", "o.txt"]);
    assert_eq!(text(&out), "a1\na22\n");
    let out = f.grep(&["-on", "a[0-9]*", "o.txt"]);
    assert_eq!(text(&out), "1:a1\n1:a22\n");
}

#[test]
fn files_with_and_without_matches() {
    let f = Fixture::new();
    f.write("one.txt", b"hit\n");
    f.write("two.txt", b"miss\n");
    let out = f.grep(&["-l", "hit", "one.txt", "two.txt"]);
    assert_eq!(text(&out), "one.txt\n");
    let out = f.grep(&["-L", "hit", "one.txt", "two.txt"]);
    assert_eq!(text(&out), "two.txt\n");
    assert_eq!(out.status.code(), Some(0));
    let out = f.grep(&["-L", "hit", "one.txt"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn quiet_exits_on_first_match() {
    let f = Fixture::new();
    f.write("q.txt", b"hit\n");
    let out = f.grep(&["-q", "hit", "q.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let out = f.grep(&["-q", "nope", "q.txt"]);
    assert_eq!(out.status.code(), Some(1));
    // A match before a missing file still gives 0 under -q.
    let out = f.grep(&["-q", "hit", "q.txt", "missing.txt"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn count_with_recursion_reports_every_file() {
    let f = Fixture::new();
    f.write("a.txt", b"hit\nhit\n");
    f.write("c.txt", b"none\n");
    f.write("sub/b.txt", b"hit\n");
    let out = f.grep(&["-rc", "hit", "."]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "./a.txt:2\n./c.txt:0\n./sub/b.txt:1\n");
}

#[test]
fn context_groups_are_separated_by_double_dash() {
    let f = Fixture::new();
    f.write("ctx.txt", b"one\nhit\nthree\nfour\nfive\nhit\nseven\n");
    let out = f.grep(&["-n", "-C1", "hit", "ctx.txt"]);
    assert_eq!(
        text(&out),
        "1-one\n2:hit\n3-three\n--\n5-five\n6:hit\n7-seven\n"
    );
    let out = f.grep(&["-A1", "hit", "ctx.txt"]);
    assert_eq!(text(&out), "hit\nthree\n--\nhit\nseven\n");
    let out = f.grep(&["-n", "-B1", "hit", "ctx.txt"]);
    assert_eq!(text(&out), "1-one\n2:hit\n--\n5-five\n6:hit\n");
}

#[test]
fn context_is_adjacent_without_separator() {
    let f = Fixture::new();
    f.write("adj.txt", b"hit\nhit\nx\n");
    let out = f.grep(&["-A1", "hit", "adj.txt"]);
    assert_eq!(text(&out), "hit\nhit\nx\n");
}

#[test]
fn numeric_shorthand_and_attached_context_values() {
    let f = Fixture::new();
    f.write("n.txt", b"one\nhit\nthree\n");
    assert_eq!(text(&f.grep(&["-1", "hit", "n.txt"])), "one\nhit\nthree\n");
    assert_eq!(text(&f.grep(&["-A1", "hit", "n.txt"])), "hit\nthree\n");
}

#[test]
fn max_count_stops_after_selected_lines() {
    let f = Fixture::new();
    f.write("m.txt", b"hit\nhit\n");
    let out = f.grep(&["-m1", "hit", "m.txt"]);
    assert_eq!(text(&out), "hit\n");
    assert_eq!(text(&f.grep(&["-c", "-m1", "hit", "m.txt"])), "1\n");
}

#[test]
fn max_count_keeps_trailing_context_until_next_match() {
    let f = Fixture::new();
    f.write("mc.txt", b"hit\nctx\nhit\nafter\n");
    let out = f.grep(&["-m1", "-A2", "hit", "mc.txt"]);
    assert_eq!(text(&out), "hit\nctx\n");
}

#[test]
fn multiple_patterns_are_ored() {
    let f = Fixture::new();
    f.write("p.txt", b"alpha\nbeta\ngamma\n");
    let out = f.grep(&["-e", "alpha", "-e", "gamma", "p.txt"]);
    assert_eq!(text(&out), "alpha\ngamma\n");
    let out = f.grep(&["alpha\ngamma", "p.txt"]);
    assert_eq!(text(&out), "alpha\ngamma\n", "newline separates patterns");
    f.write("pats.txt", b"beta\n");
    let out = f.grep(&["-f", "pats.txt", "p.txt"]);
    assert_eq!(text(&out), "beta\n");
}

#[test]
fn binary_file_reports_match_instead_of_printing() {
    let f = Fixture::new();
    f.write("bin.dat", b"text\0hit\nmore\n");
    let out = f.grep(&["hit", "bin.dat"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "Binary file bin.dat matches\n");
}

#[test]
fn binary_file_is_ignored_with_capital_i() {
    let f = Fixture::new();
    f.write("bin.dat", b"text\0hit\n");
    let out = f.grep(&["-I", "hit", "bin.dat"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn text_flag_prints_binary_lines_and_count_works() {
    let f = Fixture::new();
    f.write("bin.dat", b"text\0hit\n");
    let out = f.grep(&["-a", "hit", "bin.dat"]);
    assert_eq!(out.stdout, b"text\0hit\n");
    let out = f.grep(&["-c", "hit", "bin.dat"]);
    assert_eq!(text(&out), "1\n");
}

#[test]
fn missing_file_is_an_error_but_matches_still_print() {
    let f = Fixture::new();
    f.write("a.txt", b"hit\n");
    let out = f.grep(&["hit", "missing.txt", "a.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(text(&out), "a.txt:hit\n");
    let out = f.grep(&["-s", "hit", "missing.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
}

#[test]
fn directory_without_recursion_is_reported() {
    let f = Fixture::new();
    f.write("dir/a.txt", b"hit\n");
    let out = f.grep(&["hit", "dir"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn stdin_with_label_and_invert_and_fixed_strings() {
    let f = Fixture::new();
    let out = f.grep_stdin(&["-H", "hit"], b"x\nhit\n");
    assert_eq!(text(&out), "(standard input):hit\n");
    let out = f.grep_stdin(&["-v", "x"], b"x\ny\n");
    assert_eq!(text(&out), "y\n");
    let out = f.grep_stdin(&["-F", "a.b"], b"axb\na.b\n");
    assert_eq!(text(&out), "a.b\n");
    let out = f.grep_stdin(&["-x", "ok"], b"ok\nokay\n");
    assert_eq!(text(&out), "ok\n");
}

#[test]
fn lookaround_is_rejected_with_clear_error_in_perl_mode() {
    let f = Fixture::new();
    f.write("a.txt", b"a\n");
    let out = f.grep(&["-P", "(?=a)a", "a.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("lookaround"));
}
