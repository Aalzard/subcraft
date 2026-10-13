# SubCraft — implementation progress

Last updated: 2026-10-13 (session 2: compositor + paint engine, cloud brief `cloud-brief-engine.md`)

## Decisions

* Stack: Rust 2024 + wgpu 30 + egui 0.36/eframe + egui_dock 0.21 — see `adr/0001-technology-stack.md`.
* Texture engine: CPU-authoritative copy-on-write tiled images (64² tiles, f16), per-tile
  compositor with dirty-tile tracking, projection brush — `adr/0003-texture-engine.md`.
* Conventions: right-handed, +Y up, CCW front faces, UV origin top-left (glTF), MikkTSpace
  tangents → OpenGL (+Y) normal maps, wgpu camera conventions — `adr/0005-coordinate-conventions.md`.
* Undo: history of structurally shared document snapshots + merge keys; strokes push the
  post-stroke document — `adr/0006-undo.md`.

## Done (all tests passing — 99 tests)

| Crate | Contents | Tests |
|---|---|---|
| `core` | typed UUID ids, `ChannelKind`, `MeshMapKind`, sRGB/HSV math, `IRect` | 7 |
| `image` | `TiledImage` (COW tiles, diff by pointer, value `PartialEq`), `Bitmap` (u8/u16/f32, bilinear), PNG/TGA/JPEG/EXR I/O, dilation (jump-flood infinite + width) | 12 |
| `mesh` | canonical `Mesh` + `MeshBuilder`, OBJ/glTF/FBX import, OBJ writer, SAH BVH, topology, versioned binary encoding, UV rasterizer with conservative rim + **region rasterization** (`rasterize_surface_region`), seam-heavy primitives | 22 |
| `doc` | 32 blend modes, fills/projections, masks, paint/fill/folder layers, `TextureSet` tree ops, `Document`, `History`, `ResourceStore`; `same_parameters` now treats a lazily created paint channel as a raster-only change | 9 |
| `materials` | noise + patterns; **`sample`**: lazily built per-tile `TileGeometry` (normalized position, normal, coverage) cached per slot + resolution, `MeshMaps`, `eval_fill_tile` for Uniform / Bitmap / Procedural / MeshMap through UV / Planar / Triplanar | 10 |
| `compositor` | **new**: W3C separable compositing of all 32 modes (whiteout / RNM / HSV / signed height), isolated + passthrough folders, mask stack (paint, fill, generators, levels, invert), rayon per-tile evaluation, `CompositeCache` dirty-tile diff | 18 (6 unit + 12 acceptance) |
| `paint` | **new**: `BrushSettings` / `BrushMaterial` / `Camera`, path resampling (spacing, lazy mouse, straight line, jitter), symmetry via mirrored cameras, CPU depth map + backface culling, tile-parallel UV-raster stamp kernel (Tangent\|Wrap, Tangent\|Planar, Camera, UV), stroke-opacity-capped coverage written from the pre-stroke snapshot, eraser, mask painting, polygon fill (triangle / element / UV island) | 21 (10 unit + 11 acceptance) |

Acceptance tests from the brief, all passing (`crates/compositor/tests/compositor.rs`,
`crates/paint/tests/paint.rs`, `crates/materials/src/sample/tests.rs`):

* sample: uniform exact; UV procedural == `pattern::eval` at `transform_2d(uv)`; triplanar on each
  cube face == the matching planar sample; bitmap sRGB decode only for color; mesh-map fallback.
* compositor: reorder two paint layers; hidden parent folder hides children; channel opacity
  isolated to its channel; black mask hides, white mask paint reveals locally (1 dirty tile);
  fill roughness change keeps paint above; passthrough (0.25) vs isolated (0.5) under Multiply;
  single-tile edit → exactly that dirty tile (also for a lazily created channel); undo via
  `History` → earlier composite bit-exact, re-evaluating only the stroke's tile.
* paint: cube seam continuity (max coverage jump 0.04 across the edge); thin slab back island
  untouched (with culling, depth-only, Camera and Planar alignment); disabled channels share
  storage and are byte-identical; identical footprints/determinism; overlap ≤ stroke opacity;
  base color + roughness in one stroke; History undo/redo exact (and pre-stroke tiles untouched);
  X symmetry mirrored (coverage mass 374.6 vs 372.0, mirrored centroids < 0.02); UV-island polygon
  fill touches no other island.
* Mutation check: forcing `DepthMap::visible` to `true` makes the slab test fail (1554 texels
  painted through) — the visibility test is load-bearing.

Measured (release, 4-core cloud container, 2048² Body set of the sample crate):
full composite with a 6-octave Clouds procedural fill 650 ms (805 ms incl. first geometry build);
first dab incl. 1920×1080 depth map 23 ms; 200 cursor samples (95 stamps) 75 ms; incremental
recomposite after that stroke 12 ms (15 tiles).

Run: `cargo test -p subcraft-core -p subcraft-image -p subcraft-mesh -p subcraft-doc -p subcraft-materials -p subcraft-compositor -p subcraft-paint`

### Deviations from the brief (sound-engineering choices)

* **Branch**: work was pushed to `claude/practical-curie-lk9f9s` (the cloud session only allows
  that branch) instead of `engine/compositor-paint`; never to main.
* **Texel write** is source-over in premultiplied space rather than a lerp of straight values:
  lerping straight value toward the brush color from a transparent texel would blend in the
  image's fill value and produce gray halos at soft edges. Still recomputed from the pre-stroke
  snapshot by `cov × stroke_opacity`, so the opacity cap holds.
* **`roundness`** added to `BrushSettings` so `angle` / `follow_path` / angle jitter have a visible
  effect without brush-alpha bitmaps.
* **Composite 4th component** of rgb channels is the accumulated alpha (1 at the root), giving the
  renderer an RGBA16F upload.
* **Mask eraser** returns the base raster toward its fill value (masks have no alpha to reduce).
* **Stamps may center on another texture set's surface**; only the active set's triangles are
  painted (a brush straddling a material border paints its part).
* **UV alignment** skips 3D visibility (paints the UV disc, as in the 2D view).
* **Signed Addition**: neutral value adds nothing (0.5 for 0..1 channels, 0 for Height).
* **Generators**: Directional uses the low-poly interpolated normal (always available) rather
  than a baked world-normal map; Position Gradient runs along +Y within the mesh bounds.

## Next steps (in order)

1. `render` (wgpu PBR + IBL viewport consuming `Composite` + dirty tiles, UV view, brush cursor)
   + `ui` (dock layout, theme, panels) + `app` (session, command registry, shortcuts; stroke
   begin → add_point → push to `History` at stroke end) + `apps/subcraft` → **first visual
   milestone: show the user screenshots and launch the app for testing**.
2. `project` (zip container, versioned schema, atomic save), `export` (templates, packing,
   padding), `bake` (normal/world normal/position/ID/AO/curvature/thickness → `MeshMaps`), `assets`.
3. Engine follow-ups: brush alpha bitmaps + stencils; incremental dirty tracking inside mask paint
   effects; near-plane clipping in the depth rasterizer; f16 tile geometry to halve its memory;
   renormalize normals after opacity lerp; GPU compositor behind the same interface.

Remaining stub crates (empty `lib.rs`): bake, project, export, assets, render, app, ui;
`apps/subcraft/src/main.rs` is an empty `main`.
