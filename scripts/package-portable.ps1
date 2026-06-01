$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ReleaseDir = Join-Path $Root "src-tauri\target\release"
$Exe = Join-Path $ReleaseDir "yuri-rewrite.exe"
$OutDir = Join-Path $Root "portable"
$Version = (Get-Content (Join-Path $Root "package.json") | ConvertFrom-Json).version
$PackageDir = Join-Path $OutDir "YuriRewrite-v$Version-windows-x64"
$ZipPath = Join-Path $OutDir "YuriRewrite-v$Version-windows-x64.zip"

if (!(Test-Path $Exe)) {
  throw "Release executable not found. Run npm run tauri:build first."
}

if (Test-Path $PackageDir) {
  Remove-Item -LiteralPath $PackageDir -Recurse -Force
}

New-Item -ItemType Directory -Path $PackageDir | Out-Null
Copy-Item -LiteralPath $Exe -Destination $PackageDir
Copy-Item -LiteralPath (Join-Path $Root "README.md") -Destination $PackageDir

if (Test-Path $ZipPath) {
  Remove-Item -LiteralPath $ZipPath -Force
}

Compress-Archive -Path (Join-Path $PackageDir "*") -DestinationPath $ZipPath
Write-Host "Portable package created: $ZipPath"
