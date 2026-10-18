# ADR-0017 — Separate content effect stacks, effect selection and Paint-effect strokes

Status: Accepted (2026-10-18). Extends ADR-0016; neighborhood filters, Compare Mask and anchor
points remain planned there.

## Context

Until now only masks had an effect stack (`doc::MaskEffect`). A layer's content was its base
paint rasters or its fill material, with no way to add non-destructive operations on top of it,
and strokes could only reach a paint layer's base rasters or a mask's base raster. Painter-style
workflows need both domains to own independent, reorderable stacks, and a selected Paint effect
must receive the brush — including on fill layers.

## Decision

* `doc::ContentEffect` is a separate type from `MaskEffect`: content effects work on multi-
  channel material data (per-channel blend/opacity like layers; normals combine), mask effects
  on one grayscale value. Sharing one enum would force every kernel to handle both semantics
  and invite applying color operations to masks or masks operations to normals. Shared parts
  (UUID, name, visibility, reorder/duplicate/delete commands, row UI) are kept parallel.
* `Layer::content_effects` holds the stack, bottom to top. Evaluation (`compositor::tile`)
  composites each visible effect onto the layer's own content, per channel, before the layer's
  mask, blend and opacity, so content thumbnails show it and the base content never changes.
  * Sources (Paint, Fill) composite with the effect's channel blend: coverage comes from the
    effect's alpha.
  * Adjustments (Levels) transform the content below and keep its coverage: they never make
    transparent paint visible. Levels is rejected on the Normal channel (vector data).
  * Folders are rejected for now: their content is the children's composite, and passthrough
    semantics for filters need the stage graph of ADR-0016.
* Selection is a transient typed address: `Session::selected_effect()` + `LayerTarget`
  (`Command::SelectEffect`). It is never project data and never an undo step. Selecting a Paint
  effect routes strokes to it (`PaintTarget::ContentEffect` / `PaintTarget::MaskEffect`): the
  stroke writes that effect's rasters only, as one undo entry, from its pre-stroke snapshot.
  Selecting any other effect keeps painting on the domain's base.
* Dirty diffing: effect parameters compare by value, Paint-effect rasters by layout and
  changed tiles (the same rule as base paint), so painting into an effect refreshes only the
  touched tiles instead of the whole texture set. This applies to mask Paint effects too.
* Project schema 5 adds `content_effects` to every layer (absent = empty). Files of schema 1–4
  load unchanged and evaluate bit-identically (tested); older SubCraft versions refuse schema-5
  files instead of silently dropping effects. Smart materials carry content effects because they
  reuse the layer schema.

## Consequences

* New content-effect kinds add a `ContentEffectKind` variant, its kernel in the compositor,
  its schema variant and its Properties editor. Kernels that need neighbors (blur, bevel…) must
  wait for the stage graph; they must not be faked inside one 64px tile.
* Copying a layer duplicates effect IDs freshly; pasting a mask keeps using fresh IDs.
