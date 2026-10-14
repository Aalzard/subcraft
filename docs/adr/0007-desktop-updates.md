# ADR 0007: Desktop release updates

## Status
Accepted

## Context
Desktop users should receive stable releases without repeatedly downloading and installing
them by hand. The running executable is locked on Windows, and updates must preserve unsaved
work without overwriting the user's project. Existing installations and portable packages
need different installation paths. Browser deployments already update through GitHub Pages.

## Decision
Keep release checking, HTTP, validation, project checkpoints and installation in `app`.
UI controls dispatch to an asynchronous updater state machine. Native-only dependencies do
not enter WebAssembly builds. No background network request happens until the user checks.

Read the official repository's latest stable release over HTTPS and compare semantic version
precedence. Accept only the exact platform asset URL in Aalzard/subcraft, a bounded size and
a SHA-256 digest. Stream downloads to UUID cache folders and verify size/digest. Recheck the
specific release metadata in the helper before closing the parent application. These checks
trust GitHub HTTPS and repository release permissions; they are not publisher code signing.

Capture a versioned `.subcraft` project checkpoint, original save path, modified flag and
UUID selection. Copy the current executable into the job folder as a separate helper, which
validates the job and signals readiness before the UI closes. The helper waits for the parent
process to exit, verifies the package again, installs and launches the target with the job.
The restarted app restores the checkpoint. Failed installation launches the old executable
with the same checkpoint and an error message. The user's original project is never saved
over automatically. Undo history and camera/workspace preferences are not checkpointed.

Windows registry installation metadata chooses per-user or all-users Inno Setup updates.
All-users updates use Windows elevation; cancelling permission keeps the old app. Portable
updates extract only the known executable entry and replace it with rollback. Neighboring
files are untouched. Unsupported platforms retain manual downloads.

The explicit `update-fixture` build feature provides loopback-only release transport and an
isolated installer identity for end-to-end tests. It is disabled in production packaging.
It must never be enabled when publishing release artifacts.

## Consequences
Users on v0.1.3 and earlier need a single bootstrap download. Subsequent stable releases keep
the package naming contract and GitHub asset digests. The button reports packages that are
still uploading instead of selecting a partial release. Checkpoints remain available in the
data folder if restoration or restart fails. HTTP work and checkpoint serialization run off
the UI thread; baking completes before the update checkpoint is captured.
Completed jobs discard their downloaded package and temporary executables on the next start;
the project checkpoint and job metadata remain available for recovery.
