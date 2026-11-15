mod common;
use std::fs;
use std::process::Command;

#[test]
fn list_dispatch_help_and_errors() {
    let bin = env!("CARGO_BIN_EXE_agentcmd");
    let list = Command::new(bin).arg("--list").output().unwrap();
    assert!(list.status.success());
    let names = String::from_utf8(list.stdout).unwrap();
    assert_eq!(names.lines().count(), agentcmd::registry::UTILITIES.len());
    assert_eq!(
        names.lines().collect::<Vec<_>>(),
        agentcmd::registry::UTILITIES
            .iter()
            .map(|u| u.name)
            .collect::<Vec<_>>()
    );
    for args in [
        &[][..],
        &["--list", "extra"][..],
        &["unknown"][..],
        &["--unknown"][..],
    ] {
        assert_eq!(
            Command::new(bin).args(args).output().unwrap().status.code(),
            Some(1)
        );
    }
    for args in [["--help", "cat"], ["help", "cat"]] {
        assert!(Command::new(bin)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
}

#[test]
fn direct_and_multicall_produce_same_output() {
    let root = std::env::temp_dir().join(format!("agentcmd-multicall-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let data = root.join("input");
    fs::write(&data, b"one\ntwo\n").unwrap();
    let direct = Command::new(env!("CARGO_BIN_EXE_cat"))
        .arg(&data)
        .output()
        .unwrap();
    let multi = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .arg("cat")
        .arg(&data)
        .output()
        .unwrap();
    assert_eq!(direct.stdout, multi.stdout);
    assert_eq!(direct.status.code(), multi.status.code());
    let copied = root.join(format!("head{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_agentcmd"), &copied).unwrap();
    let output = Command::new(copied).arg("-n1").arg(&data).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"one\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn multi_call_diagnostics_and_version_are_stable() {
    let bin = env!("CARGO_BIN_EXE_agentcmd");
    for (args, message) in [
        (vec![], "agentcmd: missing argument\n"),
        (vec!["--list", "extra"], "agentcmd: invalid argument\n"),
        (
            vec!["unknown"],
            "agentcmd: function/utility not found: unknown\n",
        ),
        (
            vec!["--unknown"],
            "agentcmd: unrecognized option '--unknown'\n",
        ),
    ] {
        let output = Command::new(bin).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stderr, message.as_bytes());
        assert!(output.stdout.is_empty());
    }
    for flag in ["--version", "-V"] {
        let output = Command::new(bin).arg(flag).output().unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            output.stdout,
            format!(
                "agentcmd {} (multi-call binary)\n",
                env!("CARGO_PKG_VERSION")
            )
            .as_bytes()
        );
    }
}

#[test]
fn prefixed_argv0_dispatches_and_utils_suffix_enters_multi_call() {
    let root = std::env::temp_dir().join(format!("agentcmd-argv0-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let prefixed = root.join(format!("x86_64-head{}", std::env::consts::EXE_SUFFIX));
    let utils = root.join(format!("agentutils{}", std::env::consts::EXE_SUFFIX));
    let data = root.join("input");
    fs::copy(env!("CARGO_BIN_EXE_agentcmd"), &prefixed).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_agentcmd"), &utils).unwrap();
    fs::write(&data, b"one\ntwo\n").unwrap();
    let mut command = Command::new(prefixed);
    command.arg("-n1").arg(data);
    let output = common::execute(command, b"");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"one\n");
    let mut command = Command::new(utils);
    command.arg("--list");
    let output = common::execute(command, b"");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().lines().count(),
        agentcmd::registry::UTILITIES.len()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn multi_call_output_handles_closed_pipe() {
    use std::process::Stdio;
    for arg in ["--list", "--help", "--version"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
            .arg(arg)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{arg}");
        assert!(output.stderr.is_empty(), "{arg}: {:?}", output.stderr);
    }
}
