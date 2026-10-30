# Codex 實作 Prompts

依序逐階段貼給 Codex。每階段完成後先檢查其 Phase 報告與 CI，再貼下一則。
Phase 1 指令多，可在 prompt 末尾加 `Only do commands X–Y this session.` 分批。

---

## Prompt 0：基礎建設

```
Read AGENTS.md, then docs/spec/coreutils-alignment/README.md, 01-architecture.md, 02-conventions.md, 05-existing-gaps.md, 06-testing.md and 07-phases.md.

Implement Phase 0 exactly as described in 07-phases.md:
1. src/common/ (args, error, io, path, meta, size, regex, escape, process) with unit tests.
2. Behavior-preserving extraction of duplicated helpers listed in 05-existing-gaps.md §3.
3. Exit-code and message alignment for existing commands (05-existing-gaps.md §1–§2); update the affected test assertions to the new GNU values.
4. src/registry.rs and the multi-call agentcmd entry in src/main.rs (01-architecture.md §3–§4), registry containing the existing 17 commands.
5. Release profile in Cargo.toml, tests/common/mod.rs, and the GNU differential runner tests/gnu_diff.rs exactly as specified in 06-testing.md §2 and §2.1, with ≥5 cases each for cat, head, tail, grep, find, ls.
6. CI changes in 06-testing.md §5 (gnu-diff job on ubuntu-24.04).

Run cargo fmt --check, cargo clippy --all-targets --locked -- -D warnings, cargo test --locked, and AGENTCMD_GNU_DIFF=1 cargo test --locked --test gnu_diff on Linux. Commit by domain with conventional commits. Do not push. Finish with the Phase report format from 07-phases.md.
```

## Prompt 1：文字管線

```
Read AGENTS.md and docs/spec/coreutils-alignment/ (README, 01, 02, commands.md, 06, 07). Phase 0 is done.

Implement Phase 1 from 07-phases.md: the 29 commands marked P1 in commands.md, per their specs in 03-commands-core.md (wc, sort, uniq, cut, tr, tee, echo, printf, true, false, test, [, seq, tac, nl, paste, comm, fold, unexpand, yes, basename, dirname, xargs, env, printenv, sleep) and expand, egrep, fgrep. Also the P1 rows of 05-existing-gaps.md §4 (head/tail -z, cat -u, grep additions).

Follow the suggested order in 07-phases.md. For each command: src/commands/ module, src/bin shim, registry entry, unit + CLI tests, GNU diff cases (tests/gnu_cases/<cmd>.json) meeting 06-testing.md §3 minimums. Every option listed in the spec must be implemented and covered by a test.

Run all checks before each commit; do not push. Add any unavoidable deviation to the known-differences table in 07-phases.md. End with the Phase report.
```

## Prompt 2：檔案系統與系統資訊

```
Read AGENTS.md and docs/spec/coreutils-alignment/. Phases 0–1 are done.

Implement Phase 2 from 07-phases.md: the 32 commands marked P2 in commands.md per 03-commands-core.md, add common::datetime (shared by date, touch, stat, ls --time-style), and the P2 rows of 05-existing-gaps.md §4 (ls, find, df).

Safety is mandatory: rm/mv/cp/chmod/touch tests operate only inside sandbox temp dirs; preserve-root and '.'/'..' refusal must be a pure function tested by unit tests only (see the Phase 2 note in 07-phases.md). Respect the Windows rules in 02-conventions.md §7 and the link.exe packaging exclusion. Use the `mtime` field of GNU diff case files for any time-dependent output.

Run all checks, commit by domain, do not push, end with the Phase report.
```

## Prompt 3：其餘對齊指令

```
Read AGENTS.md and docs/spec/coreutils-alignment/. Phases 0–2 are done.

Implement Phase 3: join, od, split, csplit, fmt, pr, shuf, tsort, numfmt, factor, expr, pathchk, ptx per 03-commands-core.md, plus sort external merge (spill to temp files beyond -S or 256 MiB; un-ignore its placeholder tests).

Acceptance: every command in commands.md section B is listed by `agentcmd --list` and `--help` exits 0. Run all checks, commit, do not push, end with the Phase report.
```

## Prompt 4：擴充 A

```
Read AGENTS.md and docs/spec/coreutils-alignment/. Phases 0–3 are done.

Implement Phase 4 per 04-commands-extra.md: sed (first; split into parse/addr/exec/subst/mod, each ≤800 lines), which, tree, uname, whoami, timeout, kill, pgrep, pkill, free, cmp, rev, column, dos2unix, unix2dos, chmod. Also the diff changes in 05-existing-gaps.md §4 (normal format as default, -c/-C, -y/-W, --normal) and update README for that breaking change.

sed needs ≥40 unit, ≥20 CLI and ≥120 GNU diff cases. timeout on Windows must kill the whole process tree via a Job Object. Run all checks, commit, do not push, end with the Phase report.
```

## Prompt 5：擴充 B

```
Read AGENTS.md and docs/spec/coreutils-alignment/. Phases 0–4 are done.

Implement Phase 5 per 04-commands-extra.md, in this order: gzip/gunzip/zcat (flate2 rust_backend only) → tar (must enforce the path-traversal and symlink-escape rules) → awk (lexer → parser → interp → builtins → io, each file ≤800 lines; diff against `gawk --posix`; ≥120 diff cases) → patch → xxd → file.

Run all checks, commit, do not push, end with the Phase report.
```

## Prompt 6：封裝與文件

```
Read AGENTS.md and docs/spec/coreutils-alignment/07-phases.md Phase 6.

Generate packaging payloads from `agentcmd --list` (Windows: add [.exe as a copy of test.exe, exclude link.exe). Rewrite README's command section as categorized tables, add a Windows conflicts table from commands.md, and a "Known differences from GNU" section from the 07-phases.md table. Add NOTICE.md if any uutils code was ported. Record binary/package sizes and final coverage in docs/spec/coreutils-alignment/coverage.md.

Run all checks, commit, do not push, end with the final report covering all phases.
```

---

## 追問用：Codex 問到 GNU 差分器時

```
The GNU differential runner is now fully specified in docs/spec/coreutils-alignment/06-testing.md §2.1 (pull the latest main). Key points:
- tests/gnu_diff.rs is Linux-only, one #[test], passes with a "skipped" note unless AGENTCMD_GNU_DIFF=1; with it set, missing GNU tools are a failure.
- Our side always runs CARGO_BIN_EXE_agentcmd with [command, args...] (multi-call); the GNU side resolves the program from /usr/bin then /bin, never via PATH.
- Both sides: fresh sandbox cwd, env_clear() + PATH=/usr/bin:/bin LC_ALL=C TZ=UTC COLUMNS=80 HOME=<sandbox>, 10 s timeout.
- Compare stdout bytes and exit code by default; optional files / stderr_empty / stdout_sorted. Report all failures at the end.
- If you cannot run GNU locally (no Linux/WSL/Docker), still write the cases and state "GNU diff not run locally, pending CI" in the report. Never invent GNU output and never mark cases skipped for that reason.
Continue Phase 0 with this design.
```

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立（Prompt 0–6、差分器追問） | — |
