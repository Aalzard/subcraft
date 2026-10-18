# Handoff — 0.1.7 continuation (2026-10-18)

- Branch `feature/brush-roadmap`; last code/version commit `45c5bfa` (worker `8a6174c`,
  sparse GPU uploads `c584981`). Documentation is the final local commit (`git log -1`).
- Release integration: https://github.com/Aalzard/subcraft/pull/50 ; release destination
  https://github.com/Aalzard/subcraft/releases/tag/v0.1.7 . Keep this checkout on the feature
  branch. Published 2026-10-19; PR #50 merged as `71e49fd`. Windows/Linux tests, browser build and both packages passed. Release workflow `39167622401` passed; all three assets have SHA-256 digests.
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only cargo test --workspace -j 1`:
  **414 passed, 0 failed, 0 ignored**. Clippy workspace/all targets exits 0, existing warnings.
  Windows packaging path tests pass. No failing/skipped local tests remain.

Done this session:
- Finished Claude's immutable-snapshot compositor worker, latest-wins queue, progressive
  changed tiles, exact sync completion, separate Rayon pool; preserved adopted composites.
- Publication generation catches results consumed by export/sampling before GPU upload.
- GPU full uploads clear to f16 fill and transfer only allocated tiles; exact GPU regression.
- Added 10 regressions; Windows-only benchmark probe now compiles on other platforms.
- Version/notes 0.1.7, native 4K profile + benchmark, screenshot and ADR-0018 updated.

Native evidence: `docs/screenshots/release-017-layers.png`, reports in `docs/performance/`.
4K generator median/p95 14.6/33.9 ms, final refresh 315 ms; scale 15.1/28.6 ms, 576 ms.
Startup outlier 440.7 ms is interval 2; later generator max 94.6 ms. No zero-drop guarantee.
`target/release/subcraft.exe` rebuilt, SHA256
`ED34D3442846C3A1ACEA59A19E23C9878B0A790923C15FC5A3368AE9BF6C3F9C`.

Limits: no mouse input this continuation; owner already hand-tested the previous app.
Save/reopen is covered numerically; pointer/tablet gestures, complex-model 4K strokes,
Linux/browser runtime and installed updater restart were not hand-checked. Existing slider
history can split one drag into two entries (session 68), not fixed here. General filters,
Compare Mask, anchors, folder content effects, effect thumbnails/drag reorder remain pending.

Keep: CPU-authoritative tiled COW data, immutable worker inputs, invalidations survive
replacement, changed-tile union, exact synchronous export, app/compositor free of UI/GPU
libraries. Save persists authored sources, not viewport composites. Schema stays 5; presets 2.
Cold-field splitting was reverted by Claude (no measured gain); do not restore without data.
No new owner decision needed for performance; future X color-swap conflicts with X symmetry.
Protected ZIPs, output/, generate_textures.py and __pycache__/ untouched; no build cleanup.

Next 3 steps:
1. Stage/halo contracts for folder content and neighborhood filters (ADR-0016/0017).
2. Local adjustments then blur/sharpen with numeric goldens, undo/save migration and GPU parity.
3. Per-effect thumbnails/drag reorder, smart-material round trips; profile complex-model strokes
   and first-use graphics stalls. Compare Mask/anchors follow the stage graph.
