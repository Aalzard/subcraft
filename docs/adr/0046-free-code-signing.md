# ADR-0046 — Free code signing (SignPath Foundation) and never-blocked releases

Status: Accepted (2026-11-11; SignPath Foundation application pending). Supersedes the signing
provider and the "missing signing configuration blocks publication" rule of ADR-0035.

The owner does not want Azure or any paid certificate. Microsoft Artifact Signing also required
identity validation and billing that never happened, so the v0.3.3 and v0.3.4 tag runs failed at
configuration validation and the packages had to be uploaded by hand.

Windows signing uses SignPath Foundation (free for OSI-licensed projects, certificate issued to
SignPath Foundation) through `signpath/github-action-submit-signing-request`, pinned by commit.
The release job keeps ADR-0035's order: compile, sign the application, package the zip and
installer around the signed exe, sign the installer, scan, attest, upload. Each signing request
is a GitHub workflow artifact and needs a manual approval in SignPath.

Signing is enabled only when `SIGNPATH_API_TOKEN` (secret) and `SIGNPATH_ORGANIZATION_ID`,
`SIGNPATH_PROJECT_SLUG`, `SIGNPATH_SIGNING_POLICY_SLUG` (variables) exist. Without them a tag
release still publishes the unsigned installer, zip, checksums and attestations, warns in the
workflow log and adds an "Unsigned Windows build" notice to the release notes. When signing is
configured, signatures are required: a rejected or failed request fails the Windows job.

GitHub-hosted Windows images disable Defender; the job re-enables it on the throwaway runner.
If Defender still cannot be activated, the report records that no antivirus scan ran instead of
blocking the release.

Every published file on every platform, including the experimental workflow and the combined
`SHA256SUMS.txt`, gets a Sigstore build provenance attestation (`gh attestation verify`). macOS
has no free signing path (Apple Developer Program) and stays ad-hoc signed and not notarized.

Owner steps: [docs/signing-setup.md](../signing-setup.md). Policy: [CODE_SIGNING.md](../../CODE_SIGNING.md).
