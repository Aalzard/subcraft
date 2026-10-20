# Prepare an OFFLINE smoke test, not a malware-analysis verdict. Never launches on host.
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedSha256,
    [Parameter(Mandatory)][string]$Destination,
    [switch]$Launch
)
$ErrorActionPreference = 'Stop'
$file = (Get-Item -LiteralPath $Installer).FullName
if ((Get-FileHash -LiteralPath $file).Hash -ne $ExpectedSha256) { throw 'Installer hash mismatch; sandbox case was not created.' }
if (Test-Path -LiteralPath $Destination) { throw 'Choose a new, empty case directory.' }
$case = [IO.Path]::GetFullPath($Destination)
$inputDir = Join-Path $case 'input'
$results = Join-Path $case 'results'
New-Item -ItemType Directory -Path $inputDir,$results -Force | Out-Null
Copy-Item -LiteralPath $file -Destination (Join-Path $inputDir 'setup.exe')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sandbox-guest-check.ps1') -Destination $inputDir
Set-Content -LiteralPath (Join-Path $inputDir 'expected-sha256.txt') -Value $ExpectedSha256
$inputXml = [Security.SecurityElement]::Escape($inputDir)
$resultsXml = [Security.SecurityElement]::Escape($results)
$xml = @"
<Configuration>
  <Networking>Disable</Networking>
  <vGPU>Disable</vGPU>
  <AudioInput>Disable</AudioInput>
  <VideoInput>Disable</VideoInput>
  <PrinterRedirection>Disable</PrinterRedirection>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <MemoryInMB>4096</MemoryInMB>
  <MappedFolders>
    <MappedFolder><HostFolder>$inputXml</HostFolder><SandboxFolder>C:\SubCraftInput</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>$resultsXml</HostFolder><SandboxFolder>C:\SubCraftResults</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>
  </MappedFolders>
  <LogonCommand><Command>powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File C:\SubCraftInput\sandbox-guest-check.ps1</Command></LogonCommand>
</Configuration>
"@
$config = Join-Path $case 'SubCraft-offline.wsb'
Set-Content -LiteralPath $config -Value $xml -Encoding utf8
Write-Output "Prepared offline case: $config"
if ($Launch) {
    $sandbox = Join-Path $env:WINDIR 'System32\WindowsSandbox.exe'
    if (-not (Test-Path -LiteralPath $sandbox)) { throw "Windows Sandbox is unavailable. Prepared only: $config" }
    Start-Process -FilePath $sandbox -ArgumentList ('"' + $config + '"') -WindowStyle Hidden
    Write-Output 'Sandbox launched; completion requires results/result.json, not this launcher output.'
}
