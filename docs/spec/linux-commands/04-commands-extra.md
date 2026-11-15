# 04 擴充指令規格

通則同 [03-commands-core.md](03-commands-core.md)。

---

## P4：擴充 A

### sed（GNU sed 4.9 子集）
- 選項：`-n --quiet --silent`、`-e --expression=SCRIPT`、`-f --file=FILE`、`-E -r --regexp-extended`、`-i[SUFFIX] --in-place[=SUFFIX]`、`-s --separate`、`-z --null-data`、`-u --unbuffered`、`-l N --line-length=N`、`--posix`（接受）、`--follow-symlinks`、`--debug`（不支援）、`--sandbox`（接受；本實作本就拒絕 `e`）。
- 位址：`N`、`$`、`/RE/`、`\cREc`、`/RE/I`、`/RE/M`、`first~step`、`addr1,addr2`、`addr1,+N`、`addr1,~N`、`0,/RE/`、`addr!`。空 regex `//` 重用上次 regex。
- 指令：`{ }`、`=`、`a`、`i`、`c`（GNU 單行形式 `a text` 與傳統 `a\` 多行形式皆須支援）、`b`、`t`、`T`、`:label`、`d`、`D`、`e`（不支援 → exit 1）、`F`、`g G h H x`、`l [N]`、`n N p P`、`q [EXIT]`、`Q [EXIT]`、`r FILE`、`R FILE`、`w FILE`、`W FILE`（含 `/dev/stdout`、`/dev/stderr`）、`s`、`y`、`z`、`#n`（首行）、註解 `#`。
- `s` 旗標：`g`、`N`、`Ng`、`p`、`i I`、`m M`、`w FILE`、`e`（不支援）。替換：`&`、`\1`–`\9`、`\n`、`\t`、換行跳脫、`\L \U \l \u \E`。
- regex：BRE（預設）／ERE（`-E`）經 `common::regex` 轉譯；GNU 擴充 `\+ \? \|`、`\n \t`、`\w \W \s \S \b \B \< \> \` \'`。**pattern 內 backreference（`\1`）不支援** → `sed: -e expression #1, char N: unsupported backreference in regex`，exit 1。
- 要點：
  - 語法錯誤訊息格式依 GNU：`sed: -e expression #1, char 5: unterminated `s' command`，exit 1；輸入檔不存在 → 報錯續處理，exit 2；I/O 錯誤 exit 4。
  - N 於最後一行：GNU 行為（無 `POSIXLY_CORRECT` 時印出 pattern space 後結束）。
  - `-i`：每檔獨立（隱含 `-s`）；以同目錄暫存檔 + rename；SUFFIX 含 `*` 時以檔名替換 `*`；保留原權限（Unix）。`-i` 對 symlink 預設替換連結本身為一般檔（GNU），`--follow-symlinks` 改寫目標。
  - 行以 `\n` 分隔，`\r` 保留於 pattern space（Windows CRLF 檔案之 `s/x$/y/` 不會匹配，於 help 註明；不做自動轉換）。
  - 最後一行缺換行時輸出亦不補（GNU）。
- 模組拆分：`sed/parse.rs`（腳本→AST）、`sed/addr.rs`、`sed/exec.rs`、`sed/subst.rs`、`sed/mod.rs`（CLI），各 ≤ 800 行。

### which
- 選項：`-a`（列出全部匹配）、`-s`（不輸出，只回 exit code）。
- 要點：依 `PATH` 逐一搜尋可執行檔；Unix 檢查 x 權限；Windows 依 `PATHEXT` 嘗試副檔名（運算元已有副檔名者先試原名），不搜尋目前目錄。輸出經 display_path。exit：全找到 0、任一未找到 1、無效選項 2。

### tree（tree 2.1 相容子集）
- 選項：`-a`、`-d`、`-f`、`-F`、`-L LEVEL`、`-I PATTERN`（`|` 分隔多模式）、`-P PATTERN`、`--matchdirs`、`--ignore-case`、`--prune`、`--filelimit N`、`--noreport`、`-s`、`-h`、`--si`、`-D`（mtime）、`--timefmt=FMT`、`-p`、`--dirsfirst`、`--filesfirst`、`-r`、`-t`、`-v`、`-U`、`--sort=name|version|size|mtime|ctime`、`--gitignore`、`--charset=ascii|utf-8`、`-n`（接受；本就不上色）、`-J`（JSON）、`-i`（不印縮排線）。
- 要點：預設排序依名稱（C locale 位元組序，同 `tree --sort=name` 於 `LC_ALL=C`）。縮排符號：`├── `、`└── `、`│   `、`    `；`--charset=ascii` 用 `|-- `、`` `-- ``、`|   `。結尾 `\nN directories, M files`（單數 `1 directory`、`1 file`）。symlink 顯示 `name -> target`，不跟隨（除 `-l`，可不支援）。`-J` 輸出結構依 tree 2.x（`[{"type":"directory","name":".","contents":[...]},{"type":"report","directories":N,"files":M}]`）。
- Windows 封裝：與 `tree.com` 衝突，照常出貨並於 README 註明。

### uname
- 選項：`-a -s -n -r -v -m -p -i -o`（與長名 `--all --kernel-name --nodename --kernel-release --kernel-version --machine --processor --hardware-platform --operating-system`）。
- 值：Linux／macOS 取 `libc::uname`；`-o` Linux 為 `GNU/Linux`、macOS 為 `Darwin`；`-p -i` 取不到時 `unknown`（`-a` 中省略 unknown 欄，GNU 行為）。Windows：`-s Windows_NT`、`-n` 主機名、`-r` `10.0.<build>`、`-v` 顯示版本字串（sysinfo `long_os_version`）、`-m x86_64|aarch64`、`-o Windows`。

### whoami
- 印目前使用者名稱（Unix `geteuid` → passwd 名；Windows `GetUserNameW`，不含網域）。Windows 封裝：與 System32 `whoami.exe` 衝突，照常出貨並於 README 註明。

### timeout
- 不置換。`timeout [OPTION] DURATION COMMAND [ARG]...`；選項：`-f --foreground`、`-k --kill-after=DURATION`、`-p --preserve-status`、`-s --signal=SIGNAL`、`-v --verbose`。DURATION 同 sleep 字尾，`0` 停用。
- 行為：逾時送訊號（預設 TERM）；`-k` 後再送 KILL。Unix 以 `setpgid` 讓子行程自成行程群組並送訊號至整群（`--foreground` 時只送子行程）。Windows：子行程置入 Job Object（`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`），逾時以 `TerminateJobObject` 結束整棵行程樹。
- exit：逾時 124（`--preserve-status` 時回子行程狀態）；timeout 自身失敗 125；無法執行 126；找不到 127；被 KILL 137（128+9）；否則子行程 exit code。`-v` 逾時時 stderr `timeout: sending signal TERM to command 'cmd'`。

### kill
- 不置換（`-SIGNAL` 形式）。`kill [-s SIG | -SIG | -n NUM] PID...`、`kill -l [SIG|NUM]`、`kill -L`（表格）。
- SIG 名接受 `TERM`、`SIGTERM`、數字。Linux 訊號表固定為 Linux x86-64 編號（macOS 以 libc 常數）。Windows：見 02-conventions §7；`kill -l` 仍列 Linux 表。負 PID（行程群組）：Unix 支援；Windows 不支援錯誤。
- exit：任一失敗 1（`kill: (PID) - No such process`）。

### pgrep / pkill（procps-ng 4.x 子集）
- 選項：`-f --full`、`-i --ignore-case`、`-x --exact`、`-n --newest`、`-o --oldest`、`-v --inverse`、`-c --count`、`-P --parent=PPID,...`、`-u --euid=USER,...`、`-U --uid`（Unix）、`-t`（不支援）。pgrep 另 `-l --list-name`、`-a --list-full`、`-d --delimiter=STR`；pkill 另 `-SIG`、`--signal=SIG`、`-e --echo`。
- 要點：pattern 為 ERE（`common::regex`）對行程名（Linux：`comm`，最多 15 字元）或 `-f` 之完整命令列匹配。Windows 行程名去 `.exe` 後比對，且 `-i` 預設不開（與 procps 一致）。永不匹配自身。exit：有匹配 0、無 1、語法錯誤 2、致命錯誤 3。資料來源 `sysinfo`。

### free（procps-ng 4.x）
- 選項：`-b -k -m -g`、`--kibi --mebi --gibi --tebi --pebi`、`--kilo --mega --giga --tera --peta`、`-h --human`、`--si`、`-l --lohi`（接受，Windows 省略）、`-t --total`、`-w --wide`、`-v --committed`（不支援）、`-s N -c N`（重複輸出）。
- 格式：表頭 `               total        used        free      shared  buff/cache   available`；`Mem:`、`Swap:` 行。Windows／macOS 無 shared／buff/cache 時填 0；Windows Swap 為 page file。

### cmp（diffutils 3.10）
- 選項：`-b --print-bytes`、`-i --ignore-initial=SKIP1[:SKIP2]`、`-l --verbose`、`-n --bytes=LIMIT`、`-s --quiet --silent`；運算元 `FILE1 [FILE2 [SKIP1 [SKIP2]]]`。
- 輸出：`a b differ: byte N, line M`；EOF：stderr `cmp: EOF on b after byte N, line M`（空檔時 `cmp: EOF on b which is empty`）。exit 0／1／2。

### rev
- 每行字元反轉（UTF-8 解碼；含無效序列之行以位元組反轉）。保留行尾 `\n`；`\r` 為一般字元（CRLF 檔反轉後 `\r` 移至行首，與 util-linux 一致）。

### column（util-linux 2.39 子集）
- 選項：`-t --table`、`-s --separator=SEP`、`-o --output-separator=STR`（預設兩空格）、`-c --output-width=N`（預設 `COLUMNS` 或 80）、`-x --fillrows`、`-N --table-columns=NAMES`、`-R --table-right=COLS`、`-H --table-hide=COLS`、`-l --table-columns-limit=N`、`-L --keep-empty-lines`、`-n --table-name`（接受）、`-J --json`。
- 要點：非 `-t` 模式依 util-linux 欄寬（tab 對齊至 8）。`-t` 欄寬以顯示寬度計（ASCII 為主；CJK 寬字元以 2 計）。

### dos2unix / unix2dos（dos2unix 7.x 子集）
- 選項：`-n --newfile INFILE OUTFILE ...`、`-o --oldfile`（預設）、`-k --keep-date`、`-q --quiet`、`-f --force`（含二進位亦轉）、`-b --keep-bom`、`-r --remove-bom`、`-m --add-bom`、`-c ascii`（接受，唯一模式）、`-e --add-eol`、`-s --safe`（預設）。
- 要點：無檔案運算元 → stdin→stdout。偵測二進位（含 NUL 或非文字控制字元）→ 預設跳過並 stderr `dos2unix: Skipping binary file x`。原地改寫見 02-conventions §5（同目錄暫存＋rename）。訊息：`dos2unix: converting file x to Unix format...`（stderr，`-q` 抑制）。BOM 預設行為以 Ubuntu 套件差分結果為準。

### chmod
- 選項：`-c --changes`、`-f --silent --quiet`、`-v --verbose`、`--no-preserve-root`、`--preserve-root`、`--reference=RFILE`、`-R --recursive`。MODE：八進位（含 4 位 setuid 等，Unix）、符號 `[ugoa]*([-+=]([rwxXst]*|[ugo]))+` 逗號串接。
- Windows：只有「是否可寫」有效——結果之 owner `w` 位清除 → 設 readonly 屬性，否則清除 readonly；其餘位元接受但忽略，於 help 註明。

---

## P5：擴充 B

### awk（POSIX awk ＋常用 gawk 相容處）
- 選項：`-F FS`、`-v VAR=VAL`（可重複）、`-f PROGFILE`（可重複）、`--`、`--version`、`--help`。運算元中 `VAR=VAL` 依 POSIX 於讀到該位置時指定。
- 語言：完整 POSIX awk 語法與語意——pattern-action、`BEGIN`/`END`（可多個）、range pattern、regex literal、全部運算子（含 `**`、`**=`、`in`、`?:`、`~ !~`、前後置 `++ --`、字串串接）、陣列與 `SUBSEP` 多維索引、`delete arr`（整個陣列）、使用者函式（陣列傳參考、純量傳值、遞迴）。
- 敘述：`if else while do for for(in) break continue next nextfile exit [expr] return delete getline print printf { }`；`print > file`、`>> file`；`getline`、`getline var`、`getline < file`、`getline var < file`。
- 不支援（v1，執行時報錯 exit 2）：`cmd | getline`、`print | cmd`、`system()`（皆需 shell）；`close()` 對檔案有效、對指令回 -1。
- 內建變數：`NR NF FNR FS OFS ORS RS FILENAME SUBSEP RSTART RLENGTH CONVFMT OFMT ENVIRON ARGC ARGV`。`RS` 單字元、`""`（段落模式）與多字元 regex（gawk 擴充）皆須支援。`FS` 單空格（預設欄位切割）、單字元、regex、`\t`。
- 內建函式：`length([s|arr]) substr index split(s,a[,fs]) sub gsub match sprintf sin cos atan2 exp log sqrt int rand srand tolower toupper fflush([file])`；`printf`/`sprintf` 轉換同 printf 指令（不含 `%b %q`）。
- 值語意：數字／字串／strnum 三態（POSIX）；欄位、`getline` 輸入、`ARGV`、`ENVIRON`、`-v` 值為 strnum；比較規則依 POSIX；數轉字串整數直接輸出，否則用 `CONVFMT`（輸出用 `OFMT`），預設 `%.6g`。`srand()` 預設種子 0（`rand()` 序列不要求與 gawk 相同）。
- regex：ERE（POSIX 字元類、區間 `{n,m}`），動態 regex 字串需快取已編譯結果。
- 錯誤：語法錯誤 `awk: syntax error at source line N` 並指出 token，exit 2。
- 效能：1 GB 純欄位處理（`{s+=$3} END{print s}`）不得超過 gawk 之 3 倍時間（基準測試記錄，不設 CI 閘門）。
- 模組：`awk/lexer.rs parser.rs ast.rs interp.rs value.rs builtins.rs io.rs fields.rs mod.rs`，各 ≤ 800 行。差分基準：`gawk --posix`（Ubuntu `gawk` 套件），另以 `onetrue-awk` 風格案例做 golden。

### tar（GNU tar 1.35 子集，`tar` crate）
- 模式：`-c --create`、`-x --extract --get`、`-t --list`、`-r --append`（可不支援）。選項：`-f --file=ARCHIVE`（`-` 為 stdin/stdout；未給時預設 stdin/stdout）、`-z --gzip --gunzip`、`-v --verbose`、`-C --directory=DIR`（可多次、依位置生效）、`--exclude=PATTERN`、`--strip-components=N`、`-O --to-stdout`、`-k --keep-old-files`、`--overwrite`、`-p --preserve-permissions`、`-h --dereference`、`-P --absolute-names`、`--no-same-owner`（預設）、`-T --files-from=FILE`、`--null`、`--wildcards`、`-a --auto-compress`（依副檔名 `.tar.gz` `.tgz`）。舊式 bundled 第一參數（`tar xzf a.tgz`、`tar cvf`）必須支援。`-j -J --zstd --lzma`：不支援錯誤（需 C 依賴）。
- 安全（MUST）：解壓時拒絕含 `..` 段之成員與絕對路徑（除 `-P`），訊息 `tar: Removing leading '/' from member names`（絕對路徑時去前綴繼續）、`tar: x: Member name contains '..'`（跳過，最終 exit 2）；不得經由先前解出之 symlink 寫出目標目錄之外（解出 symlink 後之成員路徑若穿越 symlink 則拒絕）。
- 輸出：`-t` 列成員名（每行一個）；`-tv` 依 GNU 欄位（`-rw-r--r-- user/group SIZE YYYY-MM-DD HH:MM name`，Windows owner 顯示 `0/0`）。建立格式為 GNU（長檔名）或 pax。
- exit：0 成功、1 有檔案變動／差異、2 致命錯誤。

### gzip / gunzip / zcat（gzip 1.12 子集，`flate2`）
- 選項：`-c --stdout`、`-d --decompress`、`-f --force`、`-k --keep`、`-l --list`、`-n --no-name`、`-N --name`、`-q --quiet`、`-r --recursive`、`-S --suffix=SUF`、`-t --test`、`-v --verbose`、`-1`…`-9`、`--fast`、`--best`。
- 要點：預設壓縮後刪原檔（`-k` 保留）；目標已存在且無 `-f` → stdin 為 tty 時詢問，否則報錯跳過。多成員 gzip 須完整解出。`zcat` = `gzip -dc`；`gunzip` = `gzip -d`。stdout 為終端且要輸出壓縮資料 → 拒絕（GNU 訊息）除非 `-f`。壓縮輸出位元組不要求與 GNU 相同；差分只比「解壓結果」與 `-l` 之大小欄位、exit code。

### patch（GNU patch 2.7 子集）
- 選項：`-p N --strip=N`、`-i --input=FILE`、`-d --directory=DIR`、`-R --reverse`、`-N --forward`、`--dry-run`、`-o --output=FILE`、`-b --backup`、`-E --remove-empty-files`、`-s --silent --quiet`、`-f --force`、`-t --batch`、`-F --fuzz=N`（預設 2）、`-l --ignore-whitespace`、`--binary`（接受）、`-r --reject-file=FILE`、`--no-backup-if-mismatch`、`-u`（接受）。
- 格式：unified diff（含 `git diff` 擴充：`diff --git` 標頭、`new file mode`、`deleted file mode`、`/dev/null` 來源目標、`\ No newline at end of file`）。context diff（`-c`）與 normal diff：Phase 5 可不支援（明確報錯）。rename／binary patch：不支援。
- 行為：hunk 依行號找起點，失敗時於 ±N 行內搜尋（offset）並套用 fuzz（GNU 演算法：先 0 fuzz 全域搜尋，再 1、2）。輸出 `patching file X`、`Hunk #2 succeeded at 40 (offset 3 lines).`、`Hunk #1 FAILED at 10.`、`1 out of 2 hunks FAILED -- saving rejects to file X.rej`。偵測已套用（reverse 可套）且有 `-N`/`-t` 時略過。保留檔案原行尾（若原檔為 CRLF 而 patch 為 LF，以 `-l`／自動 CRLF 正規化匹配，寫回時保留 CRLF）。
- exit：全部成功 0；有 hunk 失敗 1；嚴重錯誤 2。
- 模組：`patch/parse.rs apply.rs mod.rs`。

### xxd（vim xxd 子集）
- 選項：`-a`、`-b`、`-c COLS`、`-C`（接受）、`-e`、`-g BYTES`、`-i`、`-n NAME`、`-l LEN`、`-o OFFSET`、`-p -ps -postscript -plain`、`-r`、`-s [+][-]SEEK`、`-u`、`-d`。xxd 之長選項為單 dash 形式（`-cols 8`、`-len 16`），須支援。
- 格式：`00000000: 6865 6c6c 6f0a                           hello.`（依 xxd 欄寬計算）。`-r` 反轉一般與 `-p` 格式。

### file（libmagic 子集）
- 選項：`-b --brief`、`-i --mime`、`--mime-type`、`--mime-encoding`、`-L --dereference`、`-h --no-dereference`、`-f --files-from=FILE`、`-0 --print0`（接受）、`-z`（不支援）。
- 偵測順序：不存在 → `cannot open \`x' (No such file or directory)`（exit 0，依 file 行為）；directory；symlink（`symbolic link to x`）；empty；magic 簽章：ELF（位元數、型別、架構）、PE32/PE32+（`PE32+ executable (console) x86-64, for MS Windows`）、Mach-O、`#!` 腳本（`Python script, ASCII text executable` 等依直譯器名）、PNG（含寬高）、JPEG、GIF、PDF（版本）、ZIP、gzip、bzip2、xz、7z、tar（ustar）、SQLite、WebAssembly；文字：ASCII／UTF-8（含 BOM）／UTF-16 LE/BE，附註 `with CRLF line terminators`、`with very long lines`、`with no line terminators`；其餘 `data`。
- 不做 GNU 差分；以 golden 測試覆蓋每一偵測分支（測試內動態產生最小樣本檔）。

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立 | — |
