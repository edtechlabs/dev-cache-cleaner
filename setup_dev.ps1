$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
. (Join-Path $PSScriptRoot 'dev_env.ps1')
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    throw 'Install Visual Studio Build Tools with Desktop development with C++ and a Windows SDK first.'
}
$localRustup = Join-Path $env:CARGO_HOME 'bin\rustup.exe'
if (-not (Test-Path -LiteralPath $localRustup)) {
    $downloadDir = Join-Path $PSScriptRoot '.tools\downloads'
    New-Item -ItemType Directory -Force -Path $downloadDir | Out-Null
    $installer = Join-Path $downloadDir 'rustup-init.exe'
    Invoke-WebRequest -UseBasicParsing 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile $installer
    & $installer -y --no-modify-path --default-host x86_64-pc-windows-msvc --default-toolchain stable --profile minimal
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
& $localRustup toolchain install stable --profile minimal --component rustfmt --component clippy
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $localRustup default stable
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& cargo --version
& rustc --version
& npm.cmd ci
exit $LASTEXITCODE
