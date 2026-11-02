# ADR-0041 — ID Color Mask

Status: Accepted (2026-10-30)

## Context

The baker already writes an ID map (`MeshMapKind::Id`, one color per connected mesh element).
Users want to mask a layer to "these parts" by picking their ID colors, with a tolerance, rather
than painting the mask or relying on parts/material domains.

## Decision

* **A new mask effect kind, not a `GeneratorKind` variant.** `MaskEffectKind::IdColor(IdColorMask)`
  holds `colors: Vec<[f32; 3]>`, `tolerance`, `softness` and `invert`. `Generator` is `Copy`,
  carries noise/level/direction fields that mean nothing here, and its threshold-on-one-value
  model does not fit a set of colors. A separate kind keeps its own Properties editor, schema
  variant and validation, and reuses the mask stack's blend/opacity/visibility/reorder.
* **Matching:** Euclidean RGB distance to the nearest picked color on the raw stored ID values
  (data, never color-transformed). `d <= tolerance` → 1; beyond that a smoothstep falloff over
  `softness` (0 = hard edge). No colors selects nothing. Limits: ≤ 64 colors, tolerance and
  softness in 0..1, finite colors; invalid data is rejected by commands and the project loader.
* **Evaluation:** a per-tile local kernel in `compositor::mask` (rayon per tile like the other
  generators; no halo). The ID map is read through `MeshMaps::read_tile` (point-sampled when its
  resolution differs). Dirty tracking needs nothing new: parameters compare by value, and baking
  already invalidates the texture set.
* **No baked ID map:** the effect outputs opaque black (even when inverted: nothing can be
  selected), and Properties shows a hint to bake one. The eyedropper reports it in the status bar.
* **Picking:** `Session::pick_id_color` ray-picks the model (existing BVH `pick`), point-samples
  the ID map at the hit UV (bilinear would invent colors at region borders) and dispatches
  `Command::SetIdColors`. That command never coalesces, so every pick and swatch removal is its
  own undo step; slider edits use the existing coalescing `SetMaskEffect`. The viewport context
  menu "Mask by ID color" adds a black mask + ID Color Mask with the color under the cursor in
  one undo step.
* **Persistence:** project schema 14 adds `{"type":"id_color","id_color":{…}}` to mask effects.
  Schemas 1–13 cannot contain it and load unchanged; older SubCraft versions refuse schema-14
  files. Smart Mask fragments carry it through the same schema.

## Consequences

The effect works with any baked ID map, including future ID sources (material color, vertex
color, imported ID textures) as long as they are stored as `MeshMapKind::Id`. Matching in a
perceptual space or by hue is a possible later option and would need a new field with a default.
