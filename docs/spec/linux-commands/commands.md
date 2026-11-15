# 指令總表

欄位說明：
- **來源**：`existing`＝本專案已有；`ms`＝microsoft/coreutils 出貨；`ms-dropped`＝uutils 有但 microsoft/coreutils 因 Windows 衝突捨棄，本專案仍提供；`extra`＝本專案擴充。
- **階段**：見 [07-phases.md](07-phases.md)。
- **差分基準**：GNU 差分測試所比對之 Ubuntu 24.04 工具（套件）；`—` 表不做差分（自訂 golden 測試）。
- **Windows 衝突**：`PS`＝PowerShell 別名；`SYS`＝`C:\Windows\System32` 既有同名程式；`VS`＝Visual Studio 工具鏈同名程式。

## A. 既有（17）

| 指令 | 來源 | 階段 | 模組 | 差分基準 | Windows 衝突 |
|---|---|---|---|---|---|
| search | existing | P0 | search.rs | — | — |
| diff | existing | P0 | diff.rs | diffutils | PS(5.1) |
| list | existing | P0 | files.rs | — | — |
| read | existing | P0 | files.rs | — | — |
| rg | existing | P0 | search.rs | ripgrep（不強制） | — |
| ls | existing | P0 | fs_commands.rs | coreutils | PS |
| pwd | existing | P0 | fs_commands.rs | coreutils | PS |
| cd | existing | P0 | bin/cd.rs | — | PS |
| cat | existing | P0 | text_commands.rs | coreutils | PS |
| less | existing | P0 | less_command.rs | — | — |
| head | existing | P0 | text_commands.rs | coreutils | — |
| tail | existing | P0 | text_commands.rs | coreutils | — |
| grep | existing | P0 | grep_command.rs | grep | — |
| find | existing | P0 | find_command/ | findutils | SYS |
| ps | existing | P0 | ps_command.rs | — | PS |
| df | existing | P0 | df_command.rs | —（數值不可比） | — |
| curl | existing | P0 | curl_command.rs | — | PS(5.1) |

## B. 對齊 microsoft/coreutils

| 指令 | 來源 | 階段 | 模組（`src/commands/`） | 差分基準 | Windows 衝突 |
|---|---|---|---|---|---|
| wc | ms | P1 | wc.rs | coreutils | — |
| sort | ms | P1 | sort/ | coreutils | PS, SYS |
| uniq | ms | P1 | uniq.rs | coreutils | — |
| cut | ms | P1 | cut.rs | coreutils | — |
| tr | ms | P1 | tr.rs | coreutils | — |
| tee | ms | P1 | tee.rs | coreutils | PS |
| echo | ms | P1 | echo.rs | coreutils | PS |
| printf | ms | P1 | printf.rs | coreutils | — |
| true | ms | P1 | truefalse.rs | coreutils | — |
| false | ms | P1 | truefalse.rs | coreutils | — |
| test | ms | P1 | test.rs | coreutils | — |
| [ | ms | P1 | test.rs | coreutils | — |
| seq | ms | P1 | seq.rs | coreutils | — |
| tac | ms | P1 | tac.rs | coreutils | — |
| nl | ms | P1 | nl.rs | coreutils | — |
| paste | ms | P1 | paste.rs | coreutils | — |
| comm | ms | P1 | comm.rs | coreutils | — |
| fold | ms | P1 | fold.rs | coreutils | — |
| unexpand | ms | P1 | expand.rs | coreutils | — |
| yes | ms | P1 | yes.rs | coreutils | — |
| basename | ms | P1 | pathname.rs | coreutils | — |
| dirname | ms | P1 | pathname.rs | coreutils | — |
| xargs | ms | P1 | xargs.rs | findutils | — |
| env | ms | P1 | env.rs | coreutils | — |
| printenv | ms | P1 | env.rs | coreutils | — |
| sleep | ms | P1 | sleep.rs | coreutils | PS |
| mkdir | ms | P2 | fsops/mkdir.rs | coreutils | PS |
| rm | ms | P2 | fsops/rm.rs | coreutils | PS |
| rmdir | ms | P2 | fsops/rmdir.rs | coreutils | PS |
| cp | ms | P2 | fsops/cp.rs | coreutils | PS |
| mv | ms | P2 | fsops/mv.rs | coreutils | PS |
| touch | ms | P2 | fsops/touch.rs | coreutils | — |
| ln | ms | P2 | fsops/ln.rs | coreutils | — |
| link | ms | P2 | fsops/link.rs | coreutils | VS（MSVC `link.exe`） |
| unlink | ms | P2 | fsops/link.rs | coreutils | — |
| realpath | ms | P2 | pathname.rs | coreutils | — |
| readlink | ms | P2 | pathname.rs | coreutils | — |
| stat | ms | P2 | stat.rs | coreutils | — |
| du | ms | P2 | du.rs | coreutils（`--apparent-size` 情形） | — |
| mktemp | ms | P2 | mktemp.rs | —（隨機名） | — |
| date | ms | P2 | date.rs | coreutils（固定 `-d`） | PS, SYS(cmd 內建) |
| md5sum | ms | P2 | hashsum.rs | coreutils | — |
| sha1sum | ms | P2 | hashsum.rs | coreutils | — |
| sha224sum | ms | P2 | hashsum.rs | coreutils | — |
| sha256sum | ms | P2 | hashsum.rs | coreutils | — |
| sha384sum | ms | P2 | hashsum.rs | coreutils | — |
| sha512sum | ms | P2 | hashsum.rs | coreutils | — |
| b2sum | ms | P2 | hashsum.rs | coreutils | — |
| cksum | ms | P2 | cksum.rs | coreutils | — |
| sum | ms | P2 | cksum.rs | coreutils | — |
| base64 | ms | P2 | basenc.rs | coreutils | — |
| base32 | ms | P2 | basenc.rs | coreutils | — |
| basenc | ms | P2 | basenc.rs | coreutils | — |
| truncate | ms | P2 | truncate.rs | coreutils | — |
| nproc | ms | P2 | sysinfo_cmds.rs | — | — |
| hostname | ms | P2 | sysinfo_cmds.rs | — | SYS |
| arch | ms | P2 | sysinfo_cmds.rs | — | — |
| uptime | ms | P2 | sysinfo_cmds.rs | — | PS |
| join | ms | P3 | join.rs | coreutils | — |
| od | ms | P3 | od.rs | coreutils | — |
| split | ms | P3 | split.rs | coreutils | — |
| csplit | ms | P3 | csplit.rs | coreutils | — |
| fmt | ms | P3 | fmt.rs | coreutils | — |
| pr | ms | P3 | pr.rs | coreutils（固定 `-D`/header） | — |
| shuf | ms | P3 | shuf.rs | —（性質測試） | — |
| tsort | ms | P3 | tsort.rs | coreutils | — |
| numfmt | ms | P3 | numfmt.rs | coreutils | — |
| factor | ms | P3 | factor.rs | coreutils | — |
| expr | ms | P3 | expr.rs | coreutils | — |
| pathchk | ms | P3 | pathchk.rs | coreutils | — |
| ptx | ms | P3 | ptx.rs | coreutils | — |

## C. 擴充

| 指令 | 來源 | 階段 | 模組 | 差分基準 | Windows 衝突 |
|---|---|---|---|---|---|
| expand | ms-dropped | P1 | expand.rs | coreutils | SYS |
| egrep | extra | P1 | grep 別名 | grep | — |
| fgrep | extra | P1 | grep 別名 | grep | — |
| sed | extra | P4 | sed/ | sed 4.9 | — |
| which | extra | P4 | which.rs | debianutils which | — |
| tree | extra | P4 | tree.rs | tree 2.1 | SYS（`tree.com`） |
| uname | extra | P4 | sysinfo_cmds.rs | coreutils（Linux 限 `-s -m -o`） | — |
| whoami | extra | P4 | sysinfo_cmds.rs | — | SYS |
| timeout | extra | P4 | timeout.rs | coreutils（exit code） | SYS |
| kill | extra | P4 | signal.rs | — | PS |
| pgrep | extra | P4 | pgrep.rs | — | — |
| pkill | extra | P4 | pgrep.rs | — | — |
| free | extra | P4 | sysinfo_cmds.rs | —（格式 golden） | — |
| cmp | extra | P4 | cmp.rs | diffutils | — |
| rev | extra | P4 | rev.rs | util-linux | — |
| column | extra | P4 | column.rs | util-linux（`bsdextrautils`） | — |
| dos2unix | extra | P4 | dos2unix.rs | dos2unix | — |
| unix2dos | extra | P4 | dos2unix.rs | dos2unix | — |
| chmod | extra | P4 | fsops/chmod.rs | coreutils（Unix） | — |
| awk | extra | P5 | awk/ | gawk `--posix` | — |
| tar | extra | P5 | tar.rs | GNU tar（`-t` 列表） | SYS（bsdtar） |
| gzip | extra | P5 | gzip.rs | gzip（解壓結果） | — |
| gunzip | extra | P5 | gzip.rs | gzip | — |
| zcat | extra | P5 | gzip.rs | gzip | — |
| patch | extra | P5 | patch/ | GNU patch（結果檔案） | — |
| xxd | extra | P5 | xxd.rs | xxd | — |
| file | extra | P5 | file.rs | —（自訂 golden） | — |

合計：既有 17、對齊 71（含 `[`）、擴充 27，共 115 個名稱。

## D. 明確不提供

`chcon chgrp chown chroot dd dircolors dir vdir groups hostid id install logname mkfifo mknod more nice nohup pinky runcon shred stdbuf stty sync tty users who`、`coreutils-manager`、DOS 版 `find`／`sort`。若使用者呼叫，因不在 registry，由 multi-call 回 `function/utility not found`。

## 變更歷史

| 版本 | 日期 | 內容 | 影響範圍 |
|---|---|---|---|
| v1.0 | 2026-10-30 | 初始建立 | — |
