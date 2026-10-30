//! Shared helpers for the CLI integration tests.
//!
//! Each test crate uses only a subset of these helpers, so every item carries
//! `#[allow(dead_code)]`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn execute(mut cmd: Command, stdin: &[u8]) -> Output {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn agentcmd");
    let mut pipe = child.stdin.take().expect("child stdin");
    let data = stdin.to_vec();
    // Write on a thread so large inputs cannot deadlock against the output pipes.
    // A write error (child exited early) is expected and ignored.
    std::thread::spawn(move || {
        let _ = pipe.write_all(&data);
    });
    child.wait_with_output().expect("wait for agentcmd")
}

/// Runs `agentcmd <util> <args...>` with `stdin` piped in.
#[allow(dead_code)]
pub fn run(util: &str, args: &[&str], stdin: &[u8]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentcmd"));
    cmd.arg(util).args(args);
    execute(cmd, stdin)
}

/// Like [`run`], with the working directory set to `dir`.
#[allow(dead_code)]
pub fn run_in(dir: &Path, util: &str, args: &[&str], stdin: &[u8]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentcmd"));
    cmd.current_dir(dir).arg(util).args(args);
    execute(cmd, stdin)
}

/// Runs the multi-call binary itself with `args` (no utility name prepended).
#[allow(dead_code)]
pub fn run_multicall(args: &[&str], stdin: &[u8]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentcmd"));
    cmd.args(args);
    execute(cmd, stdin)
}

/// A unique temporary directory under `std::env::temp_dir()`, removed on drop.
pub struct Sandbox {
    path: PathBuf,
}

impl Sandbox {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "agentcmd-test-{}-{}-{nanos}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create sandbox");
        Self { path }
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `bytes` to `rel` (relative to the sandbox), creating parent directories.
    #[allow(dead_code)]
    pub fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.path.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, bytes).expect("write sandbox file");
    }

    #[allow(dead_code)]
    pub fn mkdir(&self, rel: &str) {
        fs::create_dir_all(self.path.join(rel)).expect("create sandbox dir");
    }

    /// Creates a symlink `rel` pointing at `target`. Returns `None` when the
    /// platform or the account does not permit creating it.
    #[allow(dead_code)]
    pub fn symlink(&self, target: &str, rel: &str) -> Option<()> {
        let link = self.path.join(rel);
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent).ok()?;
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, &link).ok()
        }
        #[cfg(windows)]
        {
            let parent = link.parent().unwrap_or(&self.path).to_path_buf();
            let created = if parent.join(target).is_dir() {
                std::os::windows::fs::symlink_dir(target, &link)
            } else {
                std::os::windows::fs::symlink_file(target, &link)
            };
            // Some environments (Wine) accept the call but create nothing; report that as
            // "no symlinks here" so the test skips instead of failing.
            created
                .ok()
                .filter(|()| fs::symlink_metadata(&link).is_ok())
        }
    }

    #[allow(dead_code)]
    pub fn read(&self, rel: &str) -> Vec<u8> {
        fs::read(self.path.join(rel)).expect("read sandbox file")
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
