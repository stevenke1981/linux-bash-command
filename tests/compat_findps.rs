//! Compatibility tests for find, ps and df: the command forms coding agents use.
//! Paths are compared with forward slashes (display_path) so the same expectations hold
//! on Linux, macOS and Windows.

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    /// Creates a unique directory with a fixed source-like tree.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-findps-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        let fixture = Self(path);
        fixture.write("Cargo.toml", "[package]\n");
        fixture.write("README.md", "readme\n");
        fixture.write("readme-notes.txt", "notes\n");
        fixture.write("a.txt", "a\n");
        fixture.write("b.txt", "b\n");
        fixture.write("src/main.rs", "fn main() {}\n");
        fixture.write("src/lib.rs", "pub fn lib() {}\n");
        fixture.write("src/app.ts", "export const app = 1;\n");
        fixture.write("src/util.ts", "export const util = 2;\n");
        fixture.write("src/x.js", "x\n");
        fixture.write("src/deep/a/b/c.txt", "deep\n");
        fixture.write("target/debug/x.rs", "x\n");
        fixture.write("node_modules/pkg/index.js", "pkg\n");
        fixture.write("tmp/file.tmp", "tmp\n");
        fs::create_dir_all(fixture.0.join("empty_dir")).unwrap();
        fixture
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn run(&self, binary: &str, args: &[&str]) -> Output {
        let program = match binary {
            "find" => env!("CARGO_BIN_EXE_find"),
            "ps" => env!("CARGO_BIN_EXE_ps"),
            "df" => env!("CARGO_BIN_EXE_df"),
            "cat" => env!("CARGO_BIN_EXE_cat"),
            _ => panic!("unknown binary {binary}"),
        };
        Command::new(program)
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    /// Runs find with the given arguments and returns stdout lines.
    fn find_lines(&self, args: &[&str]) -> Vec<String> {
        let out = self.run("find", args);
        assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
        lines(&out.stdout)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn lines(bytes: &[u8]) -> Vec<String> {
    text(bytes).lines().map(str::to_string).collect()
}

#[test]
fn find_name_with_not_path_skips_build_output() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-name", "*.rs", "-not", "-path", "./target/*"]);
    assert_eq!(found, vec!["./src/lib.rs", "./src/main.rs"]);
}

#[test]
fn find_type_f_and_name_lists_in_directory_order() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-type", "f", "-name", "*.ts"]);
    assert_eq!(found, vec!["./src/app.ts", "./src/util.ts"]);
}

#[test]
fn find_print_quit_acts_like_head() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-type", "f", "-name", "*.ts", "-print", "-quit"]);
    assert_eq!(found, vec!["./src/app.ts"]);
}

#[test]
fn find_prune_skips_node_modules_before_or_branch() {
    let f = Fixture::new();
    let found = f.find_lines(&[
        ".",
        "-path",
        "./node_modules",
        "-prune",
        "-o",
        "-name",
        "*.js",
        "-print",
    ]);
    assert_eq!(found, vec!["./src/x.js"]);
}

#[test]
fn find_maxdepth_limits_directory_listing() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-maxdepth", "2", "-type", "d"]);
    assert_eq!(
        found,
        vec![
            ".",
            "./empty_dir",
            "./node_modules",
            "./node_modules/pkg",
            "./src",
            "./src/deep",
            "./target",
            "./target/debug",
            "./tmp",
        ]
    );
}

#[test]
fn find_type_f_newer_than_reference_file() {
    let f = Fixture::new();
    // Make Cargo.toml clearly older than the other files, without sleeping.
    let old = UNIX_EPOCH + Duration::from_secs(1_000_000);
    OpenOptions::new()
        .write(true)
        .open(f.0.join("Cargo.toml"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    let found = f.find_lines(&[".", "-type", "f", "-newer", "Cargo.toml"]);
    assert!(found.contains(&"./src/main.rs".to_string()), "{found:?}");
    assert!(!found.contains(&"./Cargo.toml".to_string()), "{found:?}");
}

#[test]
fn find_delete_removes_matching_files_only() {
    let f = Fixture::new();
    let out = f.run("find", &[".", "-name", "*.tmp", "-delete"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(!f.0.join("tmp/file.tmp").exists());
    assert!(f.0.join("src/main.rs").exists());
}

#[test]
fn find_exec_plus_batches_paths_into_one_command() {
    let f = Fixture::new();
    let cat = env!("CARGO_BIN_EXE_cat");
    let out = f.run(
        "find",
        &[".", "-type", "f", "-name", "*.ts", "-exec", cat, "{}", "+"],
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let output = text(&out.stdout);
    assert!(output.contains("export const app = 1;"), "{output}");
    assert!(output.contains("export const util = 2;"), "{output}");
}

#[test]
fn find_exec_semicolon_runs_once_per_entry() {
    let f = Fixture::new();
    let cat = env!("CARGO_BIN_EXE_cat");
    let out = f.run("find", &[".", "-name", "util.ts", "-exec", cat, "{}", ";"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "export const util = 2;\n");
}

#[test]
fn find_iname_matches_case_insensitively() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-iname", "readme*", "-type", "f"]);
    assert_eq!(found, vec!["./README.md", "./readme-notes.txt"]);
}

#[test]
fn find_regex_matches_whole_path() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-regex", r".*\.ts"]);
    assert_eq!(found, vec!["./src/app.ts", "./src/util.ts"]);
}

#[test]
fn find_empty_directories() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-empty", "-type", "d"]);
    assert_eq!(found, vec!["./empty_dir"]);
}

#[test]
fn find_group_with_parentheses_and_implicit_print() {
    let f = Fixture::new();
    let grouped = f.find_lines(&[
        ".", "(", "-name", "a.txt", "-o", "-name", "b.txt", ")", "-print",
    ]);
    assert_eq!(grouped, vec!["./a.txt", "./b.txt"]);
    // Without an explicit action, -print is implied for the whole expression.
    let implicit = f.find_lines(&[".", "-name", "a.txt", "-o", "-name", "b.txt"]);
    assert_eq!(implicit, vec!["./a.txt", "./b.txt"]);
}

#[test]
fn find_printf_relative_path_and_metadata() {
    let f = Fixture::new();
    let relative = f.find_lines(&["src", "-printf", r"%P\n"]);
    assert_eq!(relative.first().map(String::as_str), Some(""));
    assert!(relative.contains(&"app.ts".to_string()), "{relative:?}");
    assert!(
        relative.contains(&"deep/a/b/c.txt".to_string()),
        "{relative:?}"
    );

    let root = f.find_lines(&[".", "-maxdepth", "0", "-printf", r"%d %y %p\n"]);
    assert_eq!(root, vec!["0 d ."]);
}

#[test]
fn find_print0_terminates_each_path_with_nul() {
    let f = Fixture::new();
    let out = f.run("find", &[".", "-name", "a.txt", "-print0"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"./a.txt\0");
}

#[test]
fn find_ls_prints_mode_and_path() {
    let f = Fixture::new();
    let found = f.find_lines(&[".", "-name", "a.txt", "-ls"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("-rw"), "{found:?}");
    assert!(found[0].ends_with("./a.txt"), "{found:?}");
}

#[test]
fn find_missing_root_reports_and_exits_one() {
    let f = Fixture::new();
    let out = f.run("find", &["missing_dir_for_test", "-name", "*"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("missing_dir_for_test"));
}

#[test]
fn find_rejects_invalid_expressions_with_status_one() {
    let f = Fixture::new();
    for args in [
        vec![".", "-bogus"],
        vec![".", "-name"],
        vec![".", "-maxdepth", "x"],
        vec![".", "-type", "z"],
        vec![".", "-printf", "%u"],
        vec![".", "-ok", "rm", "{}"],
        vec![".", "(", "-name", "a.txt"],
        vec![".", "-size", "3Q"],
    ] {
        let out = f.run("find", &args);
        assert_eq!(out.status.code(), Some(1), "args {args:?}");
    }
}

#[test]
fn find_symlink_options_precede_starting_points() {
    let f = Fixture::new();
    fs::create_dir_all(f.0.join("lead")).unwrap();
    fs::write(f.0.join("lead/a.rs"), "x").unwrap();
    for option in ["-L", "-H", "-P"] {
        let lines = f.find_lines(&[option, "lead", "-name", "*.rs"]);
        assert_eq!(lines, vec!["lead/a.rs"], "option {option}");
    }
    assert_eq!(
        f.find_lines(&["-L", "-O3", "lead", "-type", "f"]),
        vec!["lead/a.rs"]
    );
}

#[cfg(unix)]
#[test]
fn find_follow_detects_symlink_loops() {
    let f = Fixture::new();
    fs::create_dir_all(f.0.join("loop")).unwrap();
    std::os::unix::fs::symlink("..", f.0.join("loop/self")).unwrap();
    // Without -L the link is listed and not followed.
    let links = f.find_lines(&["loop", "-type", "l"]);
    assert_eq!(links, vec!["loop/self"]);
    // With -L the cycle is detected and reported, and the walk still terminates.
    let out = f.run("find", &["-L", "loop", "-name", "nothing-matches-this"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("loop"), "{}", text(&out.stderr));
}

#[test]
fn ps_aux_has_bsd_header() {
    let f = Fixture::new();
    let out = f.run("ps", &["aux"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let rows = lines(&out.stdout);
    let header: Vec<&str> = rows[0].split_whitespace().collect();
    assert_eq!(
        header,
        vec![
            "USER", "PID", "%CPU", "%MEM", "VSZ", "RSS", "TTY", "STAT", "START", "TIME", "COMMAND"
        ]
    );
    assert!(rows.len() > 1);
}

#[test]
fn ps_ef_has_full_header() {
    let f = Fixture::new();
    let out = f.run("ps", &["-ef"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let rows = lines(&out.stdout);
    let header: Vec<&str> = rows[0].split_whitespace().collect();
    assert_eq!(
        header,
        vec!["UID", "PID", "PPID", "C", "STIME", "TTY", "TIME", "CMD"]
    );
}

#[test]
fn ps_selects_own_process_with_custom_fields() {
    let f = Fixture::new();
    let pid = std::process::id().to_string();
    let out = f.run("ps", &["-o", "pid,comm", "-p", &pid]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let rows = lines(&out.stdout);
    assert_eq!(
        rows[0].split_whitespace().collect::<Vec<_>>(),
        vec!["PID", "COMMAND"]
    );
    assert_eq!(rows[1].split_whitespace().next(), Some(pid.as_str()));
}

#[test]
fn ps_blank_header_and_no_headers_options() {
    let f = Fixture::new();
    let pid = std::process::id().to_string();
    let blank = f.run("ps", &["-o", "pid=", "-p", &pid]);
    assert_eq!(lines(&blank.stdout).len(), 1);
    let plain = f.run("ps", &["--no-headers", "-o", "pid", "-p", &pid]);
    assert_eq!(lines(&plain.stdout).len(), 1);
    let bsd = f.run(
        "ps",
        &["-o", "pid,ppid,%cpu,rss,stat,etime,args", "-p", &pid],
    );
    assert_eq!(bsd.status.code(), Some(0));
}

#[test]
fn ps_filter_with_no_match_exits_one() {
    let f = Fixture::new();
    let out = f.run("ps", &["-p", "4294967295"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn ps_json_lists_selected_process() {
    let f = Fixture::new();
    let pid = std::process::id();
    let out = f.run("ps", &["--json", "-p", &pid.to_string()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let records = value.as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["pid"], serde_json::json!(pid));
}

#[test]
fn ps_rejects_long_format_and_unknown_fields() {
    let f = Fixture::new();
    assert_eq!(f.run("ps", &["-l"]).status.code(), Some(1));
    assert_eq!(f.run("ps", &["-o", "nosuchfield"]).status.code(), Some(1));
    assert_eq!(f.run("ps", &["--sort=bogus"]).status.code(), Some(1));
}

#[test]
fn ps_accepts_common_selection_flags() {
    let f = Fixture::new();
    for args in [vec!["-e"], vec!["-A"], vec!["ax"], vec!["-W"], vec!["-ef"]] {
        let out = f.run("ps", &args);
        assert_eq!(
            out.status.code(),
            Some(0),
            "args {args:?}: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn df_human_with_type_column() {
    let f = Fixture::new();
    let out = f.run("df", &["-hT", "."]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let rows = lines(&out.stdout);
    let header: Vec<&str> = rows[0].split_whitespace().collect();
    assert_eq!(
        header,
        vec![
            "Filesystem",
            "Type",
            "Size",
            "Used",
            "Avail",
            "Use%",
            "Mounted",
            "on"
        ]
    );
    assert!(rows.len() >= 2);
}

#[test]
fn df_portability_header_and_single_row_per_filesystem() {
    let f = Fixture::new();
    let out = f.run("df", &["-P", "."]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let rows = lines(&out.stdout);
    let header: Vec<&str> = rows[0].split_whitespace().collect();
    assert_eq!(
        header,
        vec![
            "Filesystem",
            "1024-blocks",
            "Used",
            "Available",
            "Capacity",
            "Mounted",
            "on"
        ]
    );
    assert_eq!(rows.len(), 2, "{rows:?}");
}

#[test]
fn df_bundled_portability_and_human_flags() {
    let f = Fixture::new();
    assert_eq!(f.run("df", &["-Ph", "."]).status.code(), Some(0));
    assert_eq!(f.run("df", &["-H", "."]).status.code(), Some(0));
    assert_eq!(f.run("df", &["-B", "M", "."]).status.code(), Some(0));
    assert_eq!(
        f.run("df", &["--block-size=1G", "."]).status.code(),
        Some(0)
    );
    assert_eq!(f.run("df", &["-m", "-l", "."]).status.code(), Some(0));
}

#[test]
fn df_type_filters_and_rejections() {
    let f = Fixture::new();
    let excluded = f.run("df", &["-x", "no_such_fs_type"]);
    assert_eq!(excluded.status.code(), Some(0));
    let none = f.run("df", &["-t", "no_such_fs_type", "."]);
    assert_eq!(none.status.code(), Some(0));
    assert_eq!(lines(&none.stdout).len(), 1, "header only");
    assert_eq!(f.run("df", &["-i"]).status.code(), Some(1));
    assert_eq!(f.run("df", &["-B", "Q"]).status.code(), Some(1));
    assert_eq!(f.run("df", &["--bogus"]).status.code(), Some(1));
}

#[test]
fn df_without_arguments_and_path_outside_tree() {
    let f = Fixture::new();
    assert_eq!(f.run("df", &[]).status.code(), Some(0));
    let missing = f.run("df", &["does-not-exist-for-compat-test"]);
    assert_eq!(missing.status.code(), Some(1));
}
