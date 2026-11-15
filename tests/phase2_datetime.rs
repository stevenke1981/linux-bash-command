mod common;
use std::process::{Command, Output, Stdio};

fn date(timezone: &str, arguments: &[&str]) -> Output {
    let sandbox = common::Sandbox::new();
    sandbox.write("file", b"x");
    filetime::set_file_mtime(
        sandbox.path.join("file"),
        filetime::FileTime::from_unix_time(0, 0),
    )
    .unwrap();
    let mut command = Command::new(common::binary("date"));
    command
        .current_dir(&sandbox.path)
        .args(arguments)
        .env("TZ", timezone)
        .env("LC_ALL", "C");
    command
        .env_remove("POSIXLY_CORRECT")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn named_environment_timezone_applies_to_epochs_and_file_times() {
    for selector in [["-d", "@0"], ["-r", "file"]] {
        let output = date(":Asia/Taipei", &[selector[0], selector[1], "+%F %T %z"]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(output.stdout, b"1970-01-01 08:00:00 +0800\n");
    }
}

#[test]
fn invalid_environment_timezone_is_an_error_and_utc_aliases_are_accepted() {
    let output = date("AgentCommands/Unknown", &["-d", "@0", "+%z"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("date:"));
    for timezone in ["", "GMT", "UTC0"] {
        let output = date(timezone, &["-d", "@0", "+%z"]);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"+0000\n");
    }
}
