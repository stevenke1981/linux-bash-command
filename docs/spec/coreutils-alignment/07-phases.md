# 07 分階段實作計畫

每階段一個（或數個依領域分開之）commit／PR；階段內可依指令拆 commit。每階段完成時必須：`cargo fmt --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked` 本機全綠，Linux 差分測試全綠，更新 README 與 `coverage.md`。

Commit 訊息採 conventional commits：`feat: add wc, sort, uniq`、`refactor: extract common::meta`、`test: add GNU differential cases for sort`。

---

## Phase 0：基礎建設（必須先完成，其他階段依賴）

交付：
1. `src/common/`：`args error io path meta size regex escape process`（`datetime` 留待 P2）。各附單元測試。
2. 05-existing-gaps §3 之共用抽取（行為不變）。
3. 05-existing-gaps §1、§2：既有指令 exit code 與訊息對齊；更新對應測試。
4. `src/registry.rs`＋`src/main.rs` multi-call（01-architecture §3–4）；registry 先只含既有 17 個。
5. `Cargo.toml` release profile（01-architecture §5）。
6. `tests/common/mod.rs`、`tests/gnu_diff.rs` 執行器（先附 `cat head tail grep find ls` 各 ≥ 5 案例以驗證執行器）。
7. CI 變更（06-testing §5）。

驗收：
- 既有測試（更新 exit code 後）全綠；抽取前後既有指令於差分案例輸出相同。
- `agentcmd --list` 輸出 17 行；`agentcmd cat README.md` 與 `cat README.md` 輸出相同；把 `agentcmd` 複製為 `<tmp>/head(.exe)` 執行可分派。
- `common::args` 單元測試涵蓋 02-conventions §1 表格每一列。

## Phase 1：文字管線（29 個）

`wc sort uniq cut tr tee echo printf true false test [ seq tac nl paste comm fold expand unexpand yes basename dirname xargs env printenv sleep egrep fgrep`，以及 05 §4 標 P1 之既有指令補齊（head/tail `-z`、cat `-u`、grep 補項）。

建議順序（依賴）：`true false echo yes sleep basename dirname` → `wc cut tr paste fold expand unexpand nl tac tee` → `printf seq test [` → `sort uniq comm` → `env printenv xargs` → `egrep fgrep` 與 grep 補項。

驗收：06-testing §3 數量；`sort` 外部合併可延至 Phase 3 結束前完成（於本階段標 `#[ignore]` 測試占位）。

## Phase 2：檔案系統與系統資訊（32 個）

`mkdir rm rmdir cp mv touch ln link unlink realpath readlink stat du mktemp date md5sum sha1sum sha224sum sha256sum sha384sum sha512sum b2sum cksum sum base64 base32 basenc truncate nproc hostname arch uptime`；新增 `common::datetime`；05 §4 標 P2 之 `ls`、`find`、`df` 補齊。

注意（安全，MUST）：`rm`、`mv`、`cp`、`chmod`、`touch` 之測試只可操作 Sandbox 暫存目錄。preserve-root 與 `.`/`..` 拒絕邏輯須抽成純函式（如 `fn refuse_reason(path) -> Option<&str>`），以**單元測試**驗證 `/`、`C:/`、`C:\`、`//server/share`；CLI 測試絕不可對 `/`、磁碟根或 Sandbox 以外之路徑執行 `rm`。

## Phase 3：其餘對齊指令（13 個）

`join od split csplit fmt pr shuf tsort numfmt factor expr pathchk ptx`；`sort` 外部合併。

驗收：microsoft/coreutils 之 utility 清單（`commands.md` B 段）全部可經 `agentcmd --list` 列出且 `--help` 正常。

## Phase 4：擴充 A（16 個）

`sed which tree uname whoami timeout kill pgrep pkill free cmp rev column dos2unix unix2dos chmod`；`diff` 預設 normal 格式與 `-c`/`-y`（05 §4）。

順序：`sed` 最大，先做（parse → exec → subst → `-i`）；其餘可並行。

## Phase 5：擴充 B（8 個）

`awk tar gzip gunzip zcat patch xxd file`。

順序：`gzip` → `tar`（依賴 gzip）；`awk` 獨立（lexer → parser → interp → builtins → I/O）；`patch` 獨立。

## Phase 6：封裝與文件

1. `packaging/`：Windows／Linux payload 由 `agentcmd --list` 產生；Windows 加入 `[.exe`（複製 `test.exe`），排除 `link.exe`。
2. README：指令章節改為分類表；新增「Windows 衝突」表（取自 `commands.md` 衝突欄，比照 microsoft/coreutils README）；新增「與 GNU 之已知差異」節（彙整下方表）。
3. `NOTICE.md`（若有移植 uutils 程式碼）。
4. 安裝包體積記錄於 `coverage.md`。

---

## 已知差異表（實作中持續更新）

| 指令 | 差異 | 原因 | 差分案例 |
|---|---|---|---|
| 全部 | BrokenPipe exit 0（GNU 被 SIGPIPE 終止） | 延續既有政策 | — |
| 全部 | 不展開 Windows glob | 見 README.md §2 | — |
| expr | i128 溢位報錯（GNU bignum） | 不引入 bignum | `expr_bignum`（skip） |
| printf seq sort -g | f64 精度（GNU long double） | Rust 無 f80 | 相關 skip 案例 |
| du（Windows） | 磁碟用量以 4096 取整 | 不呼叫 cluster API | — |
| sed | pattern 內 backreference 不支援 | `regex` crate 限制 | `sed_backref`（skip） |
| awk | 不支援 pipe／`system()` | 需 shell | — |
| diff | 預設 unified → P4 改 normal | 對齊 GNU | — |
| cksum | `-a crc32b` 為 spec 所列，GNU 9.x 不支援；本專案依 CRC-32（IEEE）實作，標籤 `CRC32B` | spec 指定 | `crc32b_not_in_gnu`（skip） |
| cksum | `-a sm3` 未實作（GNU 有，spec 未列） | 不在 spec 範圍 | `sm3_not_in_spec`（skip） |
| cksum | `-c` 僅支援雜湊演算法；`-a crc32b -c` 回報不支援 | 無對應檢查格式 | — |
| cksum sum | 無運算元（讀 stdin）時 legacy 格式（crc／bsd／sysv）不印檔名，同 GNU | 對齊 GNU | `default_crc_stdin`、`sum/bsd_stdin` |
| hostname | `-f` 為盡力：不做 DNS 反查，回傳主機名 | spec 允許 | `fqdn`（skip） |
| hostname | `-d -a -A -i -I -y` 不支援；帶運算元（設定主機名）不支援 | spec 規定 | `set_name_refused`（skip） |
| hostname | 測試機之 hostname 3.23（Debian）用法錯誤與 `--help` 為 exit 255；本專案依 coreutils（1／0） | 工具差異 | `unknown_option`、`help_exit`（skip） |
| free | `-v`（committed）、`-L` 不支援（GNU procps 有） | spec 列 `-v` 不支援；`-L` 未列 | `committed_unsupported`（skip） |
| free | shared／buff-cache 僅 Linux（`/proc/meminfo`），其他平台為 0；`-l` 之 Low／High 為近似 | spec 允許 | — |
| uname | Linux 上 `-p`、`-i` 取機器名（與 Ubuntu 之 GNU 相同）；其他 Unix 為 `unknown` | 對齊 GNU Ubuntu | — |
| uptime | users 欄僅 Linux（utmp）；load average 僅 Unix；Windows 兩者皆省略 | spec 允許省略 | — |
| uptime | `-h` 為說明（同 procps；spec 未列） | 對齊 procps | — |
| whoami uname free uptime hostname nproc arch | 系統值（主機相關）以 CLI 形狀測試，差分僅比確定性輸出 | 06-testing §1 | — |
| （Codex 補充） | | | |

## 回報格式（每階段完成時，Codex 輸出）

```
Phase N 完成報告
- commits: <hash> <subject> ...
- 新增指令: ...
- 測試: cargo test 通過 X / 失敗 0；GNU 差分 Y 案例（skip Z，皆列於已知差異表）
- clippy/fmt: 通過
- 覆蓋率: common XX%、commands XX%
- 未完成/延後: ...（附理由）
- 新依賴: ...（須在白名單內）
```

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立 | — |
