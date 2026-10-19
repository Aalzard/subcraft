# Handoff — local filters and unresolved wear (2026-10-19)

- Branch feature/brush-roadmap; implementation **1a84d31**. Final docs commit is HEAD.
- cargo test --workspace: **482 pass, 0 failed/ignored** (12 added). Workspace/all-target Clippy
  exits 0 with existing warnings. Incremental=0, dev debug=line-tables-only, -j 1. No push/release.
- Owner STILL rejects wear after a22eb6f/4f6d8ea. Treat it as unresolved; do not cite those tests
  as owner acceptance. Claude prompt: docs/PROMPT-claude-edge-wear.md. No wear kernel edits here.
- Done: Contrast/HSL/Threshold/Clamp content effects, Contrast/Threshold/Clamp on masks,
  dedicated inspector/menu, per-channel semantics, preserved paint alpha and original sources.
- Done: folder Filter/Levels adjustments over isolated children or passthrough lower accumulation,
  before folder mask/opacity. Folder Paint/Fill source effects remain rejected.
- Done: schema 9, explicit variants, identity migration, project/Smart Material round trips,
  invalid input rejection; masks share the Smart Mask schema. See ADR-0029.
- Done: numeric HSL/lightness/data/signed-height/gradient goldens, order/opacity/alpha/folder
  output, incremental tiles through filters, undo/reopen/latest-worker/export convergence, UI.

Updated app: E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe. Use + Add → Filter
under the selected content/mask. For a stack adjustment add an empty passthrough folder.
Native CLI captures (512, both actual crate sets, all exit 0, inspected): docs/screenshots/
filter-hsl.png, filter-contrast.png, filter-clamp.png, filter-threshold.png, filter-folder.png.
Developer fixture/profile: crates/app/examples/filters.rs; CLI --effect-panel selects last effect.
No physical mouse driven. Manual slider/tablet, production assets, browser/DPI and 4K GPU
frame pacing were NOT hand-verified. Nonidentity HSL is display-referred and clamps HDR input.
4K CPU report: docs/performance/2026-10-19-local-filters-4k.txt. Cold 1470 ms; poll p95 0.711,
max 3.675 ms; first-edit-to-settle 1003.1 ms, 58 frames. GPU/upload/render/strokes excluded.
This different workload cannot establish wear speedups; its earlier convergence issue remains.

Partial stops: materials/filter.rs has local kernels only (previous stage, same texel, halo 0).
compositor/tile.rs::apply_content_effects applies adjustments; no cross-tile stage graph exists.
Do not fake Blur/Sharpen inside each 64px tile. Render-owned GPU filter processing, references,
anchors, effect thumbnails/drag reorder, Dirt/strict lower normals/micro toggles and emission
remain pending. Preserve alpha, raw data/color rules, folder stages and exact sync export.
Open question for owner: approve current filter controls before expanding the filter library.

Unrelated changes retained: Cargo.lock, crates/ui/Cargo.toml, crates/ui/examples, artworks/Dead-Man.
Protected zips/output/generate_textures.py/__pycache__ untouched. Prior cache deletion was
rejected by approval review; do not bypass via another command. Coordinate shared files if
Claude starts wear work in this checkout; never overwrite or stage another developer's edits.

Next 3 steps:
1. Claude reproduces unresolved wear from the prompt; owner checks local filters in updated app.
2. Add previous-stage tile neighborhoods/halos and complete Blur/Sharpen, numeric + native.
3. Profile full 4K desktop frame pacing/worker convergence; continue normal-aware Dirt/references.
