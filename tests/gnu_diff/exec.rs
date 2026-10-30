//! Running one side of a differential case in a fresh sandbox (06-testing.md §2.1).

use super::case::{Case, Content, FileSpec};
use crate::common::Sandbox;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

/// Directories searched for GNU programs, in order. PATH is deliberately not used.
const GNU_DIRS: &[&str] = &["/usr/bin", "/bin"];

pub enum Program {
    /// This project: `agentcmd <command> <args>`.
    Ours,
    /// The GNU reference at this absolute path, with `gnu[1..]` as leading arguments.
    Gnu(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Entry {
    File(Vec<u8>),
    Dir,
    Link(String),
}

pub struct Side {
    pub exit: i32,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub tree: BTreeMap<String, Entry>,
}

pub fn resolve_gnu(program: &str) -> Option<PathBuf> {
    GNU_DIRS
        .iter()
        .map(|dir| Path::new(dir).join(program))
        .find(|path| path.is_file())
}

pub fn run_side(case: &Case, program: Program) -> Side {
    let sandbox = Sandbox::new();
    populate(sandbox.path(), &case.files);

    let mut cmd = match program {
        Program::Ours => {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentcmd"));
            cmd.arg(&case.command);
            cmd
        }
        Program::Gnu(path) => {
            // GNU tools name themselves in diagnostics by argv[0]; use the bare name so
            // messages read `mkdir: ...` rather than `/usr/bin/mkdir: ...`.
            let mut cmd = Command::new(&path);
            if let Some(name) = path.file_name() {
                cmd.arg0(name);
            }
            cmd.args(&case.gnu[1..]);
            cmd
        }
    };
    cmd.args(&case.args)
        .current_dir(sandbox.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("COLUMNS", "80")
        .env("HOME", sandbox.path())
        .envs(case.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("{}: spawn: {e}", case.label()));
    feed_stdin(&mut child, &case.stdin);
    let stdout = drain(child.stdout.take().expect("stdout pipe"));
    let stderr = drain(child.stderr.take().expect("stderr pipe"));

    let (status, timed_out) = wait_with_timeout(&mut child, case.timeout);
    // Reader threads finish at EOF; the bounded wait keeps a stray grandchild that
    // inherited the pipe from hanging the whole run.
    let stdout = stdout
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    let stderr = stderr
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();

    Side {
        exit: exit_code(status),
        timed_out,
        stdout,
        stderr,
        tree: snapshot(sandbox.path()),
    }
}

fn feed_stdin(child: &mut Child, data: &[u8]) {
    let mut pipe = child.stdin.take().expect("stdin pipe");
    let data = data.to_vec();
    // A write error means the child exited without reading; that is not a test failure.
    thread::spawn(move || {
        let _ = pipe.write_all(&data);
    });
}

fn drain<R: Read + Send + 'static>(mut reader: R) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> (ExitStatus, bool) {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return (status, false);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            return (child.wait().expect("wait after kill"), true);
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Exit code, with signal deaths reported as `128 + signal`.
fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

fn populate(root: &Path, files: &[(String, FileSpec)]) {
    for (rel, spec) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent for case file");
        }
        match &spec.content {
            Content::File(bytes) => fs::write(&path, bytes).expect("write case file"),
            Content::Dir => fs::create_dir_all(&path).expect("create case dir"),
            Content::Symlink(target) => {
                std::os::unix::fs::symlink(target, &path).expect("create case symlink")
            }
        }
    }
    // Attributes go last: writing children would otherwise disturb directory mtimes,
    // and a read-only mode must not block the writes above.
    for (rel, spec) in files {
        if matches!(spec.content, Content::Symlink(_)) {
            continue;
        }
        let path = root.join(rel);
        if let Some(mode) = spec.mode {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                .expect("set case file mode");
        }
        if let Some(secs) = spec.mtime {
            File::open(&path)
                .and_then(|f| f.set_modified(UNIX_EPOCH + Duration::from_secs(secs)))
                .expect("set case file mtime");
        }
    }
}

/// Recursively records every entry under `root` (symlinks are not followed).
fn snapshot(root: &Path) -> BTreeMap<String, Entry> {
    let mut tree = BTreeMap::new();
    walk(root, root, &mut tree);
    tree
}

fn walk(root: &Path, dir: &Path, tree: &mut BTreeMap<String, Entry>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .expect("entry under root")
            .to_string_lossy()
            .replace('\\', "/");
        let meta = fs::symlink_metadata(&path).expect("stat sandbox entry");
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).expect("read link");
            tree.insert(rel, Entry::Link(target.to_string_lossy().into_owned()));
        } else if meta.is_dir() {
            tree.insert(rel, Entry::Dir);
            walk(root, &path, tree);
        } else {
            tree.insert(rel, Entry::File(fs::read(&path).unwrap_or_default()));
        }
    }
}
