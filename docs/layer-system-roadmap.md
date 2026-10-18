# Layer, mask and effect roadmap

Owner priority (2026-10-18) supersedes the remaining brush roadmap until the foundational layer
work is complete. `[x]` complete, `[~]` partial with remaining work, `[ ]` not started. Architecture
audit and official Adobe workflow references: ADR-0016. Completion below is updated per session.

## 1. Thumbnails and basic masks

- [x] Evaluated square content thumbnails for paint/fill/folders, using the active channel.
- [x] Adjacent resolved grayscale mask thumbnails, updated by painting and procedural effects.
- [x] Independent content/mask selection from the thumbnails (selection is not an undo step).
- [x] Existing white/black masks on paint/fill/folders; existing disable/remove with undo.
- [x] Copy/paste and non-destructive evaluated-mask inversion, with context-menu and Properties
  controls. Paste resamples and gives effects new UUIDs.
- [~] Isolated 256px UV mask preview and thumbnail shortcuts (Alt/Shift+click); full-resolution
  2D/3D isolated mask display later.
- [x] Background, bounded preview caching with stale-result rejection and no 4K row readbacks.

## 2. Distinct content and mask effect stacks

- [x] Existing mask source/effects: paint raster, fill, image, generator, levels, invert.
- [x] Separate content/mask containers (ADR-0017): `ContentEffect` (per-channel blend, Paint/
  Fill/Levels) vs `MaskEffect`. Schema 5; v1–4 load unchanged (tested). Sources vs adjustments
  explicit (adjustments keep coverage); references still to come.
- [x] Typed effect selection (`SelectEffect`, transient) and stroke routing into selected content
  and mask Paint effects (one undo per stroke, base untouched, per-tile dirty diffing).
- [~] Paint and fill layers have content stacks; folders rejected until the stage graph exists.
  Levels rejected on Normal.
- [~] Levels on content; Generator/Levels/Invert on masks. Filter/Compare Mask/Anchor Point not yet.

## 3. Nested hierarchy and editing

- [~] Selected layer shows its selected domain's effects beneath its row. No per-layer
  expand/collapse for unselected layers yet.
- [x] Reorder/duplicate/delete/visibility for both stacks from the nested rows (context menu).
- [~] Disable, rename (Properties), duplicate, delete, per-channel blend/opacity done; drag
  reorder of effect rows not yet (context-menu Move up/down instead).
- [x] Effect-specific Properties for content and mask effects, independent of the layer material.
- [~] Layer/mask thumbnails and model update live; per-effect thumbnails remain to be added.
- [ ] Assess whether a separate effect editor window improves the existing dock workflow.

## 4. Filter evaluator and operations

- [ ] Tile-stage graph, neighborhood halos, cache/dirty propagation and CPU reference kernels.
- [ ] Render-owned GPU backend with measured parity and bounded upload/publication work.
- [ ] Blur, Directional Blur, Sharpen, Highpass.
- [ ] Levels, Contrast, HSL, Color Balance, Invert on supported content channels/masks.
- [ ] Bevel, Bevel Smooth, Mask Outline, Height Adjust.
- [ ] Histogram Scan/Shift, Threshold, Clamp.
- [ ] Warp, Gradients and an extensible parameter/kernel registry.
- [ ] Normal renormalization, raw data versus linear-color semantics, alpha and UV boundary policy.

## 5. Materials and references

- [x] Existing independent material channels; fill textures/procedurals, UV/planar/triplanar,
  tiling and projection parameters. Existing reusable Smart Materials and Smart Masks.
- [~] Masks combine base painting and existing paint/fill/generator/levels/image/invert effects;
  general neighborhood filters remain.
- [ ] Compare Mask with current content/mask/lower-stack/constant inputs and tolerance controls.
- [ ] Anchor stage outputs, reference picker/dependents, cycle/order validation and copy remapping.
- [ ] Smart asset compatibility for new stacks, filters and references.

## 6. Acceptance and delivery gates

- [ ] Per-slice numeric/golden output, halos/boundaries, undo/stroke atomicity, save/load and
  legacy migration fixtures. Full workspace tests and Clippy before each commit.
- [ ] Native screenshots and explicit hand-verification limits for each visible slice.
- [ ] Long 4K paint → effects → composite → thumbnails → GPU frame profiling on complex models.
- [x] Desktop background compositing: exact convergence, latest edit wins, synchronous export,
  sparse GPU uploads and native 4K demo generator/pattern-scale profiling (ADR-0018). Complex
  production-model stroke profiling remains the separate gate above.
- [ ] Getting-started instructions, ADR decisions and a short handoff after each session.

Next complete slices: neighborhood/stage contracts for filters and folder content effects;
then local adjustments and blur/sharpen with CPU goldens and GPU parity. Finish per-effect
thumbnails, drag reorder and native save/reopen checks alongside these slices. Compare Mask
and anchors follow the stage graph. General filters are not part of the 0.1.7 release.
