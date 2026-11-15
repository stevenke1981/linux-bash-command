use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-phase3-join-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        fs::write(self.0.join(name), bytes).unwrap();
    }

    fn run(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_join"))
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
        child.stdin.take().unwrap().write_all(input).unwrap();
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

#[test]
fn help_and_version_list_implemented_options() {
    let dir = Sandbox::new();
    let help = dir.run(&["--help"], b"");
    assert_eq!(code(&help), 0);
    let help = String::from_utf8_lossy(&help.stdout);
    for option in [
        "-a",
        "-e",
        "--ignore-case",
        "-j",
        "-o",
        "-t",
        "-v",
        "-1",
        "-2",
        "--check-order",
        "--nocheck-order",
        "--header",
        "-z",
    ] {
        assert!(help.contains(option), "help is missing {option}");
    }
    let version = dir.run(&["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("join (agentcmd) "));
}

#[test]
fn joins_default_fields_after_ignoring_leading_blank_runs() {
    let dir = Sandbox::new();
    dir.write("left", b"  1 alpha  red\n2 beta blue\n");
    dir.write("right", b"1 first green\n2 second yellow\n");
    let out = dir.run(&["left", "right"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(
        out.stdout,
        b"1 alpha red first green\n2 beta blue second yellow\n"
    );
}

#[test]
fn selected_keys_casefolding_and_explicit_output_work() {
    let dir = Sandbox::new();
    dir.write("left", b"Alice,01,blue\nBob,02,green\n");
    dir.write("right", b"01,ALICE,one\n02,BOB,two\n");
    let out = dir.run(
        &[
            "-t,",
            "-1",
            "2",
            "-2",
            "1",
            "-i",
            "-o",
            "0,1.1,2.3",
            "left",
            "right",
        ],
        b"",
    );
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"01,Alice,one\n02,Bob,two\n");
}

#[test]
fn duplicate_keys_form_left_major_cartesian_product() {
    let dir = Sandbox::new();
    dir.write("left", b"k l1\nk l2\n");
    dir.write("right", b"k r1\nk r2\nk r3\n");
    let out = dir.run(&["left", "right"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(
        out.stdout,
        b"k l1 r1\nk l1 r2\nk l1 r3\nk l2 r1\nk l2 r2\nk l2 r3\n"
    );
}

#[test]
fn oversized_duplicate_group_spills_and_replays_from_tempfile() {
    let dir = Sandbox::new();
    let left_payload = "x".repeat(1_100_000);
    let right_payload = "y".repeat(1_100_000);
    dir.write("left", format!("k {left_payload}\nk tail\n").as_bytes());
    dir.write("right", format!("k {right_payload}\nk r2\n").as_bytes());
    let out = dir.run(&["left", "right"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout.iter().filter(|byte| **byte == b'\n').count(), 4);
    assert!(out.stdout.starts_with(
        format!("k {left_payload} {right_payload}\nk {left_payload} r2\n").as_bytes()
    ));
    assert!(out
        .stdout
        .ends_with(format!("k tail {right_payload}\nk tail r2\n").as_bytes()));
}

#[test]
fn unmatched_and_only_unmatched_modes_select_the_requested_side() {
    let dir = Sandbox::new();
    dir.write("left", b"a left-a\nb left-b\n");
    dir.write("right", b"b right-b\nc right-c\n");
    let all_left = dir.run(&["-a", "1", "left", "right"], b"");
    assert_eq!(all_left.stdout, b"a left-a\nb left-b right-b\n");
    let only_right = dir.run(&["-v", "2", "left", "right"], b"");
    assert_eq!(code(&only_right), 0);
    assert_eq!(only_right.stdout, b"c right-c\n");
}

#[test]
fn automatic_format_header_and_empty_replacement_work() {
    let dir = Sandbox::new();
    dir.write("left", b"id name\na Alice\nb Bob\n");
    dir.write("right", b"code city\na Oslo\nb\n");
    let out = dir.run(&["--header", "-o", "auto", "-e", "?", "left", "right"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout, b"id name city\na Alice Oslo\nb Bob ?\n");
}

#[test]
fn whole_line_separator_zero_records_and_stdin_are_supported() {
    let dir = Sandbox::new();
    dir.write("left", b"other\nwhole line\n");
    let joined = dir.run(&["-t", "", "left", "-"], b"other\nwhole line\n");
    assert_eq!(code(&joined), 0);
    assert_eq!(joined.stdout, b"other\nwhole line\n");
    dir.write("left-zero", b"a,x\0b,y\0");
    let zero = dir.run(&["-z", "-t", ",", "left-zero", "-"], b"a,p\0b,q\0");
    assert_eq!(code(&zero), 0);
    assert_eq!(zero.stdout, b"a,x,p\0b,y,q\0");
    dir.write("left-nul-field", b"a\0x\nb\0y\n");
    dir.write("right-nul-field", b"a\0p\nb\0q\n");
    let nul_field = dir.run(&["-t", "\\0", "left-nul-field", "right-nul-field"], b"");
    assert_eq!(code(&nul_field), 0);
    assert_eq!(nul_field.stdout, b"a\0x\0p\nb\0y\0q\n");
}

#[test]
fn ordering_modes_report_only_when_required() {
    let dir = Sandbox::new();
    dir.write("left", b"b left-b\na left-a\n");
    dir.write("right", b"a right-a\nb right-b\n");
    let checked = dir.run(&["--check-order", "left", "right"], b"");
    assert_eq!(code(&checked), 1);
    assert!(String::from_utf8_lossy(&checked.stderr).contains("left:2: is not sorted"));
    let unchecked = dir.run(&["--nocheck-order", "left", "right"], b"");
    assert_eq!(code(&unchecked), 0);
    let default = dir.run(&["left", "right"], b"");
    assert_eq!(code(&default), 0); // both disorder keys are paired, so GNU delays and suppresses the warning

    dir.write("left", b"b left-b\na left-a\n");
    dir.write("right", b"a right-a\nc right-c\n");
    let diagnosed = dir.run(&["left", "right"], b"");
    assert_eq!(code(&diagnosed), 1);
    assert!(String::from_utf8_lossy(&diagnosed.stderr).contains("left:2: is not sorted"));
    assert!(String::from_utf8_lossy(&diagnosed.stderr).contains("input is not in sorted order"));
}

#[test]
fn invalid_operands_missing_files_and_two_stdin_inputs_fail() {
    let dir = Sandbox::new();
    assert_eq!(code(&dir.run(&["one"], b"")), 1);
    assert_eq!(code(&dir.run(&["one", "two", "three"], b"")), 1);
    assert_eq!(code(&dir.run(&["-", "-"], b"")), 1);
    assert_eq!(code(&dir.run(&["-a", "3", "left", "right"], b"")), 1);
    assert_eq!(code(&dir.run(&["absent", "right"], b"")), 1);
    assert_eq!(code(&dir.run(&["--unknown", "left", "right"], b"")), 1);
}

#[test]
fn broken_pipe_exits_quietly() {
    let dir = Sandbox::new();
    dir.write("left", b"k l\n".repeat(40_000).as_slice());
    dir.write("right", b"k r\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_join"))
        .args(["left", "right"])
        .current_dir(&dir.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert_eq!(code(&out), 0);
}
