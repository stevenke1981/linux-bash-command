//! Terminal coloring for `rg`: explicit `--color`, `--colors` overrides, and the
//! default plain output when stdout is a pipe.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A temporary directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("agentcmd-color-rg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("a.txt"), "first line\nneedle here\n").unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// The label `rg` prints for `a.txt` when the directory is passed as an argument.
    fn label(&self) -> String {
        agentcmd::display_path(&self.0.join("a.txt"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs `rg` with stdout piped, without inheriting color variables from the environment.
fn rg(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rg"));
    command
        .args(args)
        .env_remove("AGENTCMD_COLOR")
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn default_piped_output_is_plain() {
    let dir = TempDir::new("default");
    let dir_arg = dir.path().to_str().unwrap();
    let out = rg(&["needle", dir_arg], &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(!text.contains('\x1b'), "unexpected escape in {text:?}");
    // No heading: the path is a prefix on the match line.
    assert_eq!(text, format!("{}:2:needle here\n", dir.label()));
}

#[test]
fn color_always_highlights_matches_and_paths() {
    let dir = TempDir::new("always");
    let dir_arg = dir.path().to_str().unwrap();
    let out = rg(&["--color", "always", "needle", dir_arg], &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(
        text.contains("\x1b[1;31mneedle\x1b[0m"),
        "match not highlighted: {text:?}"
    );
    assert!(
        text.contains(&format!("\x1b[35m{}\x1b[0m", dir.label())),
        "path not colored: {text:?}"
    );
    assert!(
        text.contains("\x1b[32m2\x1b[0m"),
        "line number not colored: {text:?}"
    );
}

#[test]
fn colors_spec_replaces_match_color_and_keeps_bold() {
    let dir = TempDir::new("spec");
    let dir_arg = dir.path().to_str().unwrap();
    let out = rg(
        &[
            "--colors",
            "match:fg:green",
            "--color",
            "always",
            "needle",
            dir_arg,
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    // The default style keeps bold, so the fg override becomes `1;32`.
    assert!(
        text.contains("\x1b[1;32mneedle\x1b[0m"),
        "override not applied: {text:?}"
    );
    assert!(
        !text.contains("\x1b[1;31m"),
        "default match color leaked: {text:?}"
    );
}

#[test]
fn color_never_and_no_color_env_emit_no_escapes() {
    let dir = TempDir::new("never");
    let dir_arg = dir.path().to_str().unwrap();
    let never = rg(&["--color", "never", "needle", dir_arg], &[]);
    assert!(!stdout(&never).contains('\x1b'));
    let no_color = rg(&["needle", dir_arg], &[("NO_COLOR", "1")]);
    assert_eq!(no_color.status.code(), Some(0));
    assert!(!stdout(&no_color).contains('\x1b'));
}

#[test]
fn files_listing_colors_paths() {
    let dir = TempDir::new("files");
    let dir_arg = dir.path().to_str().unwrap();
    let out = rg(&["--files", "--color", "always", dir_arg], &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(
        text.contains(&format!("\x1b[35m{}\x1b[0m\n", dir.label())),
        "file path not colored: {text:?}"
    );
}

#[test]
fn invalid_colors_spec_exits_2() {
    let dir = TempDir::new("bogus");
    let dir_arg = dir.path().to_str().unwrap();
    let out = rg(&["--colors", "bogus", "needle", dir_arg], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid color specification"));
}
