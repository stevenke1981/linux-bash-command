mod common;

use common::Sandbox;
use std::process::{Command, Output};

fn run(sandbox: &Sandbox, command: &str, args: &[&str]) -> Output {
    let binary = match command {
        "realpath" => env!("CARGO_BIN_EXE_realpath"),
        "readlink" => env!("CARGO_BIN_EXE_readlink"),
        "stat" => env!("CARGO_BIN_EXE_stat"),
        "du" => env!("CARGO_BIN_EXE_du"),
        other => common::binary(other),
    };
    let mut process = Command::new(binary);
    process
        .args(args)
        .current_dir(&sandbox.path)
        .env("AGENTCMD_COLOR", "never");
    common::execute(process, b"")
}
fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
fn success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[path = "phase2_metadata/du.rs"]
mod du;
#[path = "phase2_metadata/existing.rs"]
mod existing;
#[path = "phase2_metadata/pathnames.rs"]
mod pathnames;
#[path = "phase2_metadata/review.rs"]
mod review;
#[path = "phase2_metadata/stat.rs"]
mod stat;
