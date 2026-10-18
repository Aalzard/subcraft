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
- [ ] Shared effect metadata and separate content/mask containers; schema migration preserving
  old files and numerical output. Sources vs transforms vs references remain explicit.
- [ ] Typed effect selection and stroke routing into selected Paint effects.
- [ ] Full primary paint/fill/folder workflows and appropriate effect compatibility rules.
- [ ] Generator/Levels/Filter/Compare Mask/Anchor Point on supported domains with working kernels.

## 3. Nested hierarchy and editing

- [ ] Selected domain's effects beneath its layer; expand/collapse per domain.
- [~] Mask reorder/duplicate app commands implemented; nested UI and full lifecycle still needed.
- [ ] Disable, rename, drag reorder, duplicate, delete and per-channel blend/opacity controls.
- [ ] Effect-specific Properties, independent of parent material; generator vs bevel controls.
- [ ] Realtime evaluated thumbnails and model updates, including undo/redo and session changes.
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
- [~] Masks combine base painting and existing fill/generator/levels/image/invert effects;
  paint-effect targeting and general filters remain.
- [ ] Compare Mask with current content/mask/lower-stack/constant inputs and tolerance controls.
- [ ] Anchor stage outputs, reference picker/dependents, cycle/order validation and copy remapping.
- [ ] Smart asset compatibility for new stacks, filters and references.

## 6. Acceptance and delivery gates

- [ ] Per-slice numeric/golden output, halos/boundaries, undo/stroke atomicity, save/load and
  legacy migration fixtures. Full workspace tests and Clippy before each commit.
- [ ] Native screenshots and explicit hand-verification limits for each visible slice.
- [ ] Long 4K paint → effects → composite → thumbnails → GPU frame profiling on complex models.
- [ ] Getting-started instructions, ADR decisions and a short handoff after each session.

Next complete slices: finish section 1; introduce persistent effect-domain/evaluation contracts;
then nested mask effect selection and Paint targeting, before adding neighborhood filters.
