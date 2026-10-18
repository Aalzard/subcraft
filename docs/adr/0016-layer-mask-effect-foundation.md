# ADR-0016 — Layer domains, evaluated previews and extensible effect evaluation

Status: Accepted foundation; content-effect/filter/reference stages below are planned.
Date: 2026-10-18.

## Workflow research

Adobe's official documentation establishes the behavior to reproduce:

* [Masking and effects](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/masking-and-effects):
  separate selectable content/mask thumbnails and effect stacks, grayscale masks, temporary
  disabling, copying, and isolated mask preview. Smart Masks retain reusable effect setups.
* [Effects](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/effects/effects):
  ordered editable operations with opacity/blend controls, separate from original content.
* [Filters](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/effects/filters/filter):
  transforms on a layer/mask; passthrough can process the accumulated layers beneath.
* [Compare Mask](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/effects/compare-mask):
  compares channels from lower layers, this content, current mask or constants, with tolerance.
* [Anchor points](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/effects/anchor-point):
  named stack-stage references within one texture set, with ordering constraints and dependents.

These are workflow requirements, not a prescription to clone Adobe's implementation or import
its proprietary Substance execution engine.

## Existing code audit

Keep `doc::Layer` (paint/fill/folder), per-channel blends, UUIDs and tiled COW snapshots; fill
sampling/projections; `compositor::mask`'s bottom-to-top effect evaluation; app stroke/history,
resources, project blobs/schema migrations and Smart Material/Mask serialization. Existing
mask effects already cover paint rasters, fill, images, generators, levels and inversion.

Missing foundations: content effects; an explicit selected domain/effect address; layer-tree
effect rows and individual effect Properties; cross-tile neighborhoods; dependency tracking for
references; evaluated per-layer previews. The mask-only enum and one-tile compositor cannot
simply grow a blur/anchor match arm: both need input-stage and dependency-aware evaluation.

## Decisions for the first slice

`app::LayerTarget` distinguishes content from mask. Selecting a thumbnail dispatches an app
command, updates the existing brush target and does not edit the document/history. Selection
cannot point to an absent mask. Inspector focus includes this domain, not just the layer UUID.

Mask copy retains an immutable COW snapshot within the current session. Paste uses fresh effect
UUIDs, resamples paint rasters to the destination resolution and is one undoable edit. Invert
appends the existing editable inversion operation; original paint/fill/generators stay intact.
Mask disable/removal remain undoable, and removal returns the paint target to content.
No new persistent data is needed for these commands: project schema remains 4.

Evaluated thumbnails belong to app/compositor, not widgets. They sample the production
evaluator on a bounded UV grid; content previews ignore their own mask/opacity/visibility,
while mask previews show the authored effect result even when disabled. Color previews encode
linear color for display; data/masks stay raw. UI owns only texture upload and presentation.
Desktop work must use immutable background jobs and bounded caches, rejecting obsolete results
after paint, edits, mesh-map changes or session replacement. No 4K texture readback per row.
The first previews use 64px grids (256px for isolated UV inspection): existing rasters are
point-sampled and procedural inputs are evaluated on that grid. These are reduced previews,
not authoritative full-resolution images; very fine marks can disappear. Full-resolution
isolated views/mip-aware filtering belong to the subsequent render/effect-cache work.

## Planned architecture for subsequent complete slices

* Add typed effect addresses `(texture_set, layer, domain, effect UUID)`, separate content/mask
  stacks, shared effect controls and domain-specific payloads. Paint effect selection must route
  strokes to that effect's raster, never accidentally to the layer/mask base. Migration to a
  new schema must preserve v1–4 documents and numerically reproduce old mask evaluation.
* Introduce kernel metadata for channel semantics, input stages, required maps and dependency
  radius. A tile graph caches each stage/channel, expanding dirty sets for spatial halos.
  Local adjustments, neighborhood filters and global/iterative operations have explicit
  execution contracts. Do not evaluate a multi-tile blur independently inside each 64px tile.
* Preserve deterministic CPU kernels and numeric tests as the save/export/reference path.
  GPU kernels and transient textures live behind a render-owned backend; app/doc/paint retain
  no wgpu/egui dependency. Schedule work with bounded publication/upload budgets. Measure real
  end-to-end 4K frame time before claiming interactive performance.
* Anchor outputs capture a named evaluation stage, not a mutable flattened bitmap. References
  initially stay in one texture set and point backward in evaluation order; validate cycles,
  missing IDs, moves/deletion and copy remapping. Compare Mask consumes explicit channel inputs
  through the same graph. Future broader references require a deliberate cycle policy.

Do not ship placeholder filters or pretend that saving an enum provides evaluation support.
Each complete operation needs numeric output, boundaries/channel behavior, dirty propagation,
undo/save migration tests and real-app evidence. See `../layer-system-roadmap.md` for delivery.
