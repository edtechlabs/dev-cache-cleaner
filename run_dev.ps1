param([switch]$Build)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
. (Join-Path $PSScriptRoot 'dev_env.ps1')
foreach ($tool in @('node', 'npm.cmd', 'cargo', 'cl.exe')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Missing $tool. Run setup_dev.bat first. Node.js, Visual Studio C++ Build Tools, and WebView2 Runtime are required."
    }
}
if (-not (Test-Path -LiteralPath 'node_modules')) {
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
if ($Build) { & npm.cmd run tauri:build } else { & npm.cmd run tauri:dev }
exit $LASTEXITCODE
