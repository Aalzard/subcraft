# Handoff — bake inspection and live maps (2026-10-19)

- Branch `feature/brush-roadmap`; implementation **e5ae217**. Docs are the final commit
  (`git log -1`). **436 workspace tests passed, 0 failed/ignored**, seven added. Workspace /
  all-target Clippy exits 0 with existing warnings. Required no-incremental/line-tables-only
  environment used, -j 1. No branch switch, push or release.
- Done: async UV-less high-poly preview, blue high/red exposed low, front/rear cage wireframe.
- Done: each completed padded bake map installs independently and displays on low-poly UVs;
  Follow bake, manual map/Material choice, live-map toggle and current-map progress.
- Done: geometry cache source validation/reload, scene rejection, cancellation/drop handling;
  immutable cached GPU geometry and separate raw-map bind groups preserve authored materials.
- Updated test executable: E:\Applications\ArtCraft\SubCraft\target\debug\subcraft.exe.
  Published/target release executable was not rebuilt; use this debug app for the new controls.

Native RTX 4060 / DX12 checks (CLI only, no mouse): docs/screenshots/bake-high-low-overlay.png;
bake-live-normal-4k.png (Normal shown while World Normal computes); bake-live-ao.png (AO while
Curvature computes); bake-live-complete.png (all seven maps at 256 square, follows Thickness).
Source: artworks/CORE-07-Refined/models/CORE-07-low.obj + %TEMP%/subcraft-bake-native/
CORE-07-high-no-uv.obj (442,368 triangles). Isolated data: %TEMP%/subcraft-bake-preview-native.
Pinned screenshot runs quit during baking; only the 256-square run completed all seven.
Not physically checked: owner source pair, cage/reload/dropdown/close gestures, production
FBX/GLB, browser, full seven-map 4K completion or long navigation/frame-pacing. GPU tests cover
material restoration. No remaining test failures/skips or uncommitted implementation work.

Keep ADR-0021: preserve imported coordinates; never silently realign meshes. Red/blue compares
visible surfaces, NOT every cage/projection miss. Geometry and raw-map display are transient;
schema remains 6. Complete padded maps only, same exact synchronous bake output. Cancel keeps
completed maps; Normal may already have created its undoable live display layer (ADR-0019).
File cache reuse checks path/length/mtime. Mesh-map storage still follows existing non-history
save rules. Closing Bake restores the regular view without overwriting viewport preferences.

Partial scope stops: crates/render/src/bake_preview.rs::BakePreviewPass::draw does depth
comparison only; full cage-error heatmap/custom cages/matching are pending. crates/bake/src/
lib.rs::bake_streamed publishes complete maps only; intra-map tile streaming and Height mesh-map
baking remain unimplemented. Ask owner whether Height and a full projection-error heatmap take
priority over the pending layer filters. No new brush phase was marked complete.

Next 3 steps:
1. Owner tests this debug app with the original model pair: inspect, adjust cage, bake, select
   maps, close to Material; then measure full 4K baking/navigation before performance claims.
2. Add numeric projection-miss/cage diagnostics and matching/custom cages; extend this display
   when the corresponding baker data exists. Height and intra-map tiles need separate contracts.
3. Resume layer-system-roadmap stage/halo filters, Compare Mask/anchors, then brush Phase 1.
   Existing split-slider undo issue and other earlier handoff limits remain outside this change.
