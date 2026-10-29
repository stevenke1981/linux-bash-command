use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &[u8]) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        let executable = match args[0] {
            "search" => env!("CARGO_BIN_EXE_search"),
            "diff" => env!("CARGO_BIN_EXE_diff"),
            "list" => env!("CARGO_BIN_EXE_list"),
            "read" => env!("CARGO_BIN_EXE_read"),
            _ => panic!("unknown test command"),
        };
        Command::new(executable)
            .current_dir(&self.0)
            .args(&args[1..])
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn search_reports_matches_no_matches_and_errors() {
    let fixture = Fixture::new();
    fixture.write("src/a.txt", b"first\nHello agent\n");
    fixture.write(".hidden.txt", b"Hello hidden\n");
    fixture.write("binary.dat", b"Hello\0agent");
    let found = fixture.run(&["search", "--json", "Hello", "."]);
    assert_eq!(found.status.code(), Some(0));
    let lines: Vec<serde_json::Value> = String::from_utf8(found.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["line_number"], 2);
    assert_eq!(lines[0]["text"], "Hello agent");
    assert_eq!(
        fixture.run(&["search", "missing", "."]).status.code(),
        Some(1)
    );
    assert_eq!(fixture.run(&["search", "[", "."]).status.code(), Some(2));
    assert_eq!(
        fixture.run(&["search", "Hello", "absent"]).status.code(),
        Some(2)
    );
}

#[test]
fn diff_has_expected_status_and_missing_newline_marker() {
    let fixture = Fixture::new();
    fixture.write("a", b"one\ntwo\n");
    fixture.write("b", b"one\nchanged");
    assert_eq!(fixture.run(&["diff", "a", "a"]).status.code(), Some(0));
    let output = fixture.run(&["diff", "a", "b"]);
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("@@"));
    assert!(text.contains("-two\n"));
    assert!(text.contains("+changed"));
    assert!(text.contains("\\ No newline at end of file"));
    assert_eq!(
        fixture.run(&["diff", "a", "missing"]).status.code(),
        Some(2)
    );
}

#[test]
fn list_is_sorted_and_respects_depth_and_hidden_flags() {
    let fixture = Fixture::new();
    fixture.write("z.txt", b"z");
    fixture.write("nested/a.txt", b"a");
    fixture.write(".hidden", b"h");
    fixture.write("target/build", b"x");
    let listed = fixture.run(&["list"]);
    assert_eq!(listed.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(listed.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["nested/a.txt", "z.txt"]
    );
    let depth = fixture.run(&["list", "--max-depth", "1"]);
    assert_eq!(
        String::from_utf8(depth.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["z.txt"]
    );
    let empty = fixture.run(&["list", "--max-depth", "0"]);
    assert!(empty.stdout.is_empty());
    let all = fixture.run(&["list", "--all", "--no-ignore"]);
    let text = String::from_utf8(all.stdout).unwrap();
    assert!(text.contains(".hidden"));
    assert!(text.contains("target/build"));
}

#[test]
fn read_respects_line_range_and_rejects_bad_text() {
    let fixture = Fixture::new();
    fixture.write("a", b"one\r\ntwo\r\nthree\n");
    fixture.write("bad", &[0xff]);
    let output = fixture.run(&["read", "--start", "2", "--limit", "1", "a"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "     2\ttwo\n");
    assert_eq!(fixture.run(&["read", "bad"]).status.code(), Some(2));
    assert_eq!(
        fixture
            .run(&["read", "--start", "4", "--end", "2", "a"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn search_rejects_invalid_utf8_and_read_stops_at_limit() {
    let fixture = Fixture::new();
    fixture.write("bad", b"valid\n\xff\n");
    let search = fixture.run(&["search", "valid", "bad"]);
    assert_eq!(search.status.code(), Some(2));
    assert!(search.stdout.is_empty());
    assert!(!search.stderr.is_empty());
    let limited = fixture.run(&["read", "--limit", "1", "bad"]);
    assert_eq!(limited.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(limited.stdout).unwrap(),
        "     1\tvalid\n"
    );
    assert_eq!(fixture.run(&["read", "bad"]).status.code(), Some(2));
}

#[test]
fn each_standalone_command_exposes_its_own_name() {
    let fixture = Fixture::new();
    for name in ["search", "diff", "list", "read"] {
        let output = fixture.run(&[name, "--help"]);
        assert_eq!(output.status.code(), Some(0));
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains(&format!("Usage: {name} ")), "{help}");
        assert!(!help.contains("agentcmd "), "{help}");
    }
}
