# Fail-closed behavior and byte-exact checksums without executing a release binary.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('subcraft-release-test-' + [guid]::NewGuid().ToString('N'))
$count = 0
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message }; $script:count++ }
function Must-Fail([scriptblock]$Action, [string]$Pattern) {
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $_.Exception.Message -like $Pattern }
    Assert $failed "Expected rejection: $Pattern"
}
try {
    New-Item -ItemType Directory -Path $temporary | Out-Null
    $a = Join-Path $temporary 'SubCraft-1.2.3-setup.exe'
    $b = Join-Path $temporary 'SubCraft-1.2.3-windows-x64.zip'
    [IO.File]::WriteAllBytes($a, [byte[]](0..255))
    [IO.File]::WriteAllText($b, 'portable fixture')
    Set-Content -LiteralPath (Join-Path $temporary 'notes.txt') 'not a package'
    & (Join-Path $repo 'scripts/write-release-checksums.ps1') -Directory $temporary | Out-Null
    $sums = Join-Path $temporary 'SHA256SUMS.txt'
    $lines = @(Get-Content -LiteralPath $sums)
    Assert ($lines.Count -eq 2) 'Only packages belong in checksums.'
    Assert ($lines[0] -ceq ((Get-FileHash -LiteralPath $a).Hash.ToLowerInvariant() + '  SubCraft-1.2.3-setup.exe')) 'Installer checksum mismatch.'
    Assert ($lines[1] -ceq ((Get-FileHash -LiteralPath $b).Hash.ToLowerInvariant() + '  SubCraft-1.2.3-windows-x64.zip')) 'ZIP checksum mismatch.'
    $before = Get-Content -LiteralPath $sums -Raw
    & (Join-Path $repo 'scripts/write-release-checksums.ps1') -Directory $temporary | Out-Null
    Assert ((Get-Content -LiteralPath $sums -Raw) -ceq $before) 'Checksum re-run must be deterministic.'
    $bytes = [IO.File]::ReadAllBytes($sums)
    Assert ($bytes[0] -ne 239 -and $bytes[-1] -eq 10) 'Checksum format must be BOM-free with trailing newline.'
    $empty = Join-Path $temporary 'empty'
    New-Item -ItemType Directory -Path $empty | Out-Null
    Must-Fail { & (Join-Path $repo 'scripts/write-release-checksums.ps1') -Directory $empty } 'No SubCraft release artifacts*'
    Must-Fail { & (Join-Path $repo 'scripts/sign-windows.ps1') -Path $a -VerifyOnly } 'A trusted, timestamped signature is required*'
    Must-Fail { & (Join-Path $repo 'scripts/sign-windows.ps1') -Path $a -CertificateThumbprint ('0' * 40) -TimestampUrl 'file:///C:/fake' } 'Expected an absolute HTTP(S) timestamp endpoint*'
    $report = Join-Path $empty 'release-security-windows.json'
    Set-Content -LiteralPath $report '{"result":"old success"}'
    Must-Fail { & (Join-Path $repo 'scripts/test-release-windows.ps1') -Directory $empty -Version 1.2.3 -Commit ('a' * 40) } 'Missing release artifact*'
    Assert (-not (Test-Path -LiteralPath $report)) 'Failed gate must remove stale success.'
    Must-Fail { & (Join-Path $repo 'scripts/test-release-windows.ps1') -Directory $temporary -Version 1.2.3 -Commit ('a' * 40) } '*Central Directory*'
    $case = Join-Path $temporary 'case'
    Must-Fail { & (Join-Path $repo 'scripts/new-sandbox-check.ps1') -Installer $a -ExpectedSha256 ('0' * 64) -Destination $case } 'Installer hash mismatch*'
    Assert (-not (Test-Path -LiteralPath $case)) 'Hash failure must not prepare a case.'
    & (Join-Path $repo 'scripts/new-sandbox-check.ps1') -Installer $a -ExpectedSha256 (Get-FileHash -LiteralPath $a).Hash -Destination $case | Out-Null
    [xml]$config = Get-Content -LiteralPath (Join-Path $case 'SubCraft-offline.wsb') -Raw
    foreach ($option in 'Networking','vGPU','AudioInput','VideoInput','PrinterRedirection','ClipboardRedirection') { Assert ($config.Configuration.$option -eq 'Disable') "Unsafe sandbox option: $option" }
    Assert ($config.Configuration.LogonCommand.Command -eq 'powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File C:\SubCraftInput\sandbox-guest-check.ps1') 'Guest bootstrap must run only the mapped harness with a guest-process script policy.'
    $maps = @($config.Configuration.MappedFolders.MappedFolder)
    Assert ($maps.Count -eq 2 -and $maps[0].ReadOnly -eq 'true' -and $maps[1].ReadOnly -eq 'false') 'Unsafe sandbox mappings.'
    Assert ((Get-FileHash -LiteralPath (Join-Path $case 'input/setup.exe')).Hash -eq (Get-FileHash -LiteralPath $a).Hash) 'Sandbox input must match exactly.'
    Must-Fail { & (Join-Path $repo 'scripts/new-sandbox-check.ps1') -Installer $a -ExpectedSha256 (Get-FileHash -LiteralPath $a).Hash -Destination $case } 'Choose a new*'
    Must-Fail { & (Join-Path $repo 'scripts/sandbox-guest-check.ps1') } 'Guest-only script*'
    # CI fallback without Defender: the report must say no antivirus scan ran, never "no threats".
    $skip = Join-Path $temporary 'skip-av'
    $skipStage = Join-Path $skip 'SubCraft-1.2.3-windows-x64'
    New-Item -ItemType Directory -Path $skipStage | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $skipStage 'SubCraft.exe'), [byte[]](0..255))
    Copy-Item -LiteralPath $a -Destination (Join-Path $skip 'SubCraft-1.2.3-setup.exe')
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    # Explicit entry name: .NET Framework's CreateFromDirectory would write backslashes.
    $skipZip = [IO.Compression.ZipFile]::Open((Join-Path $skip 'SubCraft-1.2.3-windows-x64.zip'), [IO.Compression.ZipArchiveMode]::Create)
    try { [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($skipZip, (Join-Path $skipStage 'SubCraft.exe'), 'SubCraft-1.2.3-windows-x64/SubCraft.exe') | Out-Null } finally { $skipZip.Dispose() }
    & (Join-Path $repo 'scripts/test-release-windows.ps1') -Directory $skip -Version 1.2.3 -Commit ('a' * 40) -SkipAntivirus 3>$null | Out-Null
    $skipReport = Get-Content -LiteralPath (Join-Path $skip 'release-security-windows.json') -Raw | ConvertFrom-Json
    Assert ($skipReport.result -like 'antivirus scan skipped*') 'Skipped antivirus scan must be recorded.'
    Assert (@($skipReport.scanned).Count -eq 3) 'Skipped scan must still record artifact hashes.'
    Assert (@(Get-Content -LiteralPath (Join-Path $skip 'SHA256SUMS.txt')).Count -eq 2) 'Skipped scan must still write checksums.'
    Assert (@(Get-ChildItem -LiteralPath $skip -Filter '*.defender.txt').Count -eq 0) 'No Defender logs without a Defender scan.'
    Write-Output "Release security tests passed: $count assertions."
} finally {
    $full = [IO.Path]::GetFullPath($temporary)
    $prefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if ($full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $full).StartsWith('subcraft-release-test-')) { Remove-Item -LiteralPath $full -Recurse -Force }
}
