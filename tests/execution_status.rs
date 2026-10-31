use std::process::{Command, Output};

fn run(bin: &str, args: &[&str], agent: Option<&str>, mode: &str, no_color: bool) -> Output {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env_remove("AGENTCMD_AGENT")
        .env_remove("AGENTCMD_AGENT_COLOR")
        .env_remove("NO_COLOR")
        .env_remove("TERM")
        .env("AGENTCMD_COLOR", mode);
    if let Some(agent) = agent {
        cmd.env("AGENTCMD_AGENT", agent);
    }
    if no_color {
        cmd.env("NO_COLOR", "1");
    }
    cmd.output().unwrap()
}

#[test]
fn forced_status_preserves_data_and_exit_codes() {
    for (bin, args) in [
        (env!("CARGO_BIN_EXE_agentcmd"), &["echo", "hello"][..]),
        (env!("CARGO_BIN_EXE_echo"), &["hello"][..]),
    ] {
        let out = run(bin, args, Some("builder"), "always", false);
        assert_eq!(out.stdout, b"hello\n");
        assert_eq!(out.status.code(), Some(0));
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.starts_with("\x1b[33m[RUNNING]\x1b[0m  echo         | agent: "));
        assert!(err.contains("\x1b[32m[DONE   ]\x1b[0m  echo         | agent: "));
        assert!(err.contains("builder\x1b[0m | exit: 0 | time: "));
        let elapsed = err.lines().last().unwrap().rsplit_once("time: ").unwrap().1;
        assert!(elapsed.strip_suffix('s').unwrap().parse::<f64>().unwrap() >= 0.0);
    }
    let out = run(
        env!("CARGO_BIN_EXE_agentcmd"),
        &["false"],
        Some("tester"),
        "always",
        false,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("\x1b[31m[EXIT   ]\x1b[0m  false        | agent: "));
}

#[test]
fn disabled_and_captured_auto_are_silent() {
    for (agent, mode, no_color) in [
        (None, "auto", false),
        (Some("builder"), "auto", false),
        (Some("builder"), "never", false),
        (Some("builder"), "always", true),
        (Some("builder"), "unknown", false),
    ] {
        let out = run(
            env!("CARGO_BIN_EXE_agentcmd"),
            &["echo", "hello"],
            agent,
            mode,
            no_color,
        );
        assert_eq!(out.stdout, b"hello\n");
        assert!(out.stderr.is_empty());
        assert!(out.status.success());
    }
}

#[test]
fn agent_colors_are_stable_and_labels_escape_controls() {
    let bin = env!("CARGO_BIN_EXE_agentcmd");
    let first = run(bin, &["true"], Some("a"), "always", false);
    let repeat = run(bin, &["true"], Some("a"), "always", false);
    let other = run(bin, &["true"], Some("b"), "always", false);
    let first = String::from_utf8(first.stderr).unwrap();
    let repeat = String::from_utf8(repeat.stderr).unwrap();
    let other = String::from_utf8(other.stderr).unwrap();
    assert_eq!(first.lines().next(), repeat.lines().next());
    let first_color = first.split("agent: ").nth(1).unwrap();
    let other_color = other.split("agent: ").nth(1).unwrap();
    assert_ne!(&first_color[..5], &other_color[..5]);
    let escaped = run(bin, &["true"], Some("a\n\x1b[31m"), "always", false);
    let err = String::from_utf8(escaped.stderr).unwrap();
    assert_eq!(err.lines().count(), 2);
    assert!(err.contains("a\\n\\u{1b}[31m\x1b[0m"));
    let default = run(bin, &["true"], None, "always", false);
    assert!(String::from_utf8(default.stderr)
        .unwrap()
        .contains("agentcmd\x1b[0m"));
}

#[test]
fn explicit_palette_and_dumb_terminal() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_true"));
    cmd.env("AGENTCMD_AGENT", "tester")
        .env("AGENTCMD_COLOR", "always")
        .env("AGENTCMD_AGENT_COLOR", "cyan")
        .env_remove("NO_COLOR")
        .env_remove("TERM");
    let out = cmd.output().unwrap();
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("agent: \x1b[96mtester\x1b[0m"));
    let out = cmd.env("TERM", "dumb").output().unwrap();
    assert!(out.stderr.is_empty());
}
