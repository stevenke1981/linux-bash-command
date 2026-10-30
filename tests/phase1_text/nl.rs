use crate::common;
#[test]
fn nl_00_empty() {
    let sb = common::Sandbox::new();
    let out = sb.run("nl", &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn nl_01_default() {
    let sb = common::Sandbox::new();
    let out = sb.run("nl", &[], &[97, 10, 10, 98, 10]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![
            32, 32, 32, 32, 32, 49, 9, 97, 10, 32, 32, 32, 32, 32, 32, 32, 10, 32, 32, 32, 32, 32,
            50, 9, 98, 10
        ]
    );
}
#[test]
fn nl_02_all() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[std::str::from_utf8(&[
            45, 45, 98, 111, 100, 121, 45, 110, 117, 109, 98, 101, 114, 105, 110, 103, 61, 97,
        ])
        .unwrap()],
        &[97, 10, 10, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![
            32, 32, 32, 32, 32, 49, 9, 97, 10, 32, 32, 32, 32, 32, 50, 9, 10, 32, 32, 32, 32, 32,
            51, 9, 98, 10
        ]
    );
}
#[test]
fn nl_03_none() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[std::str::from_utf8(&[45, 98, 110]).unwrap()],
        &[97, 98, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, vec![32, 32, 32, 32, 32, 32, 32, 97, 98, 99, 10]);
}
#[test]
fn nl_04_pattern() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[std::str::from_utf8(&[45, 98, 112, 94, 97]).unwrap()],
        &[97, 98, 99, 10, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![32, 32, 32, 32, 32, 49, 9, 97, 98, 99, 10, 32, 32, 32, 32, 32, 32, 32, 98, 10]
    );
}
#[test]
fn nl_05_sections() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[
                45, 45, 104, 101, 97, 100, 101, 114, 45, 110, 117, 109, 98, 101, 114, 105, 110,
                103, 61, 97,
            ])
            .unwrap(),
            std::str::from_utf8(&[
                45, 45, 102, 111, 111, 116, 101, 114, 45, 110, 117, 109, 98, 101, 114, 105, 110,
                103, 61, 97,
            ])
            .unwrap(),
        ],
        &[
            92, 58, 92, 58, 92, 58, 10, 104, 10, 92, 58, 92, 58, 10, 98, 10, 92, 58, 10, 102, 10,
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![
            10, 32, 32, 32, 32, 32, 49, 9, 104, 10, 10, 32, 32, 32, 32, 32, 49, 9, 98, 10, 10, 32,
            32, 32, 32, 32, 49, 9, 102, 10
        ]
    );
}
#[test]
fn nl_06_delimiter() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[
                45, 45, 115, 101, 99, 116, 105, 111, 110, 45, 100, 101, 108, 105, 109, 105, 116,
                101, 114, 61, 88, 89,
            ])
            .unwrap(),
            std::str::from_utf8(&[45, 104, 97]).unwrap(),
        ],
        &[
            88, 89, 88, 89, 88, 89, 10, 104, 10, 88, 89, 88, 89, 10, 98, 10,
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![10, 32, 32, 32, 32, 32, 49, 9, 104, 10, 10, 32, 32, 32, 32, 32, 49, 9, 98, 10]
    );
}
#[test]
fn nl_07_increment() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[
                45, 45, 108, 105, 110, 101, 45, 105, 110, 99, 114, 101, 109, 101, 110, 116, 61, 50,
            ])
            .unwrap(),
            std::str::from_utf8(&[
                45, 45, 115, 116, 97, 114, 116, 105, 110, 103, 45, 108, 105, 110, 101, 45, 110,
                117, 109, 98, 101, 114, 61, 45, 50,
            ])
            .unwrap(),
        ],
        &[97, 10, 98, 10, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![
            32, 32, 32, 32, 45, 50, 9, 97, 10, 32, 32, 32, 32, 32, 48, 9, 98, 10, 32, 32, 32, 32,
            32, 50, 9, 99, 10
        ]
    );
}
#[test]
fn nl_08_blanks() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[45, 98, 97]).unwrap(),
            std::str::from_utf8(&[
                45, 45, 106, 111, 105, 110, 45, 98, 108, 97, 110, 107, 45, 108, 105, 110, 101, 115,
                61, 50,
            ])
            .unwrap(),
        ],
        &[10, 10, 10, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![
            32, 32, 32, 32, 32, 32, 32, 10, 32, 32, 32, 32, 32, 49, 9, 10, 32, 32, 32, 32, 32, 32,
            32, 10, 32, 32, 32, 32, 32, 50, 9, 10
        ]
    );
}
#[test]
fn nl_09_left() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[
                45, 45, 110, 117, 109, 98, 101, 114, 45, 102, 111, 114, 109, 97, 116, 61, 108, 110,
            ])
            .unwrap(),
            std::str::from_utf8(&[
                45, 45, 110, 117, 109, 98, 101, 114, 45, 119, 105, 100, 116, 104, 61, 51,
            ])
            .unwrap(),
            std::str::from_utf8(&[
                45, 45, 110, 117, 109, 98, 101, 114, 45, 115, 101, 112, 97, 114, 97, 116, 111, 114,
                61, 58,
            ])
            .unwrap(),
        ],
        &[97, 10, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![49, 32, 32, 58, 97, 10, 50, 32, 32, 58, 98, 10]
    );
}
#[test]
fn nl_10_zero() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[45, 110, 114, 122]).unwrap(),
            std::str::from_utf8(&[45, 119, 51]).unwrap(),
        ],
        &[97, 10, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![48, 48, 49, 9, 97, 10, 48, 48, 50, 9, 98, 10]
    );
}
#[test]
fn nl_11_no_reset() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[45, 45, 110, 111, 45, 114, 101, 110, 117, 109, 98, 101, 114])
                .unwrap(),
        ],
        &[97, 10, 92, 58, 92, 58, 10, 98, 10],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![32, 32, 32, 32, 32, 49, 9, 97, 10, 10, 32, 32, 32, 32, 32, 50, 9, 98, 10]
    );
}
#[test]
fn nl_12_files() {
    let sb = common::Sandbox::new();
    sb.write("a", [97, 10]);
    sb.write("b", [98, 10]);
    let out = sb.run(
        "nl",
        &[
            std::str::from_utf8(&[97]).unwrap(),
            std::str::from_utf8(&[98]).unwrap(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        vec![32, 32, 32, 32, 32, 49, 9, 97, 10, 32, 32, 32, 32, 32, 50, 9, 98, 10]
    );
}
#[test]
fn nl_13_missing() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[std::str::from_utf8(&[97, 98, 115, 101, 110, 116]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn nl_14_invalid() {
    let sb = common::Sandbox::new();
    let out = sb.run("nl", &[std::str::from_utf8(&[45, 119, 48]).unwrap()], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn nl_15_unknown() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[std::str::from_utf8(&[45, 45, 117, 110, 107, 110, 111, 119, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, vec![]);
}
#[test]
fn nl_16_help() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[std::str::from_utf8(&[45, 45, 104, 101, 108, 112]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
#[test]
fn nl_17_version() {
    let sb = common::Sandbox::new();
    let out = sb.run(
        "nl",
        &[std::str::from_utf8(&[45, 45, 118, 101, 114, 115, 105, 111, 110]).unwrap()],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}
