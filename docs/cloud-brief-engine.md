# Cloud task brief: compositor + paint engine

Read `CLAUDE.md` and `docs/progress.md` first. Linux, no GPU: build/test only CPU crates
(`cargo test -p subcraft-core -p subcraft-image -p subcraft-mesh -p subcraft-doc -p subcraft-materials -p subcraft-compositor -p subcraft-paint`);
do not build ui/render/apps.

Git: create branch `engine/compositor-paint` from main, commit in small logical steps and
`git push -u origin engine/compositor-paint` after each completed step. Never push to main.
End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

Implement in order; each step's acceptance tests must pass before the next.

## 1. `materials::sample` — per-tile fill-source evaluation
`TileGeometry` per texture-set tile (64×64): per texel uv, object position normalized to mesh
bounds (0..1 by max extent), normal, coverage — built from `mesh::uv_raster::rasterize_surface`
(cache per texture set + resolution). Evaluate `FillSource` Uniform / Bitmap (bilinear; linearize
sRGB only for color channels) / Procedural (lerp low..high by pattern) / MeshMap (value supplied
by caller) through `Projection`: Uv (`transform_2d`), Triplanar (3 planar samples weighted by
|n|^sharpness), Planar (axis).
Tests: uniform exact; UV procedural equals `pattern::eval` at transformed uv; triplanar on a cube
face equals the matching planar sample.

## 2. `compositor` — CPU per-tile layer-tree evaluation
Output per-channel composite `TiledImage`s (BaseColor/Emissive/Normal 4 comps rgb+1, scalars 1;
f16). Per texel per channel accumulate (value, alpha); root starts at `ChannelKind::default_value`
with alpha 1. W3C separable compositing (Cs' = (1-ab)·Cs + ab·B(Cb,Cs), straight values) so
isolated folders (start transparent) work. All 32 `BlendMode`s: Disable skips; Replace ignores
content alpha; Passthrough folder = children applied directly to the running accumulation, then
lerped by folder opacity×mask; NormalMapCombine = whiteout, NormalMapDetail = RNM, Inverse =
swapped; HSV modes via `core::color`; Linear Dodge/Subtract clamp except Height (signed). Normal
values stored encoded 0..1. Contribution alpha = content alpha × channel opacity × mask.
Mask = base raster then effects (Paint value/alpha blend, Fill uniform/procedural, Generator from
mesh maps — Edges=curvature, Cavity=1−AO, Directional=world normal·dir, PositionGradient — with
level/softness/invert/noise breakup, Levels, Invert). Paint layers contribute only channels they
have images for. Rayon over tiles.
Dirty tracking: `CompositeCache::update(&TextureSet, ..)` diffs against the last evaluated
TextureSet: ptr-equal layers skip; `same_parameters` + raster-only change → changed tile indices
(`TiledImage::changed_tiles` on paint channels and mask base); otherwise all tiles. Returns the
dirty tile list (renderer uploads those).
Tests: reordering two paint layers changes the result predictably; hiding a parent folder hides
children; channel opacity affects only that channel; black mask hides fully, painting white into
the mask reveals locally; changing a fill layer's roughness leaves a paint layer above intact in
its painted area; passthrough vs isolated folder differ as expected under Multiply; a single-tile
edit reports exactly that dirty tile; undo (`History`) to an earlier Document yields the earlier
composite bit-exactly.

## 3. `paint` — brush engine
`BrushSettings` (size % of mesh bbox diagonal in Object space, flow, stroke_opacity, spacing
fraction, hardness, angle, follow_path, size/flow/angle/position jitter, alignment Tangent|Wrap
(default) / Tangent|Planar / Camera / UV, backface culling + angle, seed), `BrushMaterial`
(per channel enabled + ChannelValue; grayscale for masks), `Camera` (view/proj, viewport size).
Stroke: screen samples resampled at spacing; each stamp ray-cast with `mesh::Bvh`. Straight line,
lazy mouse (trailing distance) and symmetry (mirror plane X/Y/Z through bounds center) all emit
stamps through the same pipeline. Visibility: CPU-rasterized camera depth map (cached until the
camera changes) + bias; backface culling vs projection direction.
Kernel: BVH volume query for candidate triangles of the active texture set → bin by tile → rayon
over tiles → `uv_raster` each triangle (expand ≈1 texel) → reconstruct 3D position/normal →
footprint in projector space → per-stamp max-combine → stroke coverage `cov += a·(1−cov)` with
flow → write texels as lerp from the PRE-STROKE snapshot (Arc COW clone at stroke start) by
`cov × stroke_opacity`, so stroke opacity caps overlap. Eraser reduces alpha. Targets: paint-layer
content (enabled brush channels only; create channel image lazily) and mask base raster.
Polygon fill: triangle / mesh element / UV island under a hit.
Tests (use `mesh::primitives`): stroke across a cube UV seam is continuous in 3D (texels on both
islands near the shared edge get similar coverage); painting the front of `thin_slab` leaves the
back island untouched; disabled channels byte-identical after a stroke; same radius twice →
identical footprints; overlapping stamps never exceed stroke_opacity; base color + roughness in
one stroke; History undo restores the pre-stroke document exactly (TiledImage PartialEq) and redo
the post-stroke one; symmetry produces mirrored coverage; polygon fill of one UV island touches no
other island.

## Finish
Write short ADRs reflecting what was implemented: `docs/adr/0003-texture-engine.md`,
`0005-coordinate-conventions.md`, `0006-undo.md`. Keep modules focused (no giant files). Last
commit: update `docs/progress.md` (done table with test counts, next steps). If anything here is
wrong or impossible, choose the sound engineering option and record why in progress.md — never
fake a passing result.
