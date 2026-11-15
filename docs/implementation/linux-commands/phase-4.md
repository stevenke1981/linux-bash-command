# Phase 4 verification (in progress)

Base: accepted Phase 3 c547543; native Phase 4 acceptance remains pending.

- New commands (16): sed which tree uname whoami timeout kill pgrep pkill free cmp rev column dos2unix unix2dos chmod; registry total107.
- Existing gap: diff defaults to normal; explicit unified, context and side-by-side formats are supported.
- Tests: complete Windows integration suite 1,962 passed, zero failures and zero ignored; native CI pending.
- clippy/fmt: frozen Windows fmt --check and all-targets clippy with warnings denied passed.
- Coverage: b627899 measured common 2,712/2,903 (93.42%) and commands 20,574/22,728 (90.53%). free and sed/inplace were below 80%; targeted regressions are included and a fresh native measurement is required.
- Incomplete/deferred: Phase 4 final integration/gates; Phase 5-6 are separate work.
- Dependencies: no new crate. Existing windows-sys adds only process/security/job/toolhelp feature APIs.

Independent Astra reviews found and verified repair candidates for sed large-subject matching and explicit quit statuses; process inverse selection/counts and macOS signals; private atomic converter temporaries, complete newfile pairs, stdin/streaming/BOM rules; cmp streams/arguments; free repeated reports/numbers/metrics; Windows executable discovery/version; chmod filename preservation/directory special modes. Coordinator regressions and native CI must verify the integrated result. The timeout leader-exit question follows GNU9.4: escalation applies while COMMAND is still running, with no delayed signal after reaping.

All mutating tests use temporary sandboxes. Dedicated owned child fixtures test process control; unrelated processes are never signaled. No GNU source was read or ported. New source files stay within800lines. Known GNU/spec differences are recorded in 07-phases.md after native comparison.

Astra independently verified the finite review repairs. The final integration additionally checks literal marker-like filenames, directory special bits, legal empty/numeric mode actions, no-clobber publication and read-only staging cleanup. Process tests compile a minimal owned child fixture in each sandbox instead of copying the complete test runner; the host antivirus had blocked two copied runner fixtures. Security settings were not changed.


Native follow-up repaired case-sensitive fixtures, platform-only helper cfgs,
GNU fixture ID validation, and macOS filesystems that reject invalid UTF-8
filenames. The raw-byte process-name unit still runs on Unix; the real filename
probe reports unsupported filesystems explicitly. Tree root counts and column
blank-row/table-option semantics now follow the Ubuntu reference. Reference
configuration and intentional specification differences are recorded in 06/07.
The final local gate log is `.git/agent-evidence/p4-ci-repairs-full.log`.
