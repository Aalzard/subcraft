# Free code signing setup (owner steps)

SubCraft signs its Windows release for free through **SignPath Foundation** (free code signing
for open-source projects: an Authenticode certificate issued to SignPath Foundation, with signing
done by SignPath.io from GitHub Actions). There is no Azure account and no paid certificate.

The workflow is ready. Until the steps below are done, every tag release still completes: it
publishes the **unsigned** Windows installer, zip, `SHA256SUMS.txt` and provenance attestations,
and adds an "Unsigned Windows build" notice to the release notes. Once the configuration exists,
the same workflow signs `SubCraft.exe` and the installer.

Only the owner can do these steps: they need the owner's identity, accounts and MFA.
Nothing has been submitted to SignPath on the owner's behalf.

## 1. Check eligibility

SignPath Foundation's [terms](https://signpath.org/terms) (checked 2026-11-11) require:

| Requirement | SubCraft |
|---|---|
| OSI-approved license, no commercial dual licensing | MIT OR Apache-2.0. OK. |
| No proprietary components, including from the maintainer | All source in the public repo; dependencies are open-source crates. OK. |
| Actively maintained and **already released** in the form to be signed | Releases on GitHub (v0.3.4 and earlier). OK. |
| Functionality described on the download page | README and release notes. OK. |
| No malware, PUPs, or tools that find/exploit vulnerabilities | A texturing app. OK. |
| Respects privacy: anything sent to other systems is in a privacy policy and can be turned off | The update check is described in [CODE_SIGNING.md](../CODE_SIGNING.md#privacy-policy) and can be turned off in Preferences. The terms also ask for such behaviour to be **shown during installation**, which the installer does not do yet. Reviewers may ask for an installer note (an Inno Setup info page); add one if they do. |
| Uninstaller for anything installed | The installer registers an uninstaller. OK. |
| Binaries built from source in a verifiable way, on GitHub-hosted runners | `release.yml` builds on GitHub-hosted runners only. OK. |
| Every signing request approved by hand | Approval policy, see step 3. |
| **MFA** for every team member on GitHub and SignPath | Turn on GitHub 2FA if not already on: GitHub → Settings → Password and authentication. |
| Code signing policy on the website and repository, with the line "Free code signing provided by SignPath.io, certificate by SignPath Foundation", team roles and a privacy policy | [CODE_SIGNING.md](../CODE_SIGNING.md), linked from the README download section. OK. |

## 2. Apply

1. Open <https://signpath.org/apply> and fill in the form. It asks for roughly:
   - **Project / repository URL:** `https://github.com/Aalzard/subcraft`
   - **License:** MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`)
   - **Download / release URL:** `https://github.com/Aalzard/subcraft/releases`
   - **Code signing policy URL:** `https://github.com/Aalzard/subcraft/blob/main/CODE_SIGNING.md`
   - **Description:** what SubCraft is, who uses it, and which artifacts are signed:
     the Windows x64 `SubCraft.exe` and the Inno Setup installer `SubCraft-<version>-setup.exe`,
     built by GitHub Actions.
   - Maintainer name and contact email.
2. Answer reviewer follow-up questions by email. They check the items in step 1.
3. After approval, SignPath sends an invitation to a SignPath.io organization (the Open Source
   subscription). Accept it, sign in to <https://app.signpath.io>, and turn on MFA there.

## 3. Configure SignPath (after approval)

In <https://app.signpath.io> (some of this may already be set up by SignPath Foundation; keep the
names below or use your own and put them into the GitHub variables in step 4):

1. **Trusted build system:** add the predefined **GitHub.com** trusted build system to the
   organization. Link it to the project below. Optionally install the SignPath GitHub App on the
   repository (only needed for audit-log checks).
2. **Project:** slug `subcraft`, repository URL `https://github.com/Aalzard/subcraft`.
3. **Artifact configuration:** create one, for example with slug `windows-exe`, paste the contents of
   [`.signpath/artifact-configuration.xml`](../.signpath/artifact-configuration.xml), and make it
   the project's default. It expects each signing request to be a zip (that is how GitHub stores
   workflow artifacts) with a single `.exe` named "SubCraft" at its root, and declares the `version`
   parameter the workflow sends.
4. **Signing policy:** slug `release-signing`, using the SignPath Foundation release certificate.
   - Approval: **manual approval** by the approver (you) for every request.
   - Trusted build system: GitHub.com (the request must come from a GitHub-hosted workflow run).
   - Optionally restrict it further with pipeline policies (for example, no reruns).
   A `test-signing` policy with the test certificate is useful for a first trial, but it is not
   needed.
5. **CI user / API token:** create a CI user (for example `github-actions`), give it the
   **Submitter** role on the `release-signing` policy, and copy its API token. Note your
   **organization ID** (Organization → Settings).

## 4. Add the configuration to GitHub

Repository → **Settings → Secrets and variables → Actions**:

| Kind | Name | Value |
|---|---|---|
| Secret | `SIGNPATH_API_TOKEN` | The CI user's API token |
| Variable | `SIGNPATH_ORGANIZATION_ID` | Your SignPath organization ID (a GUID) |
| Variable | `SIGNPATH_PROJECT_SLUG` | `subcraft` |
| Variable | `SIGNPATH_SIGNING_POLICY_SLUG` | `release-signing` |
| Variable (recommended) | `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG` | `windows-exe` (leave unset to use the project's default) |

Signing turns on when the secret and the three required variables all exist. If any is missing,
the release is published unsigned with a workflow warning that names what is missing.

The old Azure variables (`AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`,
`SIGNING_ENDPOINT`, `SIGNING_ACCOUNT`, `SIGNING_PROFILE`) and the `release-signing` GitHub
environment are no longer used and can be deleted if they exist. The `release-publish` environment
is still used by the publish jobs; adding yourself as a required reviewer there remains optional.

## 5. What a signed release looks like

1. Publish a GitHub release for tag `vX.Y.Z` (matching `Cargo.toml`) as before.
2. The Release workflow builds `subcraft.exe` and submits it to SignPath. **SignPath emails you;
   approve the request** in app.signpath.io. The job waits up to 2 hours per request.
3. The workflow packages the signed exe into the zip and the installer, then submits the installer.
   **Approve the second request.**
4. The scan step verifies both signatures, the files are attested and uploaded, and no
   "Unsigned Windows build" notice is added.

If a signing request is rejected, times out or fails the artifact configuration's checks, the
Windows job fails and nothing is published for Windows. Re-run the workflow (Actions → Release →
Run workflow, with the tag) after fixing the cause; SignPath policies may limit reruns.

## What changes for users

- Windows shows **"SignPath Foundation"** as the verified publisher, instead of "Unknown publisher".
- **SmartScreen does not go away immediately.** Its reputation is built per certificate and per
  file over downloads and time, even with a valid certificate. Early signed releases can still show
  "Windows protected your PC" for a while; it fades as more people download signed releases.
  (Only EV certificates got instant reputation, and Microsoft has since removed that too.)
- Fewer antivirus false positives are typical for signed files, but not guaranteed.
- The installed uninstaller (`unins000.exe`) is created by Inno Setup on the user's machine and is
  not signed. Signing it too needs Inno Setup's `SignTool` directive with a local signing command,
  which SignPath's CI flow does not provide.

## Other platforms

- **macOS: no free path.** Gatekeeper only trusts apps signed with an Apple **Developer ID**
  certificate and notarized by Apple, which needs the Apple Developer Program (99 USD per year).
  SignPath Foundation's certificate is a Windows Authenticode certificate and does not help on macOS.
  The macOS package stays ad-hoc signed and not notarized (users use **Open Anyway**, see the
  README), and it has a build provenance attestation.
- **Linux:** there is no OS-level code signing for a tarball. Each package has a build provenance
  attestation and is listed in a checksum file. Detached Sigstore signatures (for example
  `cosign sign-blob` with GitHub's OIDC identity, keyless) could be added later for people who
  prefer `cosign verify-blob`; the provenance attestation already covers the same need for
  `gh attestation verify` users.
- **Windows on Arm (experimental):** unsigned for now. It could go through the same SignPath project
  later (one more signing request and approval per release).

## Verifying provenance (all platforms, free, active now)

Every published file (Windows, Linux, and the experimental macOS, Linux aarch64 and Windows arm64
packages, plus the checksum files) has a GitHub build provenance attestation signed with Sigstore:

    gh attestation verify SubCraft-0.3.5-setup.exe --repo Aalzard/subcraft

It confirms the file was built by this repository's workflow, from which tag and commit. Releases
from before this change (v0.3.4 and earlier) have attestations for some files only.

## References

- SignPath Foundation terms: <https://signpath.org/terms>
- Apply: <https://signpath.org/apply>
- GitHub trusted build system and the action: <https://docs.signpath.io/trusted-build-systems/github>
- Action used: `signpath/github-action-submit-signing-request` v3 (commit `f6d04783b4569d051e0c80105fe66e82819d0092`)
- Artifact configuration syntax: <https://docs.signpath.io/artifact-configuration/syntax>
