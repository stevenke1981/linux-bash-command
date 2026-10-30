# 規格：對齊 microsoft/coreutils 並擴充常用 Linux 指令

狀態：待實作（交由 Codex）
撰寫日期：2026-10-30
參考來源（唯讀）：
- microsoft/coreutils（Coreutils for Windows，preview，版本 2026.9.3）：https://github.com/microsoft/coreutils
  - 其子模組固定版本：`deps/coreutils` = uutils/coreutils @ `1c5cd93c`、`deps/findutils` = uutils/findutils @ `d6ae4f98`、`deps/grep` = uutils/grep @ `8dc8daf0`
- GNU coreutils 9.4、findutils 4.9、grep 3.11、sed 4.9、diffutils 3.10（Ubuntu 24.04 套件版本，作為行為差分基準）

## 1. 目標

1. **指令集對齊**：microsoft/coreutils 所出貨之全部 utility（見 [commands.md](commands.md) 之 `ms` 欄），本專案皆須提供，且行為與 GNU 相容（以 Ubuntu 24.04 之 GNU 工具為差分基準）。
2. **擴充常用 Linux 指令**：加入 coding agent 與開發者在 Linux 常用、但 microsoft/coreutils 未收錄者（`sed`、`awk`、`which`、`tree`、`tar`、`gzip`、`timeout`、`kill`、`pgrep` 等，見 [commands.md](commands.md) 之 `extra` 欄）。
3. **既有 17 個指令補齊落差**：exit code、錯誤訊息格式、缺漏選項依 GNU 對齊（見 [05-existing-gaps.md](05-existing-gaps.md)）。
4. **跨平台**：Windows、Linux、macOS 皆須可建置與通過測試（現有 CI 三 OS 矩陣）。microsoft/coreutils 只支援 Windows；本專案不得因對齊而犧牲 Linux/macOS。
5. **multi-call 入口**：比照 microsoft/coreutils，`agentcmd` 成為 multi-call binary（`agentcmd <util> ...`、依 argv[0] 分派、`agentcmd --list`）；同時保留每指令獨立執行檔（Git Bash 無法使用 Windows hardlink 以外之別名機制，見 [01-architecture.md](01-architecture.md)）。

## 2. 非目標

- 不移植 GNU 原始碼（GPL）。可參考 uutils（MIT）程式碼，若有實質移植須於檔頭註明來源並更新 `NOTICE.md`。
- 不做 locale／翻譯：一律 C/POSIX locale 語意（位元組比較排序、ASCII 大小寫折疊），訊息為英文。
- 不做彩色輸出（`--color` 可接受但不上色，延續現有政策）。
- 不支援 Windows 參數 glob 展開（microsoft/coreutils 用 `wild` crate）。理由：Claude Code 等 agent 在 Windows 透過 Git Bash 呼叫，MSYS 傳入之 `'*.rs'` 未必帶引號，展開會破壞 `find -name '*.rs'` 之語意。記為有意偏離。
- 不出貨 microsoft/coreutils 亦捨棄之 POSIX 專屬指令：`chcon chgrp chown chroot dd dircolors dir vdir groups hostid id install logname mkfifo mknod more nice nohup pinky runcon shred stdbuf stty sync tty users who`。
- 不整合 DOS 版 `find`／`sort`（microsoft/coreutils 之 `ntfind`、`ntsort`）。

## 3. 文件索引

| 檔案 | 內容 |
|---|---|
| [01-architecture.md](01-architecture.md) | 模組佈局、registry、multi-call、共用函式庫、依賴白名單 |
| [02-conventions.md](02-conventions.md) | 全域 CLI 行為契約：參數解析、exit code、錯誤訊息、I/O、路徑、Windows 規則 |
| [commands.md](commands.md) | 指令總表（來源、優先序、階段、模組、Windows 衝突） |
| [03-commands-core.md](03-commands-core.md) | 對齊 microsoft/coreutils 之各指令規格 |
| [04-commands-extra.md](04-commands-extra.md) | 擴充指令規格 |
| [05-existing-gaps.md](05-existing-gaps.md) | 既有 17 指令之對齊修正 |
| [06-testing.md](06-testing.md) | 測試策略：單元、CLI、GNU 差分測試、覆蓋率、CI |
| [07-phases.md](07-phases.md) | 實作分階段、每階段交付物與驗收標準 |
| [codex-prompts.md](codex-prompts.md) | 交給 Codex 之逐階段 prompts |

## 4. 規範用語

- **必須（MUST）**：未達成即驗收失敗。
- **應（SHOULD）**：除非有記錄之理由，否則須達成。
- **可（MAY）**：選做。
- 「依 GNU」＝以 Ubuntu 24.04 上對應 GNU 工具，在 `LC_ALL=C TZ=UTC` 下之 stdout 位元組與 exit code 為準。本規格未列之細節一律依 GNU；與 GNU 衝突時以本規格明文為準。

## 5. 總驗收（全部階段完成時）

1. `cargo fmt --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked` 於 Windows／Linux／macOS CI 全綠。
2. Linux CI 之 GNU 差分測試（`AGENTCMD_GNU_DIFF=1`）全綠。
3. `agentcmd --list` 列出 [commands.md](commands.md) 全部指令；每一指令 `--help` exit 0、`--version` exit 0。
4. 新增模組行覆蓋率 ≥ 80%（`cargo llvm-cov`，本機量測並記錄於 `docs/spec/coreutils-alignment/coverage.md`）。
5. README 之指令章節、Windows 衝突表、`packaging/` 與 CI 收集清單同步更新。
