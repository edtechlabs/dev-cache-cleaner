# Build the Windows installers and collect them in .\deploy
#   .\deploy_windows.ps1            # release build, then copy installers
#   .\deploy_windows.ps1 -NoBuild   # only copy the last release build
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
if (-not $NoBuild) {
    & (Join-Path $PSScriptRoot 'run_dev.ps1') -Build
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
$release = Join-Path $PSScriptRoot 'src-tauri\target\release'
$deploy = Join-Path $PSScriptRoot 'deploy'
New-Item -ItemType Directory -Force -Path $deploy | Out-Null
$files = @(
    Get-ChildItem (Join-Path $release 'bundle\msi') -Filter *.msi -ErrorAction SilentlyContinue
    Get-ChildItem (Join-Path $release 'bundle\nsis') -Filter *-setup.exe -ErrorAction SilentlyContinue
    Get-Item (Join-Path $release 'cache-cleaner.exe') -ErrorAction SilentlyContinue
)
if (-not $files) { throw 'No installers found. Run without -NoBuild first.' }
foreach ($f in $files) {
    Copy-Item -LiteralPath $f.FullName -Destination $deploy -Force
    $hash = (Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash
    '{0,-52} {1,6:N1} MB  sha256 {2}' -f $f.Name, ($f.Length / 1MB), $hash
}
Write-Host "Installers are in $deploy"
