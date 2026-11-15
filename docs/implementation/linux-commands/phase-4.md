# Phase 4 complete report

- commits: f31680c adds the 16 commands and diff formats; b627899/1284748 repair platform/behavior checks; 6770af1 finalizes reference spelling and padding comparisons.
- New commands: sed which tree uname whoami timeout kill pgrep pkill free cmp rev column dos2unix unix2dos chmod; registry total 107.
- Tests: Windows 1,962, Linux 1,989 and macOS 1,980 passed; zero failures and zero ignored. GNU differential 2,715 passed, 0 failed, 39 documented skips. The separate gated metadata/hash/system oracle suites passed.
- clippy/fmt: passed. All six [CI jobs](https://github.com/stevenke1981/linux-bash-command/actions/runs/43705680348) are green, including the three optimized release builds.
- Coverage: common 2,712/2,903 (93.42%); commands 20,641/22,741 (90.77%); every measured module >=80%.
- Incomplete/deferred: none for Phase 4. Phases 5-6 and final merge/install are separate deliverables.
- Dependencies: no new crate; existing windows-sys enables process/security/job/toolhelp APIs.

Diff now defaults to normal format and supports explicit unified, context and side-by-side formats. Reference configuration and intentional GNU differences are recorded in 06/07: required UTF-8 tree branches with ordinary spaces, shared short-option forms, which information options, sed syntax statuses, UTF-8 rev, and converter aliases/statuses.

Independent Astra reviews verified the finite sed, process, converter, cmp, system-information and chmod repairs. Windows process tests compile minimal owned child fixtures; copied full test runners had triggered the host antivirus. Security settings were never changed. Mac filesystems that reject invalid UTF-8 filenames report that capability explicitly; raw process bytes remain unit-tested on Unix and the real path probe runs on capable filesystems.

Mutations stay in temporary sandboxes, and process control targets only owned children. No GNU source was read or ported; all new source files stay within 800 lines. Full per-file coverage is in coverage.md.
