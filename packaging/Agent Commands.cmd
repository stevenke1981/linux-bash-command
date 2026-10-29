@echo off
set "PATH=%~dp0;%PATH%"
echo Agent Commands is available in this window.
echo Enter command names such as rg, grep, head, tail, ps, df and curl.
echo Type exit to close this window. The system PATH is not modified.
cmd.exe /k
