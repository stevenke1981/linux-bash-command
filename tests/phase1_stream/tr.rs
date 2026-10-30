use crate::common::{run, Sandbox};

#[test]
fn tr_fixed_repeat_in_first_set() {
    let out = run("tr", &["-d", "[x*2]"], b"xyz");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"yz");
}

#[test]
fn tr_same_case_classes() {
    let out = run("tr", &["[:upper:]", "[:upper:]"], b"ABC xyz\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"ABC xyz\n");
}

#[test]
fn tr_short_class_cannot_pad() {
    let out = run("tr", &["-c", "[:upper:]", "[:lower:]"], b"ABC xyz\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn tr_translate() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["a-z", "A-Z"], &[97, 98, 99, 32, 88, 89, 90, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[65, 66, 67, 32, 88, 89, 90, 10]);
}
#[test]
fn tr_padding() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["abc", "xy"], &[97, 98, 99, 32, 99, 97, 98, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[120, 121, 121, 32, 121, 120, 121, 10]);
}
#[test]
fn tr_truncate() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["--truncate-set1", "abc", "x"], &[97, 98, 99]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[120, 98, 99]);
}
#[test]
fn tr_delete() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["--delete", "a-z"], &[97, 98, 99, 49, 50, 51, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 50, 51, 10]);
}
#[test]
fn tr_squeeze() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "tr",
        &["--squeeze-repeats", " "],
        &[97, 32, 32, 32, 98, 32, 32, 99, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 32, 98, 32, 99, 10]);
}
#[test]
fn tr_translate_squeeze() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "tr",
        &["-s", "ab", "x"],
        &[97, 97, 98, 98, 98, 32, 99, 98, 97],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[120, 32, 99, 120]);
}
#[test]
fn tr_delete_squeeze() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "tr",
        &["-ds", "0-9", " "],
        &[49, 32, 32, 50, 97, 32, 32, 51],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[32, 97, 32]);
}
#[test]
fn tr_complement() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["--complement", "a-z", "_"], &[97, 66, 49, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 95, 95, 95]);
}
#[test]
fn tr_complement_upper_c() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-Cd", "a-z"], &[97, 66, 49, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97]);
}
#[test]
fn tr_paired_classes() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "tr",
        &["[:upper:]", "[:lower:]"],
        &[65, 66, 67, 32, 120, 121, 122, 10],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 99, 32, 120, 121, 122, 10]);
}
#[test]
fn tr_digit_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "[:digit:]"], &[97, 49, 98, 50, 99, 51]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 99]);
}
#[test]
fn tr_space_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "tr",
        &["-s", "[:space:]"],
        &[97, 9, 9, 10, 10, 11, 11, 32, 98],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 9, 10, 11, 32, 98]);
}
#[test]
fn tr_alnum_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-cd", "[:alnum:]"], &[97, 66, 49, 33, 10]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 66, 49]);
}
#[test]
fn tr_alpha_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "[:alpha:]"], &[97, 66, 49, 50, 51]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[49, 50, 51]);
}
#[test]
fn tr_blank_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "[:blank:]"], &[97, 32, 9, 10, 98]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 10, 98]);
}
#[test]
fn tr_cntrl_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "[:cntrl:]"], &[97, 9, 10, 98]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98]);
}
#[test]
fn tr_graph_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-cd", "[:graph:]"], &[32, 97, 33, 9, 98]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 33, 98]);
}
#[test]
fn tr_print_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-cd", "[:print:]"], &[32, 97, 33, 9, 98]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[32, 97, 33, 98]);
}
#[test]
fn tr_punct_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "[:punct:]"], &[97, 33, 63, 46, 98]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98]);
}
#[test]
fn tr_xdigit_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "[:xdigit:]"], &[48, 97, 102, 71, 90]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[71, 90]);
}
#[test]
fn tr_equivalence() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["[=a=]", "x"], &[97, 98, 97]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[120, 98, 120]);
}
#[test]
fn tr_repeat_fixed() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["abc", "[x*2]y"], &[97, 98, 99]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[120, 120, 121]);
}
#[test]
fn tr_repeat_octal() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "tr",
        &["a-j", "[x*010]yz"],
        &[97, 98, 99, 100, 101, 102, 103, 104, 105, 106],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(
        out.stdout,
        &[120, 120, 120, 120, 120, 120, 120, 120, 121, 122]
    );
}
#[test]
fn tr_repeat_fill() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["abc", "[x*]"], &[97, 98, 99]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[120, 120, 120]);
}
#[test]
fn tr_octal_escapes() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["\\141\\142", "xy"], &[97, 98, 99]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[120, 121, 99]);
}
#[test]
fn tr_control_escapes() {
    let sandbox = Sandbox::new();
    let out = sandbox.run(
        "tr",
        &["-d", "\\a\\b\\f\\n\\r\\t\\v\\\\"],
        &[97, 7, 8, 12, 10, 13, 9, 11, 92, 98],
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98]);
}
#[test]
fn tr_empty_truncate() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-t", "abc", ""], &[97, 98, 99]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[97, 98, 99]);
}
#[test]
fn tr_reverse_range() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["z-a", "x"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tr_missing() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["a"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tr_extra() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["a", "b", "c"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tr_bad_class() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "[:unknown:]"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tr_misaligned() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["a[:upper:]", "[:lower:]"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tr_bad_option() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["--oops"], &[]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[]);
    assert!(!out.stderr.is_empty());
}
#[test]
fn tr_binary() {
    let sandbox = Sandbox::new();
    let out = sandbox.run("tr", &["-d", "\\000"], &[0, 255, 97, 0, 98]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, &[255, 97, 98]);
}
#[test]
fn tr_help() {
    let out = run("tr", &["--help"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
#[test]
fn tr_version() {
    let out = run("tr", &["--version"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
