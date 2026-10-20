# Static wiring checks, not execution of GitHub/Azure signing.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$workflow = Get-Content -LiteralPath (Join-Path $repo '.github/workflows/release.yml') -Raw
$count = 0
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message }; $script:count++ }
$actions = [regex]::Matches($workflow, '(?m)^\s*(?:- )?uses: ([^\s#]+)')
Assert ($actions.Count -ge 10) 'Expected all release actions.'
foreach ($action in $actions) { Assert ($action.Groups[1].Value -match '@[0-9a-f]{40}$') 'Release actions must use immutable SHAs.' }
Assert ($workflow -match 'RUST_VERSION: 1\.98\.0') 'Release toolchain must be explicit.'
Assert ($workflow -match 'CARGO_TARGET_DIR: .*github.workspace.*/target') 'Signing and compilation must share an explicit target directory.'
Assert ($workflow -notmatch '--clobber') 'Published artifacts must not be silently replaced.'
$compile = $workflow.IndexOf('- name: Compile application')
$appSign = $workflow.IndexOf('- name: Sign application before packaging')
$package = $workflow.IndexOf('- name: Package signed application')
$setupSign = $workflow.IndexOf('- name: Sign installer')
$scan = $workflow.IndexOf('- name: Scan final artifacts and verify signatures')
$attest = $workflow.IndexOf('- name: Attest final Windows artifacts')
$upload = $workflow.IndexOf('uses: actions/upload-artifact@')
Assert ($compile -ge 0 -and $compile -lt $appSign -and $appSign -lt $package -and $package -lt $setupSign -and $setupSign -lt $scan -and $scan -lt $attest -and $attest -lt $upload) 'Signing, packaging, scanning, attestation or upload order regressed.'
Assert ($workflow -match '-RequireSignature:\(\$env:PUBLISH -eq ''true''\)') 'Publishing must enforce signatures.'
Assert ($workflow -match 'ref: .*inputs.tag.*github.ref') 'Manual release must build requested tag.'
Assert ($workflow.Contains('needs: [windows, linux]')) 'Publish must wait for both checked packages.'
Assert ($workflow.Contains('environment: release-publish')) 'Publishing needs a final owner review environment.'
foreach ($script in 'build-windows.ps1','build-linux.sh') { Assert ((Get-Content -LiteralPath (Join-Path $repo "scripts/$script") -Raw).Contains('cargo build --locked')) "Unlocked build: $script" }
$errors = @()
foreach ($script in @('scripts/sign-windows.ps1','scripts/test-release-windows.ps1','scripts/write-release-checksums.ps1','scripts/new-sandbox-check.ps1','scripts/sandbox-guest-check.ps1','scripts/build-windows.ps1','scripts/build-installer.ps1','tests/release-security.ps1','tests/release-workflow.ps1')) {
    $tokens = $null; $parseErrors = $null
    [Management.Automation.Language.Parser]::ParseFile((Join-Path $repo $script), [ref]$tokens, [ref]$parseErrors) | Out-Null
    $errors += $parseErrors
}
Assert ($errors.Count -eq 0) "PowerShell parse errors: $errors"
Write-Output "Release workflow tests passed: $count assertions."
