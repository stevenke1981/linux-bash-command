use crate::common;
#[test]
fn fold_00_empty() {
    let sb = common::Sandbox::new();
    let out = sb.run("fold", &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn fold_01_hard() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 119, 51]).unwrap()],
        &[97, 98, 99, 100, 101, 102, 103, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 99, 10, 100, 101, 102, 10, 103, 10]);
}
#[test]
fn fold_02_spaces() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[
            std::str::from_utf8(&[45, 45, 115, 112, 97, 99, 101, 115]).unwrap(),
            std::str::from_utf8(&[45, 45, 119, 105, 100, 116, 104, 61, 53]).unwrap(),
        ],
        &[97, 97, 32, 98, 98, 32, 99, 99, 32, 100, 100, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![97, 97, 32, 10, 98, 98, 32, 10, 99, 99, 32, 100, 100, 10]
    );
}
#[test]
fn fold_03_bytes() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[
            std::str::from_utf8(&[45, 45, 98, 121, 116, 101, 115]).unwrap(),
            std::str::from_utf8(&[45, 119, 50]).unwrap(),
        ],
        &[97, 9, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 9, 10, 98, 10]);
}
#[test]
fn fold_04_tab() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 119, 56]).unwrap()],
        &[97, 9, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 9, 10, 98, 10]);
}
#[test]
fn fold_05_backspace() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 119, 51]).unwrap()],
        &[97, 98, 99, 8, 100, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 99, 8, 100, 10]);
}
#[test]
fn fold_06_carriage() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 119, 51]).unwrap()],
        &[97, 98, 13, 97, 98, 99, 100, 101, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 13, 97, 98, 99, 10, 100, 101, 10]);
}
#[test]
fn fold_07_old() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 52]).unwrap()],
        &[97, 98, 99, 100, 101, 102, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 99, 100, 10, 101, 102, 10]);
}
#[test]
fn fold_08_partial() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 119, 50]).unwrap()],
        &[97, 98, 99],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 10, 99]);
}
#[test]
fn fold_09_raw() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 119, 49]).unwrap()],
        &[195, 169, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![195, 10, 169, 10]);
}
#[test]
fn fold_10_files() {
    let sb = common::Sandbox::new();
    sb.write("a", [97, 98, 99, 10]);
    sb.write("b", [100, 101, 102, 10]);
    let out = sb.run(
        "fold",
        &[
            std::str::from_utf8(&[45, 119, 50]).unwrap(),
            std::str::from_utf8(&[97]).unwrap(),
            std::str::from_utf8(&[98]).unwrap(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 10, 99, 10, 100, 101, 10, 102, 10]);
}
#[test]
fn fold_11_stdin_dash() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[
            std::str::from_utf8(&[45, 119, 50]).unwrap(),
            std::str::from_utf8(&[45]).unwrap(),
        ],
        &[97, 98, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![97, 98, 10, 99, 10]);
}
#[test]
fn fold_12_missing() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[97, 98, 115, 101, 110, 116]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn fold_13_invalid() {
    let sb = common::Sandbox::new();
    let out = sb.run("fold", &[std::str::from_utf8(&[45, 119, 48]).unwrap()], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn fold_14_unknown() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 45, 117, 110, 107, 110, 111, 119, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn fold_15_help() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 45, 104, 101, 108, 112]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
#[test]
fn fold_16_version() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "fold",
        &[std::str::from_utf8(&[45, 45, 118, 101, 114, 115, 105, 111, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
