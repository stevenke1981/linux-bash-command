//! GNU differential tests (docs/spec/coreutils-alignment/06-testing.md §2, §2.1).
//!
//! Runs every case in `tests/gnu_cases/*.json` against this project and the GNU
//! reference program, then compares stdout, exit code, the resulting files and
//! stderr presence as the case requests. Gated by `AGENTCMD_GNU_DIFF=1`.
//!
//! `AGENTCMD_GNU_DIFF_FILTER` selects cases: `cmd` runs one command, and
//! `cmd/glob*` also matches case names with `*` and `?`.

#![cfg(target_os = "linux")]

#[path = "common/mod.rs"]
mod common;
mod gnu_diff {
    pub mod case;
    pub mod exec;
}

use gnu_diff::case::{self, Case};
use gnu_diff::exec::{self, Entry, Program, Side};
use std::collections::BTreeSet;
use std::path::Path;

/// Longest stdout/stderr excerpt shown per side in a failure report.
const EXCERPT_BYTES: usize = 2048;

struct Filter {
    command: Option<String>,
    name_glob: Option<String>,
}

impl Filter {
    fn from_env() -> Self {
        let Ok(raw) = std::env::var("AGENTCMD_GNU_DIFF_FILTER") else {
            return Self {
                command: None,
                name_glob: None,
            };
        };
        match raw.split_once('/') {
            Some((command, glob)) => Self {
                command: Some(command.to_owned()),
                name_glob: Some(glob.to_owned()),
            },
            None => Self {
                command: Some(raw),
                name_glob: None,
            },
        }
    }

    fn matches(&self, case: &Case) -> bool {
        self.command.as_deref().is_none_or(|c| c == case.command)
            && self
                .name_glob
                .as_deref()
                .is_none_or(|g| glob_match(g.as_bytes(), case.name.as_bytes()))
    }
}

fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            glob_match(&pattern[1..], text) || (!text.is_empty() && glob_match(pattern, &text[1..]))
        }
        (Some(b'?'), Some(_)) => glob_match(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob_match(&pattern[1..], &text[1..]),
        _ => false,
    }
}

/// Install hint printed when a GNU program is missing.
const APT_HINT: &str = "sudo apt-get install -y gawk tree xxd dos2unix patch bsdextrautils \
     (coreutils, findutils, grep, sed, diffutils, tar, gzip and debianutils are preinstalled)";

#[test]
fn gnu_differential() {
    if std::env::var("AGENTCMD_GNU_DIFF").as_deref() != Ok("1") {
        eprintln!("gnu_diff: skipped (set AGENTCMD_GNU_DIFF=1)");
        return;
    }

    let filter = Filter::from_env();
    let cases_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/gnu_cases");
    let cases: Vec<Case> = case::load_all(&cases_dir)
        .into_iter()
        .filter(|case| filter.matches(case))
        .collect();
    assert!(!cases.is_empty(), "gnu_diff: no cases match the filter");

    // Resolve every GNU program up front so a missing tool fails loudly.
    let mut missing = BTreeSet::new();
    for case in cases.iter().filter(|c| c.skip_reason.is_none()) {
        if exec::resolve_gnu(&case.gnu[0]).is_none() {
            missing.insert(case.gnu[0].clone());
        }
    }
    assert!(
        missing.is_empty(),
        "gnu_diff: GNU program(s) not found in /usr/bin or /bin: {}\n  install: {APT_HINT}",
        missing.into_iter().collect::<Vec<_>>().join(", ")
    );

    let (mut passed, mut failed, mut skipped) = (0usize, 0usize, 0usize);
    let mut reports = Vec::new();
    for case in &cases {
        if let Some(reason) = &case.skip_reason {
            skipped += 1;
            eprintln!("gnu_diff: skip {}: {reason}", case.label());
            continue;
        }
        let ours = exec::run_side(case, Program::Ours);
        let gnu_path = exec::resolve_gnu(&case.gnu[0]).expect("resolved above");
        let gnu = exec::run_side(case, Program::Gnu(gnu_path));
        let reasons = compare(case, &ours, &gnu);
        if reasons.is_empty() {
            passed += 1;
        } else {
            failed += 1;
            reports.push(report(case, &ours, &gnu, &reasons));
        }
    }

    let summary = format!("gnu_diff: {passed} passed, {failed} failed, {skipped} skipped");
    eprintln!("{summary}");
    if !reports.is_empty() {
        panic!("{}\n\n{summary}", reports.join("\n"));
    }
}

/// Returns the reasons the two sides differ under the case's `compare` list.
fn compare(case: &Case, ours: &Side, gnu: &Side) -> Vec<String> {
    let mut reasons = Vec::new();
    if ours.timed_out || gnu.timed_out {
        reasons.push(format!(
            "timeout after {} ms (ours timed out: {}, gnu timed out: {})",
            case.timeout.as_millis(),
            ours.timed_out,
            gnu.timed_out
        ));
    }
    if case.wants("exit") && ours.exit != gnu.exit {
        reasons.push(format!("exit differs: ours {} gnu {}", ours.exit, gnu.exit));
    }
    if case.wants("stdout") && ours.stdout != gnu.stdout {
        reasons.push(format!(
            "stdout differs at byte {}",
            first_diff(&ours.stdout, &gnu.stdout)
        ));
    }
    if case.wants("stdout_sorted") && sorted_lines(&ours.stdout) != sorted_lines(&gnu.stdout) {
        reasons.push("stdout differs after sorting lines".to_owned());
    }
    if case.wants("files") && ours.tree != gnu.tree {
        reasons.push(format!("files differ: {}", tree_diff(ours, gnu)));
    }
    if case.wants("stderr_empty") && (!ours.stderr.is_empty() || !gnu.stderr.is_empty()) {
        reasons.push("stderr is not empty on both sides".to_owned());
    }
    reasons
}

fn sorted_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
    lines.sort_unstable();
    lines
}

/// Offset of the first differing byte; the shorter length when one is a prefix of the other.
fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b.iter())
        .position(|(x, y)| x != y)
        .unwrap_or_else(|| a.len().min(b.len()))
}

fn tree_diff(ours: &Side, gnu: &Side) -> String {
    let mut paths: BTreeSet<&String> = ours.tree.keys().collect();
    paths.extend(gnu.tree.keys());
    let differing: Vec<String> = paths
        .into_iter()
        .filter(|p| ours.tree.get(*p) != gnu.tree.get(*p))
        .take(10)
        .map(|p| {
            format!(
                "{p} (ours {}, gnu {})",
                describe(ours.tree.get(p)),
                describe(gnu.tree.get(p))
            )
        })
        .collect();
    differing.join("; ")
}

fn describe(entry: Option<&Entry>) -> String {
    match entry {
        None => "absent".to_owned(),
        Some(Entry::Dir) => "dir".to_owned(),
        Some(Entry::Link(target)) => format!("symlink -> {target}"),
        Some(Entry::File(bytes)) => format!("file of {} bytes", bytes.len()),
    }
}

/// Shows at most `EXCERPT_BYTES` of `bytes` as `{:?}`, noting any truncation.
fn excerpt(bytes: &[u8]) -> String {
    let shown = &bytes[..bytes.len().min(EXCERPT_BYTES)];
    let mut text = format!("{:?}", String::from_utf8_lossy(shown));
    if bytes.len() > EXCERPT_BYTES {
        text.push_str(&format!(" (truncated, {} bytes total)", bytes.len()));
    }
    text
}

fn report(case: &Case, ours: &Side, gnu: &Side, reasons: &[String]) -> String {
    let mut out = format!("FAIL {}\n", case.label());
    out.push_str(&format!("  file: {}\n", case.file));
    out.push_str(&format!("  args: {:?}\n", case.args));
    out.push_str(&format!("  gnu:  {:?}\n", case.gnu));
    out.push_str(&format!("  reasons: {}\n", reasons.join("; ")));
    out.push_str(&format!("  exit: ours {} gnu {}\n", ours.exit, gnu.exit));
    out.push_str(&format!("  stdout ours: {}\n", excerpt(&ours.stdout)));
    out.push_str(&format!("  stdout gnu:  {}\n", excerpt(&gnu.stdout)));
    if !ours.stderr.is_empty() || !gnu.stderr.is_empty() {
        out.push_str(&format!("  stderr ours: {}\n", excerpt(&ours.stderr)));
        out.push_str(&format!("  stderr gnu:  {}\n", excerpt(&gnu.stderr)));
    }
    out
}
