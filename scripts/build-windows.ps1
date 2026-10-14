<#
.SYNOPSIS
    Builds SubCraft for Windows and packages it as a ready-to-run zip.

.DESCRIPTION
    Produces dist\SubCraft-<version>-windows-x64\ (SubCraft.exe, README, licenses) and
    dist\SubCraft-<version>-windows-x64.zip. The exe opens without a console window.
    Works in Windows PowerShell 5.1 and PowerShell 7.

.EXAMPLE
    .\scripts\build-windows.ps1
    .\scripts\build-windows.ps1 -SkipBuild    # package an existing dist build
#>
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $match = Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $match) { throw 'Could not read the version from Cargo.toml' }
    $version = $match.Matches[0].Groups[1].Value
    $onWindows = ($env:OS -eq 'Windows_NT')
    $exeName = if ($onWindows) { 'subcraft.exe' } else { 'subcraft' }

    if (-not $SkipBuild) {
        Write-Host "Building SubCraft $version (dist profile)..." -ForegroundColor Cyan
        cargo build --profile dist -p subcraft --features windows-gui
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)" }
    }

    $exe = Join-Path 'target\dist' $exeName
    if (-not (Test-Path $exe)) { throw "Build output not found: $exe" }

    $name = "SubCraft-$version-windows-x64"
    $dist = Join-Path $root 'dist'
    $stage = Join-Path $dist $name
    $zip = Join-Path $dist "$name.zip"
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    if (Test-Path $zip) { Remove-Item $zip -Force }
    New-Item -ItemType Directory -Path $stage | Out-Null

    $outExe = if ($onWindows) { 'SubCraft.exe' } else { 'SubCraft' }
    Copy-Item $exe (Join-Path $stage $outExe)
    foreach ($f in 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE') {
        Copy-Item $f $stage
    }
    $fonts = Join-Path $stage 'third-party'
    New-Item -ItemType Directory -Path $fonts | Out-Null
    Copy-Item 'crates\ui\assets\fonts\*-LICENSE.txt' $fonts

    Compress-Archive -Path $stage -DestinationPath $zip
    $size = '{0:N1} MB' -f ((Get-Item $zip).Length / 1MB)
    Write-Host ''
    Write-Host "Done: $zip ($size)" -ForegroundColor Green
    Write-Host "Run it: $(Join-Path $stage $outExe)"
}
finally {
    Pop-Location
}
