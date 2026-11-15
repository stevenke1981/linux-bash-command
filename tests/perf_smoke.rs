//! Optional evidence: cargo test --release --test perf_smoke -- --ignored --nocapture.
mod common;
use common::Sandbox;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Instant,
};

struct Case {
    name: &'static str,
    reference: &'static str,
    args: &'static [&'static str],
    stdin: bool,
    target: f64,
}

struct Oracle {
    path: PathBuf,
    version: String,
}

#[derive(Debug, PartialEq, Eq)]
struct Fingerprint {
    bytes: u64,
    sha256: String,
}

const CASES: &[Case] = &[
    Case {
        name: "wc",
        reference: "wc",
        args: &["-l", "input"],
        stdin: false,
        target: 3.0,
    },
    Case {
        name: "sort",
        reference: "sort",
        args: &["-S", "16M", "-T", "temporary", "input"],
        stdin: false,
        target: 5.0,
    },
    Case {
        name: "cut",
        reference: "cut",
        args: &["-f2", "input"],
        stdin: false,
        target: 3.0,
    },
    Case {
        name: "tr",
        reference: "tr",
        args: &["a-z", "A-Z"],
        stdin: true,
        target: 3.0,
    },
    Case {
        name: "sed",
        reference: "sed",
        args: &["s/a/b/g", "input"],
        stdin: false,
        target: 3.0,
    },
    Case {
        name: "awk",
        reference: "gawk",
        args: &["{s+=$2} END {print s}", "input"],
        stdin: false,
        target: 3.0,
    },
    Case {
        name: "sha256sum",
        reference: "sha256sum",
        // GNU's default mode differs on Windows; select the same marker on both sides.
        args: &["--binary", "input"],
        stdin: false,
        target: 3.0,
    },
];

fn oracle(name: &str) -> Oracle {
    let directory = std::env::var_os("AGENTCMD_GNU_BIN")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let candidates = directory.into_iter().chain(
        [
            "/usr/bin",
            "/bin",
            "C:/Program Files/Git/usr/bin",
            "/opt/homebrew/opt/coreutils/libexec/gnubin",
            "/usr/local/opt/coreutils/libexec/gnubin",
            "/opt/homebrew/opt/gnu-sed/libexec/gnubin",
            "/usr/local/opt/gnu-sed/libexec/gnubin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
        ]
        .map(PathBuf::from),
    );
    let mut rejected = Vec::new();
    for directory in candidates {
        for executable in [name.to_owned(), format!("gnu{name}")] {
            for suffix in ["", ".exe"] {
                let path = directory.join(format!("{executable}{suffix}"));
                if !path.is_file() {
                    continue;
                }
                match version(&path) {
                    Some(version) if version.contains("GNU") => return Oracle { path, version },
                    Some(version) => rejected.push(format!("{}: {version}", path.display())),
                    None => rejected.push(format!("{}: --version failed", path.display())),
                }
            }
        }
    }
    panic!(
        "missing GNU reference {name}; install GNU tools or set AGENTCMD_GNU_BIN; rejected candidates: {rejected:?}"
    );
}

fn version(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let output = Command::new(path)
        .arg("--version")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_owned)
}

fn input(path: &Path, bytes: usize) {
    let mut block = Vec::new();
    for row in (0..8192).rev() {
        writeln!(block, "{row:07}\t3\talpha beta").unwrap();
    }
    let mut output = File::create(path).unwrap();
    let mut remaining = bytes;
    while remaining > 0 {
        let count = remaining.min(block.len());
        output.write_all(&block[..count]).unwrap();
        remaining -= count;
    }
    output.flush().unwrap();
}

fn fingerprint(path: &Path) -> Fingerprint {
    let mut file = File::open(path).unwrap();
    let bytes = file.metadata().unwrap().len();
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let sha256 = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Fingerprint { bytes, sha256 }
}

fn run(case: &Case, program: &Path, ours: bool, root: &Path, output: &Path) -> f64 {
    let mut command = Command::new(program);
    if ours {
        command.arg(case.name);
    } else if case.name == "awk" {
        command.arg("--posix");
    }
    command
        .args(case.args)
        .current_dir(root)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("AGENTCMD_COLOR", "never")
        .env_remove("POSIXLY_CORRECT")
        .env("HOME", root)
        .env("TMPDIR", root.join("temporary"))
        .env("TEMP", root.join("temporary"))
        .env("TMP", root.join("temporary"))
        .stdout(File::create(output).unwrap())
        .stderr(Stdio::piped());
    if case.stdin {
        command.stdin(File::open(root.join("input")).unwrap());
    } else {
        command.stdin(Stdio::null());
    }
    let start = Instant::now();
    let result = command.output().unwrap();
    let seconds = start.elapsed().as_secs_f64();
    checked_output(case, program, &result);
    seconds
}

fn checked_output(case: &Case, program: &Path, result: &Output) {
    assert!(
        result.status.success(),
        "{} ({}): {}",
        case.name,
        program.display(),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.stderr.is_empty(),
        "{} ({}): unexpected diagnostics: {}",
        case.name,
        program.display(),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn benchmark(case: &Case, sandbox: &Sandbox, candidate: &Path) -> serde_json::Value {
    let reference = oracle(case.reference);
    let actual = sandbox.path.join("actual");
    let expected = sandbox.path.join("expected");
    let (mut ours, mut gnu) = (Vec::new(), Vec::new());
    for iteration in 0..2 {
        if iteration == 0 {
            gnu.push(run(case, &reference.path, false, &sandbox.path, &expected));
            ours.push(run(case, candidate, true, &sandbox.path, &actual));
        } else {
            ours.push(run(case, candidate, true, &sandbox.path, &actual));
            gnu.push(run(case, &reference.path, false, &sandbox.path, &expected));
        }
        assert_eq!(
            fingerprint(&actual),
            fingerprint(&expected),
            "{} output",
            case.name
        );
    }
    let output = fingerprint(&actual);
    let ratio = ours.iter().sum::<f64>() / gnu.iter().sum::<f64>();
    serde_json::json!({"command":case.name, "args":case.args,
        "reference":reference.version, "reference_path":reference.path,
        "ours_seconds":ours, "gnu_seconds":gnu, "ratio":ratio,
        "target":case.target, "target_met":ratio<=case.target,
        "output_bytes":output.bytes, "output_sha256":output.sha256, "output_equal":true})
}

#[test]
#[ignore = "100 MiB GNU comparison; release-only optional performance evidence"]
fn hundred_mib_pipelines_match_gnu_and_report_timings() {
    if cfg!(debug_assertions) {
        panic!("run this test with cargo test --release");
    }
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path.join("temporary")).unwrap();
    let bytes = 100 * 1024 * 1024;
    input(&sandbox.path.join("input"), bytes);
    let candidate = Path::new(env!("CARGO_BIN_EXE_agentcmd"));
    let results: Vec<_> = CASES
        .iter()
        .map(|case| benchmark(case, &sandbox, candidate))
        .collect();
    // Performance targets are observations, not gates; output equality is required.
    eprintln!(
        "perf_smoke: {}",
        serde_json::json!({"os":std::env::consts::OS,
        "arch":std::env::consts::ARCH, "candidate":candidate,
        "candidate_version":version(candidate).expect("candidate --version failed"),
        "candidate_sha256":fingerprint(candidate).sha256,
        "input_bytes":bytes, "lc_all":"C", "tz":"UTC",
        "order":[["gnu","ours"],["ours","gnu"]],
        "targets_observational":true, "results":results})
    );
}
