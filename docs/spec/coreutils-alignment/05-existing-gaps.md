# 05 既有指令之對齊修正

既有 17 指令須於 **Phase 0** 完成「Exit code／訊息」對齊，選項補齊依下表所列階段。現有測試中約 55 處斷言 exit code 2，須逐一依本章更新（不得為了通過而改回舊值）。

## 1. Exit code 對齊（Phase 0）

| 指令 | 現況 | 目標（GNU） |
|---|---|---|
| cat head tail | 錯誤 2（`text_commands::status`） | 1；用法錯誤 1 |
| ls | 錯誤 2 | 運算元無法存取／用法錯誤 2；子目錄讀取失敗等次要錯誤 1 |
| pwd | 2 | 1 |
| find | 2 | 1（含用法錯誤） |
| df | 2 | 1 |
| ps | 2 | 1（procps：用法錯誤 1） |
| less | 2 | 1 |
| cd（獨立執行檔） | 2 | 1 |
| grep rg search diff | 2 | 維持 2 |
| curl | 2 | 依 curl：用法錯誤 2、無法解析主機 6、無法連線 7、逾時 28、`-f` HTTP 錯誤 22、寫檔失敗 23；Phase 0 只要求 2／22 維持並補 6、7、28、23 |
| list read | 2 | 維持 2（本專案專屬指令，不在 GNU 範圍） |

並同步更新 README「Most other commands return 0 for success and 2 for errors」段落。

## 2. 訊息格式（Phase 0）

- `Try 'X --help' for usage.` → `Try 'X --help' for more information.`（全部指令）。
- OS 錯誤文字經 `common::error` 映射（02-conventions §4），移除 `(os error N)`。
- 既有指令改用 `common::args` **不強制**（避免大範圍重寫）；但下列 GNU 行為必須達成：長選項唯一前綴縮寫（`--line-n` → `--line-number`）、`POSIXLY_CORRECT` 停止置換。可在既有解析器前加一層「長選項縮寫展開」共用函式。

## 3. 共用抽取（Phase 0）

| 重複實作 | 抽至 | 現位置 |
|---|---|---|
| `mode_string`、`permission_bits`、`kind_char` | `common::meta` | `fs_commands.rs:747`、`find_command/printf.rs:219`、`find_command/expr.rs:192,229` |
| `human_size` / `human` | `common::size` | `fs_commands.rs:666`、`df_command.rs:416` |
| `parse_size` | `common::size` | `search.rs:525`、`find_command/parse.rs:435`、`text_commands.rs:412` |
| `bre_to_regex` | `common::regex` | `grep_command.rs:465` |
| `is_closed_pipe`、`report_open`、`report_failure`、`io_text` | `common::io`／`common::error` | `text_commands.rs:939-959`、`fs_commands.rs:13` |
| `display_path` | `common::path`（`lib.rs` 保留 re-export） | `lib.rs:15` |

抽取須為**行為不變重構**：抽取前後全部既有測試輸出逐位元組相同（以 commit 前後跑 `cargo test` 與 Linux GNU 差分驗收）。

## 4. 選項補齊

| 指令 | 補齊項目 | 階段 |
|---|---|---|
| ls | `-i`、`-c`、`-u`、`--time=WORD`、`-X`、`--sort=WORD`、`-I --ignore=PAT`、`-B`、`--hide=PAT`、`--full-time`、`-Q --quote-name`、`-N --literal`、`--quoting-style=literal|shell|shell-always|shell-escape|c|escape`（預設依 GNU：stdout 非 tty 時 literal）、`-m`、`-C`、`-L`、`-H`、`-w COLS`、`-T`（接受）、`-s --size`、`-k`、`--block-size`、`--si`、`--indicator-style`、`--file-type`、`-D`（不支援） | P2 |
| head tail | `-z --zero-terminated` | P1 |
| cat | `-u`（接受，忽略） | P1 |
| grep | `-z --null-data`、`-Z --null`、`-b --byte-offset`、`-T --initial-tab`、`--label=LABEL`、`--line-buffered`、`-U --binary`（接受）、`--no-ignore-case`、`-y`（同 -i）、`--exclude-from=FILE`、`-D ACTION`、`-d ACTION` | P1 |
| find | 現已支援 `-a/c/m time`、`-a/c/m min`、`-anewer -cnewer`、`-perm`、`-iregex`、`-readable -writable -executable`、`-exec {} +`（見 `find_command/parse.rs:287`）。須補：`-newerXY`、`-user -group -nouser -nogroup -uid -gid`（Windows 以 uid/gid 0、名稱 `-` 判斷）、`-links N`、`-inum N`、`-samefile F`、`-lname -ilname`、`-regextype posix-basic|posix-extended|emacs`（emacs 以 ERE 處理）、`-true -false`、`-xtype`、`-xdev -mount`、`-ignore_readdir_race -noleaf`（接受）、`-ok -okdir`（目前拒絕 → 改為互動式，讀 stdin `y`/`Y`）、`-fprint -fprint0 -fprintf -fls` | P2 |
| df | 數值不可差分；補 `--output[=FIELD_LIST]`、`-i`（不支援錯誤）、`--total` | P2 |
| pwd | 已足 | — |
| diff | `-c`/`-C N`（context 格式）、`-y`/`-W`（side-by-side）、`--normal`、預設格式改為 normal（見下） | P4 |

**diff 預設格式**：GNU diff 無選項時為 normal 格式（`1c1`／`< a`／`---`／`> b`），現行實作預設 unified。為對齊 GNU，P4 起無格式選項時輸出 normal 格式；`-u` 不變。此為破壞性變更，README 須明載；`search`/`list`/`read` 不受影響。

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立 | — |
