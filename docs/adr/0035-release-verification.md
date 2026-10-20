# ADR-0035 — Signed, scanned and attributable releases

Status: Accepted (2026-10-20; production identity enrollment pending)

Keep verification outside app crates. Release builds pin Rust/actions, use Cargo --locked,
check out the requested tag, and match its version. Sign the application before packaging,
then sign the installer. Published packages require trusted timestamped signatures with
matching publisher subjects; missing signing configuration blocks publication. Developer
artifact-only jobs may remain unsigned and must record that explicitly.

The hosted integration uses Microsoft Artifact Signing/OIDC and a protected release-signing
environment. The owner must establish eligibility, identity and billing. Other providers
can use the certificate-store/HSM SignTool helper locally; hosted provider changes must
preserve both signing steps and verification. No private keys/passwords are accepted by scripts.

Scan final installer/ZIP/extracted executable using active Microsoft Defender, fail on errors,
stale definitions or artifact changes, record scope/hashes/signatures, generate SHA-256 sums,
attest provenance and upload without replacing existing assets.

Antivirus, Authenticode, provenance and runtime tests answer different questions; none proves
absence of malware. The offline Sandbox harness tests install, real screenshot rendering and
uninstall, with network/devices/clipboard/GPU sharing disabled. Only dedicated input (read-only)
and results directories are mapped. It is not API tracing or an explanation of CAPE flags.

Tests cover packaging, rejection paths, checksums, sandbox settings and workflow order.
Actual 0.1.7 static scans passed. Hosted signing/attestation and guest execution remain unverified
until an enrolled publisher and compatible Windows host are available.
