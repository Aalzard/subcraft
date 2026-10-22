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

## Publisher options — owner enrollment required

The owner has no existing signing account and requested options. No purchase or identity
submission has been made. Confirm current eligibility, full fees and CI support before buying.

| Option | Fit and remaining action |
|---|---|
| [Microsoft Artifact Signing](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart) | Prepared hosted integration. Individual public-trust onboarding currently requires US/Canada residence; organizations have a wider country list. Owner must complete identity validation and billing. |
| [Certum](https://www.certum.eu/en/code-signing-certificates/) | Individual/company cloud certificates. The [open-source plan](https://support.certum.eu/en/code-signing-required-documents/) restricts commercial distribution; a public repository alone does not establish eligibility. Confirm country, project and automation eligibility. Supported certificate-store integration can use our local helper. |
| [SSL.com eSigner](https://www.ssl.com/products/software-integrity/signing-service/) | Cloud signing with a vendor [GitHub Action](https://github.com/SSLcom/esigner-codesign). Confirm individual/country eligibility and certificate plus signing-operation fees. Hosted signing steps need adapting after enrollment. |

Pricing/availability checked 2026-10-20: SSL.com's linked page displays IV certificates at
$129/year plus a cloud Tier 1 displayed at $15/month; confirm billing period/tax/quotas.
Certum's [open-source shop listing](https://shop.certum.eu/open-source-code-signing-on-simplysign.html)
advertises a starting EUR 49 price but currently says out of stock. These are options for
review, not purchases or a recommendation to enroll before eligibility is confirmed.

Signing verifies a publisher and helps reputation; it does not guarantee absence of
[SmartScreen warnings](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
or antivirus detections.

## Hosted workflow setup

After enrollment, create a protected GitHub environment named release-signing, restrict it
to release tags and require owner review. Configure AZURE_CLIENT_ID, AZURE_TENANT_ID,
AZURE_SUBSCRIPTION_ID, SIGNING_ENDPOINT, SIGNING_ACCOUNT and SIGNING_PROFILE environment
variables. Scope provider OIDC trust to this repository/environment and grant only the
certificate-profile signing role. No cloud account, role or repository variable was changed here.

Also configure release-publish with required owner review. Before approving this final job,
download the exact signed build artifacts, run the isolated smoke test on a compatible host,
inspect its screenshot/log/result and record the tested hashes. GitHub environment protection
is a server setting; a workflow name alone does not enforce reviewers. Do not enable publishing
until both protected environments are configured. API/module tracing remains a separate audit.

An empty workflow-dispatch tag builds unsigned test artifacts. Publishing requires configured
signing, timestamped signatures, passing scans and provenance. It builds the requested tag;
old tags without these scripts cannot use the new workflow. Existing assets are not overwritten.

Verify checksums and GitHub provenance after a future hosted run:

    gh attestation verify <downloaded-package> --repo Aalzard/subcraft

The workflow was validated locally; hosted signing, attestation and publication were not run.

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
