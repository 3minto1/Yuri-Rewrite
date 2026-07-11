[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = "Medium")]
param(
  [switch]$DryRun
)

# Usage:
#   powershell -ExecutionPolicy Bypass -File .\clean-debug-cache.ps1 -DryRun
#   powershell -ExecutionPolicy Bypass -File .\clean-debug-cache.ps1 -WhatIf
#   powershell -ExecutionPolicy Bypass -File .\clean-debug-cache.ps1

Set-StrictMode -Version Latest
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
  if ($Bytes -ge 1MB) {
    return "{0:N2} MiB" -f ($Bytes / 1MB)
  }
  if ($Bytes -ge 1KB) {
    return "{0:N2} KiB" -f ($Bytes / 1KB)
  }
  return "$Bytes bytes"
}

function Assert-NotReparsePoint([string]$Path, [string]$Label) {
  if (!(Test-Path -LiteralPath $Path)) {
    return
  }
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "Safety check failed: $Label is a symbolic link or directory junction: $($item.FullName)"
  }
}

function Assert-CargoDryRunPaths([string[]]$Output, [string]$AllowedRoot, [int64]$ExpectedBytes) {
  $allowed = [System.IO.Path]::GetFullPath($AllowedRoot).TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
  )
  $allowedPrefix = $allowed + [System.IO.Path]::DirectorySeparatorChar
  $pathCount = 0

  foreach ($line in $Output) {
    $candidate = $line.Trim()
    if ($candidate -match '^Removing\s+(.+)$') {
      $candidate = $matches[1].Trim().Trim('"')
    } else {
      $candidate = $candidate.Trim('"')
    }
    if (![System.IO.Path]::IsPathRooted($candidate)) {
      continue
    }

    $fullPath = [System.IO.Path]::GetFullPath($candidate)
    if (
      !$fullPath.Equals($allowed, [System.StringComparison]::OrdinalIgnoreCase) -and
      !$fullPath.StartsWith($allowedPrefix, [System.StringComparison]::OrdinalIgnoreCase)
    ) {
      throw "Safety check failed: Cargo dry run would remove a path outside target\debug: $fullPath"
    }
    $pathCount++
  }

  if ($ExpectedBytes -gt 0 -and $pathCount -eq 0) {
    throw "Safety check failed: Cargo dry run did not expose any paths for a non-empty debug directory."
  }
  return $pathCount
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

$resolvedTauriDir = [System.IO.Path]::GetFullPath($TauriDir).TrimEnd(
  [System.IO.Path]::DirectorySeparatorChar,
  [System.IO.Path]::AltDirectorySeparatorChar
)
$resolvedTargetDir = [System.IO.Path]::GetFullPath($TargetDir).TrimEnd(
  [System.IO.Path]::DirectorySeparatorChar,
  [System.IO.Path]::AltDirectorySeparatorChar
)
$tauriPrefix = $resolvedTauriDir + [System.IO.Path]::DirectorySeparatorChar
$targetPrefix = $resolvedTargetDir + [System.IO.Path]::DirectorySeparatorChar
if (!$resolvedTargetDir.StartsWith($tauriPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
  throw "Safety check failed: Cargo target directory is outside src-tauri."
}

Assert-NotReparsePoint $TargetDir "Cargo target directory"
Assert-NotReparsePoint $DebugDir "Cargo debug directory"

$activeBuildProcesses = Get-CimInstance Win32_Process | Where-Object {
  $name = $_.Name.ToLowerInvariant()
  if ($name -notin @("cargo.exe", "rustc.exe", "tauri.exe", "cargo-tauri.exe", "yuri-rewrite.exe")) {
    return $false
  }
  $executablePath = [string]$_.ExecutablePath
  $commandLine = [string]$_.CommandLine
  return $executablePath.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase) -or
    $commandLine.IndexOf($Root, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
}

if ($activeBuildProcesses) {
  $processList = ($activeBuildProcesses | ForEach-Object { "$($_.Name) (PID $($_.ProcessId))" }) -join ", "
  throw "Cargo/Tauri is currently using this project: $processList. Close the development app or build first."
}

$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($null -eq $cargoCommand) {
  throw "Cargo was not found in PATH. Install or activate the Rust toolchain first."
}
$CargoExecutable = [string]$cargoCommand.Source
if ([string]::IsNullOrWhiteSpace($CargoExecutable)) {
  $CargoExecutable = [string]$cargoCommand.Path
}
if ([string]::IsNullOrWhiteSpace($CargoExecutable)) {
  throw "Cargo was found, but its executable path could not be resolved."
}

$debugBefore = Get-DirectoryBytes $DebugDir
Write-Host "Debug cache before cleanup: $(Format-Size $debugBefore)"
Write-Host "Release artifacts will be preserved at: $ReleaseDir"

$cargoArguments = @(
  "clean",
  "--manifest-path", $ManifestPath,
  "--profile", "dev",
  "--target-dir", $TargetDir,
  "--color", "never"
)
$preflightArguments = $cargoArguments + @("--dry-run", "--verbose")
$previousErrorActionPreference = $ErrorActionPreference
try {
  # Windows PowerShell 5.1 wraps native stderr as non-terminating ErrorRecord objects.
  $ErrorActionPreference = "Continue"
  $preflightOutput = @(& $CargoExecutable @preflightArguments 2>&1 | ForEach-Object { $_.ToString() })
  $preflightExitCode = $LASTEXITCODE
} finally {
  $ErrorActionPreference = $previousErrorActionPreference
}
if ($preflightExitCode -ne 0) {
  $details = ($preflightOutput | Select-Object -Last 8) -join [Environment]::NewLine
  throw "Cargo clean preflight failed with exit code $preflightExitCode.$([Environment]::NewLine)$details"
}

$verifiedPathCount = Assert-CargoDryRunPaths $preflightOutput $DebugDir $debugBefore
$summary = $preflightOutput | Where-Object { $_ -match '^\s*Summary\s+' } | Select-Object -Last 1
Write-Host "Cargo preflight verified $verifiedPathCount paths inside target\debug."
if ($summary) {
  Write-Host $summary.Trim()
}

if ($DryRun) {
  Write-Host "Dry run completed; no files were deleted."
  return
}

if (!$PSCmdlet.ShouldProcess($DebugDir, "Delete Cargo dev-profile artifacts")) {
  Write-Host "Preview completed; no files were deleted."
  return
}

& $CargoExecutable @cargoArguments
$cleanExitCode = $LASTEXITCODE
if ($cleanExitCode -ne 0) {
  throw "cargo clean failed with exit code $cleanExitCode."
}

$debugAfter = Get-DirectoryBytes $DebugDir
$freed = [Math]::Max([int64]0, $debugBefore - $debugAfter)
Write-Host "Debug cache after cleanup: $(Format-Size $debugAfter)"
Write-Host "Disk space released: $(Format-Size $freed)"
Write-Host "Release artifacts and portable packages were not touched."
