# Static release gate; never launches the downloaded installer/application.
param(
    [Parameter(Mandatory)][string]$Directory,
    [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?$')][string]$Version,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$Commit,
    [switch]$RequireSignature
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$dir = (Get-Item -LiteralPath $Directory).FullName
$reportPath = Join-Path $dir 'release-security-windows.json'
# A failed re-run must never leave an earlier passing report behind.
if (Test-Path -LiteralPath $reportPath) { Remove-Item -LiteralPath $reportPath }
$installer = Join-Path $dir "SubCraft-$Version-setup.exe"
$portable = Join-Path $dir "SubCraft-$Version-windows-x64.zip"
foreach ($file in @($installer, $portable)) { if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing release artifact: $file" } }
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('subcraft-release-scan-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $binary = Join-Path $temporary 'SubCraft.exe'
    $zip = [IO.Compression.ZipFile]::OpenRead($portable)
    try {
        $entries = @($zip.Entries | Where-Object { $_.FullName -eq "SubCraft-$Version-windows-x64/SubCraft.exe" })
        if ($entries.Count -ne 1 -or $entries[0].Length -lt 64 -or $entries[0].Length -gt 512MB) { throw 'Missing, duplicate or oversized application in portable archive.' }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $binary)
    } finally { $zip.Dispose() }
    $signatures = @()
    $publisher = $null
    foreach ($file in @($installer, $binary)) {
        if ($RequireSignature) { & (Join-Path $PSScriptRoot 'sign-windows.ps1') -Path $file -VerifyOnly }
        $sig = Get-AuthenticodeSignature -LiteralPath $file
        if ($RequireSignature) {
            $subject = $sig.SignerCertificate.Subject
            if ($publisher -and $subject -ne $publisher) { throw 'Installer and application publishers do not match.' }
            $publisher = $subject
        }
        $signatures += @{ file = [IO.Path]::GetFileName($file); status = $sig.Status.ToString(); timestamped = [bool]$sig.TimeStamperCertificate }
    }
    $status = Get-MpComputerStatus
    if (-not $status.AntivirusEnabled -or -not $status.RealTimeProtectionEnabled) { throw 'Microsoft Defender must be active; no release gate was performed.' }
    if ($status.AntivirusSignatureLastUpdated -lt (Get-Date).AddDays(-2)) { throw 'Defender signatures are older than two days; update them before testing.' }
    $scanner = Get-ChildItem -LiteralPath "$env:ProgramData\Microsoft\Windows Defender\Platform" -Directory |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+-\d+$' } | Sort-Object { [version]($_.Name -replace '-.*$','') } -Descending |
        ForEach-Object { Join-Path $_.FullName 'MpCmdRun.exe' } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $scanner) { throw 'Defender scanner was not found.' }
    $scanned = foreach ($file in @($installer, $portable, $binary)) {
        $before = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
        $scanOutput = & $scanner -Scan -ScanType 3 -File $file -DisableRemediation 2>&1
        $code = $LASTEXITCODE
        $scanOutput | Out-File -LiteralPath (Join-Path $dir ([IO.Path]::GetFileName($file) + '.defender.txt')) -Encoding utf8
        if ($code -ne 0) { throw "Defender did not pass ($code): $file" }
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $before) { throw 'Artifact changed during scanning.' }
        @{ file = [IO.Path]::GetFileName($file); sha256 = $before.ToLowerInvariant(); exit_code = $code }
    }
    @{ schema = 1; result = 'no threats detected'; scope = 'static antivirus and optional Authenticode checks; not isolated runtime analysis'; commit = $Commit; version = $Version; checked_at_utc = [DateTime]::UtcNow.ToString('o'); signatures_required = [bool]$RequireSignature; signatures = $signatures; defender_signature_version = $status.AntivirusSignatureVersion; scanned = @($scanned) } |
        ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $reportPath -Encoding utf8
    & (Join-Path $PSScriptRoot 'write-release-checksums.ps1') -Directory $dir
    Write-Output "Release static checks passed. Report: $reportPath"
} finally {
    $full = [IO.Path]::GetFullPath($temporary)
    $prefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if ($full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $full).StartsWith('subcraft-release-scan-')) { Remove-Item -LiteralPath $full -Recurse -Force }
}
