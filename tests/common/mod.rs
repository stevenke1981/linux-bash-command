#![allow(dead_code)]
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn binary(util: &str) -> &'static str {
    match util {
        "fold" => env!("CARGO_BIN_EXE_fold"),
        "expand" => env!("CARGO_BIN_EXE_expand"),
        "unexpand" => env!("CARGO_BIN_EXE_unexpand"),
        "nl" => env!("CARGO_BIN_EXE_nl"),
        "tac" => env!("CARGO_BIN_EXE_tac"),
        "cd" => env!("CARGO_BIN_EXE_cd"),
        "curl" => env!("CARGO_BIN_EXE_curl"),
        "df" => env!("CARGO_BIN_EXE_df"),
        "diff" => env!("CARGO_BIN_EXE_diff"),
        "less" => env!("CARGO_BIN_EXE_less"),
        "list" => env!("CARGO_BIN_EXE_list"),
        "ps" => env!("CARGO_BIN_EXE_ps"),
        "pwd" => env!("CARGO_BIN_EXE_pwd"),
        "read" => env!("CARGO_BIN_EXE_read"),
        "rg" => env!("CARGO_BIN_EXE_rg"),
        "search" => env!("CARGO_BIN_EXE_search"),
        "wc" => env!("CARGO_BIN_EXE_wc"),
        "cut" => env!("CARGO_BIN_EXE_cut"),
        "tr" => env!("CARGO_BIN_EXE_tr"),
        "paste" => env!("CARGO_BIN_EXE_paste"),
        "tee" => env!("CARGO_BIN_EXE_tee"),
        "printf" => env!("CARGO_BIN_EXE_printf"),
        "seq" => env!("CARGO_BIN_EXE_seq"),
        "test" => env!("CARGO_BIN_EXE_test"),
        "sort" => env!("CARGO_BIN_EXE_sort"),
        "uniq" => env!("CARGO_BIN_EXE_uniq"),
        "comm" => env!("CARGO_BIN_EXE_comm"),
        "env" => env!("CARGO_BIN_EXE_env"),
        "printenv" => env!("CARGO_BIN_EXE_printenv"),
        "xargs" => env!("CARGO_BIN_EXE_xargs"),
        "egrep" => env!("CARGO_BIN_EXE_egrep"),
        "fgrep" => env!("CARGO_BIN_EXE_fgrep"),
        "agentcmd" => env!("CARGO_BIN_EXE_agentcmd"),
        "true" => env!("CARGO_BIN_EXE_true"),
        "false" => env!("CARGO_BIN_EXE_false"),
        "echo" => env!("CARGO_BIN_EXE_echo"),
        "yes" => env!("CARGO_BIN_EXE_yes"),
        "sleep" => env!("CARGO_BIN_EXE_sleep"),
        "basename" => env!("CARGO_BIN_EXE_basename"),
        "dirname" => env!("CARGO_BIN_EXE_dirname"),
        "cat" => env!("CARGO_BIN_EXE_cat"),
        "head" => env!("CARGO_BIN_EXE_head"),
        "tail" => env!("CARGO_BIN_EXE_tail"),
        "grep" => env!("CARGO_BIN_EXE_grep"),
        "find" => env!("CARGO_BIN_EXE_find"),
        "ls" => env!("CARGO_BIN_EXE_ls"),
        _ => panic!("no standalone binary registered for {util}"),
    }
}

pub struct Sandbox {
    pub path: PathBuf,
}
impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}
impl Sandbox {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-p1-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
    pub fn write(&self, name: &str, contents: impl AsRef<[u8]>) {
        let path = self.path.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    pub fn run(&self, util: &str, args: &[&str], stdin: &[u8]) -> Output {
        let mut cmd = Command::new(binary(util));
        cmd.args(args).current_dir(&self.path);
        execute(cmd, stdin)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}

/// Tests copy executables into sandboxes while other test threads fork. A child
/// forked during the copy briefly inherits the write descriptor, so Linux may
/// report ETXTBSY until that child execs; retry for a bounded time.
fn spawn_retrying_busy(cmd: &mut Command) -> std::process::Child {
    const ATTEMPTS: u32 = 50;
    for _ in 1..ATTEMPTS {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => return result.unwrap(),
        }
    }
    cmd.spawn().unwrap()
}

pub fn execute(mut cmd: Command, stdin: &[u8]) -> Output {
    cmd.env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("COLUMNS", "80")
        .env_remove("POSIXLY_CORRECT");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_retrying_busy(&mut cmd);
    let mut pipe = child.stdin.take().unwrap();
    let bytes = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        if let Err(e) = pipe.write_all(&bytes) {
            assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
        }
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    out
}

pub fn run(util: &str, args: &[&str], stdin: &[u8]) -> Output {
    Sandbox::new().run(util, args, stdin)
}
pub fn run_multicall(args: &[&str], stdin: &[u8]) -> Output {
    run("agentcmd", args, stdin)
}

pub fn limited(util: &str, args: &[&str], size: usize) -> (Vec<u8>, i32) {
    let mut child = Command::new(binary(util))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut bytes = vec![0; size];
    stdout.read_exact(&mut bytes).unwrap();
    drop(stdout);
    let status = child.wait().unwrap().code().unwrap();
    (bytes, status)
}

pub fn prefix(mut command: Command, size: usize) -> Output {
    let mut child = command
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env_remove("POSIXLY_CORRECT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut bytes = vec![0; size];
    stdout.read_exact(&mut bytes).unwrap();
    drop(stdout);
    let mut out = child.wait_with_output().unwrap();
    out.stdout = bytes;
    out
}
