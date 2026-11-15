# 驗證與覆蓋率

## 2026-11-15 Phase 0 candidate

- Windows `cargo test --locked`: 1,053 passed, 0 failed, 1 ignored.
- Windows fmt and clippy (all targets, warnings denied): PASS.
- Current Linux GNU, macOS, and coverage: pending CI; do not substitute the
  historical measurements below for the current candidate.
- Source/test snapshot: `c9718a1ff486b5655409d4a17d726ed9457ef17f11a7319bdbe8cdeba8b4df44`.
- Linux measurement at `82b4f81`: common 1,039/1,149 (90.43%), commands
  4,748/5,167 (91.89%); newer path helper tests await an updated measurement.
- Full evidence and outstanding gates: [Phase 0 report](../../implementation/linux-commands/phase-0.md).

## Historical Phase 1 verification

- 日期：2026-10-30（Asia/Taipei）。
- Revision：本文件所屬的 `feat: implement phase 1 text pipeline commands` 提交；分支 `feat/phase1-coreutils`，parent `ca2e828`。可用 `git log -1 -- docs/spec/linux-commands/coverage.md` 取得完整 commit。
- 已測 `src/` 與 `tests/` 快照 SHA-256：`42d036fc9dd7d4cd49a795d91a3a3f87fba1eb0ba8838fc3207e91ece2bd5690`。依路徑排序，逐檔串接 UTF-8 相對路徑、NUL、LF 正規化內容、NUL 計算。
- Phase 1 新增 29 個 registry 名稱，連同既有指令共 46 個。原工作區同時進行的 Phase 0 修改保留在原處；本提交以獨立 worktree 驗證。

## 驗證結果

| 檢查 | 結果 |
|---|---|
| Windows `cargo test --locked` | 963 通過、0 失敗、1 ignored |
| Ubuntu 24.04 `cargo test --locked` | 966 通過、0 失敗、1 ignored |
| 兩平台 `cargo fmt --check` | PASS |
| 兩平台 `cargo clippy --all-targets --locked -- -D warnings` | PASS |
| GNU 差分 | 974 案例：958 通過、0 失敗、16 skip |
| 選項覆蓋檢查 | 全部 29 個 P1 名稱的 help 長選項均有案例；每個名稱至少 15 個有效 GNU 案例 |
| 檔案大小規則 | 全部新增 Rust 檔案不超過 800 行 |

GNU 對照使用隔離的 Ubuntu 24.04 VM、coreutils 9.4、grep 3.11，固定 `LC_ALL=C TZ=UTC`。執行 `AGENTCMD_GNU_DIFF=1 cargo test --locked --test gnu_diff -- --nocapture`，包含 P1 既有指令補項。所有 skip 都列在 [07-phases.md 的已知差異表](07-phases.md)。比較只使用 GNU 執行檔，未讀取或移植其原始碼。

唯一 ignored 測試是 `sort::tests::external_merge_memory_limit`：P1 使用記憶體排序，外部合併依 07 明確延至 Phase 3。

## 行覆蓋率

Ubuntu 24.04，Rust 1.99.1，cargo-llvm-cov 0.9.1。先執行 `cargo llvm-cov --summary-only`，再以 `cargo llvm-cov --no-clean --test phase1_coverage --summary-only` 累加針對實際未覆蓋分支的 CLI 回歸案例；保留完整測試的 profiles。下表使用最終合併報告，不平均各檔百分比，而以實際 covered / total lines 加權。

Windows 也執行過量測，但該環境未收錄 CLI 子程序的完整 profiles；因此本階段覆蓋率閘門採用 Ubuntu 的實際報告。

| Scope | Covered / lines | Coverage |
|---|---:|---:|
| `src/common/` | 753 / 847 | **88.90%** |
| `src/commands/` | 4996 / 5392 | **92.66%** |

| Module | Covered / lines | Coverage |
|---|---:|---:|
| `src/commands/comm.rs` | 227 / 236 | 96.19% |
| `src/commands/cut.rs` | 190 / 195 | 97.44% |
| `src/commands/echo.rs` | 52 / 53 | 98.11% |
| `src/commands/egrep.rs` | 24 / 24 | 100.00% |
| `src/commands/env.rs` | 271 / 313 | 86.58% |
| `src/commands/expand.rs` | 214 / 221 | 96.83% |
| `src/commands/fgrep.rs` | 24 / 24 | 100.00% |
| `src/commands/fold.rs` | 127 / 130 | 97.69% |
| `src/commands/nl.rs` | 196 / 204 | 96.08% |
| `src/commands/paste.rs` | 135 / 138 | 97.83% |
| `src/commands/pathname.rs` | 122 / 147 | 82.99% |
| `src/commands/printenv.rs` | 81 / 83 | 97.59% |
| `src/commands/printf.rs` | 449 / 518 | 86.68% |
| `src/commands/seq.rs` | 203 / 234 | 86.75% |
| `src/commands/sleep.rs` | 53 / 59 | 89.83% |
| `src/commands/sort/compare.rs` | 318 / 336 | 94.64% |
| `src/commands/sort/keys.rs` | 123 / 132 | 93.18% |
| `src/commands/sort/mod.rs` | 288 / 293 | 98.29% |
| `src/commands/tac.rs` | 181 / 190 | 95.26% |
| `src/commands/tee.rs` | 135 / 143 | 94.41% |
| `src/commands/test.rs` | 300 / 306 | 98.04% |
| `src/commands/tr.rs` | 325 / 337 | 96.44% |
| `src/commands/truefalse.rs` | 44 / 45 | 97.78% |
| `src/commands/uniq.rs` | 230 / 262 | 87.79% |
| `src/commands/wc.rs` | 221 / 230 | 96.09% |
| `src/commands/xargs.rs` | 425 / 500 | 85.00% |
| `src/commands/yes.rs` | 38 / 39 | 97.44% |
| `src/common/args.rs` | 236 / 250 | 94.40% |
| `src/common/error.rs` | 36 / 38 | 94.74% |
| `src/common/escape.rs` | 127 / 152 | 83.55% |
| `src/common/io.rs` | 70 / 83 | 84.34% |
| `src/common/path.rs` | 9 / 10 | 90.00% |
| `src/common/process.rs` | 107 / 117 | 91.45% |
| `src/common/regex.rs` | 122 / 151 | 80.79% |
| `src/common/size.rs` | 46 / 46 | 100.00% |

## 未執行範圍

- macOS 本機驗證、GitHub Actions：NOT_RUN；依本次指示未 push。
- 06 §6 的 100 MB 效能煙霧測試、release 包裝與安裝：NOT_RUN；不屬於本階段閘門。

新依賴只有白名單內的 `libc = "0.2"`（`cfg(unix)`）；既有 lockfile 中已包含該版本。


## Accepted Phase 0 / Phase 1 measurement, 2026-11-15

Source: `7fa2e6cbbdc0e1b44318a9f5d5d5067794a80456`, Ubuntu 24.04 CI,
[cargo-llvm-cov artifact](https://github.com/stevenke1981/linux-bash-command/actions/runs/43677395391).
This supersedes the earlier local-only status above. All six CI jobs passed,
including Windows/Linux/macOS builds, GNU differential, lint and coverage.

| Scope | Covered / lines | Coverage |
|---|---:|---:|
| common | 1,055 / 1,158 | 91.11% |
| commands | 4,748 / 5,167 | 91.89% |

Every measured module exceeds 80%; the lowest is common/meta at 80.85%.
Phase 2 has its own subsequent measurement; the above is the immutable P0/P1 result.


## Accepted Phase 2 measurement, 2026-11-15

Source: `071f6a9e448e4a0ef34f93b25673724ed9169361`, Ubuntu 24.04,
[CI coverage artifact](https://github.com/stevenke1981/linux-bash-command/actions/runs/43690770481).
All six CI jobs passed. This supersedes earlier Phase 2 candidate measurements.

| Scope | Covered / lines | Coverage |
|---|---:|---:|
| common | 1,901 / 2,043 | 93.05% |
| commands | 9,555 / 10,488 | 91.10% |

Every measured common/command file exceeds 80%.


## Accepted Phase 3 measurement, 2026-11-15

Source: `c547543874dbc75bfefa3963b574657de7fbf994`, Ubuntu24.04,
[CI coverage artifact](https://github.com/stevenke1981/linux-bash-command/actions/runs/43697989178).
All six jobs passed; every measured file exceeds80%.

| Scope | Covered / lines | Coverage |
|---|---:|---:|
| common |2,675 /2,868|93.27%|
| commands |15,693 /17,314|90.64%|

| File | Covered / lines | Coverage |
|---|---:|---:|
| `src/commands/basenc/codec.rs` | 352/370 | 95.14% |
| `src/commands/basenc/mod.rs` | 69/79 | 87.34% |
| `src/commands/cksum.rs` | 129/150 | 86.00% |
| `src/commands/comm.rs` | 225/236 | 95.34% |
| `src/commands/csplit.rs` | 311/367 | 84.74% |
| `src/commands/csplit/data.rs` | 93/103 | 90.29% |
| `src/commands/cut.rs` | 189/200 | 94.50% |
| `src/commands/date.rs` | 128/137 | 93.43% |
| `src/commands/du.rs` | 300/331 | 90.63% |
| `src/commands/echo.rs` | 52/53 | 98.11% |
| `src/commands/egrep.rs` | 24/24 | 100.00% |
| `src/commands/env.rs` | 261/306 | 85.29% |
| `src/commands/expand.rs` | 209/216 | 96.76% |
| `src/commands/expr/eval.rs` | 128/140 | 91.43% |
| `src/commands/expr/mod.rs` | 31/31 | 100.00% |
| `src/commands/expr/parse.rs` | 66/76 | 86.84% |
| `src/commands/factor.rs` | 233/249 | 93.57% |
| `src/commands/fgrep.rs` | 24/24 | 100.00% |
| `src/commands/fmt/linebreak.rs` | 285/322 | 88.51% |
| `src/commands/fmt/mod.rs` | 140/150 | 93.33% |
| `src/commands/fmt/parasplit.rs` | 417/434 | 96.08% |
| `src/commands/fold.rs` | 125/128 | 97.66% |
| `src/commands/fsops/cp.rs` | 464/534 | 86.89% |
| `src/commands/fsops/link.rs` | 53/54 | 98.15% |
| `src/commands/fsops/ln.rs` | 250/289 | 86.51% |
| `src/commands/fsops/mkdir.rs` | 109/112 | 97.32% |
| `src/commands/fsops/mod.rs` | 35/36 | 97.22% |
| `src/commands/fsops/mode.rs` | 73/82 | 89.02% |
| `src/commands/fsops/mv.rs` | 199/241 | 82.57% |
| `src/commands/fsops/rm.rs` | 239/273 | 87.55% |
| `src/commands/fsops/rmdir.rs` | 70/74 | 94.59% |
| `src/commands/fsops/touch.rs` | 175/197 | 88.83% |
| `src/commands/fsops/transfer.rs` | 82/97 | 84.54% |
| `src/commands/fsops/unlink.rs` | 52/54 | 96.30% |
| `src/commands/hashsum/check.rs` | 264/298 | 88.59% |
| `src/commands/hashsum/engine.rs` | 198/225 | 88.00% |
| `src/commands/hashsum/mod.rs` | 195/204 | 95.59% |
| `src/commands/join.rs` | 464/520 | 89.23% |
| `src/commands/join/group.rs` | 139/157 | 88.54% |
| `src/commands/mktemp.rs` | 139/147 | 94.56% |
| `src/commands/nl.rs` | 178/187 | 95.19% |
| `src/commands/numfmt/binary.rs` | 109/123 | 88.62% |
| `src/commands/numfmt/mod.rs` | 225/243 | 92.59% |
| `src/commands/numfmt/number.rs` | 394/431 | 91.42% |
| `src/commands/od.rs` | 480/512 | 93.75% |
| `src/commands/od/strings.rs` | 75/81 | 92.59% |
| `src/commands/paste.rs` | 137/140 | 97.86% |
| `src/commands/pathchk.rs` | 191/207 | 92.27% |
| `src/commands/pathname.rs` | 125/150 | 83.33% |
| `src/commands/pathname/canonical.rs` | 225/249 | 90.36% |
| `src/commands/pr/layout.rs` | 133/153 | 86.93% |
| `src/commands/pr/mod.rs` | 274/323 | 84.83% |
| `src/commands/pr/options.rs` | 220/234 | 94.02% |
| `src/commands/printenv.rs` | 75/77 | 97.40% |
| `src/commands/printf.rs` | 442/513 | 86.16% |
| `src/commands/ptx/layout.rs` | 148/178 | 83.15% |
| `src/commands/ptx/mod.rs` | 221/238 | 92.86% |
| `src/commands/ptx/options.rs` | 154/185 | 83.24% |
| `src/commands/seq.rs` | 199/231 | 86.15% |
| `src/commands/shuf.rs` | 326/376 | 86.70% |
| `src/commands/sleep.rs` | 51/55 | 92.73% |
| `src/commands/sort/compare.rs` | 292/310 | 94.19% |
| `src/commands/sort/keys.rs` | 116/126 | 92.06% |
| `src/commands/sort/mod.rs` | 265/278 | 95.32% |
| `src/commands/sort/storage.rs` | 116/128 | 90.62% |
| `src/commands/split.rs` | 239/249 | 95.98% |
| `src/commands/split/stream.rs` | 334/417 | 80.10% |
| `src/commands/stat.rs` | 86/88 | 97.73% |
| `src/commands/stat/filesystem.rs` | 123/126 | 97.62% |
| `src/commands/stat/format.rs` | 322/372 | 86.56% |
| `src/commands/sysinfo_cmds.rs` | 330/344 | 95.93% |
| `src/commands/tac.rs` | 175/183 | 95.63% |
| `src/commands/tee.rs` | 127/136 | 93.38% |
| `src/commands/test.rs` | 259/268 | 96.64% |
| `src/commands/tr.rs` | 299/313 | 95.53% |
| `src/commands/truefalse.rs` | 44/45 | 97.78% |
| `src/commands/truncate.rs` | 140/152 | 92.11% |
| `src/commands/tsort.rs` | 184/190 | 96.84% |
| `src/commands/uniq.rs` | 222/253 | 87.75% |
| `src/commands/wc.rs` | 217/226 | 96.02% |
| `src/commands/xargs.rs` | 394/467 | 84.37% |
| `src/commands/yes.rs` | 36/37 | 97.30% |
| `src/common/args.rs` | 269/277 | 97.11% |
| `src/common/bre.rs` | 428/473 | 90.49% |
| `src/common/color.rs` | 136/149 | 91.28% |
| `src/common/datetime/format.rs` | 122/134 | 91.04% |
| `src/common/datetime/mod.rs` | 26/28 | 92.86% |
| `src/common/datetime/parse.rs` | 449/482 | 93.15% |
| `src/common/error.rs` | 42/45 | 93.33% |
| `src/common/escape.rs` | 123/143 | 86.01% |
| `src/common/execution.rs` | 3/3 | 100.00% |
| `src/common/external.rs` | 332/344 | 96.51% |
| `src/common/io.rs` | 87/99 | 87.88% |
| `src/common/meta.rs` | 125/130 | 96.15% |
| `src/common/meta/identity.rs` | 111/113 | 98.23% |
| `src/common/meta/owners.rs` | 86/92 | 93.48% |
| `src/common/path.rs` | 26/26 | 100.00% |
| `src/common/process.rs` | 105/115 | 91.30% |
| `src/common/regex.rs` | 144/154 | 93.51% |
| `src/common/size.rs` | 61/61 | 100.00% |
