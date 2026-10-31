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
        assert!(err.contains("[builder]\x1b[0m \x1b[33mRUNNING\x1b[0m echo"));
        assert!(err.contains("\x1b[32mDONE\x1b[0m echo (exit 0)"));
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
        .contains("\x1b[31mEXIT\x1b[0m false (exit 1)"));
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
    assert_eq!(first.stderr, repeat.stderr);
    assert_ne!(&first.stderr[..5], &other.stderr[..5]);
    let escaped = run(bin, &["true"], Some("a\n\x1b[31m"), "always", false);
    let err = String::from_utf8(escaped.stderr).unwrap();
    assert_eq!(err.lines().count(), 2);
    assert!(err.contains("[a\\n\\u{1b}[31m]"));
    let default = run(bin, &["true"], None, "always", false);
    assert!(String::from_utf8(default.stderr)
        .unwrap()
        .contains("[agentcmd]"));
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
    assert!(out.stderr.starts_with(b"\x1b[96m[tester]"));
    let out = cmd.env("TERM", "dumb").output().unwrap();
    assert!(out.stderr.is_empty());
}
