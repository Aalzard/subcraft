# ADR-0011 — Image sources for brush channels

Status: Accepted (2026-10-17), static projections implemented; camera/per-stamp deferred.

## Decision

`paint::BrushMaterial` keeps its existing flat channel values/enabled switches and adds a map
from channel kind to resource UUID, one shared `doc::Projection`, repeat-vs-transparent addressing,
and Replace/Detail normal painting. The Brush UI dispatches `Command::SetBrushMaterial` and app
image-import methods; it never constructs or modifies document rasters. Whole-material import
uses the existing filename planner and conversions. It enables only matched channels already
present in the texture set. It assigns the material atomically after all selected images load.

`Stroke::with_resources` captures immutable Arc-shared images at stroke begin. The existing stamp
kernel still owns coverage, culling, symmetry, stencil, Parts, spacing and opacity. At tile write,
each image is sampled using the fill sampler's UV/planar/triplanar frames. UV needs no geometry
cache; object projections use the existing lazily cached tile geometry. Work and temporary
sample buffers are tile-sized, never a dense layer canvas. No image is decoded inside a stamp.

Before raster work, tangent stamp spheres are conservatively mapped through each triangle's
world→UV affine gradients; UV stamps use their UV disk bound. Both include the raster rim and
intersect the triangle bounds. Camera alignment retains full triangle bins. Numeric acceptance
compares resulting coverage bit-for-bit with the unbounded kernel, including curved meshes,
seams, square alpha tips and mirrored cameras. This avoids broad triangle work for tiny 4K dabs.

Values are straight; color sampling decodes sRGB, data channels stay raw. Source alpha multiplies
the final coverage × stroke opacity, so source transparency is capped within a stroke. Erasing
ignores the image and reduces paint alpha through the existing path. Non-repeating images are
transparent outside transformed [0,1]², clamped at the image boundary. Missing images are transparent.

Normal Replace uses normalized source-over; Detail uses W3C-style backdrop-alpha weighting of
reoriented normal mapping against the **pre-stroke** layer snapshot. It does not repeatedly combine
detail as stamps overlap. The final encoded vector is renormalized (zero falls back to flat).
The RNM function lives in `core::normal` and is shared with the compositor. Detail affects
normals already painted on this layer; blending against underlying layers is still a layer mode.

## Persistence

These are tool settings, not document source data. Paint outputs are ordinary existing tiled
rasters, so project schema 4 remains unchanged and old projects load without a new migration.
No editable stroke/source record has been introduced. Reopening a project restores its painted
result; use a preset to restore the tool. Future editable strokes must add an explicit migration.

Brush preset version 2 embeds the encoded bytes/name/old UUID for every channel image. Applying
it imports/deduplicates images into the destination session and remaps UUIDs before changing
the brush. Missing/broken embedded images leave the tool unchanged; failed imports may retain
unused immutable resources. Version 1 defaults to flat channels, UV, tiling, Replace. Unsupported
preset versions are rejected. JSON byte arrays favor the existing preset format over introducing
a second archive container; large materials can produce large presets.

## Limits and follow-ups

Camera/per-stamp textures need stamp-dependent source accumulation (including symmetry and
batch-splitting determinism); they must not be implemented as a single post-coverage sample.
Projection samples tangent-space normal components just as fill layers do. Reorienting normal
vectors for rotated projection frames/mirrored UV tangents is not part of this slice; do not
claim world-space normal projection correctness. Starter-pack async preview destinations still
target fill channels; imported shelf textures support brush drops now.

The synchronous CPU write/composite path needs measured 4K frame-pacing work before promising
no dropped frames on arbitrary models. The example in `crates/app/examples/brush_projection.rs`
reproduces image conversion, normal paint, project fixtures and a 1,000-sample 4K CPU profile.
