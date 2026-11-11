# Dot-source this file to use this repository's isolated Rust installation.
$env:CARGO_HOME = Join-Path $PSScriptRoot '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $PSScriptRoot '.tools\rustup'
$env:PATH = "$(Join-Path $env:CARGO_HOME 'bin');$env:PATH"
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vsPath) {
            Import-Module (Join-Path $vsPath 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
            Enter-VsDevShell -VsInstallPath $vsPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
        }
    }
}
