use crate::common;
#[test]
fn tac_00_empty() {
    let sb = common::Sandbox::new();
    let out = sb.run("tac", &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn tac_01_terminated() {
    let sb = common::Sandbox::new();
    let out = sb.run("tac", &[], &[97, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![98, 10, 97, 10]);
}
#[test]
fn tac_02_partial() {
    let sb = common::Sandbox::new();
    let out = sb.run("tac", &[], &[97, 10, 98]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![98, 97, 10]);
}
#[test]
fn tac_03_before() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[std::str::from_utf8(&[45, 45, 98, 101, 102, 111, 114, 101]).unwrap()],
        &[97, 10, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![10, 10, 98, 97]);
}
#[test]
fn tac_04_separator() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[std::str::from_utf8(&[
            45, 45, 115, 101, 112, 97, 114, 97, 116, 111, 114, 61, 58, 58,
        ])
        .unwrap()],
        &[97, 58, 58, 98, 58, 58],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![98, 58, 58, 97, 58, 58]);
}
#[test]
fn tac_05_regex() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[
            std::str::from_utf8(&[45, 45, 114, 101, 103, 101, 120]).unwrap(),
            std::str::from_utf8(&[45, 115]).unwrap(),
            std::str::from_utf8(&[91, 48, 45, 57, 93, 91, 48, 45, 57, 93, 42]).unwrap(),
        ],
        &[97, 49, 98, 50, 50, 99],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![99, 50, 98, 50, 97, 49]);
}
#[test]
fn tac_06_regex_before() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[
            std::str::from_utf8(&[45, 114, 98]).unwrap(),
            std::str::from_utf8(&[45, 115]).unwrap(),
            std::str::from_utf8(&[91, 48, 45, 57, 93]).unwrap(),
        ],
        &[97, 49, 98, 50],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![50, 49, 98, 97]);
}
#[test]
fn tac_07_nul() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[
            std::str::from_utf8(&[45, 115]).unwrap(),
            std::str::from_utf8(&[]).unwrap(),
        ],
        &[97, 0, 98, 0],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![98, 0, 97, 0]);
}
#[test]
fn tac_08_blank() {
    let sb = common::Sandbox::new();
    let out = sb.run("tac", &[], &[97, 10, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![98, 10, 10, 97, 10]);
}
#[test]
fn tac_09_single() {
    let sb = common::Sandbox::new();
    let out = sb.run("tac", &[], &[97, 98, 99]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 99]);
}
#[test]
fn tac_10_files() {
    let sb = common::Sandbox::new();
    sb.write("a", [97, 10, 98, 10]);
    sb.write("b", [99, 10, 100]);
    let out = sb.run(
        "tac",
        &[
            std::str::from_utf8(&[97]).unwrap(),
            std::str::from_utf8(&[98]).unwrap(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![98, 10, 97, 10, 100, 99, 10]);
}
#[test]
fn tac_11_before_file() {
    let sb = common::Sandbox::new();
    sb.write("a", [97, 10, 98, 10]);
    let out = sb.run(
        "tac",
        &[
            std::str::from_utf8(&[45, 98]).unwrap(),
            std::str::from_utf8(&[97]).unwrap(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![10, 10, 98, 97]);
}
#[test]
fn tac_12_stdin_dash() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[std::str::from_utf8(&[45]).unwrap()],
        &[97, 10, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![98, 10, 97, 10]);
}
#[test]
fn tac_13_missing() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[std::str::from_utf8(&[97, 98, 115, 101, 110, 116]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn tac_14_invalid() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[
            std::str::from_utf8(&[45, 114]).unwrap(),
            std::str::from_utf8(&[45, 115]).unwrap(),
            std::str::from_utf8(&[91]).unwrap(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn tac_15_unknown() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[std::str::from_utf8(&[45, 45, 117, 110, 107, 110, 111, 119, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn tac_16_help() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[std::str::from_utf8(&[45, 45, 104, 101, 108, 112]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
#[test]
fn tac_17_version() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "tac",
        &[std::str::from_utf8(&[45, 45, 118, 101, 114, 115, 105, 111, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
