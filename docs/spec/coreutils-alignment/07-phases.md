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
| sed | 替換 `e` 指令與 `s///e` 旗標不支援（exit 1；GNU 會執行指令） | spec 規定，不引入子行程執行 | `cmd_e_unsupported`、`cmd_e_flag_unsupported`（skip） |
| sed | 選擇（alternation）為 leftmost-first，非 POSIX leftmost-longest（`s/a\|ab/X/` 於 `ab` 結果不同） | `regex` crate 語意 | `s_alternation_longest`（skip） |
| sed | `--debug` 不支援（exit 1，不列於 `--help`） | spec 規定 | `opt_debug_unsupported`（skip） |
| sed | `--help`／`--version` 文字與 GNU 不同 | 本專案自有版本字串與說明 | `opt_help`、`opt_version`（skip） |
| sed | `M` 旗標下 `[^…]` 仍可匹配換行（REG_NEWLINE 不完整） | regex 模式限制 | — |
| sed | 比對以位元組進行（C locale）；`I` 旗標僅折疊 ASCII | 對齊 `LC_ALL=C` | — |
| sed | 缺少 `-f` 腳本檔為 exit 4（同 GNU）；`-i` 無檔案 exit 4（同 GNU） | 對齊 GNU | — |
| awk | 不支援 pipe／`system()` | 需 shell | — |
| diff | 預設格式已於 P4 由 unified 改為 normal（破壞性變更，README 已載明） | 對齊 GNU | — |
| grep | `-L` 退出碼依列出之檔案（GNU 3.11 依是否選取到行） | 既有行為，P1 不改 | `grep/null_data_list_without_match`（skip） |
| grep | 無效 `-d ACTION` 退出 2（GNU 3.11 為 1） | 規格：grep 用法錯誤為 2 | `grep/directories_unknown_action`（skip） |
| grep | `-T` 於無法取得大小之輸入（管線）欄寬固定 18（GNU 3.11 觀察值） | GNU 規則未明載，依觀察實作 | 僅 `-T` 於一般檔案比對（`initial_tab_*`） |
| grep | `-A0`/`-B0`/`-C0` 不輸出 `--` 分隔線（GNU 會輸出） | 既有行為，P1 不改 | — |
| grep | 二進位命中訊息寫 stdout（GNU 3.11 寫 stderr） | 既有行為，P1 不改 | `grep/binary_file_default`（skip） |
| grep | 無 `--version` | 既有行為，P1 不改 | — |
| cat | 接受 `--unbuffered` 長選項（GNU 無此選項） | 既有擴充，P1 不改 | `cat/unbuffered_long_option`（skip） |
| tail | 接受位於其他選項之後的 `-N`（GNU 拒絕） | 既有行為，P1 不改 | `tail/zero_dash_number`（skip） |
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
| cp | `--reflink=always` 與 `--attributes-only` 不支援（用法錯誤，exit 1） | 規格 03 指定不支援；無 clone 支援 | `reflink_always_unsupported`、`attributes_only_unsupported`（skip） |
| mkdir | `-Z`／`--context` 不支援（GNU 於無 SELinux 時靜默成功） | 規格 03；SELinux 不在範圍 | `context_option_not_supported`（skip） |
| ln | `-d`／`--directory`、`-F` 不支援 | 規格 03 ln | `directory_option_unsupported`、`capital_f_option_unsupported`（skip） |
| rm cp mv（Windows） | `--one-file-system` 無作用（Windows 無 device 號）；`--preserve=ownership` 為 no-op | 平台無 uid/gid 與 dev 號 | — |
| chmod（Windows） | 只有 owner 寫入位有效（清除即設 read-only）；其餘位元接受但忽略 | 規格 04 chmod | — |
| mkdir（Windows） | `-m` 只驗證語法，不套用（目錄唯讀語意不同） | 規格 03 mkdir | — |
| cp（Windows） | symlink 建立需 Developer Mode 或提升權限，失敗訊息附註說明 | 02-conventions §7 | — |
| truncate | `-o` 之區塊大小取檔案 `st_blksize`（Unix），Windows 固定 4096 | 不呼叫 Windows 區塊 API | — |
| cp | 不輸出 GNU 9.4 `cp -n` 之 non-portable 警告（僅 stderr） | 警告為 GNU 過渡訊息；stderr 不比對 | — |
| od | `-t fL`／`f16`（long double）以 f64 解碼後輸出 | Rust 無 f80 | — |
| od | `-w0` GNU 中止（exit 134）；本專案報 `invalid width`，exit 1 | GNU 缺陷不複製 | — |
| od | `-S` 的字串只在 NUL 結尾時輸出（依 GNU 黑箱行為實作） | 規格未明 | — |
| split | `--filter` 不支援（exit 1，規格允許） | 需 shell | `split/c36_filter_cat_ten`（skip） |
| split | `-n N`／`-n l/N` 需要總大小：stdin 先複製到暫存檔（TMPDIR） | 無法在不知總長時分割 | — |
| split | 未指定 `-a` 時，第 651 個以後名稱（`zaaa` 起）依黑箱觀察實作；超過 `-a N` 的範圍報 `output file suffixes exhausted` | 黑箱推導 | — |
| csplit | 整份輸入先讀入記憶體（向後搜尋與負偏移需要） | 設計 | — |
| fmt | 最佳斷行成本函數為黑箱推導（目標寬 `round(0.93·w)`、末行單字加罰、行寬 < w）；與 GNU 在測試輸入一致，其他輸入可能有差異 | 未讀 GNU 原始碼 | `fmt` 差分 39 案例（全部一致） |
| pr | `-c`（控制字元記號）、`-i`（輸出 tab）與多欄、`-n` 併用之對齊未逐一黑箱比對 | 規格未細述 | — |
| pr | `-F`（換頁字元）以黑箱觀察之頁尾格式實作 | 規格未細述 | — |
| pr | `-D` 以 jiff 的 strftime 格式化，特殊格式可能與 GNU 不同 | 時區與格式由 jiff 提供 | — |
| join | `-t ''` 視為換行分隔（黑箱觀察） | 規格未明 | — |
| find | `-ls` owner/group 顯示 `-`、日期格式固定 | 無 uid→name 解析 | `find/ls_action_skipped_owner_columns`（skip） |
| ls | `-l` owner/group 顯示 `-`，時間欄格式不同 | 無 uid→name 解析 | `ls/long_format_owner_and_date`（skip） |
| find | `-exec {} +` 批次順序：同目錄內排序，GNU 依 readdir 順序 | 決定性輸出 | — |
| printf | `%a` 與部分 `%.Nf` 末位以 f64 計算 | Rust 無 long double | `printf/hex_float_long_double`、`printf/long_double_rounding_case`（skip） |
| test [ | `<`、`>` 依規格為二元運算子（GNU 9.4 拒絕） | 規格明文 | `test/string_less_than` 等（skip） |
| test | `( ( a ) )` 依文法求值（GNU 報錯） | 文法一致 | `test/nested_parentheses`（skip） |
| [ | `--help`／`--version` 文字與 GNU 不同 | 本專案自訂文字 | `[/help_is_only_argument` 等（skip） |
| test | 整數錯誤訊息 `invalid integer`（GNU `integer expression expected`） | 規格明文 | — |
| yes | 無限輸出不納入差分 | 差分器無管線 | `yes/*`（skip，CLI 測試涵蓋） |
| sleep | 不接受十六進位浮點數 | 不實作 strtod hex | `sleep/hex_operand`（skip） |
| env | `--block-signal` 等訊號選項不支援 | Windows 無訊號 | — |
| sort | `--debug` 不支援；`-R` 次序與 GNU 不同；`-T` 只用第一個目錄；`--parallel` 接受但單執行緒；`-S` 不接受 `%` | 範圍取捨 | `sort/debug_unsupported`（skip） |
| xargs | `-p`、`-o` 不支援；`--show-limits` 數值不同（預設 `-s` 固定 128 KiB）；`-P` 接受但序列執行 | 互動／平台相依 | `xargs/unsupported_*`、`xargs/show_limits_values_differ`（skip） |
| wc | `-m`、`-L` 依規格以 UTF-8 字元計（GNU C locale 以位元組計） | 規格明文（02 §5） | `wc/chars_utf8_skip`、`wc/max_line_wide_utf8_skip`（skip） |
| tac | 整檔讀入記憶體（無 seekable 反向分塊讀） | 簡化 | — |
| tac | `-r` regex 由左至右匹配 | regex crate | — |
| date | 接受 `noon`／`midnight`（GNU 9.4 拒絕）；拒絕軍用時區字母 `x`（GNU 接受） | 規格明文 | `date/noon_keyword`、`date/military_zone_letter`（skip） |
| date | `-s`、`--debug` 不支援；單一非 `+` 運算元回 `cannot set date` | 不改系統時間 | — |
| du | `--inodes` 不支援 | 範圍取捨 | `du/inodes_option_unsupported`（skip） |
| stat | `-f` 僅 Unix；`%T` 只認 10 種檔案系統；`%m` 以裝置邊界推得；`%C` 恆為 `?` | 無 SELinux／mount 表解析 | — |
| realpath readlink | `-L` 先以字面方式消去 `..` 再解析 symlink | 近似 GNU | — |
| shuf | `--random-source` 可重現但序列與 GNU 不同 | 自有 PRNG | 差分以 `stdout_sorted` |
| numfmt | 以 f64 計算；超大數不報 `value too large` | 不引入 bignum | `numfmt/huge_plain_number_is_gnu_error`（skip） |
| factor | 僅 u128；更大者報 `is too large` | 不引入 bignum | `factor/beyond_u128_is_skipped`（skip） |
| expr | 除以零、非整數引數 exit 3（GNU 2） | 規格：運算錯誤 3 | `expr/division_by_zero_is_skipped` 等（skip） |
| pathchk | 預設上限固定 4096／255（不呼叫 pathconf） | 跨平台 | — |
| ptx | 過長行之 wrap 欄（`/` 標記）、`-R` 版面、極小 `-w` 未重現 | 範圍取捨 | `ptx/*_is_skipped`（skip） |
| which | 用法錯誤寫 stderr、`--help` exit 0、接受 `--`（debianutils 皆不同） | 02 §1、§2、§4 | `which/invalid_option` 等（skip） |
| tree | 預設 box-drawing 連接線（GNU C locale 為 ASCII）；續行用一般空白（GNU 用 U+00A0）；規格外選項拒絕 | 規格明文 | `tree/default_charset_is_utf8`、`tree/utf8_nested_nbsp_continuation`（skip） |
| timeout | 接受 `-p`（GNU 9.4 無）；不轉送收到之訊號給子行程 | 規格明文／簡化 | `timeout/short_p_is_preserve_status`（skip） |
| kill | `-l` 未知值 exit 1（procps 0） | 規格：失敗 exit 1 | `kill/list_*_rejected`（skip） |
| pgrep pkill | 規格外選項拒絕；`pkill -e` 輸出格式自訂 | 範圍取捨 | — |
| cmp | 預設訊息 `differ: byte N, line M`（GNU 3.10 為 `char N`） | 規格明文 | `cmp/default_differ`（skip） |
| rev | `-` 為 stdin（util-linux 視為檔名）；無效 UTF-8 逐位元組反轉 | 02 §5／規格 | `rev/dash_is_a_file_name`（skip） |
| column | 規格外選項不支援；寬字元寬度以 CJK 範圍表近似 | 範圍取捨 | — |
| dos2unix unix2dos | `-` 為 stdin；`-c` 只支援 ascii；二進位 stdin 不輸出；用法錯誤寫 stderr；UTF-16 視為二進位；unix2dos 對 CR CR LF 處理不同 | 02 §4–§5／規格 | `dos2unix/*`、`unix2dos/*`（skip） |
| diff | `-u` 標頭不含時間；`-y -B` 不列隱藏空行；`--context N`（空白分隔）不再接受 | 05 §4／GNU | `diff/unified_without_labels`、`diff/side_blank_lines_with_B`（skip） |
| awk | `printf` 引數不足時補空字串／0（gawk --posix 致命錯誤） | 寬鬆處理 | `awk/printf_missing_args`（skip） |
| awk | `cmd \| getline`、`print \| cmd`、`system()` 不支援（exit 2） | 規格明文 | `awk/*_unsupported`（skip） |
| awk | 接受 `length(array)`、多字元 regex `RS`、`**`／`**=`（gawk --posix 拒絕）；語法錯誤 exit 2（gawk 1） | 規格明文 | `awk/length_of_array` 等（skip） |
| awk | 字串轉數不認十六進位（gawk 經 strtod 認） | POSIX 十進位轉換 | `awk/hex_string_is_zero`（skip） |
| awk | regex 交替為 leftmost-first（POSIX 為 leftmost-longest）；`length` 等以 UTF-8 字元計；初始 `srand()` 回傳 0 | regex crate／規格 | — |
| gzip | 壓縮位元組與 GNU 不同（flate2）；不支援 `.Z`／`.z` | 不同 deflate 實作 | 差分只比解壓結果 |
| tar | 無模式／未知選項 exit 1（GNU 2／64）；`-j -J --zstd --lzma`、`-r -u --delete` 不支援；建立之封存非逐位元組相同；硬連結存為複本；裝置／FIFO 不封存 | 規格／`tar` crate | `tar/no_mode_is_usage_error` 等（skip） |
| ls | `-l` 預設時間欄為 `YYYY-MM-DD HH:MM`（GNU C locale 為 `Nov 14  2023`）；`-Z`、`-D` 不支援；`--sort=width` 近似 | 既有行為／範圍 | `ls/long_format_owner_and_date`（skip） |
| find | 輸出依名稱排序；`-inum`、`-samefile` 僅 Unix | 決定性輸出 | — |
| df | `-i` 不支援；`--output` 無清單時不含 inode 與 file 欄 | 跨平台 | — |
| patch | context diff 不支援；CRLF 目標正規化後套用（GNU 失敗）；拒絕絕對路徑與 `..` 目標；互動問題一律答 n；全部解析完才套用 | 規格明文／安全 | `patch/*`（skip） |
| xxd | `-E`、`-R` 不支援；`-r` 向後移動須可 seek 之輸出 | 範圍取捨 | — |
| file | libmagic 子集，自訂措辭；只檢查前 1 MiB | 規格 | golden 測試 |

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
