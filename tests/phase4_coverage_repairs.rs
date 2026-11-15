mod common;

use common::Sandbox;
use std::{fs, process::Command};

fn invoke(s: &Sandbox, program: &str, args: &[&str]) -> std::process::Output {
    let executable = match program {
        "free" => env!("CARGO_BIN_EXE_free"),
        "sed" => env!("CARGO_BIN_EXE_sed"),
        _ => unreachable!(),
    };
    let mut command = Command::new(executable);
    command.current_dir(&s.path).args(args);
    common::execute(command, b"")
}

fn memory_total(output: &[u8]) -> u64 {
    String::from_utf8_lossy(output)
        .lines()
        .find_map(|line| line.strip_prefix("Mem:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn free_unit_options_scale_the_same_physical_memory_total() {
    let s = Sandbox::new();
    let bytes = invoke(&s, "free", &["--bytes"]);
    assert!(bytes.status.success());
    let total = memory_total(&bytes.stdout);
    assert!(total > 0);
    for (option, divisor) in [
        ("--tebi", 1024_u64.pow(4)),
        ("--pebi", 1024_u64.pow(5)),
        ("--kilo", 1000_u64),
        ("--mega", 1000_u64.pow(2)),
        ("--giga", 1000_u64.pow(3)),
        ("--tera", 1000_u64.pow(4)),
        ("--peta", 1000_u64.pow(5)),
        ("--si", 1000_u64),
    ] {
        let output = invoke(&s, "free", &[option]);
        assert!(output.status.success(), "{option}: {output:?}");
        let expected = format!("{:.0}", total as f64 / divisor as f64)
            .parse()
            .unwrap();
        assert_eq!(memory_total(&output.stdout), expected, "{option}");
    }
    let output = invoke(&s, "free", &["--human", "--si", "--total"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Total:"));
    let output = invoke(&s, "free", &["unexpected"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("extra operand"));
}

#[cfg(unix)]
#[test]
fn free_lohi_reports_low_and_high_rows() {
    let s = Sandbox::new();
    let output = invoke(&s, "free", &["--lohi", "--bytes"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.lines().any(|line| line.starts_with("Low:")));
    assert!(text.lines().any(|line| line.starts_with("High:")));
}

#[test]
fn sed_in_place_reports_missing_input_and_rejects_directories() {
    let s = Sandbox::new();
    fs::create_dir(s.path.join("directory")).unwrap();
    for (args, status, diagnostic) in [
        (&["-i", "s/a/b/"][..], 4, "no input files"),
        (&["-i", "s/a/b/", "missing"][..], 2, "can't read missing"),
        (
            &["--follow-symlinks", "-i", "s/a/b/", "missing"][..],
            2,
            "can't read missing",
        ),
        (&["-i", "s/a/b/", "directory"][..], 4, "not a regular file"),
    ] {
        let output = invoke(&s, "sed", args);
        assert_eq!(output.status.code(), Some(status), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
    }
    assert!(s.path.join("directory").is_dir());
    assert_eq!(fs::read_dir(&s.path).unwrap().count(), 1);
}
