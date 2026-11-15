# Phase 0 verification

Date: 2026-11-15 (Asia/Taipei).
Base: `617a90c73554ccb5a6c6eebc631e26c78948927a`.
Candidate: `feat/linux-commands`, `fix: finish Linux command foundations`.
Source/test snapshot SHA-256:
`2b9415eea68287fd99cb106c2cbf156a07e6f62ff418cde2e0f14b7e9ea502e5`.
The digest concatenates each sorted UTF-8 path, NUL, LF-normalized file contents,
and NUL for all 183 files under `src/` and `tests/`.

- commits: pending candidate commit; base recorded above
- New commands: none; preserve all 46 inherited names, including the original 17
- Tests: Windows `cargo test --locked`: 1,053 passed, 0 failed, 1 ignored
- clippy/fmt: Windows PASS, including all targets and warnings denied
- GNU differential: NOT_RUN locally; fixed Ubuntu 24.04 CI required before acceptance
- Coverage: current measurement pending Linux CI artifact; historical P1 results
  remain separately recorded in `docs/spec/linux-commands/coverage.md`
- Incomplete/deferred: Linux/macOS CI, current coverage; later phases have not started
- New dependencies: none

The ignored test is the explicitly deferred sort external-merge test, due in P3.
Windows legacy permission synthesis is recorded in the known-differences table
and will be reconciled during P2. The full acceptance result is pending CI.

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
