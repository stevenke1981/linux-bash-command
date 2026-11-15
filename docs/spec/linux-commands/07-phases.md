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
| ls grep rg diff df ps | 預設 `auto`：stdout 為終端機時自行上色並排版（ls 分欄、rg 分組標題）；GNU grep/diff/ls 預設不上色 | 2026-10-31 使用者要求每個指令自行上色與格式化；管線輸出不變；`AGENTCMD_COLOR`／`NO_COLOR` 可關閉 | `color_*` CLI 測試 |
| 全部 | 不展開 Windows glob | 見 README.md §2 | — |
| registry（P0） | 驗收保留 46 個繼承名稱，確認原有 17 個皆存在，不回退至 17 行 | 基準已提交 P1；保留既有指令優先，P0 報告記錄完整名稱數 | `registry::tests::registry_matches_existing_commands`、`tests/multicall.rs` |
| ls / find（Windows，P0） | 保留既有不同的合成權限：ls 0666/0777、find 0644/0755（唯讀 0444） | 05 §3 要求 P0 抽取行為不變，與 02 §7 的統一模式衝突；P2 統一 | `common::meta::tests::legacy_windows_file_modes_are_preserved` |
| false | 單一 `--help` / `--version` 印出資訊但回傳 1 | 03 的指令規格優先於 02 的一般資訊選項回傳 0；與 GNU 一致 | `false/help`、`false/version` |
| expr | i128 溢位報錯（GNU bignum） | 不引入 bignum | `expr_bignum`（skip） |
| printf seq sort -g | f64 精度（GNU long double） | Rust 無 f80 | 相關 skip 案例 |
| du（Windows） | 磁碟用量以 4096 取整 | 不呼叫 cluster API | — |
| sed | pattern 內 backreference 不支援 | `regex` crate 限制 | `sed_backref`（skip） |
| awk | 不支援 pipe／`system()` | 需 shell | — |
| diff | 預設 unified → P4 改 normal | 對齊 GNU | — |
| test / [ | `<`、`>` 字串比較依本規格支援；GNU test 9.4 回傳 2 | 規格優先；CLI golden 驗證 | `test/less`, `test/greater`, `[/less`, `[/greater`（skip） |
| seq | 小數位數取所有運算元最大值；GNU 不從 LAST 推導精度 | 03 明定規則優先；CLI golden 驗證 | `seq/last_precision_spec`（skip） |
| printenv | NAME 後的選項仍置換解析；GNU 9.4 遇 NAME 即停止 | 02 預設置換，例外未列 printenv | `printenv/permutation`（skip） |
| wc | `-m` 計 UTF-8 字元，無效位元組各 1；`-L` 計顯示欄位 | 03 明定規則優先；GNU C locale 的非 ASCII 計數不同 | `wc/utf8_spec`（skip） |
| printenv | 選項錯誤回傳 1；GNU 9.4 回傳 2 | 02 一般用法錯誤碼優先；CLI 驗證先報錯再處理 help | `printenv/unknown`, `printenv/unknown_before_help`, `printenv/invalid_value_before_help`（skip） |
| grep / egrep / fgrep | 無效 `-d ACTION` 回傳 2；GNU 9.4 回傳 1 | 02 grep 用法錯誤碼優先；CLI golden 驗證 | `grep/bad_directory`, `egrep/bad_directory`, `fgrep/bad_directory`（skip） |
| grep / egrep / fgrep | `-L` 依是否列出檔案決定狀態；GNU 依是否有選取行決定 | AGENTS 要求保留非 05 P1 範圍的既有行為；保留既有測試並加 alias golden | 三者的 `long_files_without_match`（skip） |

| ln | When a backup aliases the source, the new link retains the original source contents; GNU9.4 overwrites those contents before linking | Ambiguous source/backup alias cannot retain source data under GNU's post-backup reread; preserve both original payloads and report the difference | `ln/review_backup_source_collision` (skip), sandbox goldens |
| cksum | IEEE CRC32B is required by 03 but absent from Ubuntu GNU 9.4 | Specification wins; polynomial/check value have unit and CLI goldens | `cksum/crc32b` (skip) |
| hostname | Help is status 0 and usage errors status 1; Ubuntu hostname returns 255 | Explicit 02 contract wins | Three hostname GNU status cases (skip), CLI goldens |
| ls | Inherited `--colour` alias and `-` owner/group placeholders are preserved | AGENTS forbids unrelated legacy behavior changes; timestamp GNU cases compare size/time/name after four leading columns | `ls/p2_long_colour` (skip), legacy CLI tests |
| rm | A trailing-slash symlink is refused before traversal; GNU 9.4 can remove its target children before failing | 03 explicitly forbids following symlinks/junctions | `rm/review_no_symlink_traversal` (skip), sandbox target-preservation CLI regression |
| hostname | Hostname-setting operands are rejected with exit 1; GNU setter cases are not invoked | Explicit specification rejects system mutation; safe CLI goldens exercise all operand forms | `phase2_system` hostname cases |
| rm | Every filesystem root remains protected even with `--no-preserve-root` | The unconditional root-refusal rule takes precedence over the override option | Pure `fsops::rm` tests; no CLI root invocation |

| which | `--help` and `--version` exit 0; Ubuntu's which does not support them | 02 explicitly requires both information options | `which/help`, `which/version` (skip), CLI goldens |
| sed | An undefined branch label exits 1; GNU sed exits 4 | 04 explicitly specifies status 1 for script syntax errors | `sed/syntax_missing_label` (skip), CLI golden |
| rev | Valid UTF-8 is reversed by character even in the C locale; util-linux rev may stall on such input in that locale | 04 explicitly requires UTF-8 decoding independent of locale | `rev/unicode` (skip), Unicode and invalid-byte CLI goldens |
| dos2unix / unix2dos | Opening a missing file exits 1; Ubuntu dos2unix exits 2 | 02 explicitly specifies status 1 for ordinary file-open errors | Sandbox CLI missing-file goldens |
| dos2unix / unix2dos | The specified `--keep-date` and `--convmode=ascii` forms are accepted; Ubuntu dos2unix uses `--keepdate` and requires a separate conversion-mode argument | 04 specifies `--keep-date` and 02 requires `--name=VALUE` | Both `long_keep-date` GNU cases (skip), `-k` and separate `--convmode` GNU cases, sandbox CLI goldens |

| cp / mv | `--update=none-fail` is implemented although Ubuntu 24.04 coreutils 9.4 does not offer it | Explicit specification wins | `phase2_fs` absent/existing destination and flag-order goldens |
| date / touch | Calendar representation is limited to years -9999 through 9999 | Allowlisted jiff representation | `common::datetime` range checks |
| nproc | `--all` ignores both OpenMP variables; otherwise OMP_NUM_THREADS supplies the count and OMP_THREAD_LIMIT caps it | Resolve the parenthetical ambiguity by GNU behavior | GNU cases and `sysinfo_cmds` units |
| stat -f (Windows) | Linux filesystem magic and inode capacity are unavailable, shown as `?` / `-`; native type, serial and blocks are reported | Native Windows APIs cannot express Linux-only fields | Windows CLI metadata tests |

| od fL | Long double is represented and formatted as f64 | 03 explicitly requires this portable representation | od numeric CLI goldens |

| expr | Runtime errors use status 3; Ubuntu GNU 9.4 uses 2 for zero division and noninteger arithmetic | Explicit 02/03 status contract | `expr/division_error_spec`, `expr/integer_error_spec` (skip), phase3_root CLI goldens |
| tsort | FIFO Kahn queue preserves first-seen ordering among available nodes | Explicit 03 algorithm/order contract | `tsort/first_seen_kahn` (skip), phase3_small FIFO CLI golden |
| csplit | Refuses output aliases before truncating input; GNU 9.4 can remove its input before failing | 02 failed in-place rewrite preservation takes precedence | `csplit/input_output_alias` (skip), phase3_numeric preservation golden |

| tree | Default branches are UTF-8 even under C locale; attached short-option values follow the shared CLI parser | Explicit 04 branch glyphs and 02 argument contract; GNU tree 2.1 defaults to ASCII in C locale and requires separate short-option values | Native cases normalize only reference NBSP indentation and use an explicit GNU `--charset=utf-8` prefix and separate arguments; `phase4_root` checks the required default and attached forms |

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

## Phase 1 整合備註

以下為先前整合狀態；2026-11-15 的重新驗證記錄見 [Phase 0 report](../../implementation/linux-commands/phase-0.md)。

開始時 HEAD 的 Phase 0 僅有部分共用模組與 registry；工作樹補入 P1 必需的 args/error/io/escape/process、GNU 測試器及 CI 設定，並繼續調整既有指令的狀態碼、訊息與部分參數解析。Phase 0 仍有未完成的對齊項目；Linux GNU 差分與覆蓋率尚待驗證，Phase 0 和 Phase 1 均不可視為已驗收。
