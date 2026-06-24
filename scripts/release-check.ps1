$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
chcp 65001 | Out-Null

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $Root
try {
  npm run verify
  if ($LASTEXITCODE -ne 0) { throw "Verification failed." }

  npm run package:portable:fresh
  if ($LASTEXITCODE -ne 0) { throw "Portable packaging failed." }

  $Version = (Get-Content -Encoding UTF8 (Join-Path $Root "package.json") | ConvertFrom-Json).version
  $ZipPath = Join-Path $Root "portable\YuriRewrite-v$Version-windows-x64.zip"
  if (!(Test-Path -LiteralPath $ZipPath)) {
    throw "Expected portable ZIP was not created: $ZipPath"
  }

  Write-Host "Release check completed. Portable ZIP: $ZipPath"
  Write-Host "Git status:"
  git status -sb
  if ($LASTEXITCODE -ne 0) { throw "git status failed." }
} finally {
  Pop-Location
}
