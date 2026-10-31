# Phase 1 驗證與覆蓋率

- 日期：2026-10-30（Asia/Taipei）。
- Revision：本文件所屬的 `feat: implement phase 1 text pipeline commands` 提交；分支 `feat/phase1-coreutils`，parent `ca2e828`。可用 `git log -1 -- docs/spec/coreutils-alignment/coverage.md` 取得完整 commit。
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
