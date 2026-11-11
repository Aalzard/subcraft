# Release verification and signing

On 2026-10-20, the 0.1.7 installer hash matched the published GitHub artifact. Defender scans
of installer, portable ZIP and extracted executable reported no threats. This is static
evidence, not a confirmed false positive. CAPE mitigation-policy/unhooking flags are unexplained.
Review requests to Arctic Wolf, Trapmine and SecureAge support were sent through Gmail and
verified in Sent. Vendor analysis/reclassification is pending. Requests include only public
release/source links, hashes and the investigation limitations.

Arctic Wolf acknowledged receipt automatically. Its required report link, expected behavior
and developer/contact details were included; this acknowledgment is not a clean verdict.

## Checks

From the repository in PowerShell:

    ./tests/packaging-windows.ps1
    ./tests/release-security.ps1
    ./tests/release-workflow.ps1
    ./scripts/test-release-windows.ps1 -Directory <package-folder> -Version 0.1.7 -Commit 71e49fde7e4e1dba188a6589c9e5a139d1e3d9d5

The final command does not launch the files. It writes release-security-windows.json,
per-file Defender logs and SHA256SUMS.txt. Add -RequireSignature for production; original
unsigned 0.1.7 must fail that stricter gate. The supplied commit identifies the release
revision; it does not establish a reproducible source-to-binary match.

With a code-signing certificate in CurrentUser/My (including supported HSM adapters):

    ./scripts/build-installer.ps1 -CertificateThumbprint <40-hex-thumbprint> -RequireSignature

Never put a private key/password in Git, commands, logs or chat.
The local helper uses DigiCert's [documented RFC3161 endpoint](https://knowledge.digicert.com/general-information/rfc3161-compliant-time-stamp-authority-server);
timestamp responses must be cryptographically trusted, regardless of HTTP(S) transport.

## Hosted signing (SignPath Foundation, free)

Windows release signing uses SignPath Foundation's free open-source program instead of Azure or a
paid certificate ([ADR-0046](adr/0046-free-code-signing.md)). The owner steps (application, SignPath
project, policy, artifact configuration, repository secret and variables) are in
[signing-setup.md](signing-setup.md); the public policy is [CODE_SIGNING.md](../CODE_SIGNING.md).

Until SignPath is configured, a tag release publishes unsigned Windows packages with a notice in the
release notes instead of failing. Once configured, release.yml signs the application before
packaging and the installer after, and the scan step requires trusted, timestamped signatures with
matching publishers. Signing verifies a publisher and helps reputation; it does not guarantee
absence of [SmartScreen warnings](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
or antivirus detections.

GitHub's Windows runner images disable Defender. The workflow re-enables it on the throwaway runner
before the static scan; if that fails, release-security-windows.json records that no antivirus scan
ran (-SkipAntivirus) and no Defender logs are published.

The release-publish environment is still used by the publish jobs; adding required reviewers to it
is a server setting. An empty workflow-dispatch tag builds unsigned test artifacts. Publishing
builds the requested tag; old tags without these scripts cannot use the new workflow. Existing
assets are not overwritten.

Every published file on every platform has a build provenance attestation:

    gh attestation verify <downloaded-package> --repo Aalzard/subcraft

## Isolated runtime test

    ./scripts/new-sandbox-check.ps1 -Installer <setup.exe> -ExpectedSha256 <64-hex-hash> -Destination <new-case-directory>

Inspect and open the generated .wsb on a compatible Windows Sandbox host. Alternatively,
prepare a new case with -Launch. Input is read-only; only the dedicated results folder is
writable on the host. Network, GPU, device and clipboard sharing are disabled.
Never launch sandbox-guest-check.ps1 on the host.
The disposable guest's bootstrap PowerShell process uses RemoteSigned for the locally authored
harness; it does not change the host's execution policy or Defender settings.

Completion requires results/result.json reporting passed, install.log and viewport.png.
A prepared configuration is not a passing test. This PC has Windows 11 Home, no Sandbox
executable and reports virtualization disabled, so guest execution was not run.
The harness is a smoke test, not malware API tracing. To resolve CAPE's outstanding flags,
obtain its underlying trace or use a disposable Windows VM with module/call tracing.

Vendor routes were checked in [VirusTotal's directory](https://docs.virustotal.com/docs/false-positive-contacts)
and [SecureAge support](https://knowledgebase.secureage.com/secureaplus/en-us/Content/Home.htm).
