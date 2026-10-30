use crate::common;
#[test]
fn unexpand_00_empty() {
    let sb = common::Sandbox::new();
    let out = sb.run("unexpand", &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn unexpand_01_default() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[],
        &[
            32, 32, 32, 32, 32, 32, 32, 32, 120, 32, 32, 32, 32, 32, 32, 32, 32, 121, 10,
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![9, 120, 32, 32, 32, 32, 32, 32, 32, 32, 121, 10]
    );
}
#[test]
fn unexpand_02_all() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 45, 97, 108, 108]).unwrap()],
        &[97, 32, 32, 32, 32, 32, 32, 32, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 9, 98, 10]);
}
#[test]
fn unexpand_03_first() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[
            std::str::from_utf8(&[45, 97]).unwrap(),
            std::str::from_utf8(&[45, 45, 102, 105, 114, 115, 116, 45, 111, 110, 108, 121])
                .unwrap(),
        ],
        &[
            32, 32, 32, 32, 32, 32, 32, 32, 120, 32, 32, 32, 32, 32, 32, 32, 121, 10,
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![9, 120, 32, 32, 32, 32, 32, 32, 32, 121, 10]
    );
}
#[test]
fn unexpand_04_size() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 45, 116, 97, 98, 115, 61, 52]).unwrap()],
        &[97, 32, 32, 32, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 9, 98, 10]);
}
#[test]
fn unexpand_05_list() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 116, 50, 44, 53]).unwrap()],
        &[32, 32, 32, 32, 32, 120, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![9, 9, 120, 10]);
}
#[test]
fn unexpand_06_absolute() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 116, 51, 44, 47, 52]).unwrap()],
        &[32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 120, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![9, 9, 9, 9, 120, 10]);
}
#[test]
fn unexpand_07_relative() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 116, 51, 44, 43, 52]).unwrap()],
        &[32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 120, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![9, 9, 9, 120, 10]);
}
#[test]
fn unexpand_08_mixed() {
    let sb = common::Sandbox::new();
    let out = sb.run("unexpand", &[], &[32, 9, 32, 32, 120, 10]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![9, 32, 32, 120, 10]);
}
#[test]
fn unexpand_09_partial() {
    let sb = common::Sandbox::new();
    let out = sb.run("unexpand", &[], &[32, 32, 32, 32, 32, 32, 32, 32]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![9]);
}
#[test]
fn unexpand_10_files() {
    let sb = common::Sandbox::new();
    sb.write("a", [32, 32, 32, 32, 32, 32, 32, 32, 120, 10]);
    sb.write("b", [32, 32, 32, 32, 32, 32, 32, 32, 121, 10]);
    let out = sb.run(
        "unexpand",
        &[
            std::str::from_utf8(&[97]).unwrap(),
            std::str::from_utf8(&[98]).unwrap(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![9, 120, 10, 9, 121, 10]);
}
#[test]
fn unexpand_11_stdin_dash() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45]).unwrap()],
        &[32, 32, 32, 32, 32, 32, 32, 32, 120, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![9, 120, 10]);
}
#[test]
fn unexpand_12_missing() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[97, 98, 115, 101, 110, 116]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn unexpand_13_invalid() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 116, 48]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn unexpand_14_unknown() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 45, 117, 110, 107, 110, 111, 119, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn unexpand_15_help() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 45, 104, 101, 108, 112]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
#[test]
fn unexpand_16_version() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "unexpand",
        &[std::str::from_utf8(&[45, 45, 118, 101, 114, 115, 105, 111, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
