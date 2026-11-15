# Linux commands implementation

Baseline: `617a90c73554ccb5a6c6eebc631e26c78948927a`.
The maintainer authorized implementation, commits, push, PR, merge to main,
release builds, and replacement of the local binaries. The specification is
in `docs/spec/linux-commands/`; its requirements and dependency order apply.

| Task | Owner | Scope | Dependencies |
|---|---|---|---|
| P0 advice and independent review | Astra, high | Read-only contracts and findings | Baseline, candidate |
| P0 common helpers and multi-call | Terra, xhigh | `common/`, entry point, related tests | Baseline |
| P0 legacy diagnostics and status | Sol, xhigh | Existing command modules, related tests | Baseline |
| Release inventory | Luna | Read-only packaging and installed-copy inventory | Baseline |
| Integration, test runner, docs, CI | Coordinator | Isolated integration branch | Worker handoffs |
| P1 acceptance | Coordinator and reviewer | Existing text pipelines | P0 accepted |
| P2 filesystem and date | Terra xhigh | fsops, mktemp, truncate, date, datetime | P0 and P1 accepted |
| P2 metadata and existing gaps | Sol xhigh | realpath, readlink, stat, du, ls/find/df | P0 and P1 accepted |
| P2 system information | Luna | nproc, hostname, arch, uptime | P0 and P1 accepted |
| P2 checksums, codecs and integration | Coordinator | 12 commands, registry, fixtures, CI | Worker handoffs |
| P2 independent review | Astra / Terra / Sol | Filesystem/date / metadata / checksums and system | Frozen author snapshots |
| P3 remaining aligned commands | Sol/Terra xhigh, Luna, coordinator; Astra high review | 13 commands, external sort; accepted c547543 | P2 accepted |
| P4 extra commands A | Sol/Terra xhigh, Luna, coordinator; Astra high review | 16 commands and diff formats | P3 accepted |
| P5 extra commands B | Sol xhigh awk, Terra xhigh archives/repair, Luna initial xxd/file, coordinator patch; Astra high review | 8 commands | P4 accepted before delivery |
| P6 packaging and delivery | Coordinator and reviewer | Packaging, release, local installation | P5 accepted |

Each author has a separate worktree. Workers hand off uncommitted files;
the coordinator integrates them and runs the required checks before commits.
Shared registry, module exports, dependencies, and documentation are serialized.
Review findings are advisory evidence, not additional GitHub account approvals.

Completion requires the actual test results, phase reports, a merged PR, verified
release assets, and a local executable smoke test. Missing evidence is recorded as
`NOT_RUN` or `BLOCKED`; it is never replaced by an inferred pass.
