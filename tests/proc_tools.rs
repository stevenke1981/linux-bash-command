//! CLI tests for the process tools: `timeout`, `kill`, `pgrep` and `pkill`.
//!
//! Only processes this file spawns are ever signalled. The pgrep and pkill tests use a copy of
//! `sleep` under a unique file name, so the pattern can match nothing else.

mod common;

#[cfg(unix)]
use std::process::{Command, Stdio};

use common::run;

fn code(output: &std::process::Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(unix)]
fn sleep_program() -> std::path::PathBuf {
    ["/bin/sleep", "/usr/bin/sleep"]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|path| path.is_file())
        .expect("a sleep binary")
}

/// Kills and reaps a spawned child when dropped, so a failing test leaves nothing behind.
#[cfg(unix)]
struct Guard(std::process::Child);

#[cfg(unix)]
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits for `child` to exit, with a deadline so a broken test cannot hang the run.
#[cfg(unix)]
fn wait_status(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "child did not exit in time"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Starts a copy of `sleep` whose file name is unique to this test.
#[cfg(unix)]
fn marked_sleeper(sandbox: &common::Sandbox) -> (Guard, String) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let token = format!("agentcmd-uniq-{}-{nanos}", std::process::id());
    let target = sandbox.path().join(&token);
    std::fs::copy(sleep_program(), &target).expect("copy sleep");
    let child = Command::new(&target)
        .arg("1000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn marked sleeper");
    (Guard(child), token)
}

// ---------------------------------------------------------------- timeout

#[cfg(unix)]
#[test]
fn timeout_expiry_exits_124() {
    let out = run("timeout", &["0.1", "sleep", "5"], b"");
    assert_eq!(code(&out), 124, "{}", text(&out.stderr));
}

#[cfg(unix)]
#[test]
fn timeout_passes_command_status_through() {
    let out = run("timeout", &["5", "sh", "-c", "exit 3"], b"");
    assert_eq!(code(&out), 3);
    let ok = run("timeout", &["5", "true"], b"");
    assert_eq!(code(&ok), 0);
}

#[cfg(unix)]
#[test]
fn timeout_kill_signal_exits_137() {
    let out = run("timeout", &["-s", "KILL", "0.1", "sleep", "5"], b"");
    assert_eq!(code(&out), 137);
}

#[cfg(unix)]
#[test]
fn timeout_kill_after_escalates() {
    let out = run(
        "timeout",
        &["-k", "0.1", "0.1", "sh", "-c", "trap '' TERM; sleep 5"],
        b"",
    );
    assert_eq!(code(&out), 137);
}

#[cfg(unix)]
#[test]
fn timeout_preserve_status_reports_the_signal_status() {
    let out = run("timeout", &["--preserve-status", "0.1", "sleep", "5"], b"");
    assert_eq!(code(&out), 128 + 15);
}

#[cfg(unix)]
#[test]
fn timeout_verbose_names_the_signal() {
    let out = run("timeout", &["-v", "0.1", "sleep", "5"], b"");
    assert_eq!(code(&out), 124);
    assert!(text(&out.stderr).contains("sending signal TERM to command 'sleep'"));
}

#[test]
fn timeout_missing_command_exits_127() {
    let out = run("timeout", &["5", "agentcmd-no-such-command-zz"], b"");
    assert_eq!(code(&out), 127);
    assert!(text(&out.stderr).contains("failed to run command 'agentcmd-no-such-command-zz'"));
}

#[test]
fn timeout_zero_duration_runs_without_limit() {
    let out = run("timeout", &["0", "true"], b"");
    assert_eq!(code(&out), 0);
}

#[test]
fn timeout_rejects_bad_duration_and_signal() {
    let duration = run("timeout", &["x", "true"], b"");
    assert_eq!(code(&duration), 125);
    assert!(text(&duration.stderr).contains("invalid time interval 'x'"));
    let signal = run("timeout", &["-s", "BOGUS", "1", "true"], b"");
    assert_eq!(code(&signal), 125);
    assert!(text(&signal.stderr).contains("invalid signal"));
}

#[test]
fn timeout_missing_operands_and_unknown_option_exit_125() {
    assert_eq!(code(&run("timeout", &[], b"")), 125);
    assert_eq!(code(&run("timeout", &["5"], b"")), 125);
    let bad = run("timeout", &["--bogus", "5", "true"], b"");
    assert_eq!(code(&bad), 125);
    assert!(text(&bad.stderr).contains("Try 'timeout --help'"));
}

#[test]
fn timeout_help_and_version_exit_0() {
    let help = run("timeout", &["--help"], b"");
    assert_eq!(code(&help), 0);
    let help_text = text(&help.stdout);
    for option in [
        "--foreground",
        "--kill-after",
        "--preserve-status",
        "--signal",
        "--verbose",
    ] {
        assert!(help_text.contains(option), "{option}");
    }
    let version = run("timeout", &["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(text(&version.stdout).starts_with("timeout (agentcmd) "));
}

// ---------------------------------------------------------------- kill

#[test]
fn kill_lists_signal_names_and_converts_both_ways() {
    let all = run("kill", &["-l"], b"");
    assert_eq!(code(&all), 0);
    let names = text(&all.stdout);
    assert!(names.split_whitespace().any(|name| name == "TERM"));
    assert!(names.split_whitespace().any(|name| name == "KILL"));
    assert_eq!(text(&run("kill", &["-l", "15"], b"").stdout), "TERM\n");
    assert_eq!(text(&run("kill", &["-l", "SIGKILL"], b"").stdout), "9\n");
    assert_eq!(text(&run("kill", &["-l", "term"], b"").stdout), "15\n");
}

#[test]
fn kill_table_starts_with_hup_and_has_31_signals() {
    let out = run("kill", &["-L"], b"");
    assert_eq!(code(&out), 0);
    let table = text(&out.stdout);
    assert!(table.starts_with(" 1 HUP      2 INT"), "{table:?}");
    assert!(table.contains("31 SYS"));
    assert_eq!(table.lines().count(), 5);
}

#[test]
fn kill_without_arguments_is_a_usage_error() {
    let out = run("kill", &[], b"");
    assert_eq!(code(&out), 1);
    assert!(text(&out.stderr).contains("Try 'kill --help'"));
}

#[test]
fn kill_missing_process_reports_no_such_process() {
    let out = run("kill", &["2000000000"], b"");
    assert_eq!(code(&out), 1);
    assert!(text(&out.stderr).contains("(2000000000) - No such process"));
}

#[test]
fn kill_rejects_unknown_signals() {
    let by_option = run("kill", &["-s", "BOGUS", "1"], b"");
    assert_eq!(code(&by_option), 1);
    assert!(text(&by_option.stderr).contains("unknown signal: BOGUS"));
    let by_dash = run("kill", &["-BOGUS", "1"], b"");
    assert_eq!(code(&by_dash), 1);
    assert_eq!(code(&run("kill", &["-l", "999"], b"")), 1);
}

#[test]
fn kill_help_and_version_exit_0() {
    assert_eq!(code(&run("kill", &["--help"], b"")), 0);
    let version = run("kill", &["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(text(&version.stdout).starts_with("kill (agentcmd) "));
}

#[cfg(unix)]
#[test]
fn kill_sends_signals_to_a_spawned_child() {
    let mut child = Command::new(sleep_program())
        .arg("1000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    let pid = child.id().to_string();
    let probe = run("kill", &["-0", &pid], b"");
    assert_eq!(code(&probe), 0, "{}", text(&probe.stderr));
    let sent = run("kill", &["-s", "KILL", &pid], b"");
    assert_eq!(code(&sent), 0);
    let status = wait_status(&mut child);
    assert!(!status.success());
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(9)
    );
}

// ---------------------------------------------------------------- pgrep / pkill

#[test]
fn pgrep_usage_errors_exit_2() {
    let none = run("pgrep", &[], b"");
    assert_eq!(code(&none), 2);
    assert!(text(&none.stderr).contains("no matching criteria specified"));
    assert_eq!(code(&run("pgrep", &["("], b"")), 2);
    assert_eq!(code(&run("pgrep", &["-Q", "x"], b"")), 2);
    assert_eq!(
        code(&run("pgrep", &["-u", "agentcmd-no-such-user-zz", "x"], b"")),
        2
    );
    assert_eq!(code(&run("pgrep", &["a", "b"], b"")), 2);
}

#[test]
fn pgrep_no_match_exits_1() {
    let out = run("pgrep", &["agentcmd-zz-no-such-process-9f3e"], b"");
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
}

#[test]
fn pgrep_count_prints_zero_for_no_match() {
    let out = run("pgrep", &["-c", "agentcmd-zz-no-such-process-9f3e"], b"");
    assert_eq!(text(&out.stdout), "0\n");
    assert_eq!(code(&out), 1);
}

#[test]
fn pgrep_help_and_version() {
    let help = run("pgrep", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help.stdout).contains("--count"));
    let version = run("pgrep", &["--version"], b"");
    assert!(text(&version.stdout).starts_with("pgrep (agentcmd) "));
}

#[test]
fn pkill_help_version_and_no_match() {
    assert_eq!(code(&run("pkill", &["--help"], b"")), 0);
    assert!(text(&run("pkill", &["--version"], b"").stdout).starts_with("pkill (agentcmd) "));
    let none = run("pkill", &["agentcmd-zz-no-such-process-9f3e"], b"");
    assert_eq!(code(&none), 1);
    assert_eq!(code(&run("pkill", &[], b"")), 2);
}

#[cfg(unix)]
#[test]
fn pgrep_finds_a_marked_child_by_full_command_line() {
    let sandbox = common::Sandbox::new();
    let (guard, token) = marked_sleeper(&sandbox);
    let pid = guard.0.id().to_string();
    let out = run("pgrep", &["-f", &token], b"");
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("{pid}\n"));
    let listed = run("pgrep", &["-l", "-f", &token], b"");
    assert!(text(&listed.stdout).starts_with(&format!("{pid} ")));
    let counted = run("pgrep", &["-c", "-f", &token], b"");
    assert_eq!(text(&counted.stdout), "1\n");
    let exact = run("pgrep", &["-x", &token], b"");
    assert_eq!(
        code(&exact),
        1,
        "a full-name pattern must not match the command line"
    );
}

#[cfg(unix)]
#[test]
fn pgrep_inverse_and_parent_filters() {
    let sandbox = common::Sandbox::new();
    let (guard, token) = marked_sleeper(&sandbox);
    let pid = guard.0.id();
    let other = run("pgrep", &["-v", "-f", &token], b"");
    assert!(!text(&other.stdout)
        .lines()
        .any(|line| line == pid.to_string()));
    // The sleeper was started by this test process, so it is a child of it.
    let own_pid = std::process::id().to_string();
    let children = run("pgrep", &["-P", &own_pid, "-f", &token], b"");
    assert_eq!(code(&children), 0, "{}", text(&children.stderr));
    assert_eq!(text(&children.stdout), format!("{pid}\n"));
    let other_parent = run("pgrep", &["-P", "2000000000", "-f", &token], b"");
    assert_eq!(code(&other_parent), 1);
}

#[cfg(unix)]
#[test]
fn pkill_terminates_only_matching_children() {
    let sandbox = common::Sandbox::new();
    let (mut guard, token) = marked_sleeper(&sandbox);
    let out = run("pkill", &["-f", &token], b"");
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    let status = wait_status(&mut guard.0);
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(15)
    );
}

#[cfg(unix)]
#[test]
fn pkill_accepts_signal_shorthand_and_option_forms() {
    let sandbox = common::Sandbox::new();
    let (mut guard, token) = marked_sleeper(&sandbox);
    let out = run("pkill", &["-9", "-f", &token], b"");
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    let status = wait_status(&mut guard.0);
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(9)
    );

    let (mut second, token) = marked_sleeper(&sandbox);
    let out = run("pkill", &["--signal=KILL", "-f", &token], b"");
    assert_eq!(code(&out), 0);
    let status = wait_status(&mut second.0);
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(9)
    );
}

#[cfg(unix)]
#[test]
fn pkill_rejects_unknown_signal() {
    let out = run(
        "pkill",
        &["--signal=BOGUS", "agentcmd-zz-no-such-process-9f3e"],
        b"",
    );
    assert_eq!(code(&out), 2);
    assert!(text(&out.stderr).contains("unknown signal"));
}

#[cfg(unix)]
#[test]
fn pgrep_case_list_delimiter_and_selection_options() {
    let sandbox = common::Sandbox::new();
    let (guard, token) = marked_sleeper(&sandbox);
    let pid = guard.0.id().to_string();
    let ignored_case = run("pgrep", &["-i", "-f", &token.to_uppercase()], b"");
    assert_eq!(text(&ignored_case.stdout), format!("{pid}\n"));
    let full = run("pgrep", &["-a", "-f", &token], b"");
    let listed = text(&full.stdout);
    assert!(
        listed.starts_with(&format!("{pid} ")) && listed.contains(&token),
        "{listed}"
    );
    let delimited = run("pgrep", &["-d", ",", "-f", &token], b"");
    assert_eq!(text(&delimited.stdout), format!("{pid}\n"));
    let newest = run("pgrep", &["-n", "-f", &token], b"");
    assert_eq!(text(&newest.stdout), format!("{pid}\n"));
    let oldest = run("pgrep", &["-o", "-f", &token], b"");
    assert_eq!(text(&oldest.stdout), format!("{pid}\n"));
    // The sleeper runs as this user, so both user selectors find it.
    let uid = unsafe { libc::geteuid() }.to_string();
    let by_effective = run("pgrep", &["-u", &uid, "-f", &token], b"");
    assert_eq!(text(&by_effective.stdout), format!("{pid}\n"));
    let real_uid = unsafe { libc::getuid() }.to_string();
    let by_real = run("pgrep", &["-U", &real_uid, "-f", &token], b"");
    assert_eq!(text(&by_real.stdout), format!("{pid}\n"));
}

#[cfg(unix)]
#[test]
fn pkill_echo_names_each_process_it_signals() {
    let sandbox = common::Sandbox::new();
    let (mut guard, token) = marked_sleeper(&sandbox);
    let pid = guard.0.id();
    let out = run("pkill", &["-e", "-f", &token], b"");
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    let echoed = text(&out.stdout);
    assert!(echoed.contains(&format!("({pid}) TERM")), "{echoed}");
    let status = wait_status(&mut guard.0);
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(15)
    );
}
