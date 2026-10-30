//! CLI tests for the file-system commands. Every test works inside a `Sandbox` temporary
//! directory: nothing here removes or rewrites paths outside it. Paths such as `/` are
//! covered by the pure unit tests in `src/commands/fsops/safety.rs`, never by running `rm`.

mod common;

use std::fs;
use std::process::Output;

use common::{run, run_in, Sandbox};

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `--help` and `--version` behave the same for every command.
fn assert_help_and_version(util: &str) {
    let help = run(util, &["--help"], b"");
    assert_eq!(code(&help), 0, "{util} --help");
    assert!(
        stdout(&help).starts_with(&format!("Usage: {util}")),
        "{util} help text"
    );
    let version = run(util, &["--version"], b"");
    assert_eq!(code(&version), 0, "{util} --version");
    assert!(
        stdout(&version).starts_with(&format!("{util} (agentcmd) ")),
        "{util} version"
    );
}

fn assert_unknown_option(util: &str) {
    let out = run(util, &["--no-such-option"], b"");
    assert_eq!(code(&out), 1, "{util} unknown option");
    assert!(stderr(&out).contains("unrecognized option '--no-such-option'"));
    assert!(stderr(&out).contains(&format!("Try '{util} --help'")));
}

#[path = "fsops/mv_touch.rs"]
mod mv_touch;

#[path = "fsops/links.rs"]
mod links;

#[path = "fsops/modes.rs"]
mod modes;

// ---------------------------------------------------------------- mkdir

#[test]
fn mkdir_help_and_version() {
    assert_help_and_version("mkdir");
}

#[test]
fn mkdir_creates_a_directory() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "mkdir", &["d"], b"");
    assert_eq!(code(&out), 0);
    assert!(sb.path().join("d").is_dir());
}

#[test]
fn mkdir_existing_directory_is_an_error() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let out = run_in(sb.path(), "mkdir", &["d"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("mkdir: cannot create directory 'd': File exists"));
}

#[test]
fn mkdir_parents_creates_the_chain_and_accepts_existing() {
    let sb = Sandbox::new();
    assert_eq!(code(&run_in(sb.path(), "mkdir", &["-p", "a/b/c"], b"")), 0);
    assert_eq!(code(&run_in(sb.path(), "mkdir", &["-p", "a/b/c"], b"")), 0);
    assert!(sb.path().join("a/b/c").is_dir());
}

#[test]
fn mkdir_verbose_reports_each_created_directory() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "mkdir", &["-vp", "x/y"], b"");
    assert_eq!(
        stdout(&out),
        "mkdir: created directory 'x'\nmkdir: created directory 'x/y'\n"
    );
}

#[test]
fn mkdir_invalid_mode_is_rejected_before_creating() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "mkdir", &["-m", "bogus", "d"], b"");
    assert_eq!(code(&out), 1);
    assert_eq!(stderr(&out), "mkdir: invalid mode 'bogus'\n");
    assert!(!sb.path().join("d").exists());
}

#[test]
fn mkdir_missing_operand_prints_try_line() {
    let out = run("mkdir", &[], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("mkdir: missing operand"));
    assert!(stderr(&out).contains("Try 'mkdir --help'"));
}

#[test]
fn mkdir_context_option_is_not_supported() {
    let out = run("mkdir", &["-Z", "d"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("option '-Z' is not supported by agentcmd"));
}

#[test]
fn mkdir_unknown_option_is_a_usage_error() {
    assert_unknown_option("mkdir");
}

#[cfg(unix)]
#[test]
fn mkdir_mode_sets_the_final_directory_exactly() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new();
    assert_eq!(
        code(&run_in(sb.path(), "mkdir", &["-m", "700", "m"], b"")),
        0
    );
    let bits = fs::metadata(sb.path().join("m"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(bits & 0o777, 0o700);
}

// ---------------------------------------------------------------- rm

#[test]
fn rm_help_and_version() {
    assert_help_and_version("rm");
}

#[test]
fn rm_removes_a_file() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    assert_eq!(code(&run_in(sb.path(), "rm", &["a"], b"")), 0);
    assert!(!sb.path().join("a").exists());
}

#[test]
fn rm_missing_file_is_an_error_and_force_silences_it() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "rm", &["nope"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("rm: cannot remove 'nope': No such file or directory"));
    let forced = run_in(sb.path(), "rm", &["-f", "nope"], b"");
    assert_eq!(code(&forced), 0);
    assert_eq!(stderr(&forced), "");
}

#[test]
fn rm_directory_needs_recursive_or_dir() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let out = run_in(sb.path(), "rm", &["d"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("cannot remove 'd': Is a directory"));
    assert!(sb.path().join("d").is_dir());
}

#[test]
fn rm_recursive_removes_a_tree_and_reports_with_verbose() {
    let sb = Sandbox::new();
    sb.write("d/f", b"1");
    let out = run_in(sb.path(), "rm", &["-rv", "d"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "removed 'd/f'\nremoved directory 'd'\n");
    assert!(!sb.path().join("d").exists());
}

#[test]
fn rm_dir_flag_refuses_non_empty_directories() {
    let sb = Sandbox::new();
    sb.write("d/f", b"1");
    let out = run_in(sb.path(), "rm", &["-d", "d"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("cannot remove 'd': Directory not empty"));
}

#[test]
fn rm_interactive_answer_decides_removal() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    sb.write("b", b"y");
    assert_eq!(code(&run_in(sb.path(), "rm", &["-i", "a"], b"n\n")), 0);
    assert!(sb.path().join("a").exists());
    assert_eq!(code(&run_in(sb.path(), "rm", &["-i", "b"], b"y\n")), 0);
    assert!(!sb.path().join("b").exists());
}

#[test]
fn rm_once_declines_removing_four_files() {
    let sb = Sandbox::new();
    for name in ["p1", "p2", "p3", "p4"] {
        sb.write(name, b"x");
    }
    let out = run_in(sb.path(), "rm", &["-I", "p1", "p2", "p3", "p4"], b"n\n");
    assert_eq!(code(&out), 0);
    assert!(stderr(&out).contains("rm: remove 4 arguments?"));
    assert!(sb.path().join("p4").exists());
}

#[test]
fn rm_refuses_the_current_directory_name() {
    let sb = Sandbox::new();
    sb.write("keep", b"k");
    let out = run_in(sb.path(), "rm", &["-r", "."], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("refusing to remove '.' or '..' directory: skipping '.'"));
    assert!(sb.path().join("keep").exists());
}

#[test]
fn rm_missing_operand_and_force_without_operands() {
    let sb = Sandbox::new();
    let missing = run_in(sb.path(), "rm", &[], b"");
    assert_eq!(code(&missing), 1);
    assert!(stderr(&missing).contains("rm: missing operand"));
    assert_eq!(code(&run_in(sb.path(), "rm", &["-f"], b"")), 0);
}

#[test]
fn rm_unknown_option_is_a_usage_error() {
    assert_unknown_option("rm");
}

#[test]
fn rm_bad_interactive_argument_lists_choices() {
    let out = run("rm", &["--interactive=sometimes", "x"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("invalid argument 'sometimes' for '--interactive'"));
}

// ---------------------------------------------------------------- rmdir

#[test]
fn rmdir_help_and_version() {
    assert_help_and_version("rmdir");
}

#[test]
fn rmdir_removes_an_empty_directory() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    assert_eq!(code(&run_in(sb.path(), "rmdir", &["d"], b"")), 0);
    assert!(!sb.path().join("d").exists());
}

#[test]
fn rmdir_non_empty_is_an_error() {
    let sb = Sandbox::new();
    sb.write("d/f", b"1");
    let out = run_in(sb.path(), "rmdir", &["d"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("rmdir: failed to remove 'd': Directory not empty"));
}

#[test]
fn rmdir_parents_removes_the_chain() {
    let sb = Sandbox::new();
    sb.mkdir("a/b/c");
    sb.write("keep", b"k");
    assert_eq!(code(&run_in(sb.path(), "rmdir", &["-p", "a/b/c"], b"")), 0);
    assert!(!sb.path().join("a").exists());
    assert!(sb.path().join("keep").exists());
}

#[test]
fn rmdir_ignore_non_empty_is_silent() {
    let sb = Sandbox::new();
    sb.write("d/f", b"1");
    let out = run_in(
        sb.path(),
        "rmdir",
        &["--ignore-fail-on-non-empty", "d"],
        b"",
    );
    assert_eq!(code(&out), 0);
    assert_eq!(stderr(&out), "");
}

#[test]
fn rmdir_verbose_announces_each_directory() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let out = run_in(sb.path(), "rmdir", &["-v", "d"], b"");
    assert_eq!(stdout(&out), "rmdir: removing directory, 'd'\n");
}

#[test]
fn rmdir_missing_operand_and_unknown_option() {
    let missing = run("rmdir", &[], b"");
    assert_eq!(code(&missing), 1);
    assert!(stderr(&missing).contains("rmdir: missing operand"));
    assert_unknown_option("rmdir");
}

#[test]
#[cfg(unix)]
fn rmdir_file_operand_is_not_a_directory() {
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let out = run_in(sb.path(), "rmdir", &["f"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("rmdir: failed to remove 'f': Not a directory"));
}

// ---------------------------------------------------------------- cp

#[test]
fn cp_help_and_version() {
    assert_help_and_version("cp");
}

#[test]
fn cp_copies_a_file() {
    let sb = Sandbox::new();
    sb.write("a", b"hello");
    assert_eq!(code(&run_in(sb.path(), "cp", &["a", "b"], b"")), 0);
    assert_eq!(sb.read("b"), b"hello");
}

#[test]
fn cp_into_directory_and_multiple_sources() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    sb.write("b", b"2");
    sb.mkdir("d");
    assert_eq!(code(&run_in(sb.path(), "cp", &["a", "b", "d"], b"")), 0);
    assert_eq!(sb.read("d/a"), b"1");
    assert_eq!(sb.read("d/b"), b"2");
}

#[test]
fn cp_directory_without_recursive_is_omitted() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let out = run_in(sb.path(), "cp", &["d", "e"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("cp: -r not specified; omitting directory 'd'"));
}

#[test]
fn cp_recursive_copies_the_tree() {
    let sb = Sandbox::new();
    sb.write("d/s/f", b"xy");
    assert_eq!(code(&run_in(sb.path(), "cp", &["-R", "d", "e"], b"")), 0);
    assert_eq!(sb.read("e/s/f"), b"xy");
}

#[test]
fn cp_verbose_arrow_format() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    let out = run_in(sb.path(), "cp", &["-v", "a", "b"], b"");
    assert_eq!(stdout(&out), "'a' -> 'b'\n");
}

#[test]
fn cp_no_clobber_keeps_the_destination() {
    let sb = Sandbox::new();
    sb.write("a", b"new");
    sb.write("b", b"old");
    assert_eq!(code(&run_in(sb.path(), "cp", &["-n", "a", "b"], b"")), 0);
    assert_eq!(sb.read("b"), b"old");
}

#[test]
fn cp_interactive_answer_no_is_a_failure() {
    let sb = Sandbox::new();
    sb.write("a", b"new");
    sb.write("b", b"old");
    let out = run_in(sb.path(), "cp", &["-i", "a", "b"], b"n\n");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("cp: overwrite 'b'?"));
    assert_eq!(sb.read("b"), b"old");
}

#[test]
fn cp_backup_keeps_the_old_contents() {
    let sb = Sandbox::new();
    sb.write("a", b"new");
    sb.write("b", b"old");
    assert_eq!(code(&run_in(sb.path(), "cp", &["-b", "a", "b"], b"")), 0);
    assert_eq!(sb.read("b~"), b"old");
    assert_eq!(sb.read("b"), b"new");
}

#[test]
fn cp_target_directory_and_no_target_directory() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    sb.mkdir("d");
    assert_eq!(code(&run_in(sb.path(), "cp", &["-t", "d", "a"], b"")), 0);
    assert_eq!(sb.read("d/a"), b"1");
    let out = run_in(sb.path(), "cp", &["-T", "a", "d"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("cannot overwrite directory 'd' with non-directory"));
}

#[test]
fn cp_parents_keeps_the_source_path() {
    let sb = Sandbox::new();
    sb.write("src/deep/f", b"q");
    sb.mkdir("dst");
    assert_eq!(
        code(&run_in(
            sb.path(),
            "cp",
            &["--parents", "src/deep/f", "dst"],
            b""
        )),
        0
    );
    assert_eq!(sb.read("dst/src/deep/f"), b"q");
}

#[test]
fn cp_same_file_and_into_itself_are_errors() {
    let sb = Sandbox::new();
    sb.write("a", b"x");
    let same = run_in(sb.path(), "cp", &["a", "a"], b"");
    assert_eq!(code(&same), 1);
    assert!(stderr(&same).contains("cp: 'a' and 'a' are the same file"));
    sb.mkdir("d");
    let itself = run_in(sb.path(), "cp", &["-r", "d", "d/sub"], b"");
    assert_eq!(code(&itself), 1);
    assert!(stderr(&itself).contains("cannot copy a directory, 'd', into itself, 'd/sub'"));
}

#[test]
fn cp_missing_operands_and_multiple_sources_to_missing_target() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    let none = run_in(sb.path(), "cp", &[], b"");
    assert!(stderr(&none).contains("cp: missing file operand"));
    let one = run_in(sb.path(), "cp", &["a"], b"");
    assert!(stderr(&one).contains("cp: missing destination file operand after 'a'"));
    let many = run_in(sb.path(), "cp", &["a", "a", "nodir"], b"");
    assert_eq!(code(&many), 1);
    assert!(stderr(&many).contains("cp: target 'nodir': No such file or directory"));
}

#[test]
fn cp_unsupported_options_fail_cleanly() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    let reflink = run_in(sb.path(), "cp", &["--reflink=always", "a", "b"], b"");
    assert_eq!(code(&reflink), 1);
    assert!(stderr(&reflink).contains("option '--reflink=always' is not supported by agentcmd"));
    assert_unknown_option("cp");
}

#[test]
fn cp_preserve_rejects_unknown_attributes() {
    let sb = Sandbox::new();
    sb.write("a", b"1");
    let out = run_in(sb.path(), "cp", &["--preserve=bogus", "a", "b"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("invalid argument 'bogus' for '--preserve'"));
}

#[cfg(unix)]
#[test]
fn cp_symbolic_link_is_copied_as_a_link_by_default_when_recursive() {
    let sb = Sandbox::new();
    sb.write("t", b"x");
    sb.symlink("t", "l");
    assert_eq!(code(&run_in(sb.path(), "cp", &["-R", "l", "m"], b"")), 0);
    assert_eq!(
        fs::read_link(sb.path().join("m")).unwrap(),
        std::path::Path::new("t")
    );
}
