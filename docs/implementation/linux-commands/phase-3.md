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

First CI candidate: `6b14214`, [run 43692908098](https://github.com/stevenke1981/linux-bash-command/actions/runs/43692908098). Windows 1,675, Ubuntu 1,698 and macOS 1,690 tests passed, with no failures or ignored tests; all three builds and lint passed. GNU JSON: 2,146 passed, 11 failed, 29 skipped. Measured common coverage: 2,672/2,866 (93.23%); commands: 15,179/17,095 (88.79%). Four individual files remain below80% pending the targeted repair batch; this candidate is not accepted.

Repair candidate Windows: 1,698 passed, zero failures and zero ignored; fmt/clippy passed. Repairs cover measured split chunk boundaries, R/Q scales, full-domain shuf rejection, and additional real coverage paths. Contextual BRE matching now uses a single real right-context byte with fixed-size regexes; a 100 KB boundary regression passes without compiling a subject-sized regex. Four confirmed GNU/spec differences are documented rather than silently changing required behavior. Final native CI and coverage remain pending.
