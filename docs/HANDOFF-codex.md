# Handoff — material workflow, first complete batch (2026-10-19)

- Branch `feature/brush-roadmap`; implementation **8c0bd67**. Docs are the final commit
  (`git log -1`). **446 workspace tests pass, 0 failed/ignored** (10 added); workspace/all-target
  Clippy exits 0 with existing warnings. CARGO_INCREMENTAL=0, dev debug=line-tables-only, -j 1.
- Done: Pattern size, Seed/Next, organic volume breakup; new Edges/Cavity starts at 0.3.
- Done: fill/Fill-effect/brush channel buttons, independent click/Alt-solo and hidden rows.
- Done: high-source preview auto refresh; successful bakes return to Material.
- Done: red exact symmetry surface/plane guides and visibility preference; Rotation under
  Tiling; Albedo filtering exclusively in Texture Sets (sampler/export behavior retained).
- Current test executable: E:\Applications\ArtCraft\SubCraft\target\debug\subcraft.exe.
  Installed/shipping applications and target/release were not rebuilt. No push or release.

Native screenshots inspected: docs/screenshots/generator-organic-controls.png,
generator-organic-large.png, generator-organic-seed-8.png,
material-channel-toggles-symmetry-x.png, material-symmetry-z.png, bake-complete-material.png.
RTX 4060 / DX12, CLI only; no mouse input. Fixture source: refined artwork, unchanged.
Reproduce with crates/app/examples/generator_breakup.rs and --generator-panel / --symmetry x|z.
Data/fixtures: %TEMP%/subcraft-material-workflow-native. 4K profiling writes no project copies.
Debug exact synchronous cold recomputes: organic 14–28 s, legacy 28.5 s; unchanged 0–0.65 ms.
These are uncontrolled debug timings, not release or native frame/long-stroke performance.

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

Not hand-verified: physical UI gestures, native file replacement (actual-file app unit test
passes), Y/multiple axes, large-mesh first-guide latency, owner assets, browser, full 4K bake,
long strokes/frame pacing. Initial symmetry intersections compute synchronously once per scene.

Next 3 steps:
1. Complete shared surface-distance and stage/halo contracts for Edges/Dirt, then numeric tests
   across seams, thin/curved meshes and dirty updates. Owner's new scope has priority.
2. Implement strict lower-layer normal dependencies + Disable Micro Effects and paint-channel
   visibility with reorder/undo/save tests. Use layer-system-roadmap/ADR-0016 foundations.
3. Implement emission HDR/post-processing and persistence/UI, then GPU parity/native captures.
   Decide per-layer versus per-set glow scope and distance units explicitly; the owner hasn't
   specified those details. Older brush phases remain pending after this newer checklist.
