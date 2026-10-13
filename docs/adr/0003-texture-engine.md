# ADR-0003 — Texture engine: CPU tiled images, per-tile compositor, projection brush

Status: Accepted (2026-10-13, implemented in session 2)

## Context

Painter-class texturing needs many layers × many channels at 2K–4K, live parameter edits,
seam-free 3D painting and cheap undo. wgpu has no sparse residency (ADR-0001), and the engine
must be testable without a GPU.

## Decision

**Storage.** Every raster (paint channel, mask base, mask paint effect, composite, mesh map) is a
`TiledImage`: 64² tiles of interleaved `f16`, unallocated tiles implicitly hold a fill value, and
both the tile table and each tile are `Arc`-shared (copy-on-write). Paint channel images hold
`components + 1` values (straight value…, alpha). Composite images hold 4 components
(`rgb + alpha`) for 3-component channels and 1 for scalars. Values are linear; normals are
stored encoded 0..1.

**Fill sampling (`materials::sample`).** A `TextureSetGeometry` per (material slot, resolution)
lazily builds a `TileGeometry` per tile with `rasterize_surface_region` (1-texel rim): object
position normalized to the mesh bounds (min corner, divided by the largest extent), shading
normal and coverage per texel; UV is derived from the texel index. Fill sources (uniform,
bitmap with sRGB decode only for color channels, procedural `low..high`, caller-supplied mesh
maps) are evaluated a tile at a time through UV / planar / triplanar (`|n|^sharpness`)
projection.

**Compositor.** Tiles are independent, evaluated in parallel (rayon) and deterministic. Per
channel a running straight `(value, alpha)` accumulation starts at the channel default with
alpha 1. Layers composite with W3C separable compositing,
`Cs' = (1 − ab)·Cs + ab·B(Cb, Cs)`, `ao = as + ab(1 − as)`, so:

* isolated folders start transparent and their first child passes through unblended;
* passthrough folders run children on a copy of the accumulation and lerp it in by
  folder opacity × mask;
* contribution alpha = content alpha × channel opacity × mask; `Replace` ignores content alpha;
  `Disable` skips the channel; paint layers affect only channels they have images for.

Blend notes: arithmetic modes clamp to 0..1 except on signed channels (Height); Signed Addition
treats the neutral value as zero change (0.5 for 0..1 channels, 0 for Height); Tint / Saturation
/ Color / Value go through HSV (gray scalars therefore keep the backdrop except under Value);
Normal Map Combine = whiteout, Detail = reoriented normal mapping, Inverse Detail = swapped
operands; normal-only modes act as Normal on other channels.

Masks: base raster, then each visible effect yields `(e, ea)` blended onto the mask with its
mode and opacity — Paint (raster), Fill (uniform / procedural through a projection), Generator
(Edges = curvature, Cavity = 1 − AO, Directional = normal · direction, Position Gradient = Y in
bounds, raw mesh map; smoothstep threshold with softness, invert and procedural breakup),
Levels, Invert.

**Dirty tracking.** `CompositeCache::update` diffs the texture set against the last evaluated
one: pointer-equal layers are skipped; layers with `same_parameters` contribute
`TiledImage::changed_tiles` of their paint channels (a lazily created channel compares against
an empty image) and mask base; anything else, or a channel / resolution / input change, dirties
every tile. The dirty list is what the renderer uploads.

**Brush engine.** Screen samples → resampled at spacing (lazy mouse, straight line) → jitter →
ray cast per camera (symmetry = the same screen samples through cameras mirrored across planes
through the bounds center). Per batch of stamps: BVH volume query (sphere for tangent modes,
projected screen rect for Camera, UV tile bins for UV) for candidate triangles of the active
texture set → bin by tile → rayon over tiles → UV-rasterize each triangle with a 1-texel rim →
reconstruct position / geometric normal → footprint (Tangent|Wrap: 3D distance; Tangent|Planar:
tangent-plane projection, depth-limited; Camera: screen disc; UV: UV disc) → per-stamp max over
triangles → `cov += a·(1 − cov)` with `a` = footprint × flow. Texels are recomputed from the
pre-stroke snapshot by `k = cov × stroke_opacity`: paint = source-over in premultiplied space
(no halo of the image's fill value at soft edges), erase multiplies alpha by `1 − k`, a mask base
lerps toward the mask value (erase: toward its fill). Visibility = CPU depth map of the whole
mesh (cached per camera; slope-scaled bias, lenient 2×2 lookup) + backface culling against the
projection direction. UV alignment skips 3D visibility (it paints the UV disc, like the 2D view).
Polygon fill (triangle / mesh element / UV island) writes coverage 1 through the same path.

## Consequences

* Painting a stroke or editing a fill touches only the tiles involved; history and the composite
  cache share everything else by pointer.
* Seams are invisible by construction: islands are painted from 3D positions, not UV adjacency.
* Known limits: cached tile geometry costs ~25 bytes per texel of covered tiles; the depth
  rasterizer skips triangles crossing the near plane; a change inside a mask *paint effect*
  re-evaluates every tile; normals are lerped, not renormalized, by opacity; round brush only
  (with `roundness`/`angle`), no brush alpha bitmaps yet. A GPU compositor can replace the
  per-tile evaluator behind the same `Composite` / dirty-tile interface.
