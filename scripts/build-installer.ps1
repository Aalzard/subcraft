<#
.SYNOPSIS
    Builds the SubCraft Windows installer (SubCraft-<version>-setup.exe) with Inno Setup 6.

.DESCRIPTION
    Runs build-windows.ps1 (optimized exe + portable zip), then compiles installer\subcraft.iss
    with ISCC. The version comes from the root Cargo.toml. Output goes to dist\.
    Works in Windows PowerShell 5.1 and PowerShell 7.

    Needs Inno Setup 6: winget install JRSoftware.InnoSetup

.EXAMPLE
    .\scripts\build-installer.ps1
    .\scripts\build-installer.ps1 -SkipBuild                       # reuse the existing dist build
    .\scripts\build-installer.ps1 -Iscc 'D:\Tools\Inno\ISCC.exe'   # ISCC in a custom location
#>
param(
    [switch]$SkipBuild,
    [string]$Iscc
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Find-Iscc([string]$explicit) {
    $candidates = @(
        $explicit,
        $env:ISCC,
        (Get-Command 'iscc.exe' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
    )
    foreach ($c in $candidates) {
        if ($c -and (Test-Path $c)) { return $c }
    }
    return $null
}

Push-Location $root
try {
    $match = Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $match) { throw 'Could not read the version from Cargo.toml' }
    $version = $match.Matches[0].Groups[1].Value
    # Windows file versions are four numbers: 0.2.0-beta.1 -> 0.2.0.0
    $numeric = [regex]::Match($version, '^\d+(\.\d+){0,3}').Value
    $parts = @($numeric.Split('.'))
    while ($parts.Count -lt 4) { $parts += '0' }
    $versionInfo = $parts -join '.'

    $iscc = Find-Iscc $Iscc
    if (-not $iscc) {
        throw "Inno Setup 6 (ISCC.exe) not found. Install it with: winget install JRSoftware.InnoSetup  (or pass -Iscc <path>)"
    }

    $buildArgs = @{}
    if ($SkipBuild) { $buildArgs['SkipBuild'] = $true }
    & (Join-Path $PSScriptRoot 'build-windows.ps1') @buildArgs

    $dist = Join-Path $root 'dist'
    $stage = Join-Path $dist "SubCraft-$version-windows-x64"
    if (-not (Test-Path (Join-Path $stage 'SubCraft.exe'))) { throw "Staged build not found: $stage" }

    Write-Host ''
    Write-Host "Compiling installer with $iscc ..." -ForegroundColor Cyan
    & $iscc /Qp "/DAppVersion=$version" "/DAppVersionInfo=$versionInfo" "/DStageDir=$stage" `
        "/DRepoRoot=$root" "/DOutputDir=$dist" (Join-Path $root 'installer\subcraft.iss')
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed (exit code $LASTEXITCODE)" }

    $setup = Join-Path $dist "SubCraft-$version-setup.exe"
    if (-not (Test-Path $setup)) { throw "Installer not found after compiling: $setup" }
    $size = '{0:N1} MB' -f ((Get-Item $setup).Length / 1MB)
    Write-Host ''
    Write-Host "Done: $setup ($size)" -ForegroundColor Green
}
finally {
    Pop-Location
}
