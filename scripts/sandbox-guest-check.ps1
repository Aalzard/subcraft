# Only the Windows Sandbox guest runs this file. Guard before any application launch.
$ErrorActionPreference = 'Stop'
if ($env:USERNAME -ne 'WDAGUtilityAccount' -or -not (Test-Path -LiteralPath 'C:\SubCraftInput\expected-sha256.txt')) { throw 'Guest-only script: use the generated Windows Sandbox configuration.' }
$result = @{ result = 'failed'; scope = 'offline install/render/uninstall smoke test; not a malware verdict'; started_at_utc = [DateTime]::UtcNow.ToString('o') }
function Run-Bounded([string]$Executable, [string]$Arguments, [int]$Seconds) {
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit($Seconds * 1000)) { $process.Kill(); throw "Timed out: $Executable" }
    if ($process.ExitCode -ne 0) { throw "Exit $($process.ExitCode): $Executable" }
}
try {
    $expected = (Get-Content -LiteralPath 'C:\SubCraftInput\expected-sha256.txt' -Raw).Trim()
    $actual = (Get-FileHash -LiteralPath 'C:\SubCraftInput\setup.exe').Hash
    if ($actual -ne $expected) { throw 'Guest installer hash mismatch.' }
    $result.installer_sha256 = $actual.ToLowerInvariant()
    Copy-Item -LiteralPath 'C:\SubCraftInput\setup.exe' -Destination 'C:\Users\WDAGUtilityAccount\Desktop\setup.exe'
    Run-Bounded 'C:\Users\WDAGUtilityAccount\Desktop\setup.exe' '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER /DIR="C:\SubCraftTest" /LOG="C:\SubCraftResults\install.log"' 180
    if (-not (Test-Path -LiteralPath 'C:\SubCraftTest\SubCraft.exe')) { throw 'Installed executable is missing.' }
    $result.installed = $true
    Run-Bounded 'C:\SubCraftTest\SubCraft.exe' '--screenshot C:\SubCraftResults\viewport.png' 180
    $png = [IO.File]::ReadAllBytes('C:\SubCraftResults\viewport.png')
    if ($png.Length -lt 100 -or [BitConverter]::ToString($png[0..7]) -ne '89-50-4E-47-0D-0A-1A-0A') { throw 'Application did not produce a PNG screenshot.' }
    $result.rendered = $true
    $dataDir = Join-Path $env:LOCALAPPDATA 'SubCraft'
    if (Test-Path -LiteralPath (Join-Path $dataDir 'crashes')) {
        $crashes = @(Get-ChildItem -LiteralPath (Join-Path $dataDir 'crashes') -File)
        if ($crashes.Count) { throw 'Application produced a crash report; inspect the guest before closing.' }
    }
    Run-Bounded 'C:\SubCraftTest\unins000.exe' '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' 120
    if (Test-Path -LiteralPath 'C:\SubCraftTest\SubCraft.exe') { throw 'Uninstall left the application executable behind.' }
    $result.uninstalled = $true
    $result.result = 'passed'
} catch { $result.error = $_.Exception.Message }
$result.finished_at_utc = [DateTime]::UtcNow.ToString('o')
$result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath 'C:\SubCraftResults\result.json' -Encoding utf8
# Leave the guest open for inspection; only the dedicated results directory is writable on host.
