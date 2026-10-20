# Sign using a code-signing certificate already provisioned in CurrentUser/My.
# Private keys/passwords are never accepted by this script. Cloud/HSM CI signing can
# instead sign the build output first and use -VerifyOnly to enforce the same gate.
[CmdletBinding(DefaultParameterSetName='Sign')]
param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory, ParameterSetName='Sign')][ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$CertificateThumbprint,
    [Parameter(Mandatory, ParameterSetName='Verify')][switch]$VerifyOnly,
    [string]$SignTool,
    [uri]$TimestampUrl = 'http://timestamp.digicert.com'
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Authenticode signing/verification requires Windows.' }
$file = (Get-Item -LiteralPath $Path -ErrorAction Stop).FullName
if ([IO.Path]::GetExtension($file) -ne '.exe') { throw 'Expected a Windows executable.' }
if (-not $VerifyOnly) {
    # RFC3161 responses are signed; use the provider's documented HTTP(S) endpoint.
    if (-not $TimestampUrl.IsAbsoluteUri -or $TimestampUrl.Scheme -notin @('http','https') -or $TimestampUrl.UserInfo) { throw 'Expected an absolute HTTP(S) timestamp endpoint without credentials.' }
    $cert = Get-Item -LiteralPath "Cert:\CurrentUser\My\$CertificateThumbprint" -ErrorAction Stop
    if (-not $cert.HasPrivateKey -or $cert.NotAfter -le (Get-Date) -or $cert.NotBefore -gt (Get-Date)) { throw 'Signing certificate is expired, not yet valid, or has no private key.' }
    $usage = $cert.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' } | Select-Object -First 1
    $usages = @($usage.EnhancedKeyUsages | ForEach-Object { $_.Value })
    if ($usages -notcontains '1.3.6.1.5.5.7.3.3') { throw 'Certificate is not valid for code signing.' }
    if (-not $SignTool) {
        $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
        $SignTool = Get-ChildItem -LiteralPath $sdk -Directory | Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
    if (-not $SignTool) { throw 'Windows SDK SignTool was not found.' }
    & $SignTool sign /sha1 $CertificateThumbprint /s My /fd SHA256 /tr $TimestampUrl.AbsoluteUri /td SHA256 $file
    if ($LASTEXITCODE -ne 0) { throw "SignTool failed: $LASTEXITCODE" }
}
$signature = Get-AuthenticodeSignature -LiteralPath $file
if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw "A trusted, timestamped signature is required: $($signature.Status) ($file)" }
if ($CertificateThumbprint -and $signature.SignerCertificate.Thumbprint -ne $CertificateThumbprint) { throw 'Unexpected signer certificate.' }
Write-Output "Verified trusted, timestamped signature: $file"
