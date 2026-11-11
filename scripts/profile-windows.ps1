param(
    [int]$Seconds = 12,
    [string]$Executable = (Join-Path $PSScriptRoot '..\src-tauri\target\release\dev-cache-cleaner.exe')
)
$ErrorActionPreference = 'Stop'
if ($Seconds -lt 3 -or $Seconds -gt 120) { throw 'Seconds must be between 3 and 120.' }
if (-not ('WindowProbe' -as [type])) {
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WindowProbe {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SendMessageTimeout(IntPtr window, uint message,
        UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out UIntPtr result);
}
'@
}
$probeProcess = Start-Process -FilePath $Executable -WindowStyle Hidden -PassThru
try {
    Start-Sleep -Seconds 2
    $probeProcess.Refresh()
    $window = $probeProcess.MainWindowHandle
    if ($window -eq [IntPtr]::Zero) { throw 'App did not create a window.' }
    $samples = [Collections.Generic.List[double]]::new()
    $failures = 0
    $errors = @{}
    $duration = [Diagnostics.Stopwatch]::StartNew()
    while ($duration.Elapsed.TotalSeconds -lt $Seconds) {
        $result = [UIntPtr]::Zero
        $sampleTimer = [Diagnostics.Stopwatch]::StartNew()
        # WM_NULL measures dispatch latency without changing application state.
        $response = [WindowProbe]::SendMessageTimeout($window, 0, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 1000, [ref]$result)
        $sampleTimer.Stop()
        if ($response -eq [IntPtr]::Zero) {
            $failures++
            $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error().ToString()
            $errors[$code] = 1 + $errors[$code]
        }
        $samples.Add($sampleTimer.Elapsed.TotalMilliseconds)
        Start-Sleep -Milliseconds 20
    }
    $ordered = @($samples | Sort-Object)
    [pscustomobject]@{
        executable = (Resolve-Path -LiteralPath $Executable).Path
        seconds = $Seconds
        sampleCount = $samples.Count
        medianMs = [math]::Round($ordered[[int][math]::Floor($ordered.Count * 0.5)], 3)
        p95Ms = [math]::Round($ordered[[int][math]::Floor(($ordered.Count - 1) * 0.95)], 3)
        maxMs = [math]::Round($ordered[-1], 3)
        over50Ms = @($samples | Where-Object { $_ -ge 50 }).Count
        timeoutCount = [int]$errors['1460']
        failedPings = $failures
        win32Errors = $errors
    } | ConvertTo-Json
} finally {
    if (-not $probeProcess.HasExited) { Stop-Process -Id $probeProcess.Id }
}
