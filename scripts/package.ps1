[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string] $BinaryDirectory,
    [Parameter(Mandatory = $true)] [string] $StageDirectory,
    [Parameter(Mandatory = $true)] [string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Resolve-FileSystemPath([string] $Path) {
    $provider = $null
    $drive = $null
    $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath(
        $Path, [ref] $provider, [ref] $drive
    )
    if ($provider.Name -ne 'FileSystem') { throw "Path must use the filesystem provider: $Path" }
    return [IO.Path]::GetFullPath($resolved)
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binaries = Resolve-FileSystemPath $BinaryDirectory
$stage = (Resolve-FileSystemPath $StageDirectory).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
$output = Resolve-FileSystemPath $OutputDirectory
if ($stage.Length -lt 3 -or $stage -eq [IO.Path]::GetPathRoot($stage)) { throw 'Stage directory must not be a filesystem root' }
$stagePrefix = $stage + [IO.Path]::DirectorySeparatorChar
if ($output.Equals($stage, [StringComparison]::OrdinalIgnoreCase) -or
    $output.StartsWith($stagePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Output directory must not be the stage directory or inside it'
}
if (-not (Test-Path -LiteralPath $binaries -PathType Container)) {
    throw "Binary directory does not exist: $binaries"
}
if (Test-Path -LiteralPath $stage) {
    if (-not (Test-Path -LiteralPath $stage -PathType Container)) {
        throw "Stage path is not a directory: $stage"
    }
    if (@(Get-ChildItem -LiteralPath $stage -Force).Count -ne 0) {
        throw "Stage directory must be empty; existing files are preserved: $stage"
    }
}
if ((Test-Path -LiteralPath $output) -and -not (Test-Path -LiteralPath $output -PathType Container)) {
    throw "Output path is not a directory: $output"
}

$manifest = Get-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Raw
$package = [regex]::Match($manifest, '(?ms)^\[package\]\s*$(.*?)(?=^\[|\z)')
$match = [regex]::Match($package.Groups[1].Value, '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?)"\s*$')
if (-not $package.Success -or -not $match.Success) { throw 'Could not read package version from Cargo.toml' }
$version = $match.Groups[1].Value
$agent = Join-Path $binaries 'agentcmd.exe'
if (-not (Test-Path -LiteralPath $agent -PathType Leaf)) { throw "Missing binary: $agent" }
$reportedVersion = @(& $agent --version)
if ($LASTEXITCODE -ne 0 -or $reportedVersion.Count -ne 1 -or $reportedVersion[0] -ne "agentcmd $version (multi-call binary)") {
    throw "The agentcmd executable does not match Cargo.toml version $version"
}
$names = @(& $agent --list)
if ($LASTEXITCODE -ne 0 -or $names.Count -eq 0) { throw 'Could not obtain the command registry from agentcmd --list' }
$seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
$files = [Collections.Generic.List[object]]::new()
$files.Add(@{ Source = $agent; Name = 'agentcmd.exe' })
foreach ($name in $names) {
    if ($name -ne '[' -and $name -notmatch '^[a-zA-Z0-9_.+-]+$') { throw "Invalid registry name: $name" }
    if (-not $seen.Add($name)) { throw "Duplicate registry name: $name" }
    if ($name -eq '[' -or $name -eq 'link') { continue }
    $path = Join-Path $binaries "$name.exe"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing binary: $path" }
    $files.Add(@{ Source = $path; Name = "$name.exe" })
}
if (-not $seen.Contains('[') -or -not $seen.Contains('test')) { throw 'Registry must include both [ and test' }
$files.Add(@{ Source = (Join-Path $binaries 'test.exe'); Name = '[.exe' })
foreach ($name in @('README.md', 'LICENSE', 'NOTICE.md')) {
    $path = Join-Path $repo $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing documentation: $path" }
    $files.Add(@{ Source = $path; Name = $name })
}
foreach ($name in @('QuickStart.txt', 'Agent Commands.cmd')) {
    $path = Join-Path (Join-Path $repo 'packaging') $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing packaging file: $path" }
    $files.Add(@{ Source = $path; Name = $name })
}
$shell = Join-Path $repo 'shell'
if (-not (Test-Path -LiteralPath $shell -PathType Container)) { throw "Missing shell directory: $shell" }
$shellFiles = @(Get-ChildItem -LiteralPath $shell -File)
if ($shellFiles.Count -eq 0) { throw "Shell directory is empty: $shell" }

$archive = Join-Path $output "agent-commands-$version-windows-x86_64.zip"
$checksums = Join-Path $output 'SHA256SUMS.txt'
foreach ($path in @($archive, $checksums)) {
    if (Test-Path -LiteralPath $path) { throw "Output already exists and will not be overwritten: $path" }
}
if (-not (Test-Path -LiteralPath $stage)) { New-Item -ItemType Directory -Path $stage | Out-Null }
if (-not (Test-Path -LiteralPath $output)) { New-Item -ItemType Directory -Path $output | Out-Null }
foreach ($entry in $files) { Copy-Item -LiteralPath $entry.Source -Destination (Join-Path $stage $entry.Name) }
$shellStage = Join-Path $stage 'shell'
New-Item -ItemType Directory -Path $shellStage | Out-Null
foreach ($file in $shellFiles) { Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $shellStage $file.Name) }

Add-Type -AssemblyName System.IO.Compression
$stream = [IO.File]::Open($archive, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try {
    $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $false)
    try {
        foreach ($file in Get-ChildItem -LiteralPath $stage -File -Recurse) {
            $relative = $file.FullName.Substring($stage.Length).TrimStart([char[]]'\/').Replace('\', '/')
            $entry = $zip.CreateEntry($relative, [IO.Compression.CompressionLevel]::Optimal)
            $source = [IO.File]::OpenRead($file.FullName)
            try {
                $destination = $entry.Open()
                try { $source.CopyTo($destination) } finally { $destination.Dispose() }
            } finally { $source.Dispose() }
        }
    } finally { $zip.Dispose() }
} finally { $stream.Dispose() }
$hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumStream = [IO.File]::Open($checksums, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try {
    $bytes = [Text.Encoding]::ASCII.GetBytes("$hash  $([IO.Path]::GetFileName($archive))`n")
    $checksumStream.Write($bytes, 0, $bytes.Length)
} finally { $checksumStream.Dispose() }
Write-Output "Staged $($files.Count + $shellFiles.Count) files in $stage"
Write-Output "Archive: $archive"
Write-Output "Checksums: $checksums"
