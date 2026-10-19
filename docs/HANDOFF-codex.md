# Handoff — albedo interpolation (2026-10-19)

- Branch `feature/brush-roadmap`; implementation commit **4ac31a0**. This handoff is in the
  final docs commit (`git log -1`). **429 workspace tests passed, 0 failed/ignored** (6 added),
  Clippy workspace/all-targets exits 0 with warnings. CARGO_INCREMENTAL=0,
  CARGO_PROFILE_DEV_DEBUG=line-tables-only; -j 1. No push/release or branch switch.

Done:
- Per-texture-set Bilinear/Nearest Albedo controls beneath Resolution in 3D and Texture Sets;
  shared app command, undo/redo, no-op preservation. Narrow 3D docks use View menu.
- Source Base Color image filtering affects fills/content fills, future textured strokes,
  working-resolution paint resampling and exports. Masks/other channels retain old behavior.
- Cache/thumbnail invalidation includes the setting; unchanged PBR channels retain GPU tiles.
- Per-material GPU Base Color sampler; Material/unlit Base Color and 2D magnification agree.
  Samplers/textures/pipelines reused. CPU/GPU numeric tests and real exported PNG checks.
- Schema **6** writes albedo_filter. Schemas 1–5 migrate absent fields to Bilinear. New saves
  need this reader or newer; published 0.1.7 cannot read schema 6. Existing projects still open.
- Reproducible fixture: crates/app/examples/retro_albedo.rs. Comparison projects and exports
  in %TEMP%/subcraft-albedo-native. A four-color 16-square source exports 4 colors in Nearest
  and 180 in Bilinear at 128 square. Tests also cover both filters in all 3 brush projections.
- Native screenshots inspected: docs/screenshots/albedo-bilinear-3d.png,
  albedo-nearest-3d.png, albedo-nearest-2d.png. Native project loading/CLI, no mouse input.
- Fixed known test-only image-save timing flake: bounded real-time wait instead of busy frames.
  Responsive acceptance updated for the two-row left column; 200–1280 widths pass.

Updated test app: E:\Applications\ArtCraft\SubCraft\target\debug\subcraft.exe.
Owner's open target/release/subcraft.exe, original artwork and protected root files unchanged.
No partial work, remaining failures or skipped tests. Not hand-checked: physical dropdown/pen
input, owner's pixel assets, external engine imports, browser, 4K long strokes/frame pacing.
Prior high-poly bake regressions pass; owner's exact high/low pair remains unverified.

Keep ADR-0020 decisions: document state rather than viewport preference; Base Color only;
point sampling before compositing/export; color/alpha rules; schema migration; cache reuse.
Existing blurred paint cannot be recovered by a filter change. Opacity, soft brush and triplanar
blends remain intentional. Exported PNGs do not configure engine sampling: choose nearest/point
there too. No PS1 geometry wobble/affine mapping/dithering/quantization feature is implied.

Next 3 steps:
1. Owner tests patched debug app with their pixel assets and exact high/low bake pair; save work
   before changing app windows. Rebuild normal release executable when it is not in use.
2. Check real engine imports and native 4K strokes/bakes with this setting; no 4K claim yet.
3. Resume layer-system-roadmap stage/halo contracts and tested filters, then Compare Mask and
   anchors. Brush phase order unchanged. Existing slider split-undo issue remains outside scope.
