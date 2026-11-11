param([switch]$Build)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
# Rust installed by rustup may not be in an already-open terminal's PATH.
$rustBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path -LiteralPath $rustBin) { $env:PATH = "$rustBin;$env:PATH" }
foreach ($tool in @('node', 'npm.cmd', 'cargo', 'cl.exe')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Missing $tool. Install Node.js LTS, Rust (MSVC), and Visual Studio Build Tools with Desktop development with C++. Run from Developer PowerShell for Visual Studio. WebView2 Runtime is also required."
    }
}
if (-not (Test-Path -LiteralPath 'node_modules')) {
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
if ($Build) { & npm.cmd run tauri:build } else { & npm.cmd run tauri:dev }
exit $LASTEXITCODE
