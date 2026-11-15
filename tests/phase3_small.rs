use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn exe(command: &str) -> &'static str {
    match command {
        "shuf" => env!("CARGO_BIN_EXE_shuf"),
        "tsort" => env!("CARGO_BIN_EXE_tsort"),
        "pathchk" => env!("CARGO_BIN_EXE_pathchk"),
        _ => panic!("unknown P3 test command: {command}"),
    }
}

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-p3-small-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn run(&self, command: &str, args: &[&str], input: &[u8]) -> Output {
        let mut child = Command::new(exe(command))
            .args(args)
            .current_dir(&self.0)
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env_remove("POSIXLY_CORRECT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        child.wait_with_output().unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}
fn text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

mod shuf_cli {
    use super::*;

    #[test]
    fn help() {
        let out = Sandbox::new().run("shuf", &["--help"], b"");
        assert_eq!(code(&out), 0);
        let help = text(&out);
        for option in [
            "--echo",
            "--input-range",
            "--head-count",
            "--output",
            "--random-source",
            "--repeat",
            "--zero-terminated",
        ] {
            assert!(help.contains(option), "missing {option}");
        }
    }

    #[test]
    fn version() {
        let out = Sandbox::new().run("shuf", &["--version"], b"");
        assert_eq!(code(&out), 0);
        assert!(text(&out).starts_with("shuf (agentcmd) "));
    }

    #[test]
    fn echo_outputs_a_permutation_without_duplicates() {
        let out = Sandbox::new().run("shuf", &["--echo", "alpha", "beta", "gamma"], b"");
        assert_eq!(code(&out), 0);
        let mut rows = text(&out).lines().map(str::to_owned).collect::<Vec<_>>();
        rows.sort();
        assert_eq!(rows, ["alpha", "beta", "gamma"]);
    }

    #[test]
    fn singleton_range_is_deterministic() {
        let out = Sandbox::new().run("shuf", &["--input-range=8-8", "--head-count=1"], b"");
        assert_eq!(code(&out), 0);
        assert_eq!(out.stdout, b"8\n");
    }

    #[test]
    fn huge_range_uses_sparse_unique_sampling() {
        let out = Sandbox::new().run("shuf", &["-i", "1-18446744073709551615", "-n", "8"], b"");
        assert_eq!(code(&out), 0);
        let values = text(&out)
            .lines()
            .map(|n| n.parse::<u64>().unwrap())
            .collect::<Vec<_>>();
        let unique = values
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(values.len(), 8);
        assert_eq!(unique.len(), 8);
    }

    #[test]
    fn maximum_unsigned_singleton_and_empty_range_are_supported() {
        let dir = Sandbox::new();
        let max = dir.run(
            "shuf",
            &[
                "--input-range=18446744073709551615-18446744073709551615",
                "-n",
                "1",
            ],
            b"",
        );
        assert_eq!(code(&max), 0);
        assert_eq!(max.stdout, b"18446744073709551615\n");
        let empty = dir.run("shuf", &["--input-range=1-0"], b"");
        assert_eq!(code(&empty), 0);
        assert!(empty.stdout.is_empty());
        assert_eq!(code(&dir.run("shuf", &["--input-range=-1-5"], b"")), 1);
    }

    #[test]
    fn repeat_and_count_allow_duplicate_records() {
        let out = Sandbox::new().run("shuf", &["-e", "only", "--repeat", "--head-count=4"], b"");
        assert_eq!(code(&out), 0);
        assert_eq!(text(&out), "only\nonly\nonly\nonly\n");
        let short = Sandbox::new().run("shuf", &["-e", "only", "-r", "-n", "3"], b"");
        assert_eq!(code(&short), 0);
        assert_eq!(text(&short), "only\nonly\nonly\n");
    }

    #[test]
    fn random_source_is_repeatable_for_identical_inputs() {
        let first = Sandbox::new();
        first.write("seed", b"stable seed bytes for deterministic shuf\n");
        first.write("input", b"a\nb\nc\nd\ne\nf\n");
        let a = first.run("shuf", &["--random-source=seed", "input"], b"");
        let second = Sandbox::new();
        second.write("seed", b"stable seed bytes for deterministic shuf\n");
        second.write("input", b"a\nb\nc\nd\ne\nf\n");
        let b = second.run("shuf", &["--random-source=seed", "input"], b"");
        assert_eq!(code(&a), 0);
        assert_eq!(code(&b), 0);
        assert_eq!(a.stdout, b.stdout);
    }

    #[test]
    fn output_file_and_zero_delimiter_work() {
        let dir = Sandbox::new();
        let file = dir.run("shuf", &["--echo", "single", "-o", "result"], b"");
        assert_eq!(code(&file), 0);
        assert!(file.stdout.is_empty());
        assert_eq!(fs::read(dir.0.join("result")).unwrap(), b"single\n");

        let zero = dir.run("shuf", &["-z"], b"a\0b\0");
        assert_eq!(code(&zero), 0);
        assert!(zero.stdout.ends_with(b"\0"));
        let mut rows = zero
            .stdout
            .split(|b| *b == 0)
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>();
        rows.sort();
        assert_eq!(rows, [b"a".as_slice(), b"b".as_slice()]);
    }

    #[test]
    fn invalid_range_and_missing_random_source_fail() {
        let dir = Sandbox::new();
        assert_eq!(code(&dir.run("shuf", &["-i", "5-1"], b"")), 1);
        assert_eq!(
            code(&dir.run("shuf", &["--random-source=missing"], b"x\ny\n")),
            1
        );
        assert_eq!(code(&dir.run("shuf", &["--bad"], b"")), 1);
    }

    #[test]
    fn random_source_errors_only_if_a_draw_needs_bytes() {
        let dir = Sandbox::new();
        dir.write("empty-seed", b"");
        let no_count = dir.run("shuf", &["--random-source=missing", "-n", "0"], b"a\nb\n");
        assert_eq!(code(&no_count), 0);
        assert!(no_count.stdout.is_empty());
        let singleton = dir.run("shuf", &["--random-source=missing"], b"only\n");
        assert_eq!(code(&singleton), 0);
        assert_eq!(singleton.stdout, b"only\n");
        let empty_rows = dir.run("shuf", &["--random-source=missing"], b"");
        assert_eq!(code(&empty_rows), 0);
        let eof = dir.run("shuf", &["--random-source=empty-seed"], b"a\nb\n");
        assert_eq!(code(&eof), 1);
        assert!(String::from_utf8_lossy(&eof.stderr).contains("random source empty-seed"));
    }

    #[test]
    fn short_random_source_reuses_bits_and_fails_after_entropy_runs_out() {
        let dir = Sandbox::new();
        dir.write("seed", &[0]);
        let repeated = dir.run(
            "shuf",
            &[
                "--echo",
                "a",
                "b",
                "--repeat",
                "-n",
                "2",
                "--random-source=seed",
            ],
            b"",
        );
        assert_eq!(code(&repeated), 0);
        assert_eq!(
            repeated
                .stdout
                .iter()
                .filter(|byte| **byte == b'\n')
                .count(),
            2
        );

        let partial = dir.run(
            "shuf",
            &["--echo", "a", "b", "c", "-n", "2", "--random-source=seed"],
            b"",
        );
        assert_eq!(code(&partial), 0);
        assert_eq!(
            partial.stdout.iter().filter(|byte| **byte == b'\n').count(),
            2
        );

        let exhausted = dir.run(
            "shuf",
            &[
                "--echo",
                "a",
                "b",
                "--repeat",
                "-n",
                "9",
                "--random-source=seed",
            ],
            b"",
        );
        assert_eq!(code(&exhausted), 1);
        assert_eq!(exhausted.stdout, b"a\n".repeat(8));
        assert!(String::from_utf8_lossy(&exhausted.stderr).contains("random source seed"));

        dir.write("seed64", &[0; 8]);
        let max_range = dir.run(
            "shuf",
            &[
                "--input-range=0-18446744073709551615",
                "-n",
                "1",
                "--random-source=seed64",
            ],
            b"",
        );
        assert_eq!(code(&max_range), 1);
    }

    #[test]
    fn repeat_without_count_streams_until_reader_closes_pipe() {
        let mut child = Command::new(exe("shuf"))
            .args(["--echo", "forever", "--repeat"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let result = std::io::BufRead::read_line(&mut reader, &mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let line = receiver.recv_timeout(std::time::Duration::from_secs(5));
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(line.unwrap().unwrap(), "forever\n");
    }

    #[test]
    fn finite_large_repeat_count_streams_before_completing() {
        let mut child = Command::new(exe("shuf"))
            .args(["--echo", "live", "--repeat", "--head-count=999999999"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let result = std::io::BufRead::read_line(&mut reader, &mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let line = receiver.recv_timeout(std::time::Duration::from_secs(5));
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(line.unwrap().unwrap(), "live\n");
    }

    #[test]
    fn zero_count_returns_without_reading_stdin_or_opening_input() {
        let dir = Sandbox::new();
        let missing = dir.run("shuf", &["-n", "0", "missing"], b"");
        assert_eq!(code(&missing), 0);
        assert!(missing.stdout.is_empty());

        let source_missing = dir.run("shuf", &["-n", "0", "--random-source=absent"], b"a\nb\n");
        assert_eq!(code(&source_missing), 0);
        assert!(source_missing.stdout.is_empty());

        let output = dir.run("shuf", &["-n", "0", "-o", "empty-output"], b"");
        assert_eq!(code(&output), 0);
        assert_eq!(fs::read(dir.0.join("empty-output")).unwrap(), b"");
    }

    #[test]
    fn zero_count_exits_while_stdin_writer_remains_open() {
        let mut child = Command::new(exe("shuf"))
            .args(["-n", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                let output = child.wait_with_output().unwrap();
                assert_eq!(code(&output), 0);
                assert!(output.stdout.is_empty());
                return;
            }
            if started.elapsed() > std::time::Duration::from_secs(5) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("shuf -n 0 waited for stdin");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

mod tsort_cli {
    use super::*;

    #[test]
    fn help_and_version() {
        let help = Sandbox::new().run("tsort", &["--help"], b"");
        assert_eq!(code(&help), 0);
        assert!(text(&help).contains("Usage: tsort"));
        let version = Sandbox::new().run("tsort", &["--version"], b"");
        assert_eq!(code(&version), 0);
        assert!(text(&version).starts_with("tsort (agentcmd) "));
    }

    #[test]
    fn chain_has_topological_order() {
        let out = Sandbox::new().run("tsort", &[], b"a b\nb c\n");
        assert_eq!(code(&out), 0);
        assert_eq!(text(&out), "a\nb\nc\n");
    }

    #[test]
    fn independent_nodes_follow_first_seen_kahn_order() {
        let out = Sandbox::new().run("tsort", &[], b"a b\nc d\ne f\na c\n");
        assert_eq!(code(&out), 0);
        assert_eq!(text(&out), "a\ne\nb\nc\nf\nd\n");
    }

    #[test]
    fn file_input_and_duplicate_edges() {
        let dir = Sandbox::new();
        dir.write("pairs", b"a b\na b\nb c\n");
        let out = dir.run("tsort", &["pairs"], b"");
        assert_eq!(code(&out), 0);
        assert_eq!(text(&out), "a\nb\nc\n");
    }

    #[test]
    fn odd_token_count_is_an_error() {
        let out = Sandbox::new().run("tsort", &[], b"a b c\n");
        assert_eq!(code(&out), 1);
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("input contains an odd number of tokens")
        );
    }

    #[test]
    fn cycle_reports_members_and_still_outputs_every_node() {
        let out = Sandbox::new().run("tsort", &[], b"a b\nb a\nb c\nd e\n");
        assert_eq!(code(&out), 1);
        assert!(String::from_utf8_lossy(&out.stderr).contains("input contains a loop:"));
        let mut lines = text(&out).lines().map(str::to_owned).collect::<Vec<_>>();
        lines.sort();
        assert_eq!(lines, ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn invalid_operand_and_option_fail() {
        let dir = Sandbox::new();
        assert_eq!(code(&dir.run("tsort", &["absent"], b"")), 1);
        assert_eq!(code(&dir.run("tsort", &["one", "two"], b"")), 1);
        assert_eq!(code(&dir.run("tsort", &["--unknown"], b"")), 1);
    }
}

mod pathchk_cli {
    use super::*;

    #[test]
    fn help_and_version() {
        let help = Sandbox::new().run("pathchk", &["--help"], b"");
        assert_eq!(code(&help), 0);
        for option in ["-p", "-P", "--portability"] {
            assert!(text(&help).contains(option));
        }
        let version = Sandbox::new().run("pathchk", &["--version"], b"");
        assert_eq!(code(&version), 0);
        assert!(text(&version).starts_with("pathchk (agentcmd) "));
    }

    #[test]
    fn accepts_existing_and_portable_names() {
        let dir = Sandbox::new();
        assert_eq!(code(&dir.run("pathchk", &["ordinary.txt"], b"")), 0);
        assert_eq!(
            code(&dir.run("pathchk", &["--portability", "abc_123.txt"], b"")),
            0
        );
    }

    #[test]
    fn checks_portable_character_set_and_component_length() {
        let dir = Sandbox::new();
        let bad_char = dir.run("pathchk", &["-p", "has@char"], b"");
        assert_eq!(code(&bad_char), 1);
        assert!(String::from_utf8_lossy(&bad_char.stderr).contains("nonportable character"));
        let too_long = dir.run("pathchk", &["-p", "123456789012345"], b"");
        assert_eq!(code(&too_long), 1);
        assert!(String::from_utf8_lossy(&too_long.stderr).contains("limit 14"));
    }

    #[test]
    fn checks_portable_total_path_length() {
        let dir = Sandbox::new();
        let max_path = ("1234567890123/".repeat(18)) + "abc";
        let too_long_path = ("1234567890123/".repeat(18)) + "abcd";
        assert_eq!(code(&dir.run("pathchk", &["-p", &max_path], b"")), 0);
        let out = dir.run("pathchk", &["-p", &too_long_path], b"");
        assert_eq!(code(&out), 1);
        assert!(String::from_utf8_lossy(&out.stderr).contains("limit 256"));
    }

    #[test]
    fn checks_empty_and_leading_hyphen_components() {
        let dir = Sandbox::new();
        let empty = dir.run("pathchk", &["-P", ""], b"");
        assert_eq!(code(&empty), 1);
        assert!(String::from_utf8_lossy(&empty.stderr).contains("empty file name"));
        let dash = dir.run("pathchk", &["-P", "dir/-name"], b"");
        assert_eq!(code(&dash), 1);
        assert!(String::from_utf8_lossy(&dash.stderr).contains("leading '-'"));
    }

    #[test]
    fn portability_alias_combines_posix_and_dash_checks() {
        let dir = Sandbox::new();
        assert_eq!(
            code(&dir.run("pathchk", &["--portability", "--", "-name"], b"")),
            1
        );
        assert_eq!(
            code(&dir.run("pathchk", &["--portability", "name@"], b"")),
            1
        );
        assert_eq!(code(&dir.run("pathchk", &[""], b"")), 1);
        assert_eq!(code(&dir.run("pathchk", &["-p", r"dir\file"], b"")), 1);
    }

    #[test]
    fn existing_file_cannot_be_a_parent_component() {
        let dir = Sandbox::new();
        dir.write("plain-file", b"x");
        let out = dir.run("pathchk", &["plain-file/child"], b"");
        assert_eq!(code(&out), 1);
        assert!(String::from_utf8_lossy(&out.stderr).contains("Not a directory"));
    }

    #[test]
    fn reports_all_bad_names_and_missing_operands() {
        let dir = Sandbox::new();
        assert_eq!(
            code(&dir.run("pathchk", &["-p", "good", "bad@name"], b"")),
            1
        );
        assert_eq!(code(&dir.run("pathchk", &[], b"")), 1);
        assert_eq!(code(&dir.run("pathchk", &["--bogus"], b"")), 1);
    }
}
