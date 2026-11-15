# Phase 2 completion report

Date: 2026-11-15 (Asia/Taipei). Accepted source: `071f6a9e448e4a0ef34f93b25673724ed9169361`.

- Commits: `fe1813c` filesystem/checksum/system commands; `5018050`, `1c4f438`, `83d24ec`, `071f6a9` compatibility and regression repairs.
- New commands (32): mkdir rm rmdir cp mv touch ln link unlink realpath readlink stat du mktemp date md5sum sha1sum sha224sum sha256sum sha384sum sha512sum b2sum cksum sum base64 base32 basenc truncate nproc hostname arch uptime. Total: 78 commands.
- Existing gaps completed: ls/find metadata, formatting and actions; df --output.
- Tests: Windows 1,499 passed, macOS 1,512 passed, Ubuntu 1,520 passed; zero failures on every platform, one deferred Phase 3 sort test ignored. Ubuntu GNU JSON comparison: 1,836 passed, zero failures, 29 documented skips. A separate GNU metadata/checksum/system oracle passed 141 tests. Filesystem prompt PTY checks passed.
- clippy/fmt: passed, including all-targets warnings-as-errors lint.
- Coverage: common 1,901/2,043 (93.05%); commands 9,555/10,488 (91.10%). Every measured file exceeds 80%.
- Incomplete/deferred: none in Phase 2; Phase 3-6 remain separate work.
- New direct dependencies: allowlisted jiff 0.2.45 with bundled tzdb, filetime 0.2.29, rand 0.10.3, sha1 0.11.0, sha2 0.11.1, md-5 0.11.0, blake2 0.11.0, and Windows-only windows-sys 0.61.2 filesystem/foundation APIs; transitive dependencies are locked.

All six jobs passed in [CI run 43690770481](https://github.com/stevenke1981/linux-bash-command/actions/runs/43690770481): Windows/Linux/macOS builds and tests, Ubuntu GNU comparison, lint and coverage. The artifact supplies the coverage counts above. Older intermediate measurements are superseded by this accepted result.

Independent reviews found and verified repairs for source/backup collisions, symlink traversal, protected-root aliases, date ranges, missing file identities, stat formatting, link reference policies, du counting, df rows and find actions. Tests modify only temporary sandboxes; root refusal is tested as a pure function. The filesystem stat oracle masks only live free/available counts, retaining static fields and layout checks.

All intentional differences and required specification overrides are recorded in `docs/spec/linux-commands/07-phases.md`, with CLI goldens. These include hostname setter rejection/statuses, unconditional root protection, safe trailing-slash link handling, safe ln backup collisions, CRC32B, update=none-fail, and platform metadata/calendar limits. Git for Windows GNU 8.32 probes were provisional; Ubuntu 24.04 GNU 9.4 supplied acceptance.

New source files stay within 800 lines. No GNU source was read or ported. No uutils source was ported in this phase.
