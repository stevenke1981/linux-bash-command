use crate::common;
#[test]
fn expand_00_empty() {
    let sb = common::Sandbox::new();
    let out = sb.run("expand", &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn expand_01_default() {
    let sb = common::Sandbox::new();
    let out = sb.run("expand", &[], &[97, 9, 98, 10]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 32, 32, 32, 32, 32, 32, 32, 98, 10]);
}
#[test]
fn expand_02_initial() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 45, 105, 110, 105, 116, 105, 97, 108]).unwrap()],
        &[9, 97, 9, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![32, 32, 32, 32, 32, 32, 32, 32, 97, 9, 98, 10]
    );
}
#[test]
fn expand_03_size() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 45, 116, 97, 98, 115, 61, 52]).unwrap()],
        &[97, 9, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 32, 32, 32, 98, 10]);
}
#[test]
fn expand_04_list() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 116, 50, 44, 53]).unwrap()],
        &[9, 9, 9, 88, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![32, 32, 32, 32, 32, 32, 88, 10]);
}
#[test]
fn expand_05_absolute() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 116, 51, 44, 47, 52]).unwrap()],
        &[9, 9, 9, 120, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![32, 32, 32, 32, 32, 32, 32, 32, 120, 10]);
}
#[test]
fn expand_06_relative() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 116, 51, 44, 43, 52]).unwrap()],
        &[9, 9, 9, 120, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 120, 10]
    );
}
#[test]
fn expand_07_old() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 52]).unwrap()],
        &[9, 97, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![32, 32, 32, 32, 97, 10]);
}
#[test]
fn expand_08_backspace() {
    let sb = common::Sandbox::new();
    let out = sb.run("expand", &[], &[97, 98, 99, 8, 9, 88, 10]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![97, 98, 99, 8, 32, 32, 32, 32, 32, 32, 88, 10]
    );
}
#[test]
fn expand_09_partial() {
    let sb = common::Sandbox::new();
    let out = sb.run("expand", &[], &[9]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![32, 32, 32, 32, 32, 32, 32, 32]);
}
#[test]
fn expand_10_files() {
    let sb = common::Sandbox::new();
    sb.write("a", [97, 9, 88, 10]);
    sb.write("b", [98, 9, 89, 10]);
    let out = sb.run(
        "expand",
        &[
            std::str::from_utf8(&[97]).unwrap(),
            std::str::from_utf8(&[98]).unwrap(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![97, 32, 32, 32, 32, 32, 32, 32, 88, 10, 98, 32, 32, 32, 32, 32, 32, 32, 89, 10]
    );
}
#[test]
fn expand_11_stdin_dash() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45]).unwrap()],
        &[9, 120, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![32, 32, 32, 32, 32, 32, 32, 32, 120, 10]);
}
#[test]
fn expand_12_missing() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[97, 98, 115, 101, 110, 116]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn expand_13_invalid() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 116, 48]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn expand_14_unknown() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 45, 117, 110, 107, 110, 111, 119, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn expand_15_help() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 45, 104, 101, 108, 112]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
#[test]
fn expand_16_version() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "expand",
        &[std::str::from_utf8(&[45, 45, 118, 101, 114, 115, 105, 111, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
