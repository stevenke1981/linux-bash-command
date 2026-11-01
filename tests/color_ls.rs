//! `ls` colors names and lays out columns on terminals only; piped output stays plain.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A temporary directory owned by one test, removed when the test ends.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("agentcmd-color-ls-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Sandbox(dir)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A directory with one subdirectory, one plain file and (on unix) one executable.
fn fixture(name: &str) -> Sandbox {
    let sandbox = Sandbox::new(name);
    fs::create_dir(sandbox.0.join("dir")).unwrap();
    fs::write(sandbox.0.join("plain.txt"), b"x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tool = sandbox.0.join("tool");
        fs::write(&tool, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    }
    sandbox
}

/// A directory holding `f1`..`f6`, for checking column layouts.
fn six_files(name: &str) -> Sandbox {
    let sandbox = Sandbox::new(name);
    for index in 1..=6 {
        fs::write(sandbox.0.join(format!("f{index}")), b"x").unwrap();
    }
    sandbox
}

/// Runs the `ls` binary in `dir`. Color and layout variables are removed from the
/// child environment unless the test sets them in `env`.
fn ls(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ls"));
    command
        .current_dir(dir)
        .args(args)
        .env_remove("AGENTCMD_COLOR")
        .env_remove("NO_COLOR")
        .env_remove("LS_COLORS")
        .env_remove("COLUMNS");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn piped_default_output_has_no_escapes_and_one_name_per_line() {
    let sandbox = fixture("piped");
    let out = ls(&sandbox.0, &[], &[]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains('\x1b'), "unexpected escape in {text:?}");
    let expected = if cfg!(unix) {
        "dir\nplain.txt\ntool\n"
    } else {
        "dir\nplain.txt\n"
    };
    assert_eq!(text, expected);
}

#[test]
fn always_colors_directories_and_executables() {
    let sandbox = fixture("always");
    for args in [
        &["--color=always"][..],
        &["--color"][..],
        &["--colour=always"][..],
    ] {
        let text = stdout(&ls(&sandbox.0, args, &[]));
        assert!(text.contains("\x1b[01;34mdir\x1b[0m\n"), "{text:?}");
        assert!(text.contains("plain.txt\n"), "{text:?}");
        #[cfg(unix)]
        assert!(text.contains("\x1b[01;32mtool\x1b[0m\n"), "{text:?}");
    }
}

#[test]
fn ls_colors_overrides_directory_color() {
    let sandbox = fixture("ls-colors");
    let text = stdout(&ls(
        &sandbox.0,
        &["--color=always"],
        &[("LS_COLORS", "di=01;33")],
    ));
    assert!(text.contains("\x1b[01;33mdir\x1b[0m\n"), "{text:?}");
    assert!(!text.contains("\x1b[01;34m"), "{text:?}");
}

#[test]
fn never_and_no_color_produce_no_escapes() {
    let sandbox = fixture("never");
    let never = stdout(&ls(
        &sandbox.0,
        &["--color=never"],
        &[("LS_COLORS", "di=01;33")],
    ));
    assert!(!never.contains('\x1b'), "{never:?}");
    let no_color = stdout(&ls(&sandbox.0, &[], &[("NO_COLOR", "1")]));
    assert!(!no_color.contains('\x1b'), "{no_color:?}");
}

#[test]
fn columns_fill_down_when_forced_with_dash_c() {
    let sandbox = six_files("columns");
    let out = ls(&sandbox.0, &["-C"], &[("COLUMNS", "20")]);
    assert_eq!(stdout(&out), "f1  f3  f5\nf2  f4  f6\n");
}

#[test]
fn across_fills_rows_when_forced_with_dash_x() {
    let sandbox = six_files("across");
    let out = ls(&sandbox.0, &["-x"], &[("COLUMNS", "20")]);
    assert_eq!(stdout(&out), "f1  f2  f3\nf4  f5  f6\n");
}

#[test]
fn dash_one_wins_over_columns_and_long_format_stays_long() {
    let sandbox = six_files("one");
    let out = ls(&sandbox.0, &["-C", "-1"], &[("COLUMNS", "20")]);
    assert_eq!(stdout(&out).lines().count(), 6);
    let long = stdout(&ls(&sandbox.0, &["-C", "-l"], &[("COLUMNS", "20")]));
    assert_eq!(long.lines().count(), 6);
    assert!(long.lines().all(|line| line.starts_with('-')));
}

#[test]
fn invalid_color_argument_exits_two() {
    let sandbox = fixture("invalid");
    let out = ls(&sandbox.0, &["--color=bogus"], &[]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("ls: invalid argument 'bogus' for '--color'"),
        "{err}"
    );
}
