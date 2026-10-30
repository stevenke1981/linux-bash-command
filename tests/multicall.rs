use std::fs;
use std::process::Command;

#[test]
fn list_dispatch_help_and_errors() {
    let bin = env!("CARGO_BIN_EXE_agentcmd");
    let list = Command::new(bin).arg("--list").output().unwrap();
    assert!(list.status.success());
    let names = String::from_utf8(list.stdout).unwrap();
    assert_eq!(names.lines().count(), 115);
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
fn every_utility_has_a_binary() {
    let dir = std::path::Path::new(env!("CARGO_BIN_EXE_agentcmd"))
        .parent()
        .unwrap()
        .to_path_buf();
    let missing: Vec<_> = agentcmd::registry::UTILITIES
        .iter()
        .filter(|u| u.name != "[")
        .filter(|u| {
            !dir.join(format!("{}{}", u.name, std::env::consts::EXE_SUFFIX))
                .exists()
        })
        .map(|u| u.name)
        .collect();
    assert!(missing.is_empty(), "missing binaries: {missing:?}");
}
