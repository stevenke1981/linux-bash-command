# Phase 1 completion report

Date: 2026-11-15 (Asia/Taipei).
Implementation inherited from `531c245`; accepted at
`7fa2e6cbbdc0e1b44318a9f5d5d5067794a80456` after the Phase 0 foundation repairs.
CI: https://github.com/stevenke1981/linux-bash-command/actions/runs/43677395391.

- commits: `531c245` `feat: integrate coreutils foundation and text pipelines`;
  Phase 0 follow-ups `d770070`, `82b4f81`, `7fa2e6c`
- New commands: wc sort uniq cut tr tee echo printf true false test [ seq tac nl
  paste comm fold expand unexpand yes basename dirname xargs env printenv sleep
  egrep fgrep (29)
- Tests: Windows 1,053; Ubuntu 1,056; macOS 1,054 passed; 0 failed;
  1 explicitly deferred sort test ignored on each platform
- GNU differential: 968 passed, 0 failed, 16 documented skips
- clippy/fmt: PASS
- Coverage: common 91.11%; commands 91.89%; every measured module >=80%
- Incomplete/deferred: sort external merge is required in Phase 3;
  known differences and their CLI goldens are listed in the phase specification
- New dependencies: allowlisted Unix `libc`, already in the inherited lockfile

The source/test snapshot and independent review are in the Phase 0 report.
The inherited P1 implementation's acceptance was rechecked after Phase 0 passed.
