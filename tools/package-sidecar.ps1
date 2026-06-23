# Build the yfinance sidecar into a single executable and place it as a Tauri
# `externalBin` (named with the Rust target triple) so `cargo tauri build`
# bundles it. Run this before `cargo tauri build`.
#
#   pwsh tools/package-sidecar.ps1
#   cargo tauri build --bundles nsis
#
# The output (src-tauri/binaries/, sidecar/build/) is gitignored — it is a
# build artifact rebuilt by this script per platform.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$triple = (& rustc -vV | Select-String 'host:').ToString().Split(' ')[-1]
$ext = if ($env:OS -eq 'Windows_NT') { '.exe' } else { '' }

Write-Host "Building sidecar (PyInstaller) for $triple ..."
uv run --with pyinstaller --project "$root/sidecar" pyinstaller `
    --onefile --name fetch --collect-all yfinance `
    --distpath "$root/sidecar/build/dist" `
    --workpath "$root/sidecar/build/work" `
    --specpath "$root/sidecar/build" `
    "$root/sidecar/fetch.py"

$dest = "$root/src-tauri/binaries"
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item "$root/sidecar/build/dist/fetch$ext" "$dest/fetch-$triple$ext" -Force
Write-Host "Placed $dest/fetch-$triple$ext"
