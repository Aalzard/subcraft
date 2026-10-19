# Handoff — shelf wrapping and preview sizing (2026-10-19)

- Branch `feature/brush-roadmap`; implementation **db2ea03**.
  Docs are the final commit (`git log -1`). **454 workspace tests pass, 0 failed/ignored**
  (5 added this session); workspace/all-target
  Clippy exits 0 with existing warnings. CARGO_INCREMENTAL=0, dev debug=line-tables-only, -j 1.
- Done: shelf grid constrained to dock width, compact category header and one-line names.
- Done: removed duplicate dock scrollbars for Shelf/Layers; bounded lists scroll vertically.
- Done: independent saved preview sizes with small fixed footers; layer rows grow/cap to fit.
- Current test executable: E:\Applications\ArtCraft\SubCraft\target\debug\subcraft.exe.
  Ordinary workspace bins/examples build works again; older latest/ and release binaries were
  not refreshed. No owner processes were stopped or artwork/preferences altered; no push/release.

Native screenshot: docs/screenshots/shelf-layer-preview-controls.png. Real baked CORE-07,
vertical shelf and both fixed size sliders beside mask Levels/generator. Reproduce with
%TEMP%/subcraft-theme-native/Layer-layout.subcraft --generator-panel --shelf-panel --screenshot.
--shelf-panel is an optional narrow-dock validation layout; ordinary startup retains the
owner's layout. Isolated data/library: %TEMP%/subcraft-preview-native. CLI only, no mouse input.
Source artwork is unchanged. Prior charcoal/material/bake screenshots remain in screenshots/.
Debug exact synchronous cold recomputes: organic 14–28 s, legacy 28.5 s; unchanged 0–0.65 ms.
These are uncontrolled debug timings, not release or native frame/long-stroke performance.

Keep ADR-0024: UI only; default 48-point layer/36-point thumbnail/26-point effect rows. Reuse the async
64-pixel preview cache; no compositor or schema changes. Collapsing effects must not disable
them or change selection. Text contrast >=4.5:1 for primary/secondary text; near-square buttons.
Selection.stroke is also egui's selected-label text: keep it dark against amber selection
fill so channel toggles/menu entries remain legible. Toolbar retains 26-point icon alignment.
Shelf/Layer preview preferences default 88/36; old files use serde defaults. Sizes are display
only. Layers cap the effective size in narrow docks. Do not re-enable outer dock scrollbars
for Shelf/Layers: these cause content-width feedback and allow the footer to scroll away.

Keep ADR-0022/0023: schema **7**, missing organic_breakup migrates to false in old projects /
recipes; do not globally change old masks. Reuse the bounded field cache. Pattern size=100/scale.
Channel solo is one app history operation; disabled sources/strengths/images are retained.
Brush channel toggles control future strokes, not historical paint visibility. Guide coordinates
use the same whole-model center as mirrored painting; show-guides is a Viewport preference.
Use shader clip-depth offset for LineList (wgpu rejects pipeline depth bias). Render headless
GPU tests hold a per-process guard after parallel DX12 devices/readback stalled; production
rendering is unchanged. Every final test passed; no skipped tests or remaining compilation bugs.

Partial stops — the rest of the owner's request is in material-workflow-roadmap.md:
- compositor/src/mask.rs::eval_generator still thresholds baked maps. True surface-distance
  edge/dirt spread, increasing-Level spread semantics and near-edge intensity are NOT done.
  Cavity is displayed as Dirt and has new pattern controls; it is not the full requested tool.
- compositor/src/tile.rs::apply_layer evaluates masks without an explicit lower-normal stage.
  Add ordered stage inputs, cross-tile/seam halos and cache dependencies before automatic
  painted micro-normal detection/Disable Micro Effects. Do not read the final composite.
- render/src/viewport.wgsl::fs_main only adds existing Emissive before tonemapping. Emission
  glow/glare/starburst and their document/UI controls remain unimplemented. Reuse Emissive.
- Paint-layer channel visibility/solo is still missing; current brush buttons do not implement it.

Not hand-verified: physical mouse/wheel/slider gestures, alternate DPI/monitors, native file replacement (actual-file app unit test
passes), Y/multiple axes, large-mesh first-guide latency, owner assets, browser, full 4K bake,
long strokes/frame pacing. Initial symmetry intersections compute synchronously once per scene.

Next 3 steps:
1. Owner checks shelf browsing and both size sliders in debug/subcraft.exe, including their
   own saved dock layout and DPI. Automated actual-dock/scroll/resize/persistence tests pass.
2. Complete shared surface-distance and stage/halo contracts for Edges/Dirt, with numeric
   tests across seams and thin/curved meshes; then strict lower-normal dependencies/Disable
   Micro Effects and paint-channel visibility. Use layer-system-roadmap/ADR-0016 foundations.
3. Implement emission HDR/post-processing and persistence/UI, then GPU parity/native captures.
   Decide per-layer versus per-set glow scope and distance units explicitly; the owner hasn't
   specified those details. Older brush phases remain pending after this newer checklist.
