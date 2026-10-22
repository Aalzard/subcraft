# ADR-0038 — Browser version removed

Status: Accepted (2026-10-22)

## Context

SubCraft was also built for the browser (wasm32 + WebGPU) from the same code and published to
GitHub Pages. It had no threads, no FBX, no background autosave and a file layer that turned every
write into a download. Those differences cost a `cfg(target_arch = "wasm32")` branch in about
twenty files and a CI job, while the owner's users run the desktop app.

## Decision

Remove the browser build entirely: the `web/` page, `scripts/build-web.sh`, the web workflow,
the wasm-only dependencies and every wasm32 code path. Desktop behaviour is unchanged. The
`Build (web)` check was never a required status check, so branch protection is unaffected.
`subcraft_core::vfs` stays as a thin wrapper over `std::fs`.

## Consequences

There is one platform family to test. The old GitHub Pages site is no longer updated and has to
be switched off in the repository settings. Reintroducing a web build would be a new ADR.
