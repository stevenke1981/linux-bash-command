# Phase 6 verification (in progress)

- Version: 0.2.0; registry contains all 115 names.
- Packaging: Windows and Linux payloads derive from agentcmd --list; Windows generates [.exe from test.exe and excludes the MSVC link.exe conflict. NOTICE/LICENSE/README/shell files are included in every release path.
- Local packager: concrete registry-derived ZIP and SHA256 output; existing nonempty stage/output targets are refused. PowerShell-relative paths resolve from the current filesystem location.
- Validation: Astra source/probe review passed after the relative-path fix. A real 0.2.0 debug-binary package contained 122 files (115 executables and required documents/shell files), with no link.exe and a valid bracket alias/checksum. This was a fixture validation, not the optimized release artifact.
- Final Windows gates: fmt, all-target Clippy and cargo test pass, with 2,200 passed, 0 failed and 1 deliberately ignored release-performance test. Native release gates remain pending for this source snapshot.
- Performance: the optional 100 MiB seven-command harness verifies output digests and reports balanced GNU/candidate timings; timing targets do not gate. Linux CI runs it after optimized binaries are built.
- Correctness follow-up: whoami lookup failures report a command-prefixed error/status 1 with no stdout, rather than a numeric UID or unknown-name success fallback. Focused Windows and cross-platform compilation checks passed; full native gates remain pending.
- New dependencies: none in this phase.
- Incomplete/deferred: final native checks, optimized package size/checksum, merge and local installation. Inno Setup is unavailable locally; an optimized portable archive is the planned deliverable. No GitHub tag/release publication has been triggered.
