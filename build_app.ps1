$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'run_dev.ps1') -Build
exit $LASTEXITCODE
