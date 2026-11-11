# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

> **Status:** the SignPath Foundation application is still pending, so the current Windows
> releases are not code-signed yet. Each release says in its notes whether its Windows files are
> signed. Every release file already has a build provenance attestation (see
> [Verifying a download](#verifying-a-download)).

## What is signed

- The Windows application `SubCraft.exe` (inside the installer and the portable zip) and the
  Windows installer `SubCraft-<version>-setup.exe`, for the Windows x64 release.
- Only binaries built by this repository's GitHub Actions release workflow
  ([`.github/workflows/release.yml`](.github/workflows/release.yml)) on GitHub-hosted runners, from
  the source code and build scripts in this public repository. Nothing built on a personal computer
  is submitted for signing.
- SubCraft has no proprietary components. Its third-party dependencies are open-source Rust crates
  (pinned in `Cargo.lock`) compiled from source in the same build; bundled fonts and assets are
  open-licensed or CC0 (their licenses ship in the package under `third-party/`).

The macOS, Linux and Windows on Arm packages are not Authenticode-signed (see
[docs/signing-setup.md](docs/signing-setup.md) for why). Like every release file, they have a build
provenance attestation.

## Team roles

SubCraft is maintained by one person, so all roles are held by the maintainer:

| Role | Members | What the role means |
|---|---|---|
| Committers and reviewers | [Aalzard](https://github.com/Aalzard) | May change the source code directly; reviews every pull request from other contributors before it is merged. |
| Approvers | [Aalzard](https://github.com/Aalzard) | Approves each signing request in SignPath by hand, after checking that it comes from the release workflow for a published release tag. |

All team members use multi-factor authentication on GitHub and on SignPath.

## Privacy policy

This program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it, with this one exception, which can
be turned off:

- **Update check.** By default, when SubCraft starts it asks GitHub
  (`api.github.com/repos/Aalzard/subcraft/releases`) whether a newer release exists. The request
  contains no personal data or project content; like any web request it reveals your IP address and
  the user agent `SubCraft-updater` to GitHub. Turn it off in **File → Preferences… → Check for
  updates when SubCraft starts**. Updates are only downloaded (from GitHub Releases) when you click
  **Update**.

User-requested actions:

- **Help → Report a problem** opens a pre-filled GitHub issue page in your web browser. Nothing is
  sent until you submit the issue yourself on GitHub.
- Links such as "SubCraft on GitHub" open in your web browser.

GitHub's handling of these requests is covered by the
[GitHub General Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).
SubCraft has no telemetry, analytics, crash upload or account.

## Verifying a download

- **Signature (once signing is active):** on Windows, right-click the file → **Properties** →
  **Digital Signatures**. The signer is SignPath Foundation.
- **Build provenance (all platforms, every release):** with the [GitHub CLI](https://cli.github.com/),

      gh attestation verify <downloaded-file> --repo Aalzard/subcraft

  This checks that the file was built by this repository's GitHub Actions workflow from a release tag.
- **Checksums:** compare with `SHA256SUMS.txt` (and `SHA256SUMS-*.txt` for the experimental packages)
  on the release page.

Report a suspicious release or a policy violation through the
[issue tracker](https://github.com/Aalzard/subcraft/issues).
