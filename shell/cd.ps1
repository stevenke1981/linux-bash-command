# Dot-source this file in PowerShell after adding the executable directory to PATH.
function global:Invoke-AgentCd {
    $executable = Get-Command cd.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
    $resolved = & $executable.Source @args
    if ($LASTEXITCODE -ne 0) { return }
    Set-Location -LiteralPath $resolved
}
Set-Alias -Name cd -Value Invoke-AgentCd -Scope Global -Option AllScope -Force

