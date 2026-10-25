# Build subcraft-sbsar-bake (Substance Materials SDK required locally).
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$SdkDefault = Join-Path $Root "_sdk_extract\substance-win-x64-v9.6.2-bf39d392"
$Sdk = if ($env:SUBSTANCE_SDK_ROOT) { $env:SUBSTANCE_SDK_ROOT } else { $SdkDefault }

if (-not (Test-Path $Sdk)) {
    Write-Error "Substance SDK not found at $Sdk. Set SUBSTANCE_SDK_ROOT or extract SDK under _sdk_extract."
}

$BuildDir = Join-Path $Root "build\sbsar-bake"
$OutDir = Join-Path $Root "bin\sbsar-bake"
New-Item -ItemType Directory -Force -Path $BuildDir, $OutDir | Out-Null

cmake -S (Join-Path $Root "native\sbsar-bake") -B $BuildDir -DSUBSTANCE_SDK_ROOT="$Sdk" -A x64
cmake --build $BuildDir --config Release

$Exe = Join-Path $OutDir "subcraft-sbsar-bake.exe"
if (-not (Test-Path $Exe)) {
    Write-Error "Build finished but $Exe is missing."
}

$SdkBin = Join-Path $Sdk "bin\release"
Copy-Item -Path (Join-Path $SdkBin "*.dll") -Destination $OutDir -Force
Write-Host "SBSAR host ready: $Exe"
Write-Host "Set SUBCRAFT_SBSAR_HOST=$Exe or copy next to subcraft.exe"
