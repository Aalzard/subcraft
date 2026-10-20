# Codex handoff — 2026-10-20, session 93
Branch: feature/brush-roadmap. Pre-session HEAD: 16acd36 (Claude WIP).
Last implementation: 3304c4f8c8b2a42ec055e784e705ddc953e6e1aa; final documentation commit follows.
Workspace: 538 pass, 0 failed/ignored; Clippy workspace/all-targets exits 0 with existing
warnings (including nalgebra future compatibility). No-incremental/line-tables-only/--locked/-j 1.
Release checks: 52 assertions + five packaging scenarios
pass; actionlint 1.7.12/YAML/PowerShell syntax pass. No push/release.

Done:
- Prepared signed, scanned, attributable releases (workflow + scripts; ADR-0035).
- Ran new static gate on exact public 0.1.7 installer/ZIP/executable: Defender no threats.
- Sent three vendor review requests via Gmail; Arctic Wolf receipt acknowledged, no verdict.
- Prepared offline Sandbox smoke case; documented publisher options at owner's request.

Stops/external dependencies:
- .github/workflows/release.yml: signing requires owner-enrolled account and GitHub protected
  release-signing/release-publish environments, neither configured. Azure integration and local
  certificate-store helper are implemented, but successful production signing is untested.
- scripts/new-sandbox-check.ps1 prepares/optionally launches; sandbox-guest-check.ps1 is guest-only.
  Guest NOT run: this Windows 11 Home PC lacks Sandbox and reports virtualization disabled.
  Harness is install/render/uninstall smoke coverage, not API tracing. CAPE mitigation/unhook
  flags remain unexplained; no clean certificate/confirmed false-positive claim.
- Evidence: docs/security/0.1.7-static-scan.json. Private local case/review details remain in
  %TEMP%/SubCraft-security-f90a726e; do not commit private mailbox IDs/headers.
- No app UI, engine behavior or 4K performance verification this session; no screenshots added.
  Hosted CI/attestation/Linux packaging/signing success paths are not run locally.
- Owner chose “no account yet—prepare options”; docs/release-security.md has Microsoft eligibility,
  Certum non-commercial/availability caveats, SSL.com fees. No money/identity/roles submitted.

Keep: app before packaging, installer afterward; trusted timestamped signatures with matching
publisher; scans on FINAL files; locked build/pinned actions; tag/version checks; exact sync
app/export unaffected. No suppression/evasion or disabling antivirus. Preserve original 0.1.7
bytes/hash and immutable assets. Protected files, artworks and untracked session92 patch untouched.

Next 3:
1. Follow vendor replies; obtain CAPE trace or isolated module/API trace for outstanding flags.
2. Owner selects eligible signing provider and completes identity/billing; configure protected
   environments/OIDC least privilege, test both signed artifacts and hosted provenance.
3. Run exact signed artifacts in compatible Windows Sandbox/VM, inspect PNG/log/result/hashes,
   approve publication only after evidence; then resume Claude's edge-wear/brush/filter audit
   in docs/PROMPT-claude-edge-wear.md (previous implementation checkpoint a995822/263cb6c).
