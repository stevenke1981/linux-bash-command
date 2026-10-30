# 06 測試策略

## 1. 層次

| 層 | 位置 | 內容 |
|---|---|---|
| 單元 | 各模組 `#[cfg(test)] mod tests` | `common::*` 全函式；各指令之解析器、核心演算法（sort 鍵比較、seq 定點、date 解析、tr 集合展開、awk lexer/parser…） |
| CLI | `tests/<group>.rs`（如 `tests/text_p1.rs`、`tests/fsops.rs`、`tests/sed.rs`） | 以執行檔跑：成功路徑、錯誤路徑、exit code、stderr 訊息、`--help`、`--version`、stdin、BrokenPipe |
| GNU 差分 | `tests/gnu_diff.rs` ＋ `tests/gnu_cases/<cmd>.json` | 僅 Linux 且 `AGENTCMD_GNU_DIFF=1` 時執行；同案例分別跑 GNU 與本專案，比 stdout 位元組與 exit code |
| 平台 | CLI 測試中 `#[cfg(windows)]`／`#[cfg(unix)]` | Windows 權限合成、`\\?\` 去除、PATHEXT、唯讀檔刪除、Job Object 殺行程樹、symlink 權限錯誤訊息 |
| 互動 | `tests/*.py`（pty，僅 Linux CI） | `rm -i`、`cp -i`、`find -ok`、`gzip` 覆寫詢問（沿用 `tests/less_pty.py` 模式） |

共用 helper：`tests/common/mod.rs` 提供
```rust
pub fn run(util: &str, args: &[&str], stdin: &[u8]) -> Output; // 透過 CARGO_BIN_EXE_<util>
pub fn run_multicall(args: &[&str], stdin: &[u8]) -> Output;     // CARGO_BIN_EXE_agentcmd
pub struct Sandbox { /* 暫存目錄、寫檔、建 symlink（無權限時回 None） */ }
```
測試一律使用暫存目錄，不得寫 repo 內檔案；不得依賴網路（`curl` 既有本機 server 模式除外）。

## 2. GNU 差分測試

案例檔格式（`serde_json`，已有依賴）：

```json
{
  "command": "sort",
  "cases": [
    {
      "name": "numeric_reverse_key",
      "args": ["-t", ",", "-k2,2nr", "data.csv"],
      "stdin": "",
      "files": { "data.csv": "a,3\nb,10\nc,2\n" },
      "gnu": "sort",
      "compare": ["stdout", "exit"],
      "skip_reason": null
    }
  ]
}
```

規則：
- 每案例建立兩個獨立暫存目錄並寫入 `files`，cwd 設為該目錄；環境 `LC_ALL=C TZ=UTC POSIXLY_CORRECT` 未設、`COLUMNS=80`。
- `gnu` 為對照程式名（`sort`、`gawk --posix` 以 `["gawk","--posix"]` 陣列表示）。
- `compare` 可含 `stdout`、`exit`、`files`（比對執行後暫存目錄內所有檔案內容，用於 `cp mv rm sed -i split patch tar -x` 等）、`stderr_empty`。
- 差分失敗訊息須印出雙方 stdout（以 `{:?}` 位元組）與 exit code。
- `skip_reason` 非空之案例不執行但須保留（記錄已知差異）；每個 skip 須於 [07-phases.md](07-phases.md) 之「已知差異」表列出。

CI（`ubuntu-24.04`）須安裝：`sudo apt-get install -y gawk tree xxd dos2unix patch bsdextrautils`（`coreutils findutils grep sed diffutils tar gzip debianutils` 已內建）。

### 2.1 差分器（`tests/gnu_diff.rs`）實作細節（v1.1 新增）

**閘門**
- 整個檔案 `#![cfg(target_os = "linux")]`；非 Linux 平台編譯為空。
- 只有一個 `#[test] fn gnu_differential()`。`AGENTCMD_GNU_DIFF` 未設或非 `1` → `eprintln!("gnu_diff: skipped (set AGENTCMD_GNU_DIFF=1)")` 後直接通過，使一般 `cargo test` 不受影響。
- 設為 `1` 時，所需 GNU 程式缺少 → **失敗**（不可默默略過），訊息列出缺少之程式與 apt 套件名。
- `AGENTCMD_GNU_DIFF_FILTER`：可選，`sort` 只跑該指令；`sort/numeric_*` 以 glob 篩案例名。

**兩邊如何執行**
- 本專案側：一律 `env!("CARGO_BIN_EXE_agentcmd")`，參數為 `[command, args...]`（multi-call）。理由：`env!` 須編譯期字面值，無法依 JSON 動態取各指令 bin；multi-call 可涵蓋全部（含 `[`、`egrep`）。
- GNU 側：`gnu` 欄第一個元素於 `/usr/bin`、`/bin` 依序找絕對路徑（**不得**用 PATH 搜尋，以免找到 `target/` 內本專案之同名程式）；`gnu` 省略時預設 `[command]`。
- 兩側環境相同：`env_clear()` 後只設 `PATH=/usr/bin:/bin`、`LC_ALL=C`、`TZ=UTC`、`COLUMNS=80`、`HOME=<該側 sandbox>`，再加案例 `env` 欄。故 `xargs`／`env`／`find -exec` 等於兩側都會呼叫 `/usr/bin` 之子程式，不影響比對。
- 兩側各自一個新 sandbox（`std::env::temp_dir()` 下唯一目錄，案例結束刪除），cwd 為該 sandbox。
- 每側逾時 10 秒（`案例 timeout_ms` 可覆寫）；逾時 kill 並判該案例失敗。

**案例欄位（全部）**

| 欄位 | 型別 | 預設 | 說明 |
|---|---|---|---|
| `name` | string | 必填 | 檔內唯一 |
| `args` | string[] | `[]` | 不經 shell |
| `stdin` | string | `""` | UTF-8 文字 |
| `stdin_hex` | string | — | 二進位 stdin（與 `stdin` 互斥）；以手寫 hex 解碼，不加依賴 |
| `files` | object | `{}` | 路徑 → 內容。值為字串＝文字檔；或物件：`{"hex":"00ff"}`、`{"dir":true}`、`{"symlink":"target"}`、`{"text":"..","mtime":1700000000,"mode":"644"}`（`mode` 只在 Unix 套用；`mtime` 以 `File::set_modified` 設定，須在 `ls -l`／`stat`／`find -newer` 等案例使用以保證決定性） |
| `env` | object | `{}` | 額外環境變數 |
| `gnu` | string[] | `[command]` | 對照程式與前置參數，如 `["gawk","--posix"]` |
| `compare` | string[] | `["stdout","exit"]` | 可加 `files`、`stderr_empty`、`stdout_sorted`（逐行排序後比，供 `du`、`find` 無序輸出） |
| `timeout_ms` | number | 10000 | — |
| `skip_reason` | string\|null | null | 非 null 則不執行，但計入報告 |

**比對**
- `stdout`：位元組完全相同。
- `exit`：exit code 相同（以訊號終止者以 `128+signal` 比）。
- `files`：執行後遞迴列出 sandbox（相對路徑、型別、檔案內容位元組、symlink 目標）兩側須相同；不比 mtime 與權限，除非加 `files_mode`。
- stderr 不比內容（訊息措辭由 CLI 測試鎖定）。

**輸出**
- 跑完**全部**案例後才判定；失敗時一次列出每個失敗案例：`<command>/<name>`、args、雙方 exit、雙方 stdout（`{:?}`，過長截至 2 KiB）與第一個差異位元組位置。
- 結尾摘要：`gnu_diff: N passed, M failed, K skipped`。

**本機沒有 Linux／GNU 時**
- 可用 WSL Ubuntu 24.04，或 `docker run --rm -v "$PWD":/w -w /w ubuntu:24.04`（容器內安裝 rustup 與 §2 套件）執行。
- 都不可行時：照寫案例，並於階段報告註明「GNU 差分未在本機執行，待 CI」。**不得**憑記憶捏造 GNU 輸出寫成 golden，也不得因無法執行而把案例標 skip。

## 3. 每指令最低測試要求（驗收用）

| 優先 | 單元 | CLI | GNU 差分案例 |
|---|---|---|---|
| P1、P2 | 核心邏輯 ≥ 5 | ≥ 8（含 `--help`、`--version`、未知選項 exit code、檔案不存在、stdin、多檔） | ≥ 15（不可差分者以 CLI golden 補足至同數） |
| P3 | ≥ 3 | ≥ 6 | ≥ 10 |
| sed、awk | ≥ 40 | ≥ 20 | ≥ 120（awk 須涵蓋每一內建函式、每一敘述、strnum 比較、`getline` 各形式、`printf` 轉換） |
| P4 其他、P5 其他 | ≥ 3 | ≥ 8 | ≥ 10（若有基準） |

案例須涵蓋該指令規格「選項」欄**每一個**選項至少一次（`tests/gnu_cases` 與 CLI 合計）。以腳本檢查：`tests/option_coverage.rs` 解析各指令 `--help` 列出之長選項，確認每個長選項至少出現在一個測試參數中（未出現 → 測試失敗並列出缺漏）。

## 4. 覆蓋率

- 本機：`cargo llvm-cov --summary-only`；新模組（`src/common/`、`src/commands/`）行覆蓋率 ≥ 80%。
- 每階段結束將摘要寫入 `docs/spec/coreutils-alignment/coverage.md`（附日期、commit、各模組百分比）。
- CI 不設覆蓋率閘門（避免三平台工具安裝成本），但 Linux job 可選擇性執行並上傳報告。

## 5. CI 變更（Phase 0）

`.github/workflows/build.yml`：
1. 新增 job `lint`（ubuntu）：`cargo fmt --check`、`cargo clippy --all-targets --locked -- -D warnings`。
2. 既有 `build` matrix 不變；`Collect commands` 改為以 `target/release/agentcmd --list` 產生清單（排除 `[`，另以 `test` 複製；Windows 排除 registry 標記不封裝者，見 01-architecture、03 `link`）。
3. 新增 job `gnu-diff`，**`runs-on: ubuntu-24.04`（固定版本，不可用 `ubuntu-latest`；後者自 2026-10-19 起遷移至 Ubuntu 26，GNU 版本會變）**：安裝 §2 套件後 `AGENTCMD_GNU_DIFF=1 cargo test --locked --test gnu_diff`。
4. 現有 `Verify Bash cd and interactive less` 步驟保留並加入互動 pty 測試。

## 6. 效能煙霧測試（不設閘門）

`tests/perf_smoke.rs`（`#[ignore]`，手動 `cargo test --release -- --ignored`）：產生 100 MB 文字，量測 `wc -l`、`sort`、`cut -f2`、`tr a-z A-Z`、`sed s/a/b/g`、`awk '{s+=$2}'`、`sha256sum`，與 GNU 比值記於 `coverage.md`。目標：不慢於 GNU 3 倍（`sort` 5 倍）。

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.1 | 2026-10-30 | 新增 §2.1 差分器實作細節；CI 固定 ubuntu-24.04 | tests/gnu_diff.rs, CI |
| v1.0 | 2026-10-30 | 初始建立 | — |
