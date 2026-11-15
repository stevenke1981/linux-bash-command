# Phase 2 verification (in progress)

Date: 2026-11-15 (Asia/Taipei). Base: `7fa2e6c`.
The phase is not accepted until the final frozen source passes the complete gates.

- commits: Phase 2 candidate on `feat/linux-commands`; see the PR commit history
- New commands (32): mkdir rm rmdir cp mv touch ln link unlink realpath readlink
  stat du mktemp date md5sum sha1sum sha224sum sha256sum sha384sum sha512sum
  b2sum cksum sum base64 base32 basenc truncate nproc hostname arch uptime
- Existing command gaps: ls/find metadata, formatting and actions; df --output
- Tests: final Windows source 1,471 passed, 0 failed, 1 ignored;
  Ubuntu/macOS and GNU acceptance await CI
- GNU differential: fixtures authored; NOT_RUN locally because no Linux runtime
  is installed; Ubuntu 24.04 CI is the definitive gate
- clippy/fmt: final Windows all-targets clippy with warnings denied and fmt PASS
- Coverage: final Ubuntu cargo-llvm-cov measurement pending
- Incomplete/deferred: cross-platform and GNU gates, coverage; P3-P6
- New direct dependencies: allowlisted jiff 0.2.45 (bundled tzdb), filetime 0.2.29,
  rand 0.10.3, sha1 0.11.0, sha2 0.11.1, md-5 0.11.0, blake2 0.11.0 and
  Windows-only windows-sys 0.61.2 with filesystem/foundation APIs;
  lockfile includes their required transitive dependencies

Independent review found backup/source collisions, trailing-slash symlink
traversal, root aliases and date edge cases. Those repairs are integrated.
The independent adviser confirmed the original finite findings are resolved;
a final malformed-UNC-prefix panic was repaired with a pure regression.
The integrated metadata review repairs cover: missing identities, trailing slashes, numeric formatting,
symlink timestamp reference policy, du counting, duplicate df rows, find action
streams and ls time/extension ordering. The final-source tests and CI verify these repairs.

The known differences table records unconditional rm root refusal,
cp/mv --update=none-fail, jiff's calendar range, nproc OpenMP interpretation,
Windows statfs unavailable fields and the deliberate hostname setter rejection.
Windows permission synthesis is reconciled to the specified 0644/0755 policy.

New source files stay within 800 lines. No GNU source was read or ported.
Manual pages and installed GNU binaries were used as behavior references;
Git for Windows GNU 8.32 checks are provisional, not Ubuntu 24.04 acceptance.
No uutils source was ported, so there is no new attribution entry.

Source/test snapshot SHA-256: `49da1f1f0cd9e65da3fb22f4175f9b7394b91fa84f600cef0adb889f0772a5ea` (295 files; same sorted path/NUL/LF-normalized content/NUL format as Phase 0).


First CI candidate: `fe1813c`, run
https://github.com/stevenke1981/linux-bash-command/actions/runs/43684677570.
Windows and Ubuntu builds passed; macOS fixture assumptions and a Rust 1.100
clippy warning were found and repaired. GNU9.4: 1,792 passed, 45 failed,
22 skipped before the repair batch. The repairs follow the measured 9.4 behavior,
including declined overwrite status, explicit sum filenames and checksum CRLF.
The rm no-follow rule, CRC32B, hostname statuses and inherited ls behavior retain
the specified behavior, with linked CLI goldens and known-difference entries.

The first measurement was common 1,831/1,992 (91.92%) and commands
9,111/10,338 (88.13%). Several individual modules were below80%; additional
filesystem, stat and timezone regression cases have been added. This measurement
does not constitute final per-module acceptance; an updated CI artifact is required.

Repair candidate Windows verification: 1,495 passed, 0 failed, 1 ignored; fmt and all-target clippy PASS. Repair source/test snapshot: `f7ef536cb2bda00927d1011b43fc539a0f81863cb912be967e53e57a6a1e6d65` (299 files). Cross-platform/GNU/coverage acceptance remains pending the next CI run.
