param(
    [ValidateRange(3, 120)]
    [int]$WarmupSeconds = 8
)
$ErrorActionPreference = "Stop"
$desktopRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$exe = Join-Path $desktopRoot "target\release\sonar-desktop.exe"
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Build the native release executable first." }
$process = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
try {
    Start-Sleep -Seconds $WarmupSeconds
    $process.Refresh()
    if ($process.HasExited) { throw "Desktop preview exited with code $($process.ExitCode)." }
    [pscustomobject]@{
        Scenario = "Login screen, idle"
        WarmupSeconds = $WarmupSeconds
        WorkingSetMiB = [math]::Round($process.WorkingSet64 / 1MB, 1)
        PrivateMiB = [math]::Round($process.PrivateMemorySize64 / 1MB, 1)
    }
} finally {
    $process.Refresh()
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}
