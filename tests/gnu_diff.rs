#![cfg(target_os = "linux")]
mod common;
use common::Sandbox;
use serde_json::Value;
use std::os::unix::{
    fs::{symlink, PermissionsExt},
    process::ExitStatusExt,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn hex(s: &str) -> Vec<u8> {
    assert_eq!(s.len() % 2, 0);
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn fixtures(root: &Path, files: &Value) {
    if let Some(files) = files.as_object() {
        for (name, value) in files {
            let p = root.join(name);
            assert!(
                !Path::new(name).is_absolute()
                    && !Path::new(name)
                        .components()
                        .any(|c| c == std::path::Component::ParentDir)
            );
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            if value["dir"].as_bool() == Some(true) {
                fs::create_dir_all(&p).unwrap();
                continue;
            }
            if let Some(target) = value["symlink"].as_str() {
                symlink(target, &p).unwrap();
                continue;
            }
            let data = if let Some(s) = value.as_str() {
                s.as_bytes().to_vec()
            } else if let Some(s) = value["hex"].as_str() {
                hex(s)
            } else {
                value["text"].as_str().unwrap_or("").as_bytes().to_vec()
            };
            fs::write(&p, data).unwrap();
            if let Some(mode) = value["mode"].as_str() {
                fs::set_permissions(
                    &p,
                    fs::Permissions::from_mode(u32::from_str_radix(mode, 8).unwrap()),
                )
                .unwrap();
            }
            if let Some(seconds) = value["mtime"].as_u64() {
                fs::File::open(&p)
                    .unwrap()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(seconds))
                    .unwrap();
            }
        }
    }
}
fn inventory(root: &Path, mode: bool) -> BTreeMap<PathBuf, (u8, Vec<u8>, Option<u32>)> {
    fn walk(
        root: &Path,
        dir: &Path,
        mode: bool,
        out: &mut BTreeMap<PathBuf, (u8, Vec<u8>, Option<u32>)>,
    ) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let m = fs::symlink_metadata(&p).unwrap();
            let kind = if m.is_symlink() {
                2
            } else if m.is_dir() {
                1
            } else {
                0
            };
            let bytes = match kind {
                0 => fs::read(&p).unwrap(),
                2 => std::os::unix::ffi::OsStrExt::as_bytes(fs::read_link(&p).unwrap().as_os_str())
                    .to_vec(),
                _ => vec![],
            };
            out.insert(
                p.strip_prefix(root).unwrap().to_owned(),
                (kind, bytes, mode.then(|| m.permissions().mode() & 0o7777)),
            );
            if kind == 1 {
                walk(root, &p, mode, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, mode, &mut out);
    out
}
fn glob(pattern: &str, name: &str) -> bool {
    let mut row = vec![false; name.len() + 1];
    row[0] = true;
    for b in pattern.bytes() {
        let mut next = vec![false; row.len()];
        if b == b'*' {
            next[0] = row[0];
            for i in 1..row.len() {
                next[i] = row[i] || next[i - 1];
            }
        } else {
            for i in 1..row.len() {
                next[i] = row[i - 1] && (b == b'?' || b == name.as_bytes()[i - 1]);
            }
        }
        row = next;
    }
    row[name.len()]
}
struct Captured {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: i32,
    timed_out: bool,
}
fn capture(mut cmd: Command, input: &[u8], timeout: u64, prefix: Option<usize>) -> Captured {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = pipe.write_all(&data);
    });
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(n) = prefix {
            let _ = stdout.by_ref().take(n as u64).read_to_end(&mut bytes);
        } else {
            stdout.read_to_end(&mut bytes).unwrap();
        }
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let mut timed_out = false;
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    writer.join().unwrap();
    Captured {
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
        code: status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap()),
        timed_out,
    }
}
fn environment(cmd: &mut Command, root: &Path, env: &Value) {
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("COLUMNS", "80")
        .env("HOME", root)
        .current_dir(root);
    if let Some(vars) = env.as_object() {
        for (k, v) in vars {
            cmd.env(k, v.as_str().unwrap());
        }
    }
}
#[test]
fn gnu_differential() {
    if std::env::var("AGENTCMD_GNU_DIFF").as_deref() != Ok("1") {
        eprintln!("gnu_diff: skipped (set AGENTCMD_GNU_DIFF=1)");
        return;
    }
    let filter = std::env::var("AGENTCMD_GNU_DIFF_FILTER").ok();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/gnu_cases");
    let mut paths: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    let (mut passed, mut skipped) = (0, 0);
    let mut failures = Vec::new();
    for path in paths {
        let data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let util = data["command"].as_str().unwrap();
        let cases = data["cases"].as_array().unwrap();
        let minimum = if matches!(util, "cat" | "head" | "tail" | "grep" | "find" | "ls") {
            5
        } else {
            15
        };
        assert!(
            cases.len() >= minimum,
            "{util}: fewer than {minimum} differential cases"
        );
        let mut names = std::collections::HashSet::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            assert!(names.insert(name), "duplicate {util}/{name}");
            let label = format!("{util}/{name}");
            if let Some(f) = &filter {
                if !(if f.contains('/') {
                    glob(f, &label)
                } else {
                    glob(f, util)
                }) {
                    continue;
                }
            }
            if case["skip_reason"].as_str().is_some_and(|s| !s.is_empty()) {
                skipped += 1;
                continue;
            }
            let args: Vec<_> = case["args"]
                .as_array()
                .map(|a| a.iter().map(|a| a.as_str().unwrap()).collect())
                .unwrap_or_default();
            let input = case["stdin_hex"]
                .as_str()
                .map(hex)
                .unwrap_or_else(|| case["stdin"].as_str().unwrap_or("").as_bytes().to_vec());
            let left = Sandbox::new();
            let right = Sandbox::new();
            fixtures(&left.path, &case["files"]);
            fixtures(&right.path, &case["files"]);
            let mut actual = Command::new(env!("CARGO_BIN_EXE_agentcmd"));
            actual.arg(util).args(&args);
            environment(&mut actual, &left.path, &case["env"]);
            let parts: Vec<_> = if let Some(parts) = case["gnu"].as_array() {
                parts.iter().map(|s| s.as_str().unwrap()).collect()
            } else {
                vec![case["gnu"].as_str().unwrap_or(util)]
            };
            let program=["/usr/bin","/bin"].iter().map(|d|Path::new(d).join(parts[0])).find(|p|p.is_file()).unwrap_or_else(||panic!("missing GNU program {}: install coreutils/findutils/grep or 06-testing packages",parts[0]));
            let mut oracle = Command::new(program);
            oracle.args(&parts[1..]).args(&args);
            environment(&mut oracle, &right.path, &case["env"]);
            let timeout = case["timeout_ms"].as_u64().unwrap_or(10000);
            let prefix = case["stdout_prefix_len"].as_u64().map(|n| n as usize);
            let actual = capture(actual, &input, timeout, prefix);
            let oracle = capture(oracle, &input, timeout, prefix);
            let compare: Vec<_> = case["compare"]
                .as_array()
                .map(|v| v.iter().map(|s| s.as_str().unwrap()).collect())
                .unwrap_or_else(|| vec!["stdout", "exit"]);
            let mut reasons = Vec::new();
            if actual.timed_out || oracle.timed_out {
                reasons.push("timeout");
            }
            if compare.contains(&"stdout") && actual.stdout != oracle.stdout {
                reasons.push("stdout");
            }
            if compare.contains(&"exit") && actual.code != oracle.code {
                reasons.push("exit");
            }
            if compare.contains(&"stderr_empty")
                && (!actual.stderr.is_empty() || !oracle.stderr.is_empty())
            {
                reasons.push("stderr_empty");
            }
            if compare.contains(&"stdout_sorted") {
                let mut a: Vec<_> = actual.stdout.split(|b| *b == b'\n').collect();
                let mut b: Vec<_> = oracle.stdout.split(|b| *b == b'\n').collect();
                a.sort();
                b.sort();
                if a != b {
                    reasons.push("stdout_sorted");
                }
            }
            if (compare.contains(&"files") || compare.contains(&"files_mode"))
                && inventory(&left.path, compare.contains(&"files_mode"))
                    != inventory(&right.path, compare.contains(&"files_mode"))
            {
                reasons.push("files");
            }
            if reasons.is_empty() {
                passed += 1;
            } else {
                let diff = actual
                    .stdout
                    .iter()
                    .zip(&oracle.stdout)
                    .position(|(a, b)| a != b)
                    .unwrap_or(actual.stdout.len().min(oracle.stdout.len()));
                failures.push(format!("{label} args={args:?} {reasons:?} first difference {diff}\n actual exit={} stdout={:?} stderr={}\n GNU exit={} stdout={:?} stderr={}",actual.code,&actual.stdout[..actual.stdout.len().min(2048)],String::from_utf8_lossy(&actual.stderr),oracle.code,&oracle.stdout[..oracle.stdout.len().min(2048)],String::from_utf8_lossy(&oracle.stderr)));
            }
        }
    }
    eprintln!(
        "gnu_diff: {passed} passed, {} failed, {skipped} skipped",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
