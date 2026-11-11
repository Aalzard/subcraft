# Static wiring checks, not execution of GitHub/SignPath signing.
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
$appUpload = $workflow.IndexOf('- name: Upload unsigned application for signing')
$appSign = $workflow.IndexOf('- name: Sign application before packaging')
$appUse = $workflow.IndexOf('- name: Use the signed application')
$package = $workflow.IndexOf('- name: Package application')
$setupUpload = $workflow.IndexOf('- name: Upload unsigned installer for signing')
$setupSign = $workflow.IndexOf('- name: Sign installer')
$setupUse = $workflow.IndexOf('- name: Use the signed installer')
$scan = $workflow.IndexOf('- name: Scan final artifacts and verify signatures')
$attest = $workflow.IndexOf('- name: Attest final Windows artifacts')
$upload = $workflow.IndexOf('name: SubCraft-windows-x64')
$order = @($compile, $appUpload, $appSign, $appUse, $package, $setupUpload, $setupSign, $setupUse, $scan, $attest, $upload)
Assert (@($order | Where-Object { $_ -lt 0 }).Count -eq 0) 'A signing, packaging, scanning, attestation or upload step is missing.'
for ($i = 1; $i -lt $order.Count; $i++) { Assert ($order[$i - 1] -lt $order[$i]) 'Signing, packaging, scanning, attestation or upload order regressed.' }
Assert ($workflow -notmatch '(?i)azure') 'Signing must not depend on Azure.'
Assert (([regex]::Matches($workflow, 'uses: signpath/github-action-submit-signing-request@')).Count -eq 2) 'Application and installer must both be submitted to SignPath.'
Assert ($workflow.Contains("github-artifact-id: `${{ steps.unsigned-application.outputs.artifact-id }}")) 'Application signing must use the uploaded application artifact.'
Assert ($workflow.Contains("github-artifact-id: `${{ steps.unsigned-installer.outputs.artifact-id }}")) 'Installer signing must use the uploaded installer artifact.'
Assert ($workflow.Contains("api-token: `${{ secrets.SIGNPATH_API_TOKEN }}")) 'The SignPath token must come from a secret.'
Assert (([regex]::Matches($workflow, "if: env.SIGN_WINDOWS == 'true'")).Count -eq 6) 'Every signing step must be skipped when SignPath is not configured.'
Assert ($workflow -notmatch 'Publishing is blocked') 'Missing signing configuration must not block a release.'
Assert ($workflow -match '-RequireSignature:\(\$env:SIGN_WINDOWS -eq ''true''\)') 'Signed releases must enforce signatures.'
Assert ($workflow.Contains('## Unsigned Windows build')) 'Unsigned releases must say so in the release notes.'
Assert ($workflow.Contains('gh attestation verify')) 'Release notes must explain provenance verification.'
Assert ($workflow.Contains('subject-path: dist/SHA256SUMS.txt')) 'The combined published checksum file must be attested.'
Assert ($workflow -match 'ref: .*inputs.tag.*github.ref') 'Manual release must build requested tag.'
Assert ($workflow.Contains('needs: [windows, linux]')) 'Publish must wait for both checked packages.'
Assert ($workflow.Contains('environment: release-publish')) 'Publishing needs a final owner review environment.'
$platforms = Get-Content -LiteralPath (Join-Path $repo '.github/workflows/release-platforms.yml') -Raw
foreach ($action in [regex]::Matches($platforms, '(?m)^\s*(?:- )?uses: ([^\s#]+)')) { Assert ($action.Groups[1].Value -match '@[0-9a-f]{40}$') 'Platform release actions must use immutable SHAs.' }
Assert (([regex]::Matches($platforms, 'uses: actions/attest-build-provenance@')).Count -ge 3) 'Every experimental platform must be attested.'
foreach ($script in 'build-windows.ps1','build-linux.sh') { Assert ((Get-Content -LiteralPath (Join-Path $repo "scripts/$script") -Raw).Contains('cargo build --locked')) "Unlocked build: $script" }
$errors = @()
foreach ($script in @('scripts/sign-windows.ps1','scripts/test-release-windows.ps1','scripts/write-release-checksums.ps1','scripts/new-sandbox-check.ps1','scripts/sandbox-guest-check.ps1','scripts/build-windows.ps1','scripts/build-installer.ps1','tests/release-security.ps1','tests/release-workflow.ps1')) {
    $tokens = $null; $parseErrors = $null
    [Management.Automation.Language.Parser]::ParseFile((Join-Path $repo $script), [ref]$tokens, [ref]$parseErrors) | Out-Null
    $errors += $parseErrors
}
Assert ($errors.Count -eq 0) "PowerShell parse errors: $errors"
Write-Output "Release workflow tests passed: $count assertions."
