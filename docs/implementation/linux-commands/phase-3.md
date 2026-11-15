# Phase 3 completion report

Accepted source: `c547543874dbc75bfefa3963b574657de7fbf994`.

- Commits: `6b14214` data/formatting commands and external sorting; `b94ff48` GNU chunking/scales and coverage repairs; `c547543` GNU numeric rounding and portable early-reader tests.
- New commands (13): join od split csplit fmt pr shuf tsort numfmt factor expr pathchk ptx; total91.
- Existing gap completed: stable external sort with bounded memory and merge fan-in, unique/merge/NUL records, private spill cleanup, and atomic output replacement. The previously deferred ignored spill test now runs.
- Tests: Windows1,698, Ubuntu1,721 and macOS1,713 passed; zero failures and zero ignored. GNU JSON2,175passed, zero failures,33documented skips. All six native CI jobs passed.
- clippy/fmt: passed, including all-target warnings-as-errors.
- Coverage: common2,675/2,868 (93.27%); commands15,693/17,314 (90.64%). Every measured common/command file exceeds80%.
- Incomplete/deferred: none in Phase3. Phase4-6 remain separate work.
- Dependencies: no new crate. Three attributed MIT uutils formatting files include exact revision and full license in NOTICE.md.

Acceptance evidence: [CI run43697989178](https://github.com/stevenke1981/linux-bash-command/actions/runs/43697989178). This supersedes candidate runs43692908098 and43696088415. Per-file coverage is recorded in coverage.md.

Independent reviews and measured regressions resolved shuf streaming/zero-count/range handling, shared pr stdin and tab-character validation, numfmt scaling/rounding, split chunk distribution, od operand checks, expr capture-state growth/subgroup priority/closed output status, and sort failure-safe rewrite. Contextual BRE matching uses constant-size matchers with one real adjacent byte; a100KB regression avoids subject-sized compiled regexes. Factor's u128 primality results use complete n-1 Pocklington certificates; independent mathematical review found the checks sound. Factoring has no worst-case runtime guarantee.

The known-differences table records required expr runtime status3, FIFO Kahn tsort order, safe csplit alias refusal and portable od long-double formatting. Native GNU9.4 supplied acceptance; Git GNU8.32 probes were provisional. No GNU source was read or ported. New files stay within800lines, and mutating tests use only temporary sandboxes.
