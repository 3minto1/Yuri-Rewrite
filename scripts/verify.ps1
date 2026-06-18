$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
chcp 65001 | Out-Null

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $Root
try {
  npm test
  if ($LASTEXITCODE -ne 0) { throw "Frontend tests failed." }

  npm run build
  if ($LASTEXITCODE -ne 0) { throw "Frontend build failed." }

  cargo test --manifest-path .\src-tauri\Cargo.toml
  if ($LASTEXITCODE -ne 0) { throw "Rust tests failed." }

  cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
  if ($LASTEXITCODE -ne 0) { throw "Clippy failed." }

  git diff --check
  if ($LASTEXITCODE -ne 0) { throw "git diff --check failed." }
} finally {
  Pop-Location
}
