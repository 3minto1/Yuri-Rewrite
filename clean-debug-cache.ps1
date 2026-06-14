param(
  [switch]$DryRun
)

# Usage:
#   powershell -ExecutionPolicy Bypass -File .\clean-debug-cache.ps1
#   powershell -ExecutionPolicy Bypass -File .\clean-debug-cache.ps1 -DryRun

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
chcp 65001 | Out-Null

function Get-DirectoryBytes([string]$Path) {
  if (!(Test-Path -LiteralPath $Path -PathType Container)) {
    return [int64]0
  }
  $measurement = Get-ChildItem -LiteralPath $Path -Recurse -Force -File -ErrorAction Stop |
    Measure-Object -Property Length -Sum
  if ($null -eq $measurement.Sum) {
    return [int64]0
  }
  return [int64]$measurement.Sum
}

function Format-Size([int64]$Bytes) {
  if ($Bytes -ge 1GB) {
    return "{0:N2} GiB" -f ($Bytes / 1GB)
  }
  return "{0:N2} MiB" -f ($Bytes / 1MB)
}

$Root = [System.IO.Path]::GetFullPath((Split-Path -Parent $MyInvocation.MyCommand.Path))
$TauriDir = Join-Path $Root "src-tauri"
$ManifestPath = Join-Path $TauriDir "Cargo.toml"
$TargetDir = Join-Path $TauriDir "target"
$DebugDir = Join-Path $TargetDir "debug"
$ReleaseDir = Join-Path $TargetDir "release"

if (!(Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
  throw "Safety check failed: src-tauri\Cargo.toml was not found next to this script."
}

$manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8
if ($manifest -notmatch '(?m)^name\s*=\s*"yuri-rewrite"\s*$') {
  throw "Safety check failed: Cargo.toml is not the Yuri Rewrite package."
}

$resolvedTauriDir = [System.IO.Path]::GetFullPath($TauriDir).TrimEnd([char]'\')
$resolvedTargetDir = [System.IO.Path]::GetFullPath($TargetDir)
if (!$resolvedTargetDir.StartsWith($resolvedTauriDir + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
  throw "Safety check failed: Cargo target directory is outside src-tauri."
}

$activeBuildProcesses = Get-CimInstance Win32_Process | Where-Object {
  $name = $_.Name.ToLowerInvariant()
  if ($name -notin @("cargo.exe", "rustc.exe", "yuri-rewrite.exe")) {
    return $false
  }
  $executablePath = [string]$_.ExecutablePath
  $commandLine = [string]$_.CommandLine
  return $executablePath.StartsWith($resolvedTargetDir, [System.StringComparison]::OrdinalIgnoreCase) -or
    $commandLine.IndexOf($Root, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
}

if ($activeBuildProcesses) {
  $processList = ($activeBuildProcesses | ForEach-Object { "$($_.Name) (PID $($_.ProcessId))" }) -join ", "
  throw "Cargo/Tauri is currently using this project: $processList. Close the development app or build first."
}

if (!(Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw "Cargo was not found in PATH. Install or activate the Rust toolchain first."
}

$debugBefore = Get-DirectoryBytes $DebugDir
$releaseBefore = Get-DirectoryBytes $ReleaseDir
Write-Host "Debug cache before cleanup: $(Format-Size $debugBefore)"
Write-Host "Release artifacts preserved: $(Format-Size $releaseBefore)"

$cargoArguments = @(
  "clean",
  "--manifest-path", $ManifestPath,
  "--profile", "dev"
)
if ($DryRun) {
  $cargoArguments += "--dry-run"
  Write-Host "Dry run only; no files will be deleted."
}

& cargo @cargoArguments
if ($LASTEXITCODE -ne 0) {
  throw "cargo clean failed with exit code $LASTEXITCODE."
}

if ($DryRun) {
  Write-Host "Dry run completed."
  exit 0
}

$debugAfter = Get-DirectoryBytes $DebugDir
$releaseAfter = Get-DirectoryBytes $ReleaseDir
if ($releaseAfter -ne $releaseBefore) {
  throw "Unexpected result: release artifact size changed during dev-profile cleanup."
}

$freed = [Math]::Max([int64]0, $debugBefore - $debugAfter)
Write-Host "Debug cache after cleanup: $(Format-Size $debugAfter)"
Write-Host "Disk space released: $(Format-Size $freed)"
Write-Host "Release artifacts and portable packages were not touched."
