$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ReleaseDir = Join-Path $Root "src-tauri\target\release"
$Exe = Join-Path $ReleaseDir "yuri-rewrite.exe"
$OutDir = Join-Path $Root "portable"
$Version = (Get-Content (Join-Path $Root "package.json") | ConvertFrom-Json).version
$PackageDir = Join-Path $OutDir "YuriRewrite-v$Version-windows-x64"
$StagingDir = Join-Path $OutDir ".staging-$([guid]::NewGuid().ToString('N'))"
$ZipPath = Join-Path $OutDir "YuriRewrite-v$Version-windows-x64.zip"

if (!(Test-Path $Exe)) {
  throw "Release executable not found. Run npm run tauri:build first."
}

New-Item -ItemType Directory -Path $StagingDir | Out-Null
Copy-Item -LiteralPath $Exe -Destination $StagingDir
Copy-Item -LiteralPath (Join-Path $Root "README.md") -Destination $StagingDir

if (Test-Path $ZipPath) {
  Remove-Item -LiteralPath $ZipPath -Force
}

Compress-Archive -Path (Join-Path $StagingDir "*") -DestinationPath $ZipPath

try {
  if (Test-Path $PackageDir) {
    Remove-Item -LiteralPath $PackageDir -Recurse -Force
  }
  Move-Item -LiteralPath $StagingDir -Destination $PackageDir
} catch {
  Write-Warning "Portable folder is in use; zip was created from staging, but folder refresh was skipped."
  Remove-Item -LiteralPath $StagingDir -Recurse -Force
}

Write-Host "Portable package created: $ZipPath"
