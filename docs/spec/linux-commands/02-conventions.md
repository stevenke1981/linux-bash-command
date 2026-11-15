# 02 全域 CLI 行為契約

所有指令（新增與既有）必須遵守本章；個別指令規格另有明文者從之。

## 1. 參數解析（`common::args`）

實作 GNU `getopt_long` 語意：

| 項目 | 規則 |
|---|---|
| 短選項 | `-a -b` 可合併為 `-ab`；需值者可 `-n5`、`-n 5`；合併中遇需值選項，其後餘字即值（`-vn5`） |
| 長選項 | `--name`、`--name=VAL`、`--name VAL`（Required 時）；Optional 值只接受 `=` 形式 |
| 長選項縮寫 | 唯一前綴即可（`--verb` = `--verbose`）；不唯一 → `cmd: option '--x' is ambiguous; possibilities: '--xa' '--xb'` |
| `--` | 終止選項解析 |
| `-` | 視為運算元（stdin） |
| 置換（permute） | 預設允許選項出現在運算元之後；環境變數 `POSIXLY_CORRECT` 存在時停在第一個運算元 |
| 不置換之指令 | `echo printf test [ expr env xargs timeout nice kill`（見各指令）：首個非選項即停止，其後全為運算元／子指令參數 |
| 數字選項 | `head -5`、`tail -5`、`fold -80`、`uniq -2`、`pr -3` 等舊式寫法由各指令於解析前預處理 |

API（建議，Codex 可微調但須單元測試涵蓋上表全部列）：

```rust
pub enum HasArg { No, Required, Optional }
pub struct OptSpec { pub id: &'static str, pub short: Option<char>, pub long: Option<&'static str>, pub has_arg: HasArg }
pub enum Item { Opt { id: &'static str, value: Option<String> }, Operand(String) }
pub struct Parser<'a> { /* cmd 名、specs、permute 旗標 */ }
impl Parser<'_> { pub fn parse(&self, args: &[String]) -> Result<Vec<Item>, UsageError>; }
```

錯誤訊息（GNU 原文）：
- `cmd: invalid option -- 'x'`
- `cmd: unrecognized option '--xyz'`
- `cmd: option requires an argument -- 'n'`
- `cmd: option '--lines' requires an argument`
- `cmd: option '--quiet' doesn't allow an argument`
- 次行固定 `Try 'cmd --help' for more information.`

## 2. `--help` 與 `--version`

- 每一指令必須支援 `--help`（印至 stdout，exit 0）與 `--version`（印 `NAME (agentcmd) X.Y.Z`，exit 0）。
- 例外依 GNU：`echo`、`true`、`false`、`[`、`printf` 只在 `--help`／`--version` 為**唯一參數**時才處理；`test` 永不處理（`test --help` 為非空字串測試，exit 0）。`yes --help` 正常處理。
- `-h` 不得作為 help 別名之指令：`ls df du sort numfmt free tree` 等（`-h` 為 human-readable）。其餘指令 `-h` 依 GNU 原義或為未知選項。
- HELP 文字格式比照 GNU：`Usage: …` 一行、說明、選項表、Exit status 段（若非僅 0/1）。須列出**實作之全部選項**；未實作之 GNU 選項不得列出。

## 3. Exit code

依 GNU 各指令。通用規則：

| 情況 | exit |
|---|---|
| 成功 | 0 |
| 一般錯誤（檔案無法開啟、部分失敗） | 1 |
| 用法錯誤（未知選項、缺參數） | 1（coreutils 慣例），下列例外 |
| 例外：`ls` 嚴重錯誤（用法錯誤、命令列運算元無法存取） | 2 |
| 例外：`grep egrep fgrep rg search diff cmp sort（用法錯誤） test [ expr` | 2（`expr` 語法錯誤 2、運算錯誤 3） |
| 例外：`env nice timeout xargs` 之子行程 | 依各規格（125/126/127 …） |
| 以 `-f`／`--force` 等忽略錯誤者 | 依 GNU |
| 「不支援之 GNU 選項」（存在於 GNU 但本專案不實作） | 同用法錯誤，訊息：`cmd: option '--x' is not supported by agentcmd` |

多運算元時，任一失敗則最終 exit 為該錯誤碼，但須處理完其餘運算元（GNU 行為）。

## 4. 錯誤訊息

- 一律 stderr，格式 `cmd: <operand>: <reason>`；operand 依 GNU 以單引號包裹之情形照 GNU（例 `cat: missing: No such file or directory`、`rm: cannot remove 'x': No such file or directory`）。
- 檔名引號規則（GNU `quotearg` shell-escape 風格）：只含安全字元不加引號；含空白或特殊字元以 `'...'` 包；含單引號以 `"..."` 包。實作於 `common::escape::quote_name`。
- OS 錯誤文字須映射為 GNU（strerror）用語，不得輸出 Rust 之 `(os error 2)`：
  `NotFound → No such file or directory`、`PermissionDenied → Permission denied`、`AlreadyExists → File exists`、`IsADirectory → Is a directory`、`NotADirectory → Not a directory`、`DirectoryNotEmpty → Directory not empty`、`CrossesDevices → Invalid cross-device link`、`ReadOnlyFilesystem → Read-only file system`、`InvalidInput(EINVAL) → Invalid argument`、其他 → 去除 ` (os error N)` 之原文。Windows 錯誤碼亦映射至上述同義字（`ERROR_FILE_NOT_FOUND`、`ERROR_PATH_NOT_FOUND` → No such file or directory；`ERROR_ACCESS_DENIED`、`ERROR_SHARING_VIOLATION` → Permission denied 等）。
- 錯誤訊息比對只納入 CLI 測試，不納入 GNU 差分測試（差分只比 stdout 與 exit code）。

## 5. I/O

- **位元組導向**：檔案內容一律以位元組處理，不得要求 UTF-8（例外：`wc -m`、`rev`、`fold`（無 `-b` 時仍依 GNU 為位元組計，見規格）、`awk length()` 依 UTF-8 解碼，無效序列以單一位元組計）。
- **行**：以 `\n` 分隔，`\r` 為一般資料（GNU 於 Linux 之行為）。`-z`／`--zero-terminated` 改以 `\0`。最後一行缺換行之處理依 GNU（多數指令輸出時補上，`cat`/`tac`/`head`/`tail` 等保留原樣）。
- **stdin**：無檔案運算元或運算元為 `-` 時讀 stdin。
- **緩衝**：stdout 以 `BufWriter` 包裝（≥ 64 KiB），程序結束前 flush；flush 失敗視為寫入錯誤（exit 1，訊息 `cmd: write error: <reason>`）。互動式指令（`tee -i` 無關）、`tail -f`、`timeout`、`xargs -t` 輸出須逐行 flush。
- **BrokenPipe**：寫入遇 `BrokenPipe` 立即靜默結束，exit 0（延續現有政策；與 GNU 被 SIGPIPE 終止不同，於 README 註明）。
- **大檔**：串流處理，不得整檔讀入記憶體；必須整檔處理者（`sort`、`tac` 非 seekable、`shuf`）明列於該指令規格。`sort` 超過記憶體門檻（預設 256 MiB 或 `-S`）時須外部合併至暫存檔（Phase 2 可延後，見 07-phases）。
- **原地改寫**（`sed -i`、`dos2unix`、`gzip`、`truncate` 除外）：先寫同目錄暫存檔，成功後 rename 覆蓋；失敗不得損壞原檔；Unix 保留原檔權限位。

## 6. 路徑

- 輸出路徑一律經 `common::path::display_path`（Windows 將 `\` 轉 `/`，Unix 不變）。運算元原樣回顯時（如 `wc` 印檔名）亦同。
- `canonicalize` 結果須移除 Windows `\\?\` 前綴（UNC 轉為 `//server/share`）。
- 輸入同時接受 `/` 與 `\`（Windows）。
- 非 UTF-8 檔名：Unix 以 lossy 顯示，但檔案操作須用原始 `OsStr`（例 `find`、`rm -r` 遍歷不得因非 UTF-8 名而跳過或失敗）。

## 7. Windows 規則

| 主題 | 規則 |
|---|---|
| 權限位 | 依 `common::meta` 合成：目錄 `0755`、檔案 `0644`；唯讀屬性去除全部 `w`；副檔名 `.exe .com .bat .cmd .ps1` 加 `x`（`0755`）。須與既有 `ls -l` 一致（Phase 0 抽出共用並以測試鎖定） |
| owner／group | uid／gid 顯示 `0`；名稱顯示 `-`（延續 `ls -l` 既有行為） |
| inode／nlink／dev | 以 `GetFileInformationByHandle` 取 file index／number of links／volume serial；取不到時 inode 0、nlink 1 |
| symlink 建立 | 失敗且為權限錯誤時，訊息附加：` (creating symbolic links on Windows requires Developer Mode or an elevated shell)` |
| 刪除唯讀檔 | `rm`（GNU 於 Unix 可刪唯讀檔）：Windows 須先清除 readonly 屬性再刪；`rm` 無 `-f` 時仍依 GNU 於 stdin 為 tty 時詢問 `rm: remove write-protected regular file 'x'?` |
| 行程訊號 | 無 POSIX 訊號。`kill`／`timeout`／`pkill`：`TERM INT HUP KILL` 等一律 `TerminateProcess`（exit code 1）；`-0` 僅檢查存在；`-l` 照列 Linux 訊號表 |
| 外部指令 | `xargs env timeout find -exec` 解析指令時須套用 `PATHEXT`；不搜尋目前目錄（與 POSIX 一致，與 cmd.exe 不同） |
| 主控台 | stdout 為主控台時沿用 Rust std（WriteConsoleW）；為管線時輸出原始 UTF-8 位元組，不做 code page 轉換 |
| `/dev/null` | 運算元 `/dev/null` 於 Windows 視同 `NUL`（`common::io` 開檔時轉換）；`/dev/stdin` `/dev/stdout` `/dev/stderr` 同理對應標準串流 |

## 8. 環境變數

- `POSIXLY_CORRECT`：見 §1。
- `TZ`：`date ls stat touch` 等時間顯示；未設時用系統時區（Windows 由 jiff 取系統時區）。
- `TMPDIR`（Unix）／`TEMP`、`TMP`（Windows）：`mktemp sort split` 暫存。
- `COLUMNS`：`ls` 多欄寬度（既有）、`column`、`tree` 不使用。
- 不讀 `LANG`／`LC_*`（一律 C locale 行為）。

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立 | — |
