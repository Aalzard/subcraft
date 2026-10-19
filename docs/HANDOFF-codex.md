# Handoff — charcoal UI and layer hierarchy (2026-10-19)

- Branch `feature/brush-roadmap`; implementation **08e28ea**.
  Docs are the final commit (`git log -1`). **449 workspace tests pass, 0 failed/ignored**
  (3 added this session); workspace/all-target
  Clippy exits 0 with existing warnings. CARGO_INCREMENTAL=0, dev debug=line-tables-only, -j 1.
- Done: neutral charcoal palette, warm gray text, restrained amber, thin circular-handle sliders.
- Done: larger independent content/mask previews, two-line layer rows and quiet blend/opacity.
- Done: compact connected effect rows and per-domain collapse without document/selection edits.
- Current test executable: E:\Applications\ArtCraft\SubCraft\target\debug\latest\subcraft.exe.
  Owner's debug/subcraft.exe process (PID 9712) blocked replacement; left it running. Build with
  cargo rustc -p subcraft --bin subcraft -- -o target/debug/latest/subcraft.exe. Installed/release
  applications and owner preferences were not changed. No push or release.

Native screenshots inspected: docs/screenshots/charcoal-layer-effects.png (real baked CORE-07,
mask Levels/generator/properties), charcoal-brush-workspace.png (sample, Parts/History and
brush material controls). CLI only; no mouse input. Create a new fixture
with app/examples/layer_layout.rs from %TEMP%/subcraft-material-workflow-native/fixtures/Organic-default.subcraft,
then run --generator-panel --screenshot. New fixture: %TEMP%/subcraft-theme-native/Layer-layout.subcraft.
Source artwork is unchanged. Previous session's material/bake screenshots remain in screenshots/.
Debug exact synchronous cold recomputes: organic 14–28 s, legacy 28.5 s; unchanged 0–0.65 ms.
These are uncontrolled debug timings, not release or native frame/long-stroke performance.

Keep ADR-0024: UI only; 48-point layer/36-point thumbnail/26-point effect rows. Reuse the async
64-pixel preview cache; no compositor or schema changes. Collapsing effects must not disable
them or change selection. Text contrast >=4.5:1 for primary/secondary text; near-square buttons.
Selection.stroke is also egui's selected-label text: keep it dark against amber selection
fill so channel toggles/menu entries remain legible. Toolbar retains 26-point icon alignment.

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

Not hand-verified: physical UI gestures/folding/sliders, alternate DPI/monitors, native file replacement (actual-file app unit test
passes), Y/multiple axes, large-mesh first-guide latency, owner assets, browser, full 4K bake,
long strokes/frame pacing. Initial symmetry intersections compute synchronously once per scene.

Next 3 steps:
1. Owner reviews the new charcoal preview. After they close the old executable, a normal
   workspace build can refresh debug/subcraft.exe; do not close unsaved owner sessions.
2. Complete shared surface-distance and stage/halo contracts for Edges/Dirt, with numeric
   tests across seams and thin/curved meshes; then strict lower-normal dependencies/Disable
   Micro Effects and paint-channel visibility. Use layer-system-roadmap/ADR-0016 foundations.
3. Implement emission HDR/post-processing and persistence/UI, then GPU parity/native captures.
   Decide per-layer versus per-set glow scope and distance units explicitly; the owner hasn't
   specified those details. Older brush phases remain pending after this newer checklist.
