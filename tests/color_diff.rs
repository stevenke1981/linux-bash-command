//! Terminal coloring for diff, df and ps. Tests run with stdout piped, so `auto`
//! never colors; colors appear only with `--color=always` or `AGENTCMD_COLOR=always`.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A unique scratch directory under the system temp dir, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "agentcmd-color-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch(dir)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).expect("write fixture");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs `bin` with stdout piped and the color variables cleared, then applies `env`.
fn run(bin: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(bin);
    command
        .args(args)
        .env_remove("AGENTCMD_COLOR")
        .env_remove("NO_COLOR");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run command")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn diff_fixture(scratch: &Scratch) -> (String, String) {
    let left = scratch.write("left.txt", "one\ntwo\nthree\n");
    let right = scratch.write("right.txt", "one\nTWO\nthree\n");
    (path_text(&left), path_text(&right))
}

const DIFF: &str = env!("CARGO_BIN_EXE_diff");
const DF: &str = env!("CARGO_BIN_EXE_df");
const PS: &str = env!("CARGO_BIN_EXE_ps");

#[test]
fn diff_piped_default_has_no_escapes() {
    let scratch = Scratch::new("diff-default");
    let (left, right) = diff_fixture(&scratch);
    let output = run(DIFF, &["-u", &left, &right], &[]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("\n-two\n"), "{text}");
    assert!(!text.contains('\x1b'), "{text:?}");
}

#[test]
fn diff_color_always_paints_unified_output() {
    let scratch = Scratch::new("diff-always");
    let (left, right) = diff_fixture(&scratch);
    let output = run(DIFF, &["-u", "--color=always", &left, &right], &[]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("\x1b[31m-two\x1b[0m\n"), "{text:?}");
    assert!(text.contains("\x1b[32m+TWO\x1b[0m\n"), "{text:?}");
    assert!(text.contains("\x1b[36m@@"), "{text:?}");
    assert!(text.contains("\x1b[1m---"), "{text:?}");
    assert!(text.contains("\x1b[1m+++"), "{text:?}");
    // Context lines stay plain.
    assert!(text.contains("\n one\n"), "{text:?}");
}

#[test]
fn diff_bare_color_piped_has_no_escapes() {
    let scratch = Scratch::new("diff-bare");
    let (left, right) = diff_fixture(&scratch);
    let output = run(DIFF, &["-u", "--color", &left, &right], &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!stdout(&output).contains('\x1b'));
}

#[test]
fn diff_invalid_color_when_is_usage_error() {
    let scratch = Scratch::new("diff-bogus");
    let (left, right) = diff_fixture(&scratch);
    let output = run(DIFF, &["-u", "--color=bogus", &left, &right], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("diff: invalid argument 'bogus' for '--color'"),
        "{stderr}"
    );
    assert!(stderr.contains("Try 'diff --help'"), "{stderr}");
}

#[test]
fn diff_no_color_env_keeps_output_plain() {
    let scratch = Scratch::new("diff-no-color");
    let (left, right) = diff_fixture(&scratch);
    let output = run(DIFF, &["-u", &left, &right], &[("NO_COLOR", "1")]);
    assert!(!stdout(&output).contains('\x1b'));
}

#[test]
fn diff_agentcmd_color_env_colors_without_flag() {
    let scratch = Scratch::new("diff-env");
    let (left, right) = diff_fixture(&scratch);
    let output = run(
        DIFF,
        &["-u", &left, &right],
        &[("AGENTCMD_COLOR", "always")],
    );
    assert!(stdout(&output).contains("\x1b[31m-two\x1b[0m"));
}

#[test]
fn df_header_is_bold_when_forced() {
    let output = run(DF, &[], &[("AGENTCMD_COLOR", "always")]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.starts_with("\x1b[1m"), "{text:?}");
    // The Use% value is wrapped in one of the usage colors.
    assert!(text.contains("%\x1b[0m"), "{text:?}");
    assert!(text.contains("\x1b[3"), "{text:?}");
}

#[test]
fn df_plain_when_piped_or_not_forced() {
    let output = run(DF, &[], &[]);
    assert!(output.status.success());
    assert!(!stdout(&output).contains('\x1b'));
}

#[test]
fn ps_header_is_bold_when_forced() {
    let output = run(PS, &[], &[("AGENTCMD_COLOR", "always")]);
    assert!(output.status.success());
    let text = stdout(&output);
    let header = text.lines().next().unwrap_or_default();
    assert!(header.contains("\x1b[1mPID\x1b[0m"), "{header:?}");
}

#[test]
fn ps_plain_when_piped_or_not_forced() {
    let output = run(PS, &[], &[]);
    assert!(output.status.success());
    assert!(!stdout(&output).contains('\x1b'));
}
