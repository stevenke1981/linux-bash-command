//! Part of the fsops CLI tests; helpers come from the parent module (`tests/fsops.rs`).

use super::*;

// ---------------------------------------------------------------- chmod

#[test]
fn chmod_help_and_version() {
    assert_help_and_version("chmod");
}

#[cfg(unix)]
#[test]
fn chmod_octal_and_symbolic_modes() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let mode = |sb: &Sandbox| {
        fs::metadata(sb.path().join("f"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(code(&run_in(sb.path(), "chmod", &["600", "f"], b"")), 0);
    assert_eq!(mode(&sb), 0o600);
    assert_eq!(
        code(&run_in(sb.path(), "chmod", &["u+x,go=r", "f"], b"")),
        0
    );
    assert_eq!(mode(&sb), 0o744);
    let _ = fs::set_permissions(sb.path().join("f"), fs::Permissions::from_mode(0o600));
}

#[cfg(unix)]
#[test]
fn chmod_verbose_and_changes_reporting() {
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let changed = run_in(sb.path(), "chmod", &["-v", "600", "f"], b"");
    assert_eq!(
        stdout(&changed),
        "mode of 'f' changed from 0644 (rw-r--r--) to 0600 (rw-------)\n"
    );
    let same = run_in(sb.path(), "chmod", &["-c", "600", "f"], b"");
    assert_eq!(stdout(&same), "");
    let retained = run_in(sb.path(), "chmod", &["-v", "600", "f"], b"");
    assert_eq!(
        stdout(&retained),
        "mode of 'f' retained as 0600 (rw-------)\n"
    );
}

#[test]
fn chmod_invalid_mode_and_missing_operands() {
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let bad = run_in(sb.path(), "chmod", &["u=q", "f"], b"");
    assert_eq!(code(&bad), 1);
    assert!(stderr(&bad).contains("chmod: invalid mode: 'u=q'"));
    assert!(stderr(&run_in(sb.path(), "chmod", &[], b"")).contains("chmod: missing operand"));
    assert!(stderr(&run_in(sb.path(), "chmod", &["644"], b""))
        .contains("chmod: missing operand after '644'"));
    assert_unknown_option("chmod");
}

#[test]
fn chmod_missing_file_fails_and_silent_hides_message() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "chmod", &["644", "nope"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("chmod: cannot access 'nope': No such file or directory"));
    let silent = run_in(sb.path(), "chmod", &["-f", "644", "nope"], b"");
    assert_eq!(code(&silent), 1);
    assert_eq!(stderr(&silent), "");
}

#[cfg(unix)]
#[test]
fn chmod_reference_and_recursive() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new();
    sb.write("ref", b"r");
    sb.write("d/f", b"x");
    fs::set_permissions(sb.path().join("ref"), fs::Permissions::from_mode(0o751)).unwrap();
    assert_eq!(
        code(&run_in(
            sb.path(),
            "chmod",
            &["--reference=ref", "d/f"],
            b""
        )),
        0
    );
    let bits = fs::metadata(sb.path().join("d/f"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(bits, 0o751);
    assert_eq!(
        code(&run_in(sb.path(), "chmod", &["-R", "700", "d"], b"")),
        0
    );
    let dir_bits = fs::metadata(sb.path().join("d"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_bits, 0o700);
}

// ---------------------------------------------------------------- truncate

#[test]
fn truncate_help_and_version() {
    assert_help_and_version("truncate");
}

#[test]
fn truncate_shrinks_and_extends() {
    let sb = Sandbox::new();
    sb.write("f", b"hello");
    assert_eq!(
        code(&run_in(sb.path(), "truncate", &["-s", "2", "f"], b"")),
        0
    );
    assert_eq!(sb.read("f"), b"he");
    assert_eq!(
        code(&run_in(sb.path(), "truncate", &["-s", "+3", "f"], b"")),
        0
    );
    assert_eq!(fs::metadata(sb.path().join("f")).unwrap().len(), 5);
}

#[test]
fn truncate_creates_missing_files_unless_no_create() {
    let sb = Sandbox::new();
    assert_eq!(
        code(&run_in(
            sb.path(),
            "truncate",
            &["-c", "-s", "3", "nope"],
            b""
        )),
        0
    );
    assert!(!sb.path().join("nope").exists());
    assert_eq!(
        code(&run_in(sb.path(), "truncate", &["-s", "3", "new"], b"")),
        0
    );
    assert_eq!(fs::metadata(sb.path().join("new")).unwrap().len(), 3);
}

#[test]
fn truncate_reference_file_size() {
    let sb = Sandbox::new();
    sb.write("ref", b"abcdef");
    sb.write("f", b"x");
    assert_eq!(
        code(&run_in(sb.path(), "truncate", &["-r", "ref", "f"], b"")),
        0
    );
    assert_eq!(fs::metadata(sb.path().join("f")).unwrap().len(), 6);
    let absolute = run_in(sb.path(), "truncate", &["-r", "ref", "-s", "4", "f"], b"");
    assert_eq!(code(&absolute), 1);
    assert!(stderr(&absolute).contains("you must specify a relative '--size' with '--reference'"));
}

#[test]
fn truncate_invalid_and_overflowing_sizes() {
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let bad = run_in(sb.path(), "truncate", &["-s", "x", "f"], b"");
    assert_eq!(code(&bad), 1);
    assert_eq!(stderr(&bad), "truncate: Invalid number: 'x'\n");
    let big = run_in(sb.path(), "truncate", &["-s", "1Q", "f"], b"");
    assert!(stderr(&big).contains("Value too large for defined data type"));
}

#[test]
fn truncate_missing_size_and_operands() {
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let no_size = run_in(sb.path(), "truncate", &["f"], b"");
    assert_eq!(code(&no_size), 1);
    assert!(stderr(&no_size).contains("you must specify either '--size' or '--reference'"));
    assert!(stderr(&run_in(sb.path(), "truncate", &["-s", "1"], b""))
        .contains("truncate: missing file operand"));
    assert_unknown_option("truncate");
}

#[test]
#[cfg(unix)]
fn truncate_directory_cannot_be_opened() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let out = run_in(sb.path(), "truncate", &["-s", "1", "d"], b"");
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("truncate: cannot open 'd' for writing: Is a directory"));
}
