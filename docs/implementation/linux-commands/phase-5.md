# Phase 5 verification (in progress)

Base: Phase 4 integration 1284748; native Phase 4 and Phase 5 acceptance remain pending.

- New commands (8): awk tar gzip gunzip zcat patch xxd file; registry total 115.
- Local validation: final Windows fmt, all-target Clippy, and full cargo test passed: 2,187 passed, 0 failed, 0 ignored. Targeted Phase 5 CLI suites passed (27 archive, 40 AWK after refresh, 22 patch, 30 xxd/file).
- GNU reference: native Ubuntu 24.04 integration pending. AWK's local gawk matrix has 234 matching cases and one explicit syntax-status difference. Patch's unquoted Git space-name creation remains enabled for native determination; it is valid Git output under 04.
- Independent Astra resolution audits passed for archive, patch, xxd/file and the original three AWK findings.
- Coverage: native measurement pending; no aggregate result substitutes for the per-module 80% requirement.
- Dependencies: allowlisted flate2 1.1.11 with rust_backend only; tar 0.4.46. No new C backend, GNU source or copied third-party implementation.
- Incomplete/deferred: native gates and Phase 6 packaging/install are separate work.

AWK performance evidence was measured on Windows 11 x64 against gawk 5.4.1, LC_ALL=C TZ=UTC, rustc 1.99.0 release. The input/field/evaluation code is identical before and after the final regex-only brace fix. Exact sums, status 0 and empty stderr were checked. At 100 MiB, ours 12.983/13.735 s versus GNU 3.472/3.484 s (mean 3.84x); at 1 GiB, ours 136.234 s versus GNU 40.589 s (3.36x). The optional 3x target is missed. The 1 GiB process peak working set was 4,030,464 bytes, confirmed after exit, demonstrating bounded record streaming. These are Windows measurements, not Ubuntu measurements.

Resource limits: 1024 function-call nesting; separate 10 MiB regex validator/NFA compilation budgets; validator AST nesting 250; 128-entry dynamic regex cache. Programs exceeding supported compilation/call bounds fail cleanly with status 2. Required gawk extensions use default gawk; other reference cases use --posix.

All filesystem mutations and process-control probes are confined to temporary sandboxes and owned children. Archive differential comparisons decode/validate gzip with GNU and compare tar member semantics without extracting oracle archives. Failed normalization always fails comparison, including when both outputs are malformed.
