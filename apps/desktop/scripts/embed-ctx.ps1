# Builds the ctx CLI in release mode and copies it to src-tauri\binaries as
# the Tauri sidecar (ctx-<target-triple>.exe). Run from any directory:
#   powershell -ExecutionPolicy Bypass -File apps\desktop\scripts\embed-ctx.ps1
$ErrorActionPreference = "Stop"
$here = Resolve-Path (Join-Path $PSScriptRoot "..")
$root = Resolve-Path (Join-Path $here "..\..")
$target = (rustc -vV | Select-String '^host: ').ToString().Substring(6).Trim()
Push-Location $root
try { cargo build --release -p ctx-cli } finally { Pop-Location }
New-Item -ItemType Directory -Force -Path (Join-Path $here "src-tauri\binaries") | Out-Null
Copy-Item (Join-Path $root "target\release\ctx.exe") (Join-Path $here "src-tauri\binaries\ctx-$target.exe") -Force
Write-Host "embedded src-tauri\binaries\ctx-$target.exe"
