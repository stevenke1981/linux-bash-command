# Phase 3 verification (in progress)

Base: accepted Phase 2 `071f6a9`. This phase is accepted only after the final frozen source passes Windows/Linux/macOS, GNU comparison, lint and coverage.

- Commits: implementation candidate pending.
- New commands (13): join od split csplit fmt pr shuf tsort numfmt factor expr pathchk ptx; registry total 91.
- Existing gap: stable bounded-memory external sort, including merge, unique, binary records and atomic output replacement. The deferred ignored test is now an executed spill regression.
- Tests: Windows full suite 1,675 passed, zero failures, zero ignored; Unix and GNU gates pending.
- clippy/fmt: Windows fmt --check and all-targets clippy with warnings denied passed.
- Coverage: Ubuntu measurement pending.
- Incomplete/deferred: final Phase 3 acceptance; Phase 4-6 remain separate work.
- Dependencies: no new crate. NOTICE.md includes full MIT license and exact uutils revision for the three attributed formatting files.

Independent reviews identified and repaired repeat-output buffering, zero-count reads, range parsing, shared pr stdin, numeric option errors, unsafe tab-character slicing, line counter limits, expr capture-state growth/subgroup priority, closed stdout status, and atomic sort rewrite failure handling. Factor primality results use complete n-1 Pocklington certificates rather than probable-prime acceptance above u64; independent mathematical review found the certificate checks sound. Runtime has no worst-case factoring-time guarantee.

No GNU source was read or ported. Ubuntu 24.04 GNU 9.4 is the acceptance reference; local Git GNU 8.32 probes are provisional. Known differences appear in 07-phases.md. Source files stay within 800 lines; mutating tests are confined to temporary sandboxes.
