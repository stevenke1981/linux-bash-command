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


## Accepted Phase 4 measurement, 2026-11-15

Source: `6770af1`, Ubuntu 24.04, [CI coverage artifact](https://github.com/stevenke1981/linux-bash-command/actions/runs/43705680348).
All six CI jobs passed, including Windows/Linux/macOS tests and release builds.
GNU differential: 2,715 passed, 0 failed, 39 documented skips; extra gated
metadata/hash/system oracle tests also passed. Windows 1,962, Linux 1,989,
macOS 1,980 tests passed, with no failures or ignored tests.

| Scope | Covered / lines | Coverage |
|---|---:|---:|
| common | 2,712 / 2,903 | 93.42% |
| commands | 20,641 / 22,741 | 90.77% |

Every shared/command file and the new diff format module exceeds 80%.

| Module | Covered / lines | Coverage |
|---|---:|---:|
| src/commands/basenc/codec.rs | 352 / 370 | 95.14% |
| src/commands/basenc/mod.rs | 69 / 79 | 87.34% |
| src/commands/chmod/mod.rs | 171 / 207 | 82.61% |
| src/commands/chmod/mode.rs | 114 / 122 | 93.44% |
| src/commands/cksum.rs | 129 / 150 | 86.00% |
| src/commands/cmp.rs | 284 / 325 | 87.38% |
| src/commands/column.rs | 261 / 283 | 92.23% |
| src/commands/comm.rs | 225 / 236 | 95.34% |
| src/commands/csplit.rs | 311 / 367 | 84.74% |
| src/commands/csplit/data.rs | 93 / 103 | 90.29% |
| src/commands/cut.rs | 189 / 200 | 94.50% |
| src/commands/date.rs | 128 / 137 | 93.43% |
| src/commands/dos2unix.rs | 133 / 149 | 89.26% |
| src/commands/dos2unix/stream.rs | 135 / 139 | 97.12% |
| src/commands/du.rs | 300 / 331 | 90.63% |
| src/commands/echo.rs | 52 / 53 | 98.11% |
| src/commands/egrep.rs | 24 / 24 | 100.00% |
| src/commands/env.rs | 261 / 306 | 85.29% |
| src/commands/expand.rs | 209 / 216 | 96.76% |
| src/commands/expr/eval.rs | 128 / 140 | 91.43% |
| src/commands/expr/mod.rs | 31 / 31 | 100.00% |
| src/commands/expr/parse.rs | 66 / 76 | 86.84% |
| src/commands/factor.rs | 233 / 249 | 93.57% |
| src/commands/fgrep.rs | 24 / 24 | 100.00% |
| src/commands/fmt/linebreak.rs | 285 / 322 | 88.51% |
| src/commands/fmt/mod.rs | 140 / 150 | 93.33% |
| src/commands/fmt/parasplit.rs | 417 / 434 | 96.08% |
| src/commands/fold.rs | 125 / 128 | 97.66% |
| src/commands/free.rs | 257 / 287 | 89.55% |
| src/commands/fsops/cp.rs | 464 / 534 | 86.89% |
| src/commands/fsops/link.rs | 53 / 54 | 98.15% |
| src/commands/fsops/ln.rs | 250 / 289 | 86.51% |
| src/commands/fsops/mkdir.rs | 109 / 112 | 97.32% |
| src/commands/fsops/mod.rs | 35 / 36 | 97.22% |
| src/commands/fsops/mode.rs | 73 / 82 | 89.02% |
| src/commands/fsops/mv.rs | 199 / 241 | 82.57% |
| src/commands/fsops/rm.rs | 239 / 273 | 87.55% |
| src/commands/fsops/rmdir.rs | 70 / 74 | 94.59% |
| src/commands/fsops/touch.rs | 175 / 197 | 88.83% |
| src/commands/fsops/transfer.rs | 82 / 97 | 84.54% |
| src/commands/fsops/unlink.rs | 52 / 54 | 96.30% |
| src/commands/hashsum/check.rs | 264 / 298 | 88.59% |
| src/commands/hashsum/engine.rs | 198 / 225 | 88.00% |
| src/commands/hashsum/mod.rs | 195 / 204 | 95.59% |
| src/commands/join.rs | 464 / 520 | 89.23% |
| src/commands/join/group.rs | 139 / 157 | 88.54% |
| src/commands/kill.rs | 230 / 276 | 83.33% |
| src/commands/mktemp.rs | 139 / 147 | 94.56% |
| src/commands/nl.rs | 178 / 187 | 95.19% |
| src/commands/numfmt/binary.rs | 109 / 123 | 88.62% |
| src/commands/numfmt/mod.rs | 225 / 243 | 92.59% |
| src/commands/numfmt/number.rs | 394 / 431 | 91.42% |
| src/commands/od.rs | 480 / 512 | 93.75% |
| src/commands/od/strings.rs | 75 / 81 | 92.59% |
| src/commands/paste.rs | 137 / 140 | 97.86% |
| src/commands/pathchk.rs | 191 / 207 | 92.27% |
| src/commands/pathname.rs | 125 / 150 | 83.33% |
| src/commands/pathname/canonical.rs | 225 / 249 | 90.36% |
| src/commands/pgrep.rs | 369 / 389 | 94.86% |
| src/commands/pkill.rs | 23 / 23 | 100.00% |
| src/commands/pr/layout.rs | 133 / 153 | 86.93% |
| src/commands/pr/mod.rs | 274 / 323 | 84.83% |
| src/commands/pr/options.rs | 220 / 234 | 94.02% |
| src/commands/printenv.rs | 75 / 77 | 97.40% |
| src/commands/printf.rs | 442 / 513 | 86.16% |
| src/commands/ptx/layout.rs | 148 / 178 | 83.15% |
| src/commands/ptx/mod.rs | 221 / 238 | 92.86% |
| src/commands/ptx/options.rs | 154 / 185 | 83.24% |
| src/commands/rev.rs | 58 / 66 | 87.88% |
| src/commands/sed/addr.rs | 120 / 124 | 96.77% |
| src/commands/sed/exec.rs | 113 / 113 | 100.00% |
| src/commands/sed/exec/actions.rs | 174 / 200 | 87.00% |
| src/commands/sed/inplace.rs | 103 / 116 | 88.79% |
| src/commands/sed/input.rs | 89 / 109 | 81.65% |
| src/commands/sed/mod.rs | 27 / 27 | 100.00% |
| src/commands/sed/options.rs | 62 / 66 | 93.94% |
| src/commands/sed/output.rs | 170 / 188 | 90.43% |
| src/commands/sed/parse.rs | 372 / 438 | 84.93% |
| src/commands/sed/pattern.rs | 245 / 258 | 94.96% |
| src/commands/sed/replacement.rs | 108 / 116 | 93.10% |
| src/commands/sed/subst.rs | 77 / 77 | 100.00% |
| src/commands/seq.rs | 199 / 231 | 86.15% |
| src/commands/shuf.rs | 326 / 376 | 86.70% |
| src/commands/sleep.rs | 51 / 55 | 92.73% |
| src/commands/sort/compare.rs | 292 / 310 | 94.19% |
| src/commands/sort/keys.rs | 116 / 126 | 92.06% |
| src/commands/sort/mod.rs | 265 / 278 | 95.32% |
| src/commands/sort/storage.rs | 116 / 128 | 90.62% |
| src/commands/split.rs | 239 / 249 | 95.98% |
| src/commands/split/stream.rs | 334 / 417 | 80.10% |
| src/commands/stat.rs | 86 / 88 | 97.73% |
| src/commands/stat/filesystem.rs | 123 / 126 | 97.62% |
| src/commands/stat/format.rs | 322 / 372 | 86.56% |
| src/commands/sysinfo_cmds.rs | 330 / 344 | 95.93% |
| src/commands/tac.rs | 175 / 183 | 95.63% |
| src/commands/tee.rs | 127 / 136 | 93.38% |
| src/commands/test.rs | 259 / 268 | 96.64% |
| src/commands/timeout.rs | 163 / 174 | 93.68% |
| src/commands/timeout/unix.rs | 17 / 19 | 89.47% |
| src/commands/tr.rs | 299 / 313 | 95.53% |
| src/commands/tree/mod.rs | 323 / 358 | 90.22% |
| src/commands/tree/options.rs | 203 / 208 | 97.60% |
| src/commands/truefalse.rs | 44 / 45 | 97.78% |
| src/commands/truncate.rs | 140 / 152 | 92.11% |
| src/commands/tsort.rs | 184 / 190 | 96.84% |
| src/commands/uname.rs | 216 / 223 | 96.86% |
| src/commands/uniq.rs | 222 / 253 | 87.75% |
| src/commands/unix2dos.rs | 3 / 3 | 100.00% |
| src/commands/wc.rs | 217 / 226 | 96.02% |
| src/commands/which.rs | 268 / 281 | 95.37% |
| src/commands/whoami.rs | 60 / 63 | 95.24% |
| src/commands/xargs.rs | 394 / 467 | 84.37% |
| src/commands/yes.rs | 36 / 37 | 97.30% |
| src/common/args.rs | 270 / 277 | 97.47% |
| src/common/bre.rs | 428 / 473 | 90.49% |
| src/common/color.rs | 136 / 149 | 91.28% |
| src/common/datetime/format.rs | 122 / 134 | 91.04% |
| src/common/datetime/mod.rs | 26 / 28 | 92.86% |
| src/common/datetime/parse.rs | 449 / 482 | 93.15% |
| src/common/error.rs | 42 / 45 | 93.33% |
| src/common/escape.rs | 123 / 143 | 86.01% |
| src/common/execution.rs | 3 / 3 | 100.00% |
| src/common/external.rs | 359 / 371 | 96.77% |
| src/common/io.rs | 95 / 107 | 88.79% |
| src/common/meta.rs | 125 / 130 | 96.15% |
| src/common/meta/identity.rs | 111 / 113 | 98.23% |
| src/common/meta/owners.rs | 86 / 92 | 93.48% |
| src/common/path.rs | 26 / 26 | 100.00% |
| src/common/process.rs | 106 / 115 | 92.17% |
| src/common/regex.rs | 144 / 154 | 93.51% |
| src/common/size.rs | 61 / 61 | 100.00% |
| src/diff/formats.rs | 195 / 208 | 93.75% |


## Accepted Phase 5 measurement, 2026-11-15

Source: `26ce71e42d9809b16aa7fab10aceaf460e3fcc64`, Ubuntu 26.04 CI coverage job in [workflow run 43709753271](https://github.com/stevenke1981/linux-bash-command/actions/runs/43709753271). The exact `linux-command-coverage` artifact (ID `14160126941`) was downloaded and summarized from `coverage.json`.

Weighted line coverage from the artifact:

| Scope | Covered / lines | Coverage |
|---|---:|---:|
| common | 2,713 / 2,903 | 93.46% |
| commands | 27,368 / 30,212 | 90.59% |

All 18 shared files and 154 command files meet the 80% minimum. The lowest measured file is `src/commands/split/stream.rs` at 80.10%. The table below lists every measured shared and command file from this artifact (172 files total); percentages are weighted covered/total line counts from LLVM coverage, not averages of module percentages.

| Module | Covered / lines | Coverage |
|---|---:|---:|
| `src/commands/awk/analyze.rs` | 145 / 163 | 88.96% |
| `src/commands/awk/ast.rs` | 19 / 19 | 100.00% |
| `src/commands/awk/fields.rs` | 178 / 188 | 94.68% |
| `src/commands/awk/format.rs` | 247 / 284 | 86.97% |
| `src/commands/awk/interp.rs` | 215 / 225 | 95.56% |
| `src/commands/awk/interp/builtins.rs` | 177 / 203 | 87.19% |
| `src/commands/awk/interp/eval.rs` | 129 / 150 | 86.00% |
| `src/commands/awk/interp/input.rs` | 88 / 91 | 96.70% |
| `src/commands/awk/interp/statements.rs` | 105 / 121 | 86.78% |
| `src/commands/awk/interp/variables.rs` | 109 / 111 | 98.20% |
| `src/commands/awk/io.rs` | 274 / 291 | 94.16% |
| `src/commands/awk/lexer.rs` | 344 / 347 | 99.14% |
| `src/commands/awk/mod.rs` | 67 / 80 | 83.75% |
| `src/commands/awk/parser.rs` | 353 / 380 | 92.89% |
| `src/commands/awk/parser/expression.rs` | 177 / 199 | 88.94% |
| `src/commands/awk/regexp.rs` | 178 / 197 | 90.36% |
| `src/commands/awk/regexp/class.rs` | 67 / 77 | 87.01% |
| `src/commands/awk/regexp/nfa.rs` | 90 / 99 | 90.91% |
| `src/commands/awk/regexp/scan.rs` | 98 / 108 | 90.74% |
| `src/commands/awk/regexp/syntax.rs` | 139 / 152 | 91.45% |
| `src/commands/awk/value.rs` | 145 / 147 | 98.64% |
| `src/commands/basenc/codec.rs` | 352 / 370 | 95.14% |
| `src/commands/basenc/mod.rs` | 69 / 79 | 87.34% |
| `src/commands/chmod/mod.rs` | 171 / 207 | 82.61% |
| `src/commands/chmod/mode.rs` | 114 / 122 | 93.44% |
| `src/commands/cksum.rs` | 129 / 150 | 86.00% |
| `src/commands/cmp.rs` | 284 / 325 | 87.38% |
| `src/commands/column.rs` | 261 / 283 | 92.23% |
| `src/commands/comm.rs` | 225 / 236 | 95.34% |
| `src/commands/csplit.rs` | 311 / 367 | 84.74% |
| `src/commands/csplit/data.rs` | 93 / 103 | 90.29% |
| `src/commands/cut.rs` | 189 / 200 | 94.50% |
| `src/commands/date.rs` | 128 / 137 | 93.43% |
| `src/commands/dos2unix.rs` | 133 / 149 | 89.26% |
| `src/commands/dos2unix/stream.rs` | 135 / 139 | 97.12% |
| `src/commands/du.rs` | 300 / 331 | 90.63% |
| `src/commands/echo.rs` | 52 / 53 | 98.11% |
| `src/commands/egrep.rs` | 24 / 24 | 100.00% |
| `src/commands/env.rs` | 261 / 306 | 85.29% |
| `src/commands/expand.rs` | 209 / 216 | 96.76% |
| `src/commands/expr/eval.rs` | 128 / 140 | 91.43% |
| `src/commands/expr/mod.rs` | 31 / 31 | 100.00% |
| `src/commands/expr/parse.rs` | 66 / 76 | 86.84% |
| `src/commands/factor.rs` | 233 / 249 | 93.57% |
| `src/commands/fgrep.rs` | 24 / 24 | 100.00% |
| `src/commands/file.rs` | 414 / 500 | 82.80% |
| `src/commands/fmt/linebreak.rs` | 285 / 322 | 88.51% |
| `src/commands/fmt/mod.rs` | 140 / 150 | 93.33% |
| `src/commands/fmt/parasplit.rs` | 417 / 434 | 96.08% |
| `src/commands/fold.rs` | 125 / 128 | 97.66% |
| `src/commands/free.rs` | 257 / 287 | 89.55% |
| `src/commands/fsops/cp.rs` | 464 / 534 | 86.89% |
| `src/commands/fsops/link.rs` | 53 / 54 | 98.15% |
| `src/commands/fsops/ln.rs` | 250 / 289 | 86.51% |
| `src/commands/fsops/mkdir.rs` | 109 / 112 | 97.32% |
| `src/commands/fsops/mod.rs` | 35 / 36 | 97.22% |
| `src/commands/fsops/mode.rs` | 73 / 82 | 89.02% |
| `src/commands/fsops/mv.rs` | 199 / 241 | 82.57% |
| `src/commands/fsops/rm.rs` | 239 / 273 | 87.55% |
| `src/commands/fsops/rmdir.rs` | 70 / 74 | 94.59% |
| `src/commands/fsops/touch.rs` | 175 / 197 | 88.83% |
| `src/commands/fsops/transfer.rs` | 82 / 97 | 84.54% |
| `src/commands/fsops/unlink.rs` | 52 / 54 | 96.30% |
| `src/commands/gunzip.rs` | 3 / 3 | 100.00% |
| `src/commands/gzip.rs` | 8 / 8 | 100.00% |
| `src/commands/gzip/codec.rs` | 58 / 58 | 100.00% |
| `src/commands/gzip/decode.rs` | 115 / 119 | 96.64% |
| `src/commands/gzip/files.rs` | 475 / 533 | 89.12% |
| `src/commands/gzip/options.rs` | 105 / 106 | 99.06% |
| `src/commands/hashsum/check.rs` | 264 / 298 | 88.59% |
| `src/commands/hashsum/engine.rs` | 198 / 225 | 88.00% |
| `src/commands/hashsum/mod.rs` | 195 / 204 | 95.59% |
| `src/commands/join.rs` | 464 / 520 | 89.23% |
| `src/commands/join/group.rs` | 139 / 157 | 88.54% |
| `src/commands/kill.rs` | 230 / 276 | 83.33% |
| `src/commands/mktemp.rs` | 139 / 147 | 94.56% |
| `src/commands/nl.rs` | 178 / 187 | 95.19% |
| `src/commands/numfmt/binary.rs` | 109 / 123 | 88.62% |
| `src/commands/numfmt/mod.rs` | 225 / 243 | 92.59% |
| `src/commands/numfmt/number.rs` | 394 / 431 | 91.42% |
| `src/commands/od.rs` | 480 / 512 | 93.75% |
| `src/commands/od/strings.rs` | 75 / 81 | 92.59% |
| `src/commands/paste.rs` | 137 / 140 | 97.86% |
| `src/commands/patch/apply.rs` | 213 / 222 | 95.95% |
| `src/commands/patch/mod.rs` | 379 / 454 | 83.48% |
| `src/commands/patch/options.rs` | 85 / 93 | 91.40% |
| `src/commands/patch/parse.rs` | 274 / 300 | 91.33% |
| `src/commands/pathchk.rs` | 191 / 207 | 92.27% |
| `src/commands/pathname.rs` | 125 / 150 | 83.33% |
| `src/commands/pathname/canonical.rs` | 225 / 249 | 90.36% |
| `src/commands/pgrep.rs` | 369 / 389 | 94.86% |
| `src/commands/pkill.rs` | 23 / 23 | 100.00% |
| `src/commands/pr/layout.rs` | 133 / 153 | 86.93% |
| `src/commands/pr/mod.rs` | 274 / 323 | 84.83% |
| `src/commands/pr/options.rs` | 220 / 234 | 94.02% |
| `src/commands/printenv.rs` | 75 / 77 | 97.40% |
| `src/commands/printf.rs` | 446 / 513 | 86.94% |
| `src/commands/ptx/layout.rs` | 148 / 178 | 83.15% |
| `src/commands/ptx/mod.rs` | 221 / 238 | 92.86% |
| `src/commands/ptx/options.rs` | 154 / 185 | 83.24% |
| `src/commands/rev.rs` | 58 / 66 | 87.88% |
| `src/commands/sed/addr.rs` | 120 / 124 | 96.77% |
| `src/commands/sed/exec.rs` | 113 / 113 | 100.00% |
| `src/commands/sed/exec/actions.rs` | 174 / 200 | 87.00% |
| `src/commands/sed/inplace.rs` | 103 / 116 | 88.79% |
| `src/commands/sed/input.rs` | 89 / 109 | 81.65% |
| `src/commands/sed/mod.rs` | 27 / 27 | 100.00% |
| `src/commands/sed/options.rs` | 62 / 66 | 93.94% |
| `src/commands/sed/output.rs` | 170 / 188 | 90.43% |
| `src/commands/sed/parse.rs` | 372 / 438 | 84.93% |
| `src/commands/sed/pattern.rs` | 245 / 258 | 94.96% |
| `src/commands/sed/replacement.rs` | 108 / 116 | 93.10% |
| `src/commands/sed/subst.rs` | 77 / 77 | 100.00% |
| `src/commands/seq.rs` | 199 / 231 | 86.15% |
| `src/commands/shuf.rs` | 326 / 376 | 86.70% |
| `src/commands/sleep.rs` | 51 / 55 | 92.73% |
| `src/commands/sort/compare.rs` | 292 / 310 | 94.19% |
| `src/commands/sort/keys.rs` | 116 / 126 | 92.06% |
| `src/commands/sort/mod.rs` | 265 / 278 | 95.32% |
| `src/commands/sort/storage.rs` | 116 / 128 | 90.62% |
| `src/commands/split.rs` | 239 / 249 | 95.98% |
| `src/commands/split/stream.rs` | 334 / 417 | 80.10% |
| `src/commands/stat.rs` | 86 / 88 | 97.73% |
| `src/commands/stat/filesystem.rs` | 123 / 126 | 97.62% |
| `src/commands/stat/format.rs` | 322 / 372 | 86.56% |
| `src/commands/sysinfo_cmds.rs` | 330 / 344 | 95.93% |
| `src/commands/tac.rs` | 175 / 183 | 95.63% |
| `src/commands/tar.rs` | 16 / 18 | 88.89% |
| `src/commands/tar/create.rs` | 122 / 143 | 85.31% |
| `src/commands/tar/extract.rs` | 218 / 253 | 86.17% |
| `src/commands/tar/format.rs` | 82 / 100 | 82.00% |
| `src/commands/tar/options.rs` | 134 / 150 | 89.33% |
| `src/commands/tar/path.rs` | 64 / 68 | 94.12% |
| `src/commands/tar/select.rs` | 92 / 96 | 95.83% |
| `src/commands/tee.rs` | 127 / 136 | 93.38% |
| `src/commands/test.rs` | 259 / 268 | 96.64% |
| `src/commands/timeout.rs` | 163 / 174 | 93.68% |
| `src/commands/timeout/unix.rs` | 17 / 19 | 89.47% |
| `src/commands/tr.rs` | 299 / 313 | 95.53% |
| `src/commands/tree/mod.rs` | 323 / 358 | 90.22% |
| `src/commands/tree/options.rs` | 203 / 208 | 97.60% |
| `src/commands/truefalse.rs` | 44 / 45 | 97.78% |
| `src/commands/truncate.rs` | 140 / 152 | 92.11% |
| `src/commands/tsort.rs` | 184 / 190 | 96.84% |
| `src/commands/uname.rs` | 216 / 223 | 96.86% |
| `src/commands/uniq.rs` | 222 / 253 | 87.75% |
| `src/commands/unix2dos.rs` | 3 / 3 | 100.00% |
| `src/commands/wc.rs` | 217 / 226 | 96.02% |
| `src/commands/which.rs` | 268 / 281 | 95.37% |
| `src/commands/whoami.rs` | 60 / 63 | 95.24% |
| `src/commands/xargs.rs` | 394 / 467 | 84.37% |
| `src/commands/xxd.rs` | 519 / 612 | 84.80% |
| `src/commands/yes.rs` | 36 / 37 | 97.30% |
| `src/commands/zcat.rs` | 3 / 3 | 100.00% |
| `src/common/args.rs` | 270 / 277 | 97.47% |
| `src/common/bre.rs` | 428 / 473 | 90.49% |
| `src/common/color.rs` | 136 / 149 | 91.28% |
| `src/common/datetime/format.rs` | 122 / 134 | 91.04% |
| `src/common/datetime/mod.rs` | 26 / 28 | 92.86% |
| `src/common/datetime/parse.rs` | 449 / 482 | 93.15% |
| `src/common/error.rs` | 43 / 45 | 95.56% |
| `src/common/escape.rs` | 123 / 143 | 86.01% |
| `src/common/execution.rs` | 3 / 3 | 100.00% |
| `src/common/external.rs` | 359 / 371 | 96.77% |
| `src/common/io.rs` | 95 / 107 | 88.79% |
| `src/common/meta.rs` | 125 / 130 | 96.15% |
| `src/common/meta/identity.rs` | 111 / 113 | 98.23% |
| `src/common/meta/owners.rs` | 86 / 92 | 93.48% |
| `src/common/path.rs` | 26 / 26 | 100.00% |
| `src/common/process.rs` | 106 / 115 | 92.17% |
| `src/common/regex.rs` | 144 / 154 | 93.51% |
| `src/common/size.rs` | 61 / 61 | 100.00% |


## Phase 6 release evidence - 2026-11-15

Production source: `c35e4c5`; final harness/workflows: `b566725`. The [b566725 coverage run](https://github.com/stevenke1981/linux-bash-command/actions/runs/43713273837) measured common **2,713/2,903 (93.46%)**, commands **27,405/30,259 (90.57%)**, and `src/diff/formats.rs` **195/208 (93.75%)**. All 173 files in these scopes meet 80%; minimum `src/commands/split/stream.rs` is 334/417 (80.10%). The preceding P5 table covers the same common/command paths; the only production change is whoami error handling, now **97/110 lines (88.18%)**. Unrelated legacy modules outside these scopes are not covered by this minimum claim.

The optimized local Windows portable package contains **122 files / 115 executables**, with **68,595,992 staged bytes** and a **31,656,020-byte ZIP (30.19 MiB)**. Registry inventory, every member CRC/payload SHA-256, bracket alias and absence of standalone link.exe were verified. ZIP SHA-256: `975060a76079b2c4f209490c5bf8f6e093c9b9986e290ed2c30845dc62f9d589`.

### Optimized Windows performance

100 MiB (104,857,600 bytes), x86_64, LC_ALL=C, TZ=UTC. Two runs reverse candidate/reference order. Both outputs are compared by length plus SHA-256 after every run; exit status is 0 and stderr empty. The optimized candidate hash also matches the packaged agentcmd.exe: `b5ac253421bf2e4a768304c162750e971d955c49fc636cfe6db8670977d450b7`. GNU references are Git for Windows coreutils 8.32, sed 4.9 and gawk 5.4.1, selected by absolute path. sha256sum uses explicit --binary on both sides to make its Windows output marker comparable.

| Command | Ours mean (s) | GNU mean (s) | Ratio | Target met |
|---|---:|---:|---:|---|
| wc | 0.288 | 0.068 | 4.26x | no |
| sort | 67.657 | 1.616 | 41.86x | no |
| cut | 0.718 | 1.032 | 0.70x | yes |
| tr | 0.215 | 0.243 | 0.88x | yes |
| sed | 6.533 | 5.470 | 1.19x | yes |
| awk | 4.234 | 1.016 | 4.17x | no |
| sha256sum | 0.087 | 0.366 | 0.24x | yes |

All seven output comparisons passed. wc, sort and awk miss the optional targets; no timing threshold is enforced. sort uses a 16 MiB external-sort buffer and a sandbox temporary directory. Linux measurements follow below; installer workflow verification is recorded in the phase report.


### Optimized Linux performance

The [b566725 Ubuntu runner](https://github.com/stevenke1981/linux-bash-command/actions/runs/43713273837) passed the same 100 MiB workload, two balanced runs and all seven output/exit/stderr checks. Candidate SHA-256: `f6e04586f8225d52fdb677c9fd052e7fa830d9c59ad4cae793e24244a80ae3fd`. GNU coreutils 9.7 binaries use their `/usr/bin/gnu*` names; the behavioral compatibility suite remains pinned to Ubuntu 24.04.

| Command | Ours mean (s) | GNU mean (s) | Ratio | Target met |
|---|---:|---:|---:|---|
| wc | 0.421 | 0.014 | 30.48x | no |
| sort | 32.809 | 0.827 | 39.66x | no |
| cut | 0.416 | 0.187 | 2.22x | yes |
| tr | 0.198 | 0.103 | 1.91x | yes |
| sed | 2.670 | 1.959 | 1.36x | yes |
| awk | 2.027 | 0.470 | 4.31x | no |
| sha256sum | 0.084 | 0.084 | 1.00x | yes |

wc, sort and awk miss the optional Linux targets. The other four meet them; all output digests match GNU. This is a smoke workload, not a broad benchmark.


### CI release packages

[Build-only release run 43714554737](https://github.com/stevenke1981/linux-bash-command/actions/runs/43714554737) built source `b566725` using workflow `cfc798e`. All three packages, real Windows installer smoke and checksum bundle passed; publication was skipped. Downloaded assets passed independent SHA-256, archive CRC, exact inventory, executable-mode, bracket-alias and CPU-architecture checks. Windows has 115 executables; both Unix archives expose all 115 registry names plus agentcmd.

| Asset | Bytes | SHA-256 |
|---|---:|---|
| Agent-Commands-0.2.0-Windows-Setup.exe | 8,946,743 | `8e6800a6e4e89ccda8a7710beb2a9d59de6044d5728d9df2c30ca52c64d118b9` |
| agent-commands-0.2.0-linux-x86_64.tar.gz | 34,311,556 | `fdd312f88abb78f767dc1ddbb943f4d88eda73ad4d3c71affba06f621f853f9a` |
| agent-commands-0.2.0-macos-aarch64.tar.gz | 30,556,238 | `49c63ac2f61091973a55014ed165e8ae51ee5d42e45d1da48130d6f6fc6effac` |
| agent-commands-0.2.0-windows-x86_64.zip | 23,451,414 | `54c449f4b0228a32ab72d20a855a7f3304230b8b4c04a8f57061ae1fb2a82d1a` |
