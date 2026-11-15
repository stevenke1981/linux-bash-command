# Phase 0 verification

Date: 2026-11-15 (Asia/Taipei).
Base: `617a90c73554ccb5a6c6eebc631e26c78948927a`.
Accepted commit: `7fa2e6cbbdc0e1b44318a9f5d5d5067794a80456`.
CI: https://github.com/stevenke1981/linux-bash-command/actions/runs/43677395391 (all six jobs passed).
Source/test snapshot SHA-256:
`c9718a1ff486b5655409d4a17d726ed9457ef17f11a7319bdbe8cdeba8b4df44`.
The digest concatenates each sorted UTF-8 path, NUL, LF-normalized file contents,
and NUL for all 183 files under `src/` and `tests/`.

- commits: `d770070` `fix: finish Linux command foundations`; `82b4f81`
  `test: make output failure fixtures portable`; `7fa2e6c`
  `test: cover path normalization across platforms`
- New commands: none; preserve all 46 inherited names, including the original 17
- Tests: Windows 1,053 passed; Ubuntu 1,056 passed; macOS 1,054 passed;
  all had 0 failures and 1 explicitly deferred sort test ignored
- clippy/fmt: PASS, all targets, warnings denied
- GNU differential: Ubuntu 24.04, 968 passed, 0 failed, 16 documented skips
- Coverage: common 1,055/1,158 (91.11%); commands 4,748/5,167 (91.89%);
  every measured new module exceeded 80%
- Incomplete/deferred: sort external merge is explicitly due in Phase 3;
  Windows legacy permission synthesis reconciliation is due in Phase 2
- New dependencies: none

Phase 0 and inherited Phase 1 are accepted by the linked CI run.

The candidate adds ordered legacy option prefix/POSIX handling, checked help and
version output, normalized errors, buffered output with explicit flush, and curl
6/7/28/23 status coverage. Existing command grammar and future phase options are
preserved. GNU captures terminate their dedicated process group on timeout;
fixture timestamps are set before restrictive modes and after directory contents.
Skipped differential cases no longer count toward option coverage.

Independent review: Astra high found no remaining blocking findings in the
reviewed snapshot and independently passed the ten P0 error tests. Earlier
findings were repaired and covered by regressions. This is agent review evidence,
not a claim of an additional GitHub account's approval.

During verification, tail follow output buffering and curl post-header timeout
classification were repaired. A transient Windows HTTP fixture failure was traced
to nonblocking socket inheritance; the server now uses blocking accepted sockets,
checks successful replies, and closes them gracefully.

The first CI run also exposed a nonportable read-only stdout fixture on Unix.
The repaired fixture uses a retained, completely full nonblocking pipe to produce
a genuine non-BrokenPipe write failure. Linux/macOS helper compilation and Windows
regressions passed. GNU child captures now explicitly call `wait()` on every path
for the newer CI clippy check. CI cancels stale runs for the same branch.
Windows path normalization is now a pure helper tested on every OS; actual
platform selection uses `#[cfg]`. This preserves output and avoids counting
unreachable Windows-only branches in the Linux measurement. All command modules
already exceeded 80%; the helper tests address the path module's remaining gap.
