# SubCraft — implementation progress

Last updated: 2026-10-14 (session 1, paused by request)

## Decisions

* Stack: Rust 2024 + wgpu 30 + egui 0.36/eframe + egui_dock 0.21 — see `adr/0001-technology-stack.md`.
* Texture engine: CPU-authoritative, copy-on-write tiled images (64² tiles, f16) — GPU for display;
  GPU compositor later behind the same interface. (ADR-0003, to be written.)
* Undo: history of structurally shared document snapshots + merge keys for drags (ADR-0006, to be written).
* Conventions: right-handed, +Y up, CCW front faces, UV origin top-left (glTF), MikkTSpace tangents
  in OpenGL UV space → OpenGL (+Y) normal maps (ADR-0005, to be written).

## Done (all tests passing — 53 tests)

| Crate | Contents | Tests |
|---|---|---|
| `core` | typed UUID ids, `ChannelKind`, `MeshMapKind`, sRGB/HSV math, `IRect` | 7 |
| `image` | `TiledImage` (COW tiles, diff by pointer, value `PartialEq`), `Bitmap` (u8/u16/f32, bilinear), PNG/TGA/JPEG/EXR I/O, dilation (jump-flood infinite + width) | 12 |
| `mesh` | canonical `Mesh` + `MeshBuilder` (validation, smooth normals, MikkTSpace, dedup, material grouping), OBJ/glTF/FBX import, OBJ writer, SAH BVH (ray, occlusion, volume query), topology (UV islands, elements, UV edges), versioned binary encoding, UV rasterizer with conservative rim, seam-heavy primitives + 2-material sample crate | 21 |
| `doc` | 32 blend modes, fills/projections/procedural params, masks (base raster + effect stack, levels, generators), paint/fill/folder layers with per-channel blending, `TextureSet` tree ops (insert/remove/move/duplicate/group, cycle checks, COW `find_mut`), `Document`, `History` (undo/redo/branch/merge/trim), `ResourceStore` | 9 |
| `materials` | periodic gradient/value/fbm/Worley noise; patterns: clouds, cells, grunge, scratches, wood rings, brushed, checker, gradients (all tileable at integer scale) | 4 |

Developer preview (renders patterns, UV layouts, world normals to PNG):
`cargo run -p subcraft-preview --release -- preview_out`
It caught a UV-rasterizer bug (rim spiking ~3 texels past acute vertices → cross-island bleed);
fixed with true closest-point distance + regression test `rim_never_exceeds_expand_at_acute_vertices`.

Run: `cargo test -p subcraft-core -p subcraft-image -p subcraft-mesh -p subcraft-doc -p subcraft-materials`

## Next steps (in order)

1. `materials::sample` — per-tile fill-source evaluation (uniform / bitmap / procedural / mesh map)
   with UV, triplanar, planar projection; `TileGeometry` (uv, normalized position, normal per texel).
2. `compositor` — per-tile evaluation of the layer tree: W3C-style separable blending for all
   modes, normal-map combine/detail (whiteout/RNM), HSV modes, isolated folders + passthrough,
   mask stack evaluation; dirty-tile diff between evaluated and current `TextureSet`.
   Acceptance: reorder changes result; hidden folder; per-channel opacity isolation; black mask
   hides; fill parameter change preserves painting above.
3. `paint` — stroke resampler, jitter, lazy mouse, symmetry, straight line; Tangent|Wrap /
   Tangent|Planar / Camera / UV alignment; CPU depth map visibility + backface culling;
   tile-parallel UV-raster stamp kernel with flow/stroke-opacity accumulation; eraser; mask
   painting; polygon fill (triangle / element / UV island).
   Acceptance: seam continuity, no rear paint-through, disabled channels byte-identical,
   undo/redo exact.
4. `render` (wgpu PBR + IBL viewport, UV view, brush cursor) + `ui` (dock layout, theme, panels)
   + `app` (session, command registry, shortcuts) + `apps/subcraft` → **first visual milestone:
   show the user screenshots and launch the app for testing**.
5. `project` (zip container, versioned schema, atomic save), `export` (templates, packing,
   padding), `bake` (normal/world normal/position/ID/AO/curvature/thickness), `assets`.

Remaining stub crates (empty `lib.rs`): compositor, paint, bake, project, export, assets, render,
app, ui; `apps/subcraft/src/main.rs` is an empty `main`.
