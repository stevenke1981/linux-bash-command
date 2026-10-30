use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-compat-fs-{}-{}",
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

    fn mkdir(&self, name: &str) {
        fs::create_dir_all(self.0.join(name)).unwrap();
    }

    fn set_mtime(&self, name: &str, seconds: u64) {
        let file = File::options().write(true).open(self.0.join(name)).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    fn run(&self, name: &str, args: &[&str]) -> Output {
        self.run_with_stdin(name, args, b"")
    }

    fn run_with_stdin(&self, name: &str, args: &[&str], stdin: &[u8]) -> Output {
        let binary = match name {
            "ls" => env!("CARGO_BIN_EXE_ls"),
            "pwd" => env!("CARGO_BIN_EXE_pwd"),
            "diff" => env!("CARGO_BIN_EXE_diff"),
            _ => panic!("unknown binary"),
        };
        let mut child = Command::new(binary)
            .current_dir(&self.0)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(stdin) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
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
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn code(output: &Output) -> Option<i32> {
    output.status.code()
}

/// Finds the long-format line whose final field is `name` and returns its whitespace-separated fields.
fn long_fields<'a>(listing: &'a str, name: &str) -> Vec<&'a str> {
    listing
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .find(|fields| fields.last() == Some(&name))
        .unwrap_or_else(|| panic!("no long line for {name} in:\n{listing}"))
}

fn names(listing: &str) -> Vec<String> {
    listing.lines().map(str::to_string).collect()
}

#[test]
fn ls_long_human_sizes_work_with_combined_flags() {
    let f = Fixture::new();
    f.write("big.bin", &vec![b'x'; 4096]);
    f.write("mid.txt", &vec![b'y'; 1500]);
    f.write("tiny.txt", b"0123456789");
    for flags in [["-lh"], ["-lah"], ["-alh"], ["-alF"]] {
        let out = f.run("ls", &flags);
        assert_eq!(code(&out), Some(0), "{flags:?}");
    }
    let listing = text(&f.run("ls", &["-lh"]));
    assert_eq!(long_fields(&listing, "big.bin")[4], "4.0K");
    assert_eq!(long_fields(&listing, "mid.txt")[4], "1.5K");
    assert_eq!(long_fields(&listing, "tiny.txt")[4], "10");
}

#[test]
fn ls_long_format_has_gnu_like_columns() {
    let f = Fixture::new();
    f.write("note.txt", b"abc");
    let listing = text(&f.run("ls", &["-l"]));
    let fields = long_fields(&listing, "note.txt");
    assert_eq!(fields.len(), 8, "{fields:?}");
    assert_eq!(fields[0].len(), 10, "mode string: {}", fields[0]);
    assert!(fields[0].starts_with('-') && fields[0].contains('r'));
    assert_eq!(fields[2], "-");
    assert_eq!(fields[3], "-");
    assert_eq!(fields[4], "3");
    assert_eq!(fields[5].len(), 10, "date: {}", fields[5]);
    assert_eq!(&fields[5][4..5], "-");
    assert_eq!(fields[6].len(), 5, "time: {}", fields[6]);
    assert_eq!(&fields[6][2..3], ":");
    let full = text(&f.run("ls", &["--full-time", "-l"]));
    assert!(full.contains(" +0000 note.txt"), "{full}");
}

#[test]
fn ls_a_lists_dot_entries_and_capital_a_does_not() {
    let f = Fixture::new();
    f.write(".hidden", b"h");
    f.write("visible", b"v");
    assert_eq!(
        names(&text(&f.run("ls", &["-a"]))),
        [".", "..", ".hidden", "visible"]
    );
    assert_eq!(names(&text(&f.run("ls", &["-A"]))), [".hidden", "visible"]);
    assert_eq!(names(&text(&f.run("ls", &[]))), ["visible"]);
}

#[test]
fn ls_recursive_prints_headers_and_blank_separators() {
    let f = Fixture::new();
    f.write("top.txt", b"t");
    f.write("sub/inner.txt", b"i");
    f.write("sub/deeper/leaf.txt", b"l");
    let out = text(&f.run("ls", &["-R"]));
    assert!(out.starts_with(".:\n"), "{out}");
    assert!(out.contains("\n\n./sub:\n"), "{out}");
    assert!(out.contains("\n\n./sub/deeper:\nleaf.txt\n"), "{out}");
    assert!(!out.contains("./.."), "{out}");
    let single = text(&f.run("ls", &["-R", "sub"]));
    assert!(single.starts_with("sub:\n"), "{single}");
    assert!(single.contains("\n\nsub/deeper:\n"), "{single}");
}

#[test]
fn ls_t_sorts_newest_first_and_r_reverses() {
    let f = Fixture::new();
    f.write("old.txt", b"o");
    f.write("new.txt", b"n");
    f.write("mid.txt", b"m");
    f.set_mtime("old.txt", 1_000_000);
    f.set_mtime("mid.txt", 2_000_000);
    f.set_mtime("new.txt", 3_000_000);
    assert_eq!(
        names(&text(&f.run("ls", &["-t"]))),
        ["new.txt", "mid.txt", "old.txt"]
    );
    assert_eq!(
        names(&text(&f.run("ls", &["-tr"]))),
        ["old.txt", "mid.txt", "new.txt"]
    );
}

#[test]
fn ls_s_sorts_largest_first() {
    let f = Fixture::new();
    f.write("small", &[0; 10]);
    f.write("large", &[0; 100]);
    f.write("medium", &[0; 50]);
    assert_eq!(
        names(&text(&f.run("ls", &["-S"]))),
        ["large", "medium", "small"]
    );
}

#[test]
fn ls_r_reverses_name_order() {
    let f = Fixture::new();
    f.write("a", b"");
    f.write("b", b"");
    f.write("c", b"");
    assert_eq!(names(&text(&f.run("ls", &["-r"]))), ["c", "b", "a"]);
    assert_eq!(names(&text(&f.run("ls", &["--reverse"]))), ["c", "b", "a"]);
}

#[test]
fn ls_v_orders_numbers_by_value() {
    let f = Fixture::new();
    f.write("file10", b"");
    f.write("file2", b"");
    assert_eq!(names(&text(&f.run("ls", &["-v"]))), ["file2", "file10"]);
}

#[test]
fn ls_d_lists_the_directory_entry_itself() {
    let f = Fixture::new();
    f.write("dir/inside.txt", b"x");
    assert_eq!(names(&text(&f.run("ls", &["-d", "dir"]))), ["dir"]);
    assert_eq!(names(&text(&f.run("ls", &["dir"]))), ["inside.txt"]);
    let classified = text(&f.run("ls", &["-dp", "dir"]));
    assert_eq!(classified.trim_end(), "dir/");
}

#[test]
fn ls_file_arguments_print_as_given_and_missing_paths_fail() {
    let f = Fixture::new();
    f.write("sub/file.txt", b"x");
    assert_eq!(text(&f.run("ls", &["sub/file.txt"])), "sub/file.txt\n");
    assert_eq!(code(&f.run("ls", &["missing"])), Some(2));
    assert_eq!(code(&f.run("ls", &["-i"])), Some(2));
    assert_eq!(code(&f.run("ls", &["-s"])), Some(2));
    assert_eq!(code(&f.run("ls", &["--time-style=+%Y"])), Some(2));
    assert_eq!(code(&f.run("ls", &["--color=sometimes"])), Some(2));
}

#[test]
fn ls_group_directories_first_puts_directories_before_files() {
    let f = Fixture::new();
    f.write("a.txt", b"");
    f.mkdir("z-dir");
    assert_eq!(
        names(&text(&f.run("ls", &["--group-directories-first"]))),
        ["z-dir", "a.txt"]
    );
}

#[cfg(unix)]
#[test]
fn ls_long_symlink_shows_target() {
    let f = Fixture::new();
    f.write("target.txt", b"x");
    std::os::unix::fs::symlink("target.txt", f.0.join("link")).unwrap();
    let listing = text(&f.run("ls", &["-l"]));
    let line = listing
        .lines()
        .find(|line| line.ends_with("link -> target.txt"))
        .unwrap_or_else(|| panic!("{listing}"));
    assert!(line.starts_with('l'), "{line}");
}

#[test]
fn pwd_physical_and_logical_exit_zero_and_reject_extra_arguments() {
    let f = Fixture::new();
    let physical = f.run("pwd", &["-P"]);
    assert_eq!(code(&physical), Some(0));
    let printed = text(&physical);
    assert!(!printed.contains(r"\\?\"), "{printed}");
    assert_eq!(
        PathBuf::from(printed.trim()).canonicalize().unwrap(),
        f.0.canonicalize().unwrap()
    );
    assert_eq!(code(&f.run("pwd", &["-L"])), Some(0));
    assert_eq!(code(&f.run("pwd", &["-LP"])), Some(0));
    assert_eq!(code(&f.run("pwd", &["--physical"])), Some(0));
    assert_eq!(code(&f.run("pwd", &["-X"])), Some(2));
    assert_eq!(code(&f.run("pwd", &["extra"])), Some(2));
}

#[test]
fn diff_unified_shows_headers_hunks_and_context_options() {
    let f = Fixture::new();
    f.write("a", b"one\ntwo\nthree\nfour\nfive\n");
    f.write("b", b"one\nTWO\nthree\nfour\nfive\n");
    let out = f.run("diff", &["-u", "a", "b"]);
    assert_eq!(code(&out), Some(1));
    let body = text(&out);
    assert!(
        body.starts_with("--- a\n+++ b\n@@ -1,5 +1,5 @@\n"),
        "{body}"
    );
    assert!(body.contains("-two\n+TWO\n"), "{body}");
    let attached = f.run("diff", &["-U0", "a", "b"]);
    assert!(
        text(&attached).contains("@@ -2 +2 @@"),
        "{}",
        text(&attached)
    );
    let long = f.run("diff", &["--unified=1", "a", "b"]);
    assert!(text(&long).contains("@@ -1,3 +1,3 @@"), "{}", text(&long));
    assert_eq!(code(&f.run("diff", &["-c", "a", "b"])), Some(2));
    assert_eq!(code(&f.run("diff", &["-y", "a", "b"])), Some(2));
    assert_eq!(code(&f.run("diff", &["-e", "a", "b"])), Some(2));
}

#[test]
fn diff_q_reports_only_the_verdict() {
    let f = Fixture::new();
    f.write("a", b"same\n");
    f.write("b", b"other\n");
    f.write("c", b"same\n");
    let differ = f.run("diff", &["-q", "a", "b"]);
    assert_eq!(code(&differ), Some(1));
    assert_eq!(text(&differ), "Files a and b differ\n");
    let same = f.run("diff", &["--brief", "a", "c"]);
    assert_eq!(code(&same), Some(0));
    assert!(same.stdout.is_empty());
    assert_eq!(
        text(&f.run("diff", &["-s", "a", "c"])),
        "Files a and c are identical\n"
    );
}

#[test]
fn diff_r_compares_directory_trees() {
    let f = Fixture::new();
    f.write("left/same.txt", b"x\n");
    f.write("right/same.txt", b"x\n");
    f.write("left/changed.txt", b"old\n");
    f.write("right/changed.txt", b"new\n");
    f.write("left/only-left.txt", b"l\n");
    f.write("right/only-right.txt", b"r\n");
    f.write("left/sub/deep.txt", b"a\n");
    f.write("right/sub/deep.txt", b"b\n");
    let out = f.run("diff", &["-r", "left", "right"]);
    assert_eq!(code(&out), Some(1));
    let body = text(&out);
    assert!(body.contains("Only in left: only-left.txt\n"), "{body}");
    assert!(body.contains("Only in right: only-right.txt\n"), "{body}");
    assert!(
        body.contains("diff -r left/changed.txt right/changed.txt\n"),
        "{body}"
    );
    assert!(
        body.contains("--- left/changed.txt\n+++ right/changed.txt\n"),
        "{body}"
    );
    assert!(
        body.contains("diff -r left/sub/deep.txt right/sub/deep.txt\n"),
        "{body}"
    );
    assert!(!body.contains("same.txt"), "{body}");
    let brief = text(&f.run("diff", &["-rq", "left", "right"]));
    assert!(
        brief.contains("Files left/changed.txt and right/changed.txt differ\n"),
        "{brief}"
    );
    assert!(!brief.contains("diff -r"), "{brief}");
    let identical = f.run("diff", &["-r", "left/same.txt", "right/same.txt"]);
    assert_eq!(code(&identical), Some(0));
}

#[test]
fn diff_strip_trailing_cr_ignores_crlf_line_endings() {
    let f = Fixture::new();
    f.write("crlf.txt", b"alpha\r\nbeta\r\n");
    f.write("lf.txt", b"alpha\nbeta\n");
    assert_eq!(code(&f.run("diff", &["crlf.txt", "lf.txt"])), Some(1));
    let out = f.run("diff", &["--strip-trailing-cr", "crlf.txt", "lf.txt"]);
    assert_eq!(
        code(&out),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn diff_w_ignores_all_white_space() {
    let f = Fixture::new();
    f.write("spaced", b"fn  main ( )  {\n\treturn 1;\n}\n");
    f.write("compact", b"fn main(){\nreturn 1;\n}\n");
    assert_eq!(code(&f.run("diff", &["spaced", "compact"])), Some(1));
    assert_eq!(code(&f.run("diff", &["-w", "spaced", "compact"])), Some(0));
    f.write("spaced2", b"a  b\n");
    f.write("single", b"a b\n");
    assert_eq!(code(&f.run("diff", &["-w", "spaced2", "single"])), Some(0));
    assert_eq!(code(&f.run("diff", &["-b", "spaced2", "single"])), Some(0));
    assert_eq!(code(&f.run("diff", &["spaced2", "single"])), Some(1));
}

#[test]
fn diff_b_big_i_and_blank_line_options_filter_changes() {
    let f = Fixture::new();
    f.write("upper", b"Hello\n");
    f.write("lower", b"hello\n");
    assert_eq!(code(&f.run("diff", &["-i", "upper", "lower"])), Some(0));
    f.write("blank-a", b"x\n\ny\n");
    f.write("blank-b", b"x\ny\n");
    assert_eq!(code(&f.run("diff", &["-B", "blank-a", "blank-b"])), Some(0));
    assert_eq!(code(&f.run("diff", &["blank-a", "blank-b"])), Some(1));
}

#[test]
fn diff_label_replaces_header_names_and_binary_files_are_detected() {
    let f = Fixture::new();
    f.write("a", b"one\n");
    f.write("b", b"two\n");
    let out = text(&f.run("diff", &["--label", "old", "--label", "new", "a", "b"]));
    assert!(out.starts_with("--- old\n+++ new\n"), "{out}");
    assert_eq!(
        code(&f.run(
            "diff",
            &["--label", "a", "--label", "b", "--label", "c", "a", "b"]
        )),
        Some(2)
    );
    f.write("bin-a", &[0, 1, 2, 3]);
    f.write("bin-b", &[0, 1, 2, 4]);
    let binary = f.run("diff", &["bin-a", "bin-b"]);
    assert_eq!(code(&binary), Some(1));
    assert_eq!(text(&binary), "Binary files bin-a and bin-b differ\n");
    assert_eq!(code(&f.run("diff", &["-a", "bin-a", "bin-b"])), Some(1));
}

#[test]
fn diff_reads_standard_input_and_reports_missing_files() {
    let f = Fixture::new();
    f.write("file", b"same\n");
    let out = f.run_with_stdin("diff", &["-", "file"], b"same\n");
    assert_eq!(code(&out), Some(0));
    let changed = f.run_with_stdin("diff", &["-", "file"], b"different\n");
    assert_eq!(code(&changed), Some(1));
    assert!(
        text(&changed).contains("--- -\n+++ file\n"),
        "{}",
        text(&changed)
    );
    assert_eq!(code(&f.run("diff", &["file", "missing"])), Some(2));
}

#[test]
fn diff_new_file_treats_absent_file_as_empty() {
    let f = Fixture::new();
    f.write("present", b"line\n");
    let out = f.run("diff", &["-N", "absent", "present"]);
    assert_eq!(code(&out), Some(1));
    assert!(text(&out).contains("+line\n"), "{}", text(&out));
}
