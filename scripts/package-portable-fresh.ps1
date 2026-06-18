$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
chcp 65001 | Out-Null

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$PortableDir = Join-Path $Root "portable"
$ResolvedRoot = [System.IO.Path]::GetFullPath($Root)
$ResolvedPortable = [System.IO.Path]::GetFullPath($PortableDir)
if (!$ResolvedPortable.StartsWith($ResolvedRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
  throw "Portable directory escaped the workspace."
}

New-Item -ItemType Directory -Path $PortableDir -Force | Out-Null
Get-ChildItem -LiteralPath $PortableDir -Filter "*.zip" -File -ErrorAction SilentlyContinue |
  Remove-Item -Force

Push-Location $Root
try {
  npm run tauri:build
  if ($LASTEXITCODE -ne 0) { throw "Tauri build failed." }

  npm run package:portable
  if ($LASTEXITCODE -ne 0) { throw "Portable packaging failed." }
} finally {
  Pop-Location
}

$Version = (Get-Content -Encoding UTF8 (Join-Path $Root "package.json") | ConvertFrom-Json).version
$ZipPath = Join-Path $PortableDir "YuriRewrite-v$Version-windows-x64.zip"
if (!(Test-Path -LiteralPath $ZipPath)) {
  throw "Expected portable ZIP was not created: $ZipPath"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$Archive = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
try {
  $Entries = @($Archive.Entries | Where-Object { $_.FullName -and !$_.FullName.EndsWith("/") } |
    ForEach-Object { $_.FullName } | Sort-Object)
} finally {
  $Archive.Dispose()
}
$Expected = @("README.md", "Yuri Rewrite.exe")
if (($Entries -join "|") -ne ($Expected -join "|")) {
  throw "Portable ZIP contents are invalid: $($Entries -join ', ')"
}

$Hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ZipPath).Hash
Write-Host "Portable package verified: $ZipPath"
Write-Host "SHA-256: $Hash"
