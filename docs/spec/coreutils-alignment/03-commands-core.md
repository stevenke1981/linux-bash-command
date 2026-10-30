# 03 對齊 microsoft/coreutils 之指令規格

通則：
- 「選項」欄列出**必須實作**之全部選項（短/長形式皆須支援，長選項依 GNU 名稱）。未列之 GNU 選項 → 「不支援」錯誤（02-conventions §3）。
- 「要點」只列易錯或本專案特有之規則；其餘依 GNU。
- 每指令之測試最低要求見 [06-testing.md](06-testing.md) §3。

---

## P1：文字管線

### wc
- 選項：`-c --bytes`、`-m --chars`、`-l --lines`、`-w --words`、`-L --max-line-length`、`--files0-from=F`、`--total=auto|always|only|never`。
- 要點：
  - 輸出順序固定 lines, words, chars, bytes, max-line-length。無選項等同 `-lwc`。
  - 欄寬依 GNU：只有單一計數且單一輸入時不補空白；否則欄寬 = 所有「可 stat 之一般檔案」大小總和之位數（至少 1）；任一輸入為 stdin／非一般檔時最小寬度 7。
  - 多於一個輸入時印 `total` 行（依 `--total`）。
  - `-w`：以空白（C locale `isspace`）分隔。`-m`：UTF-8 字元數，無效位元組各計 1。`-L`：tab 進位至 8 之倍數，以顯示寬度計（C locale 下即字元數）。
  - 讀檔用 64 KiB 區塊，`-c` 單獨使用且為一般檔時可用 metadata 長度。

### sort
- 選項：`-b -d -f -g -i -M -h -n -R -r -V`、`-k KEYDEF`（可重複）、`-t SEP`、`-u`、`-s`、`-c`、`-C`、`--check[=diagnose-first|quiet|silent]`、`-m`、`-o FILE`、`-z`、`-S SIZE`、`-T DIR`、`--parallel=N`（接受，可忽略）、`--random-source=FILE`、`--sort=general-numeric|human-numeric|month|numeric|random|version`、`--files0-from=F`、`--batch-size=N`（接受）。
- 要點：
  - 比較為位元組序（C locale）。KEYDEF 語法 `F[.C][OPTS][,F[.C][OPTS]]` 完整支援，含每鍵修飾字 `bdfgiMhnRrV`；全域修飾字只在鍵未自帶修飾字時套用（GNU 規則）。
  - 無 `-s` 時，鍵全等之行以整行位元組比較作為最後決勝（`-r` 亦反轉之）。
  - `-n`：前導空白、可選 `-`、數字、`.` 小數；不接受 `+`、千分位；非數字視為 0。`-g`：以 `f64` 解析（`inf`、`nan`、指數），`nan` 排最前。`-h`：`K M G T P E Z Y R Q` 字尾（大小寫依 GNU：`k` 與 `K` 皆可）。`-M`：`JAN`..`DEC` 不分大小寫，未知者最小。`-V`：GNU `filevercmp` 演算法（含副檔名後綴規則）。
  - `-o FILE` 可與輸入同檔（先讀完全部輸入再寫）。
  - `-c`：失序時 stderr `sort: FILE:LINE: disorder: <line>`，exit 1。
  - 記憶體：Phase 1 可全部讀入；Phase 3 前必須實作外部合併（超過 `-S` 或預設 256 MiB），暫存於 `-T`／`TMPDIR`／`TEMP`。
  - 用法錯誤 exit 2。

### uniq
- 選項：`-c -d -D -u -i`、`--all-repeated[=none|prepend|separate]`、`--group[=separate|prepend|append|both]`、`-f N`、`-s N`、`-w N`、`-z`、舊式 `-N`（= `-f N`）、`+N`（= `-s N`）。
- 要點：運算元 `[INPUT [OUTPUT]]`。`-c` 格式 `%7d `。`-D` 與 `-c` 同用為用法錯誤（GNU 訊息）。

### cut
- 選項：`-b LIST`、`-c LIST`、`-f LIST`、`-d DELIM`、`--complement`、`-s`、`--output-delimiter=STR`、`-z`、`-n`（接受，忽略）。
- 要點：`-c` 依 GNU 9.4 視同 `-b`（位元組）。LIST：`N`、`N-`、`N-M`、`-M`，逗號分隔，重疊合併、遞增輸出。`-d` 必須單一位元組。`-f` 無分隔符之行原樣輸出（除非 `-s`）。必須且只能指定 `-b`/`-c`/`-f` 之一。

### tr
- 選項：`-c -C --complement`、`-d --delete`、`-s --squeeze-repeats`、`-t --truncate-set1`。
- 要點：位元組導向。SET 支援：`\\ \a \b \f \n \r \t \v \NNN`（八進位 1–3 位）、範圍 `a-z`（逆序範圍為錯誤）、`[:alnum:] [:alpha:] [:blank:] [:cntrl:] [:digit:] [:graph:] [:lower:] [:print:] [:punct:] [:space:] [:upper:] [:xdigit:]`、`[=c=]`、`[c*n]`（n 八進位若前導 0）、`[c*]`（僅 SET2）。SET2 較短時以最後字元補齊（GNU）。`[:upper:]`↔`[:lower:]` 對映須成對位置一致。運算元個數錯誤依 GNU 訊息（`missing operand`、`extra operand`）。

### tee
- 選項：`-a --append`、`-i --ignore-interrupts`（Unix 忽略 SIGINT；Windows 接受不作用）、`-p`、`--output-error[=warn|warn-nopipe|exit|exit-nopipe]`。
- 要點：每次讀取立即寫至 stdout 與所有檔案並 flush（不得整段緩衝）。檔案開啟失敗：報錯、續寫其他、最終 exit 1。

### echo
- 不置換、不解析長選項（`--help`/`--version` 僅唯一參數時）。
- 選項：`-n`、`-e`、`-E`；可合併（`-ne`）；含其他字母之參數（`-x`、`-nx`）視為文字。
- `-e` 跳脫：`\\ \a \b \c（停止輸出含換行）\e \f \n \r \t \v \0NNN \xHH`。

### printf
- 不置換。`printf FORMAT [ARG]...`；格式重複使用直到參數耗盡（至少執行一次）。
- 轉換：`%d %i %o %u %x %X %f %F %e %E %g %G %a %A %c %s %b %q %%`；旗標 `- + space # 0`；寬度／精度含 `*`。
- 數值參數：十進位、`0x`、`0` 八進位、`'c`／`"c` 取字元碼；無效 → stderr `printf: 'x': expected a numeric value`，以已解析部分輸出，最終 exit 1。整數範圍 i64/u64（溢位依 GNU 報 `Numerical result out of range`）。浮點以 `f64`（GNU 用 long double，差分測試避開需 80-bit 精度之案例）。
- `%b`：同 echo -e 跳脫，`\c` 終止整個 printf。`%q`：shell 引號（`common::escape`）。
- 格式字串跳脫：同 echo -e 加 `\NNN`（無 0 前綴）、`\uHHHH`、`\UHHHHHHHH`。

### true / false
- 忽略全部參數（`--help`/`--version` 僅唯一參數時處理）。`true` exit 0；`false` exit 1（含 `false --help` 亦 exit 1，GNU 行為）。

### test / [
- 不置換；`test` 不處理 `--help`。`[` 最後參數必須為 `]`，否則 `[: missing ']'` exit 2；`[ --help` 與 `[ --version` 唯一參數時處理。
- 一元：`-b -c -d -e -f -g -G -h -L -k -N -O -p -r -s -S -t FD -u -w -x -z -n`；二元：`= == != < >`、`-eq -ne -lt -le -gt -ge`、`-nt -ot -ef`；`!`、`-a`、`-o`、`( )`。
- 解析依 POSIX 依參數個數之規則（0–4 個參數之特例），再退回一般優先序（`!` > `-a` > `-o`）。
- 整數：可帶前後空白與正負號，否則 `test: invalid integer 'x'` exit 2。
- Windows：`-r -w -x` 依 `common::meta` 合成權限；`-O -G` 恆為存在即真；`-b -c -p -S -k -u -g` 恆假（除非檔案不存在之錯誤）。

### seq
- 選項：`-f FORMAT`、`-s STRING`、`-w`、`--`（負數運算元需支援 `seq -- -5 -1`，且 `seq -5 -1` 亦須能判斷為數字）。
- 要點：以十進位定點數運算（不可用 f64 累加，避免 `0.1` 累積誤差）：解析為（整數尾數, 小數位數），輸出小數位數 = 各運算元小數位數最大值。科學記號輸入或 `-f` 時改用 f64 格式化。`inf` 可作 LAST（無限輸出，管線關閉即止）。INCREMENT 為 0 → 錯誤。`-w` 以 0 補齊至等寬（含負號規則依 GNU）。

### tac
- 選項：`-b --before`、`-r --regex`、`-s --separator=STRING`。
- 要點：可 seek 之一般檔由尾端分塊反向讀；stdin／管線須讀入記憶體（或暫存檔）。`-r` 以 regex（BRE→`common::regex`）為分隔。

### nl
- 選項：`-b STYLE`、`-h STYLE`、`-f STYLE`（`a t n pBRE`）、`-d CC`、`-i N`、`-l N`、`-n ln|rn|rz`、`-p`、`-s STRING`、`-v N`、`-w N`。
- 要點：預設 `-b t -n rn -w 6 -s '\t'`。邏輯頁分隔 `\:\:\:`、`\:\:`、`\:`。

### paste
- 選項：`-d LIST`（支援 `\n \t \\ \0`，循環使用）、`-s`、`-z`。
- 要點：檔案長度不一時短者補空欄；同一檔名 `-` 多次出現時依序共用 stdin。

### comm
- 選項：`-1 -2 -3`、`--check-order`、`--nocheck-order`、`--output-delimiter=STR`、`--total`、`-z`。
- 要點：未指定 `--check-order`／`--nocheck-order` 時依 GNU：只在遇到未配對行時檢查順序；失序 → stderr `comm: file 1 is not in sorted order`，照常輸出完畢後 exit 1。`--check-order` 遇失序立即 exit 1。

### fold
- 選項：`-b`、`-s`、`-w WIDTH`、舊式 `-N`。
- 要點：無 `-b` 時 tab 推進至 8 倍數、`\b` 減一、`\r` 歸零（GNU 欄位計算，位元組為單位）。

### expand / unexpand
- expand：`-i --initial`、`-t LIST`（`N`、`N,M,...`、`/N`、`+N`）、舊式 `-N`。
- unexpand：`-a --all`、`--first-only`、`-t LIST`（指定 `-t` 隱含 `-a`）。
- Windows 封裝：`expand.exe` 與 System32 衝突，照常出貨，於 README 衝突表註明。

### yes
- `yes [STRING]...`（多參數以空格連接，預設 `y`）。以大區塊（≥ 8 KiB 預先填滿之緩衝）重複寫出；BrokenPipe 靜默結束。

### basename / dirname
- basename：`-a --multiple`、`-s --suffix=SUFFIX`（隱含 `-a`）、`-z`；兩運算元形式 `basename NAME SUFFIX`。
- dirname：`-z`；多運算元逐一輸出。
- 要點：純字串運算（不觸碰檔案系統），依 POSIX 規則處理尾斜線與 `//`。Windows：`\` 亦視為分隔符；磁碟根 `C:/` 或 `C:\` 之 basename 與 dirname 皆輸出 `C:/`；`C:foo` 之 dirname 輸出 `C:`、basename 輸出 `foo`。輸出經 display_path。

### xargs
- 不置換（首個非選項即為指令）。
- 選項：`-0 --null`、`-a --arg-file=FILE`、`-d --delimiter=DELIM`、`-E EOF`、`-e[EOF]`、`-I REPLACE`、`-i[REPLACE]`、`-L N`、`-l[N]`、`-n N`、`-P N`、`-r --no-run-if-empty`、`-s N`、`-t --verbose`、`-x --exit`、`--process-slot-var=VAR`、`--show-limits`。不支援：`-p`（互動）、`-o`（Phase 4 可選）。
- 要點：
  - 預設指令 `echo`（使用本套件 `echo` 語意：Windows 上若 PATH 找不到 `echo.exe`，以內建實作輸出）。
  - 預設分隔：空白與換行；支援單引號、雙引號、反斜線跳脫（GNU 規則；未閉合引號為錯誤 exit 1）。
  - `-I` 隱含 `-L 1` 且每行為一參數（前導空白去除）。
  - 指令列長度上限：Unix 依 `ARG_MAX` 減環境（預設上限 128 KiB）；Windows 32767 UTF-16 單位減指令名與引號開銷。`-s` 可降低。
  - Windows 啟動子行程使用 `std::process::Command`（引號由 std 依 MSVCRT 規則處理）。
  - exit code：子行程 1–125 → 123；255 → 124（並停止）；被訊號終止 → 125；無法執行 → 126；找不到 → 127；本身錯誤 → 1。
  - `-P N`：並行；`-P 0` = 盡可能多（上限取 CPU 數 × 4）。輸出不交錯保證不要求。

### env
- 不置換：遇第一個 `NAME=VALUE` 以外之非選項即為指令。
- 選項：`-i --ignore-environment`、`-` （同 -i）、`-u --unset=NAME`、`-0 --null`、`-C --chdir=DIR`、`-S --split-string=S`、`-v --debug`（輸出至 stderr）。
- 要點：無指令時印環境（`NAME=VALUE\n`，`-0` 改 `\0`），順序依 OS 回傳。`-S` 依 GNU 規則分割（引號、`\_`、`${VAR}`）。exit：`-C` 失敗 125、無法執行 126、找不到 127、自身錯誤 125；否則為子行程 exit code。Windows 環境變數名不分大小寫（`-u path` 應移除 `Path`）。

### printenv
- `printenv [-0] [NAME]...`；無 NAME 印全部；任一不存在 exit 1；`-0` 以 `\0` 結尾。

### sleep
- `sleep NUMBER[SUFFIX]...`；字尾 `s m h d`；小數；多參數加總；`infinity`。無參數 → `sleep: missing operand` exit 1。

### egrep / fgrep
- 等同 `grep -E` / `grep -F` 加原參數。不輸出 GNU 3.8+ 之 obsolescence 警告。

---

## P2：檔案系統與系統資訊

### mkdir
- 選項：`-m MODE`、`-p`、`-v`、`-Z`/`--context`（不支援）。
- 要點：`-v` 格式 `mkdir: created directory 'x'`。`-p` 對既存目錄不報錯；既存非目錄報 `File exists`。`-m` 於 Unix 設權限（不受 umask）；Windows 只驗證 MODE 語法後忽略（目錄唯讀屬性語意不同），並於 help 註明。`-p` 建立之中間目錄權限依 umask（GNU）。

### rm
- 選項：`-f -i -I`、`--interactive[=never|once|always]`、`-r -R --recursive`、`-d --dir`、`-v`、`--one-file-system`、`--preserve-root[=all]`（預設）、`--no-preserve-root`。
- 要點：
  - 拒絕刪除 `.`、`..`（`rm: refusing to remove '.' or '..' directory: skipping 'x'`）；拒絕 `/`；Windows 另拒絕磁碟根（`C:/`、`C:\`）與 UNC 共享根。
  - 遍歷不得跟隨 symlink／junction（Windows junction 視同 symlink，只刪連結本身）。
  - 互動提示讀 stdin 一行，`y`/`Y` 開頭為是。
  - `-f`：不存在之運算元不報錯；無運算元不報錯。
  - Windows：唯讀檔見 02-conventions §7；檔案被占用 → `Permission denied` 映射。

### rmdir
- 選項：`-p --parents`、`-v`、`--ignore-fail-on-non-empty`。

### cp
- 選項：`-a --archive`、`-b`、`--backup[=CONTROL]`、`-d`、`-f --force`、`-H`、`-i`、`-l --link`、`-L`、`-n --no-clobber`、`-P --no-dereference`、`-p`、`--preserve[=mode,ownership,timestamps,links,all]`、`--no-preserve=ATTR`、`--parents`、`-R -r --recursive`、`--remove-destination`、`--reflink[=auto|never]`（`always` → 不支援錯誤）、`--sparse=WHEN`（接受）、`-s --symbolic-link`、`-S --suffix`、`-t --target-directory`、`-T --no-target-directory`、`-u`、`--update[=all|none|none-fail|older]`、`-v`、`--attributes-only`（不支援）。
- 要點：
  - 目錄無 `-r` → `cp: -r not specified; omitting directory 'x'` exit 1。
  - 不得將目錄複製進自身（`cannot copy a directory, 'a', into itself, 'a/b'`）。
  - `-a` = `-dR --preserve=all`。ownership 保留於 Windows 為 no-op。
  - 同一檔案（含 hardlink）→ `'a' and 'b' are the same file`。
  - 複製以串流方式；Linux 可用 `std::fs::copy`（其會使用 copy_file_range）。
  - `-v` 格式 `'a' -> 'b'`。

### mv
- 選項：`-b`、`--backup[=CONTROL]`、`-f`、`-i`、`-n`、`--strip-trailing-slashes`、`-S`、`-t`、`-T`、`-u`、`--update[=…]`、`-v`、`--no-copy`。
- 要點：先 `rename`；跨裝置（`CrossesDevices`／Windows `ERROR_NOT_SAME_DEVICE`）退回「遞迴複製（保留屬性）後刪除來源」，`--no-copy` 時直接報錯。目錄覆蓋既存空目錄允許、非空目錄報 `Directory not empty`。Windows 上 `rename` 覆蓋既存檔必須成功（std 已用 `MoveFileExW(REPLACE_EXISTING)`）；大小寫改名（`a.txt`→`A.txt`）必須可行。

### touch
- 選項：`-a`、`-c --no-create`、`-d --date=STRING`、`-f`（忽略）、`-h --no-dereference`、`-m`、`-r --reference=FILE`、`-t STAMP`（`[[CC]YY]MMDDhhmm[.ss]`）、`--time=access|atime|use|modify|mtime`。
- 要點：`-d` 解析共用 `common::datetime`（見 date）。檔案不存在且無 `-c` → 建立空檔。

### ln
- 選項：`-b`、`--backup[=CONTROL]`、`-d -F`（不支援）、`-f`、`-i`、`-L`、`-n --no-dereference`、`-P`、`-r --relative`、`-s --symbolic`、`-S`、`-t`、`-T`、`-v`。
- 要點：Windows symlink：目標為目錄（或 `-s` 時依目標解析後之型別）用 `symlink_dir`，否則 `symlink_file`；目標不存在時預設 `symlink_file`。權限失敗訊息見 02-conventions §7。硬連結 `std::fs::hard_link`。

### link / unlink
- 精確兩個／一個運算元，否則用法錯誤。`unlink` 不得刪目錄（`Is a directory`）。
- Windows 封裝：`link.exe` 與 MSVC 連結器衝突——**Windows 封裝預設不放入 `link.exe`**（以 registry `windows_conflict` 標記，封裝腳本略過），multi-call `agentcmd link` 仍可用。

### realpath
- 選項：`-e --canonicalize-existing`、`-m --canonicalize-missing`、`-L --logical`、`-P --physical`、`-q --quiet`、`--relative-to=DIR`、`--relative-base=DIR`、`-s --strip --no-symlinks`、`-z`。
- 要點：預設要求除最後一段外皆存在。Windows 去 `\\?\` 前綴，輸出經 display_path（`C:/Users/...`）。`--relative-to` 跨磁碟（`C:` vs `D:`）時輸出絕對路徑。

### readlink
- 選項：`-f -e -m`、`-n`、`-q -s`、`-v`、`-z`。
- 要點：無 `-f/-e/-m` 時運算元非 symlink → 無輸出 exit 1。Windows junction 可讀為連結目標。

### stat
- 選項：`-L --dereference`、`-f --file-system`、`-c --format=FORMAT`、`--printf=FORMAT`、`-t --terse`、`--cached=WHEN`（接受）。
- 格式序列（檔案）：`%a %A %b %B %C(→'?') %d %D %f %F %g %G %h %i %m %n %N %o %s %t %T %u %U %w %W %x %X %y %Y %z %Z %%`，支援寬度／旗標（`%-10s`）與 `%Hd %Ld %Hr %Lr`。`-f` 序列：`%a %b %c %d %f %i %l %n %s %S %t %T`。
- 要點：預設輸出依 GNU 9.4 版面（含 `Birth:` 行；無則 `-`）。時間格式 `2026-10-30 05:10:00.123456789 +0000`（依 TZ）。Windows：見 02-conventions §7；`%w` 用 `created()`。`--printf` 處理跳脫且不自動換行；`-c` 自動換行。

### du
- 選項：`-0 --null`、`-a --all`、`--apparent-size`、`-B --block-size=SIZE`、`-b --bytes`、`-c --total`、`-D -H --dereference-args`、`-d --max-depth=N`、`-h --human-readable`、`--inodes`（不支援）、`-k`、`-L --dereference`、`-l --count-links`、`-m`、`-P --no-dereference`、`-S --separate-dirs`、`--si`、`-s --summarize`、`-t --threshold=SIZE`、`--time[=WORD]`、`--time-style=STYLE`、`-X --exclude-from=FILE`、`--exclude=PATTERN`、`-x --one-file-system`。
- 要點：預設區塊 1024。Unix 用 `st_blocks*512`；Windows 磁碟用量 = apparent size 向上取整至 4096（文件化差異）。硬連結（同 dev+ino）只計一次（`-l` 除外）。輸出 `SIZE\tPATH`，後序遍歷，子項依讀目錄順序（**不排序**，與 GNU 一致；測試須以單一子項或 `sort` 後比對）。

### mktemp
- 選項：`-d --directory`、`-u --dry-run`、`-q --quiet`、`--suffix=SUFF`、`-p DIR`、`--tmpdir[=DIR]`、`-t`。
- 要點：預設模板 `tmp.XXXXXXXXXX` 於 TMPDIR（Windows `TEMP`）。模板尾端 X 少於 3 → `too few X's in template` exit 1。以 `create_new` 原子建立（目錄用 `create_dir` 並重試），檔案權限 0600、目錄 0700（Unix）。隨機字元集 `[A-Za-z0-9]`。

### date
- 選項：`-d --date=STRING`、`-f --file=DATEFILE`、`-I[FMT] --iso-8601[=date|hours|minutes|seconds|ns]`、`-R --rfc-email`、`--rfc-3339=date|seconds|ns`、`-r --reference=FILE`、`-u --utc --universal`、`+FORMAT`、`--debug`（不支援）、`-s --set`（不支援，訊息 `cannot set date: Operation not permitted`，exit 1）。
- 格式：GNU strftime 全集：`%a %A %b %B %c %C %d %D %e %F %g %G %h %H %I %j %k %l %m %M %n %N %p %P %q %r %R %s %S %t %T %u %U %V %w %W %x %X %y %Y %z %:z %::z %:::z %Z %%`，旗標 `- _ 0 ^ #` 與寬度（`%-d %_H %10N %3N`）。預設格式（C locale）：`%a %b %e %H:%M:%S %Z %Y`。
- `-d` 字串（`common::datetime::parse`，`touch -d` 共用），必須支援：
  - `@EPOCH[.frac]`；ISO 8601（`2026-10-30`、`2026-10-30T05:10:00`、`2026-10-30 05:10:00.5`、帶 `Z`／`+08:00`／`+0800`）；
  - RFC 2822（`Thu, 30 Oct 2026 05:10:00 +0000`）；`date` 預設輸出格式之反解析；
  - `MM/DD/YYYY`、`DD Mon YYYY`、`Mon DD YYYY`、`HH:MM[:SS] [am|pm]`；
  - 關鍵字 `now today yesterday tomorrow`、`noon midnight`；
  - 相對：`[+-]N (year|month|fortnight|week|day|hour|minute|min|second|sec)[s] [ago]`、`next/last <unit|weekday>`、weekday 名；可多項組合（`2026-01-01 +3 days 2 hours ago`）；
  - 時區縮寫 `UTC GMT Z` 與數字偏移；`TZ="Asia/Taipei" 2026-01-01 00:00` 形式。
  - 無法解析 → `date: invalid date 'x'` exit 1。

### md5sum / sha1sum / sha224sum / sha256sum / sha384sum / sha512sum / b2sum
- 選項：`-b --binary`、`-c --check`、`--tag`、`-t --text`、`-z --zero`、`--ignore-missing`、`--quiet`、`--status`、`--strict`、`-w --warn`；b2sum 另 `-l --length=BITS`（8 的倍數，≤ 512）。
- 要點：輸出 `HASH  NAME`（text）／`HASH *NAME`（`-b`）；兩平台預設皆 text 模式。檔名含 `\` 或換行時，行首加 `\` 並跳脫（GNU）。`-c` 接受 GNU 與 BSD（`--tag`）兩種格式；輸出 `NAME: OK` / `NAME: FAILED`；結尾警告 `WARNING: N computed checksum did NOT match`（單複數依 GNU）。exit：有不符或讀取失敗 1。

### cksum / sum
- cksum：`-a --algorithm=sysv|bsd|crc|crc32b|md5|sha1|sha224|sha256|sha384|sha512|blake2b`、`--base64`、`-c --check`、`-l --length`、`--raw`、`--tag`、`--untagged`、`-z`、`--ignore-missing --quiet --status --strict -w`。預設 `crc`：POSIX CRC（多項式 0x04C11DB7，含長度），輸出 `CRC SIZE NAME`。
- sum：`-r`（BSD，預設）、`-s --sysv`。BSD 輸出 `%05d %5d`（區塊 1024），SysV `%d %d`（區塊 512）。

### base64 / base32 / basenc
- 共同：`-d --decode`、`-i --ignore-garbage`、`-w --wrap=COLS`（預設 76，0 不換行）。
- basenc 編碼選擇：`--base64 --base64url --base32 --base32hex --base16 --base2msbf --base2lsbf --z85`（必擇一）。
- 要點：解碼時忽略換行；遇非法字元（無 `-i`）→ `base64: invalid input` exit 1，已解出之資料仍輸出。

### truncate
- 選項：`-c --no-create`、`-o --io-blocks`、`-r --reference=RFILE`、`-s --size=SIZE`（前綴 `+ - < > / %`；字尾同 GNU）。
- 要點：`-s` 與 `-r` 至少其一；Windows 用 `set_len`。

### nproc
- 選項：`--all`、`--ignore=N`。尊重 `OMP_NUM_THREADS`（取逗號前第一值）、`OMP_THREAD_LIMIT`（`--all` 時不套用）。最少輸出 1。

### hostname
- 無參數印主機名；`-s --short`（第一個 `.` 前）、`-f --fqdn`（盡力；取不到時同短名）。帶運算元（設定主機名）→ 不支援錯誤 exit 1。

### arch
- 輸出 `uname -m` 之值：`x86_64`、`aarch64`（Windows ARM64 亦為 `aarch64`）、`i686`、`arm`。

### uptime
- 選項：`-p --pretty`、`-s --since`。
- 預設格式依 procps：` HH:MM:SS up D days, H:MM,  N users,  load average: x.xx, y.yy, z.zz`。Windows：users 無法取得時省略該段；無 load average 時省略該段（不得印假數字）；`-s` 輸出 `YYYY-MM-DD HH:MM:SS`。

---

## P3：其餘對齊指令

### join
- 選項：`-a FILENUM`、`-e EMPTY`、`-i --ignore-case`、`-j FIELD`、`-o FORMAT`（含 `auto`、`0`）、`-t CHAR`、`-v FILENUM`、`-1 FIELD`、`-2 FIELD`、`--check-order`、`--nocheck-order`、`--header`、`-z`。
- 要點：預設欄分隔為空白序列且忽略前導空白；未排序之偵測與訊息依 GNU（`join: FILE:LINE: is not sorted: ...`，結尾 exit 1）。

### od
- 選項：`-A --address-radix=d|o|x|n`、`-j --skip-bytes`、`-N --read-bytes`、`-S --strings[=N]`、`-t --format=TYPE`（`a c d[SIZE] f[SIZE] o[SIZE] u[SIZE] x[SIZE]`，SIZE 為數字或 `C S I L`／`F D L`，後綴 `z`）、`-v --output-duplicates`、`-w --width[=N]`、`--endian=big|little`、傳統短選項 `-a -b -c -d -f -i -l -o -s -x`。
- 要點：重複行以 `*` 表示（除非 `-v`）。`-t f` 之 long double（`fL`）以 f64 輸出並記錄差異。

### split
- 選項：`-a --suffix-length=N`、`--additional-suffix=SUFFIX`、`-b --bytes=SIZE`、`-C --line-bytes=SIZE`、`-d`、`--numeric-suffixes[=FROM]`、`-x`、`--hex-suffixes[=FROM]`、`-e --elide-empty-files`、`-l --lines=N`、`-n --number=CHUNKS`（`N`、`K/N`、`l/N`、`l/K/N`、`r/N`、`r/K/N`）、`-t --separator=SEP`、`--verbose`、`-u`（接受）。不支援：`--filter`（需 shell）。
- 要點：預設前綴 `x`、後綴長度 2 且自動延長（GNU 規則）。

### csplit
- 選項：`-b --suffix-format=FORMAT`、`-f --prefix=PREFIX`、`-k --keep-files`、`--suppress-matched`、`-n --digits=N`、`-s -q --quiet --silent`、`-z --elide-empty-files`。
- 模式：`INTEGER`、`/REGEXP/[OFFSET]`、`%REGEXP%[OFFSET]`、`{INTEGER}`、`{*}`。預設輸出各片位元組數。

### fmt
- 選項：`-c --crown-margin`、`-p --prefix=STRING`、`-s --split-only`、`-t --tagged-paragraph`、`-u --uniform-spacing`、`-w --width=N`、`-g --goal=N`、舊式 `-N`。
- 要點：實作 GNU 之最佳換行（Knuth 風格成本函數，goal 預設 width×93%）。差分測試涵蓋純英文段落、縮排、`-u`、`-s`、`-p`。

### pr
- 選項：`+FIRST[:LAST]`、`-COLUMN`、`-a`、`-c`、`-d`、`-D --date-format=FORMAT`、`-e[CHAR[WIDTH]]`、`-F -f`、`-h --header=HEADER`、`-i[CHAR[WIDTH]]`、`-J`、`-l --length=N`、`-m --merge`、`-n[SEP[DIGITS]]`、`-N --first-line-number=N`、`-o --indent=MARGIN`、`-r`、`-s[CHAR]`、`-S[STRING]`、`-t --omit-header`、`-T`、`-v`、`-w --width=N`、`-W --page-width=N`。
- 要點：表頭日期預設用檔案 mtime（stdin 用目前時間），格式 `%Y-%m-%d %H:%M`。

### shuf
- 選項：`-e --echo`、`-i --input-range=LO-HI`、`-n --head-count=N`、`-o --output=FILE`、`--random-source=FILE`、`-r --repeat`、`-z`。
- 要點：`-i` 與 `-n` 小時不得配置整個範圍（用稀疏抽樣）。`--random-source` 給定時必須可重現（同檔同輸出）。

### tsort
- 讀成對 token；輸出拓撲序（GNU 之順序：Kahn 演算法，佇列依首次出現順序）。奇數個 token → `tsort: FILE: input contains an odd number of tokens` exit 1。循環 → stderr `tsort: FILE: input contains a loop:` 與循環成員，仍輸出全部節點，exit 1。

### numfmt
- 選項：`--debug`（接受，輸出至 stderr 可略）、`-d --delimiter=X`、`--field=FIELDS`、`--format=FORMAT`、`--from=none|auto|si|iec|iec-i`、`--from-unit=N`、`--grouping`（C locale no-op）、`--header[=N]`、`--invalid=abort|fail|warn|ignore`、`--padding=N`、`--round=up|down|from-zero|towards-zero|nearest`、`--suffix=SUFFIX`、`--to=…`、`--to-unit=N`、`-z`。

### factor
- 選項：`-h --exponents`。範圍 u128（Pollard-rho + Miller-Rabin 確定性底數）；超過 u128 → `factor: 'N' is too large` exit 1。無運算元時讀 stdin（空白分隔）。

### expr
- 不置換（只特判唯一參數 `--help`/`--version`）。運算子：`| & < <= = == != >= > + - * / % : match substr index length +TOKEN ( )`。
- 要點：`:`／`match` 為 BRE、隱含 `^` 錨定；有 `\(...\)` 時輸出第一組，否則輸出匹配長度。整數用 i128（GNU 用 bignum）；溢位時 stderr `expr: integer overflow`、exit 3，於 help 註明此差異。exit：結果非空非 0 → 0；空或 0 → 1；語法錯誤 2；其他錯誤 3。

### pathchk
- 選項：`-p`、`-P`、`--portability`。

### ptx
- 選項：`-A --auto-reference`、`-G --traditional`、`-F --flag-truncation=STRING`、`-M --macro-name=STRING`（僅 `-O`/`-T` 使用）、`-O --format=roff`、`-R --right-side-refs`、`-S --sentence-regexp=REGEXP`、`-T --format=tex`、`-W --word-regexp=REGEXP`、`-b --break-file=FILE`、`-f --ignore-case`、`-g --gap-size=N`、`-i --ignore-file=FILE`、`-o --only-file=FILE`、`-r --references`、`-t --typeset-mode`（接受）、`-w --width=N`。
- 要點：優先保證預設輸出與 `-G`、`-w`、`-r`、`-i`/`-o` 之差分一致；`-O`/`-T` 以 golden 測試。

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立 | — |
