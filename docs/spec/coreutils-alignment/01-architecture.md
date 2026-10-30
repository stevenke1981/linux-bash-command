# 01 架構

## 1. 現況（2026-10-30，commit `fe271f7`）

- 單一 crate `agentcmd`（edition 2021），`src/lib.rs` 匯出各指令模組；`src/bin/<name>.rs` 為 3 行 shim：`std::process::exit(agentcmd::<module>::<fn>(&agentcmd::arguments("<name>")))`。
- 指令進入點簽名一律為 `pub fn run(args: &[String]) -> i32`（args 不含 argv[0]），參數須為 UTF-8。
- `src/main.rs` 為舊版 `agentcmd` 子指令入口（只認 `search diff list read`）。
- 各模組手寫參數解析；`mode_string`、`human_size`、`parse_size`、`is_closed_pipe`、`report_open` 等於多處重複。
- 部分檔案已逾 800 行（`search.rs` 1538、`grep_command.rs` 1268、`curl_command.rs` 1256）。本規格不要求拆舊檔，但**新檔一律 ≤ 800 行**。

## 2. 目標佈局

```
src/
├── lib.rs                 # 匯出 common、commands、registry 與既有模組
├── main.rs                # agentcmd multi-call 入口（見 §4）
├── registry.rs            # 全部 utility 之靜態註冊表（見 §3）
├── common/                # 共用函式庫（Phase 0 抽出／新增）
│   ├── mod.rs
│   ├── args.rs            # GNU getopt_long 相容解析器
│   ├── error.rs           # 錯誤訊息格式、exit code 常數、usage 錯誤
│   ├── io.rs              # Input 抽象（檔案或 "-"）、BrokenPipe 處理、行/記錄讀取（\n 或 \0）
│   ├── path.rs            # display_path、Windows 路徑正規化（去 \\?\ 前綴）
│   ├── meta.rs            # mode_string、檔案型別、Windows 權限合成、uid/gid/inode 抽象
│   ├── size.rs            # GNU 尺寸字尾解析（K M G… KB MB… b c w）、human 格式（-h/--si）
│   ├── regex.rs           # BRE/ERE → Rust regex 轉譯（由 grep_command::bre_to_regex 移入）
│   ├── datetime.rs        # 日期解析（-d STRING）與 strftime 格式化（date/touch/stat/ls 共用）
│   ├── process.rs         # 外部指令執行：PATH/PATHEXT 解析、exit code 對映 126/127
│   └── escape.rs          # 反斜線跳脫（echo -e、printf、tr、sed）、shell 引號輸出（ls -Q、printf %q）
├── commands/              # 新指令；一指令一檔，大者一目錄
│   ├── mod.rs
│   ├── wc.rs  sort/  uniq.rs  cut.rs  tr.rs ...
│   ├── sed/               # lexer.rs parse.rs exec.rs ...
│   └── awk/               # lexer.rs parser.rs interp.rs builtins.rs io.rs value.rs
├── (既有模組維持原位：text_commands.rs fs_commands.rs grep_command.rs …)
└── bin/<name>.rs          # 每指令一 shim（見 §5）
```

規則：
- 新指令放 `src/commands/`；單檔 > 800 行即拆為子目錄。函式 ≤ 50 行為原則（解析表、HELP 常數除外）。
- 既有模組只在「抽共用函式」或「修 05-existing-gaps 所列落差」時修改。
- `common/` 之函式須有單元測試。

## 3. Registry

`src/registry.rs`：

```rust
pub type RunFn = fn(&[String]) -> i32;

pub struct Utility {
    pub name: &'static str,          // 指令名，如 "sort"、"["
    pub run: RunFn,                  // 進入點；args 不含 argv[0]
    pub summary: &'static str,       // 一行說明，供 agentcmd --help
    pub origin: Origin,              // Existing | Ms | Extra
    pub windows_conflict: Option<&'static str>, // 與 Windows 內建衝突之說明；None 表無
}

pub enum Origin { Existing, Ms, Extra }

pub static UTILITIES: &[Utility] = &[ /* 依 name 位元組序排序 */ ];

pub fn find(name: &str) -> Option<&'static Utility>;
```

- `UTILITIES` 必須依 `name` 排序；單元測試驗證排序、無重名、與 [commands.md](commands.md) 一致（測試內嵌清單）。
- 須包含既有 17 個指令與新指令；`[` 與 `test` 共用實作（`test::run_bracket` 要求最後一參數為 `]`）。
- `egrep`／`fgrep` 映射至 grep 並前置 `-E`／`-F`。

## 4. Multi-call 入口（`agentcmd`）

比照 microsoft/coreutils `src/main.rs` 行為：

1. 取 argv[0] 之 basename；Windows 去除 `.exe`（不分大小寫）。
2. 若 basename 為 `agentcmd`（或結尾為 `utils`）→ 以第一參數為 utility 名；否則以 basename 查 registry（取最長後綴匹配，例 `x86_64-sort` → `sort`）。
3. 特殊參數（僅當作為 utility 名位置時）：
   - `--list`：每行一個 utility 名，依 registry 順序；附加任何參數 → `agentcmd: invalid argument`，exit 1。
   - `--version`／`-V`：`agentcmd <版本> (multi-call binary)`。
   - `--help`／`-h` [UTIL]：無 UTIL 印總說明與指令列表；有 UTIL 等同 `UTIL --help`。
   - 其他以 `-` 開頭 → `agentcmd: unrecognized option '<arg>'`，exit 1。
4. 找不到 utility → `agentcmd: function/utility not found: <name>`（stderr），exit 1。
5. **向後相容**：舊 `agentcmd help <cmd>` 仍可用；`agentcmd search …` 等因 registry 收錄而自然相容。
6. 無參數 → `agentcmd: missing argument`，exit 1（GNU 行為；舊版印 help 之行為改變，README 須註明）。

## 5. 每指令執行檔

- 每個 registry 項目（`[` 除外）必須有 `src/bin/<name>.rs` shim，**直接呼叫該指令模組之 run**（不得經 registry，否則 LTO 無法裁掉其他指令，單檔體積暴增）。
- `[`：Cargo 不允許此 bin 名。Unix 封裝以 symlink `[ -> test`；Windows 封裝複製 `test.exe` 為 `[.exe`。`test` shim 須檢查 argv[0] basename，為 `[` 時改呼 `run_bracket`。
- 新增 bin 後，以整合測試驗證「registry 每項皆有對應 `CARGO_BIN_EXE_*`」（以巨集或 build 期產生之清單比對）。
- 封裝體積：Release profile 加入（Phase 0）：
  ```toml
  [profile.release]
  codegen-units = 1
  lto = true
  opt-level = "s"
  panic = "abort"
  strip = "symbols"
  ```
  `panic = "abort"` 若與現有測試（`#[should_panic]` 等）或 BrokenPipe 處理衝突，可改 `"unwind"`，並於 PR 說明。

## 6. 依賴白名單

只可新增下列 crate（版本取當時 crates.io 最新穩定版，`Cargo.lock` 提交）。其他新依賴一律禁止；需要時於報告中提出，不得自行加入。

| crate | 用途 | 限制 |
|---|---|---|
| `sha1` `sha2` `md-5` `blake2` | `*sum`、`cksum -a` | RustCrypto；`default-features` 預設即可 |
| `jiff` | `date`／`touch -d`／`stat`／`ls --time-style` 之時區與格式化 | 啟用內建 tzdb（Windows 無系統 tzdb） |
| `filetime` | `touch -h`、`cp -p` 設定 symlink 時間 | — |
| `flate2` | `gzip`／`gunzip`／`zcat`／`tar -z` | 只用 `rust_backend`（miniz_oxide），不得引入 C 依賴 |
| `tar` | `tar` | — |
| `rand` | `shuf`、`sort -R`、`mktemp` | 可改用 `getrandom`＋自寫 PRNG |
| `libc` | Unix：`kill`、`timeout` 送訊號、`stat` 欄位、`uname` | `[target.'cfg(unix)'.dependencies]` |
| `windows-sys` | Windows：Job Object（`timeout` 殺行程樹）、`GetFileInformationByHandle`（inode/nlink）、`GetUserNameW` | `[target.'cfg(windows)'.dependencies]`，只開需要之 features |

既有依賴（`regex`、`ignore`、`sysinfo`、`serde_json`、`similar`、`crossterm`、`reqwest`、`grep-cli`）可續用。**禁止** `clap`（體積與既有手寫風格不符；以 `common::args` 取代）、`fancy-regex`、任何需 C 編譯器之 crate。

## 7. 參考 uutils 之方式

- 允許閱讀 uutils（coreutils／findutils／grep，MIT）以理解邊界行為；若逐段移植，於該檔頭加：
  `// Portions derived from uutils/coreutils (MIT), https://github.com/uutils/coreutils`
  並在 repo 根新增／更新 `NOTICE.md` 列出來源與授權全文。
- **禁止**閱讀或移植 GNU 原始碼（GPL）。行為以黑箱差分測試對齊。

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立 | — |
