//! CLI tests for `tree` and `which`. Every test works inside a temporary sandbox.

mod common;

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use common::{run, run_in, Sandbox};

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A small fixed tree: a directory with a nested directory, a hidden file and a file.
fn fixture() -> Sandbox {
    let sandbox = Sandbox::new();
    sandbox.write("b/file.txt", b"hello\n");
    sandbox.write("b/sub/deep.txt", b"x\n");
    sandbox.write("a.d/zz", b"z");
    sandbox.write(".hidden", b"h");
    sandbox.write("top.txt", b"top\n");
    sandbox
}

// ---------------------------------------------------------------- tree

#[test]
fn tree_draws_ascii_connectors_and_summary() {
    let sandbox = fixture();
    let out = run_in(sandbox.path(), "tree", &["--charset=ascii", "b"], b"");
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "b\n|-- file.txt\n`-- sub\n    `-- deep.txt\n\n2 directories, 2 files\n"
    );
}

#[test]
fn tree_uses_box_drawing_connectors_by_default() {
    let sandbox = fixture();
    let out = run_in(sandbox.path(), "tree", &["-L", "1", "b"], b"");
    let shown = text(&out.stdout);
    assert!(shown.contains("├── file.txt\n"), "{shown}");
    assert!(shown.contains("└── sub\n"), "{shown}");
}

#[test]
fn tree_hides_dotfiles_unless_asked() {
    let sandbox = fixture();
    let hidden = run_in(sandbox.path(), "tree", &["--charset=ascii"], b"");
    assert!(!text(&hidden.stdout).contains(".hidden"));
    let all = run_in(sandbox.path(), "tree", &["-a", "--charset=ascii"], b"");
    assert!(text(&all.stdout).contains(".hidden"));
}

#[test]
fn tree_level_limits_depth() {
    let sandbox = fixture();
    let out = run_in(
        sandbox.path(),
        "tree",
        &["-L", "1", "--charset=ascii", "b"],
        b"",
    );
    let shown = text(&out.stdout);
    assert!(shown.contains("sub"));
    assert!(!shown.contains("deep.txt"), "{shown}");
}

#[test]
fn tree_summary_uses_singular_words_for_one() {
    // The root directory itself counts, so a directory holding one file is "1 directory".
    let sandbox = Sandbox::new();
    sandbox.write("only.txt", b"x");
    let out = run_in(sandbox.path(), "tree", &["--charset=ascii"], b"");
    assert!(
        text(&out.stdout).ends_with("\n1 directory, 1 file\n"),
        "{}",
        text(&out.stdout)
    );
    let dirs = run_in(sandbox.path(), "tree", &["-d", "--charset=ascii"], b"");
    assert!(
        text(&dirs.stdout).ends_with("\n1 directory\n"),
        "{}",
        text(&dirs.stdout)
    );
    sandbox.mkdir("solo");
    let two = run_in(sandbox.path(), "tree", &["-d", "--charset=ascii"], b"");
    assert!(
        text(&two.stdout).ends_with("\n2 directories\n"),
        "{}",
        text(&two.stdout)
    );
}

#[test]
fn tree_noreport_drops_the_summary() {
    let sandbox = fixture();
    let out = run_in(
        sandbox.path(),
        "tree",
        &["--noreport", "--charset=ascii", "b"],
        b"",
    );
    assert!(!text(&out.stdout).contains("directories"));
}

#[test]
fn tree_ignore_and_prune_patterns() {
    let sandbox = fixture();
    let ignored = run_in(
        sandbox.path(),
        "tree",
        &["-I", "a.d|sub", "--charset=ascii"],
        b"",
    );
    let shown = text(&ignored.stdout);
    assert!(!shown.contains("a.d"));
    assert!(!shown.contains("sub"));
    let pruned = run_in(
        sandbox.path(),
        "tree",
        &["-P", "*.txt", "--prune", "--charset=ascii"],
        b"",
    );
    let shown = text(&pruned.stdout);
    assert!(shown.contains("deep.txt"));
    assert!(!shown.contains("zz"), "{shown}");
}

#[test]
fn tree_marks_symlinks_without_following_them() {
    let sandbox = fixture();
    if sandbox.symlink("b", "link").is_none() {
        return;
    }
    let out = run_in(sandbox.path(), "tree", &["--charset=ascii"], b"");
    let shown = text(&out.stdout);
    assert!(shown.contains("link -> b"), "{shown}");
    assert!(!shown.contains("link/"), "{shown}");
}

#[test]
fn tree_human_sizes_use_units() {
    let sandbox = Sandbox::new();
    sandbox.write("big.bin", &vec![0u8; 2048]);
    let out = run_in(
        sandbox.path(),
        "tree",
        &["-h", "-L", "1", "--noreport", "--charset=ascii"],
        b"",
    );
    assert!(
        text(&out.stdout).contains("[2.0K]  big.bin"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn tree_time_format_uses_modification_time() {
    let sandbox = Sandbox::new();
    sandbox.write("old.txt", b"o");
    let when = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let file = std::fs::File::options()
        .write(true)
        .open(sandbox.path().join("old.txt"))
        .expect("open");
    file.set_modified(when).expect("set mtime");
    drop(file);
    let out = run_in(
        sandbox.path(),
        "tree",
        &[
            "-D",
            "--timefmt=%Y-%m-%d",
            "-L",
            "1",
            "--noreport",
            "--charset=ascii",
        ],
        b"",
    );
    assert!(
        text(&out.stdout).contains("[2023-11-14]  old.txt"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn tree_json_has_directory_and_report_objects() {
    let sandbox = fixture();
    let out = run_in(sandbox.path(), "tree", &["-J", "b"], b"");
    assert_eq!(code(&out), 0);
    let shown = text(&out.stdout);
    assert!(shown.starts_with("[\n  {\"type\":\"directory\",\"name\":\"b\",\"contents\":["));
    assert!(shown.contains("{\"type\":\"file\",\"name\":\"deep.txt\"}"));
    assert!(
        shown.ends_with("{\"type\":\"report\",\"directories\":2,\"files\":2}\n]\n"),
        "{shown}"
    );
}

#[test]
fn tree_missing_directory_exits_2() {
    let sandbox = Sandbox::new();
    let out = run_in(sandbox.path(), "tree", &["nosuch-dir-zz"], b"");
    assert_eq!(code(&out), 2);
    assert!(text(&out.stdout).starts_with("nosuch-dir-zz  [error opening dir]"));
}

#[test]
fn tree_filelimit_skips_large_directories() {
    let sandbox = fixture();
    let out = run_in(
        sandbox.path(),
        "tree",
        &["--filelimit", "1", "--charset=ascii", "b"],
        b"",
    );
    assert!(text(&out.stdout).contains("b  [2 entries exceeds filelimit, not opening dir]"));
}

#[test]
fn tree_rejects_bad_level_and_options() {
    let level = run("tree", &["-L", "0"], b"");
    assert_eq!(code(&level), 1);
    assert!(text(&level.stderr).contains("Invalid level, must be greater than 0."));
    let option = run("tree", &["-y"], b"");
    assert_eq!(code(&option), 1);
    assert!(text(&option.stderr).contains("Try 'tree --help'"));
    let charset = run("tree", &["--charset=latin1"], b"");
    assert_eq!(code(&charset), 1);
}

#[test]
fn tree_help_lists_options_and_version_exits_0() {
    let help = run("tree", &["--help"], b"");
    assert_eq!(code(&help), 0);
    let shown = text(&help.stdout);
    for option in [
        "--prune",
        "--filelimit",
        "--gitignore",
        "--charset",
        "--sort",
        "--timefmt",
        "-J",
    ] {
        assert!(shown.contains(option), "{option}");
    }
    let version = run("tree", &["--version"], b"");
    assert!(text(&version.stdout).starts_with("tree (agentcmd) "));
}

#[test]
fn tree_gitignore_hides_matching_entries() {
    let sandbox = Sandbox::new();
    sandbox.write(".gitignore", b"*.log\n");
    sandbox.write("keep.txt", b"k");
    sandbox.write("drop.log", b"d");
    let out = run_in(
        sandbox.path(),
        "tree",
        &["--gitignore", "--charset=ascii"],
        b"",
    );
    let shown = text(&out.stdout);
    assert!(shown.contains("keep.txt"));
    assert!(!shown.contains("drop.log"), "{shown}");
}

#[test]
fn tree_unsorted_and_accepted_color_flag_still_list_everything() {
    let sandbox = fixture();
    let out = run_in(
        sandbox.path(),
        "tree",
        &["-U", "-n", "--charset=ascii", "b"],
        b"",
    );
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    let shown = text(&out.stdout);
    assert!(
        shown.contains("file.txt") && shown.contains("deep.txt"),
        "{shown}"
    );
    assert!(shown.ends_with("2 directories, 2 files\n"), "{shown}");
}

// ---------------------------------------------------------------- which

/// Runs `agentcmd which` with an explicit `PATH`.
fn which_with_path(dir: &Path, path_var: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .arg("which")
        .args(args)
        .current_dir(dir)
        .env("PATH", path_var)
        .output()
        .expect("run which")
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

#[test]
fn which_finds_a_program_on_path() {
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let out = run("which", &[shell], b"");
    assert_eq!(code(&out), 0);
    assert!(
        text(&out.stdout).to_lowercase().contains(shell),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn which_missing_program_exits_1_with_no_output() {
    let out = run("which", &["agentcmd-no-such-program-zz"], b"");
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
}

#[test]
fn which_without_operands_exits_1() {
    let out = run("which", &[], b"");
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
}

#[test]
fn which_invalid_option_exits_2() {
    let out = run("which", &["-x"], b"");
    assert_eq!(code(&out), 2);
    assert!(text(&out.stderr).contains("invalid option -- 'x'"));
}

#[test]
fn which_silent_mode_prints_nothing() {
    let sandbox = Sandbox::new();
    // Windows finds programs by PATHEXT extension, so the file needs one there.
    let tool = if cfg!(windows) { "tool.exe" } else { "tool" };
    sandbox.write(tool, b"x");
    make_executable(&sandbox.path().join(tool));
    let path = sandbox.path().display().to_string();
    let found = which_with_path(sandbox.path(), &path, &["-s", "tool"]);
    assert_eq!(code(&found), 0);
    assert!(found.stdout.is_empty());
    let missing = which_with_path(sandbox.path(), &path, &["-s", "nope-zz"]);
    assert_eq!(code(&missing), 1);
}

#[cfg(unix)]
#[test]
fn which_all_lists_every_match_in_path_order() {
    let sandbox = Sandbox::new();
    sandbox.write("one/tool", b"#!/bin/sh\n");
    sandbox.write("two/tool", b"#!/bin/sh\n");
    make_executable(&sandbox.path().join("one/tool"));
    make_executable(&sandbox.path().join("two/tool"));
    let path = format!(
        "{}:{}",
        sandbox.path().join("one").display(),
        sandbox.path().join("two").display()
    );
    let first = which_with_path(sandbox.path(), &path, &["tool"]);
    assert_eq!(code(&first), 0);
    assert_eq!(
        text(&first.stdout),
        format!("{}/tool\n", sandbox.path().join("one").display())
    );
    let all = which_with_path(sandbox.path(), &path, &["-a", "tool"]);
    assert_eq!(text(&all.stdout).lines().count(), 2);
}

#[cfg(unix)]
#[test]
fn which_skips_files_without_execute_permission() {
    let sandbox = Sandbox::new();
    sandbox.write("tool", b"plain");
    let out = which_with_path(
        sandbox.path(),
        &sandbox.path().display().to_string(),
        &["tool"],
    );
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn which_path_names_are_checked_where_they_point() {
    let sandbox = Sandbox::new();
    sandbox.write("d1/tool", b"#!/bin/sh\n");
    make_executable(&sandbox.path().join("d1/tool"));
    let out = which_with_path(sandbox.path(), "/nonexistent-zz", &["d1/tool"]);
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out.stdout), "d1/tool\n");
    let absent = which_with_path(sandbox.path(), "/nonexistent-zz", &["d1/other"]);
    assert_eq!(code(&absent), 1);
}

#[cfg(unix)]
#[test]
fn which_empty_path_entry_means_current_directory() {
    let sandbox = Sandbox::new();
    sandbox.write("tool", b"#!/bin/sh\n");
    make_executable(&sandbox.path().join("tool"));
    let out = which_with_path(sandbox.path(), ":/nonexistent-zz", &["tool"]);
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out.stdout), "./tool\n");
}

#[test]
fn which_help_and_version() {
    let help = run("which", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).contains("-a"));
    let version = run("which", &["--version"], b"");
    assert!(text(&version.stdout).starts_with("which (agentcmd) "));
}
