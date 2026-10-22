param([Parameter(Mandatory)][string]$Directory)
$ErrorActionPreference = 'Stop'
$dir = (Get-Item -LiteralPath $Directory).FullName
# Only distribution artifacts: reports/checksum files must not hash themselves.
$files = @(Get-ChildItem -LiteralPath $dir -File | Where-Object { $_.Name -match '^SubCraft-[0-9][A-Za-z0-9.+-]*(-setup\.exe|-windows-x64\.zip|-linux-x86_64\.tar\.gz)$' } | Sort-Object Name)
if (-not $files.Count) { throw 'No SubCraft release artifacts found.' }
$lines = foreach ($file in $files) { '{0}  {1}' -f (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $file.Name }
[IO.File]::WriteAllText((Join-Path $dir 'SHA256SUMS.txt'), ($lines -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
Write-Output "Checksummed $($files.Count) release artifacts."
