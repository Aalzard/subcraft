# Start SubCraft once, the way a user starts it, and fail unless it probes the GPU, opens its
# window, draws a frame, writes a screenshot and exits cleanly. On a GitHub runner the only
# adapter is the Microsoft Basic Render Driver (WARP): a Windows machine without a usable GPU.
# Usage: scripts/ci-smoke-start.ps1 -Executable target\debug\subcraft.exe -OutDir $env:RUNNER_TEMP\smoke
# The output folder receives screenshot.png, stdout/stderr and the data folder (startup.log,
# logs\, crashes\) so a failed run can be inspected from the CI artifact.
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutDir,
    [int]$TimeoutSeconds = 300
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$env:SUBCRAFT_DATA = Join-Path $OutDir 'data'
$env:SUBCRAFT_LIBRARY = Join-Path $OutDir 'library'
$shot = Join-Path $OutDir 'screenshot.png'
$exe = (Resolve-Path $Executable).Path
$process = Start-Process -FilePath $exe -ArgumentList '--demo', '--screenshot', "`"$shot`"" -PassThru -NoNewWindow `
    -RedirectStandardOutput (Join-Path $OutDir 'stdout.txt') -RedirectStandardError (Join-Path $OutDir 'stderr.txt')
$null = $process.Handle # Windows PowerShell 5.1 reports no ExitCode unless the handle was read before the exit
$exited = $process.WaitForExit($TimeoutSeconds * 1000)
if (-not $exited) { $process.Kill() }
Write-Output '--- stderr (tail)'
Get-Content (Join-Path $OutDir 'stderr.txt') -Tail 40 -ErrorAction SilentlyContinue
Write-Output '--- startup.log (tail)'
$log = Join-Path $env:SUBCRAFT_DATA 'startup.log'
if (Test-Path $log) { Get-Content $log -Tail 40 } else { Write-Output '(no startup.log)' }
$crashes = Join-Path $env:SUBCRAFT_DATA 'crashes'
if ((Test-Path $crashes) -and @(Get-ChildItem $crashes -File).Count) {
    Write-Output '--- crash report'
    Get-ChildItem $crashes -File | ForEach-Object { Get-Content $_.FullName }
    throw 'SubCraft wrote a crash report'
}
if (-not $exited) { throw "SubCraft did not exit within $TimeoutSeconds s" }
if ($process.ExitCode -ne 0) { throw "SubCraft exited with code $($process.ExitCode)" }
if (-not (Test-Path $shot)) { throw 'no screenshot was written' }
$bytes = [IO.File]::ReadAllBytes($shot)
if ($bytes.Length -lt 100 -or [BitConverter]::ToString($bytes[0..7]) -ne '89-50-4E-47-0D-0A-1A-0A') { throw 'screenshot.png is not a PNG' }
Write-Output 'SubCraft started, drew a frame and exited cleanly.'
