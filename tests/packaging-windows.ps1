# Black-box packaging regression: custom targets must never package a stale default-target exe.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('subcraft-package-test-' + [guid]::NewGuid().ToString('N'))
$previousTarget = $env:CARGO_TARGET_DIR
$previousOs = $env:OS

function Assert-Equal($actual, $expected, [string]$message) {
    if ($actual -ne $expected) { throw "$message (expected '$expected', got '$actual')" }
}

try {
    New-Item -ItemType Directory -Path $testRoot | Out-Null
    $fixture = Join-Path $testRoot 'fixture'
    foreach ($dir in 'scripts', 'crates/ui/assets/fonts', 'assets/starter-pack', 'assets/environments', 'target/dist', 'relative-build/dist') {
        New-Item -ItemType Directory -Path (Join-Path $fixture $dir) -Force | Out-Null
    }
    Copy-Item -LiteralPath (Join-Path $repo 'scripts/build-windows.ps1') -Destination (Join-Path $fixture 'scripts/build-windows.ps1')
    Copy-Item -LiteralPath (Join-Path $repo 'scripts/sign-windows.ps1') -Destination (Join-Path $fixture 'scripts/sign-windows.ps1')
    Set-Content -LiteralPath (Join-Path $fixture 'Cargo.toml') -Value 'version = "1.2.3"'
    foreach ($file in 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'crates/ui/assets/fonts/test-LICENSE.txt', 'assets/starter-pack/CREDITS-CC0.txt', 'assets/environments/CREDITS-CC0.txt') {
        Set-Content -LiteralPath (Join-Path $fixture $file) -Value $file
    }
    $absoluteTarget = Join-Path $testRoot 'absolute-build'
    New-Item -ItemType Directory -Path (Join-Path $absoluteTarget 'dist') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $fixture 'target/dist/subcraft.exe') -Value 'default binary'
    Set-Content -LiteralPath (Join-Path $fixture 'relative-build/dist/subcraft.exe') -Value 'relative binary'
    Set-Content -LiteralPath (Join-Path $absoluteTarget 'dist/subcraft.exe') -Value 'absolute binary'
    $env:OS = 'Windows_NT'
    $stage = Join-Path $fixture 'dist/SubCraft-1.2.3-windows-x64'
    $cases = @(
        @{ Target = ''; Expected = 'default binary' },
        @{ Target = 'relative-build'; Expected = 'relative binary' },
        @{ Target = $absoluteTarget; Expected = 'absolute binary' }
    )
    foreach ($case in $cases) {
        $env:CARGO_TARGET_DIR = $case.Target
        & (Join-Path $fixture 'scripts/build-windows.ps1') -SkipBuild | Out-Null
        Assert-Equal (Get-Content -LiteralPath (Join-Path $stage 'SubCraft.exe') -Raw).Trim() $case.Expected 'Wrong staged binary'
        if (-not (Test-Path -LiteralPath (Join-Path $stage 'third-party/test-LICENSE.txt'))) { throw 'Missing font license' }
        if (-not (Test-Path -LiteralPath (Join-Path $stage 'third-party/starter-pack-CC0.txt'))) { throw 'Missing starter pack credits' }
        $zip = [IO.Compression.ZipFile]::OpenRead((Join-Path $fixture 'dist/SubCraft-1.2.3-windows-x64.zip'))
        try {
            $entry = $zip.Entries | Where-Object { $_.Name -eq 'SubCraft.exe' } | Select-Object -First 1
            if (-not $entry) { throw 'Zip is missing SubCraft.exe' }
            $reader = [IO.StreamReader]::new($entry.Open())
            try { Assert-Equal $reader.ReadToEnd().Trim() $case.Expected 'Wrong binary in zip' } finally { $reader.Dispose() }
        } finally { $zip.Dispose() }
    }
    $failed = $false
    try { & (Join-Path $fixture 'scripts/build-windows.ps1') -SkipBuild -RequireSignature | Out-Null } catch { $failed = $_.Exception.Message -like 'A trusted, timestamped signature is required*' }
    if (-not $failed) { throw 'Unsigned staged application passed RequireSignature.' }
    # An invalid custom path must fail even when a default-target binary exists.
    $env:CARGO_TARGET_DIR = Join-Path $testRoot 'missing-build'
    $failed = $false
    try { & (Join-Path $fixture 'scripts/build-windows.ps1') -SkipBuild | Out-Null } catch { $failed = $_.Exception.Message -like 'Build output not found:*' }
    if (-not $failed) { throw 'Missing custom target did not fail' }
    Write-Output 'Windows packaging tests passed: default, relative, absolute, missing target and unsigned rejection; stage/zip contents and licenses.'
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
    $env:OS = $previousOs
    $full = [IO.Path]::GetFullPath($testRoot)
    $tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if ($full.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $full).StartsWith('subcraft-package-test-')) {
        Remove-Item -LiteralPath $full -Recurse -Force
    }
}
