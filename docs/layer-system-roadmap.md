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
- [~] Paint/fill content stacks; folders now accept Filter/Levels adjustments (isolated
  children or passthrough lower accumulation). Folder Paint/Fill sources remain rejected.
  Levels/these filters reject Normal; HSL accepts color only.
- [~] Local and neighborhood Filters/Levels on content; Generator/Levels/Invert/Filters on masks.
  Compare Mask/Anchor Point and the remaining filter operations are pending.

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

- [x] Immutable previous-stage CPU tile graph, shared local kernels, bounded parallel stage cache
  and conservative chained dirty halos for Blur/Sharpen and Directional Blur/Highpass (ADRs 0030-0032).
- [ ] Render-owned GPU backend with measured parity and bounded upload/publication work.
- [x] Gaussian Blur and unsharp Sharpen on content/masks and folder adjustments.
- [x] Directional Blur (UV angle, premultiplied bilinear samples) and Highpass (neutral detail,
  original coverage, signed Height) on both domains and adjustment folders.
- [~] Levels, Contrast and HSL on supported content; Levels/Contrast and existing Invert on
  masks. Color Balance and content Invert remain pending.
- [x] Contour Bevel (inward relief/Amount) and Mask Outline (Inner/Outer/Both), with threshold,
  UV radius, bounded Euclidean distance stages and preserved content coverage (ADR-0032).
- [ ] Bevel Smooth and Height Adjust.
- [~] Threshold/Clamp on content and masks; Histogram Scan/Shift remain pending.
- [ ] Warp, Gradients and an extensible parameter/kernel registry.
- [~] Linear/raw channel semantics, premultiplied Blur coverage, Sharpen coverage preservation
  and UV image-edge clamping specified/tested. Normal filters and mesh-aware seam/island policies
  remain pending.

## 5. Materials and references

- [x] Existing independent material channels; fill textures/procedurals, UV/planar/triplanar,
  tiling and projection parameters. Existing reusable Smart Materials and Smart Masks.
- [~] Masks combine base painting and existing paint/fill/generator/levels/image/invert effects;
  Blur/Sharpen/Directional Blur/Highpass/Bevel/Mask Outline work in the same stack; remaining operations pending.
- [ ] Compare Mask with current content/mask/lower-stack/constant inputs and tolerance controls.
- [ ] Anchor stage outputs, reference picker/dependents, cycle/order validation and copy remapping.
- [~] Schema-12 smart filter stacks round-trip; reference compatibility remains pending.

## 6. Acceptance and delivery gates

- [~] Current local/neighborhood filter slices have CPU goldens, halos/boundaries, undo, save/load,
  worker/export and migration fixtures. Remaining slices/GPU parity pending. Workspace tests
  and Clippy run before each commit.
- [~] Native captures and explicit limits recorded for current filters; repeat for future slices.
- [ ] Long 4K paint → effects → composite → thumbnails → GPU frame profiling on complex models.
- [x] Desktop background compositing: exact convergence, latest edit wins, synchronous export,
  sparse GPU uploads and native 4K demo generator/pattern-scale profiling (ADR-0018). Complex
  production-model stroke profiling remains the separate gate above.
- [~] Guide, ADRs and handoff updated through session 88; maintain for each future slice.

Next complete slices: per-effect thumbnails/drag reorder, render-owned GPU parity and full
desktop profiling; the six neighborhood filters have CPU stage/halo goldens and native evidence.
Finish per-effect
thumbnails, drag reorder and native save/reopen checks alongside these slices. Compare Mask
and anchors follow the stage graph. General filters are not part of the 0.1.7 release.
