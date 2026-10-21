# ADR-0037 — UDIM / multi-tile UVs (proposal, not implemented)

Status: Option 1 implemented (2026-10-21, progress.md sessions 100-101): tile = texture set, one resolution per material, 1001-style names or per-tile folders. Open: 2D tile picker, apply-to-all-tiles for smart materials, shared layer stacks.

## Context

Production meshes often lay UVs out over several 0..1 squares ("UDIM tiles"): tile 1001 is
u 0..1, v 0..1, 1002 is u 1..2, 1011 is v 1..2, and so on. Today SubCraft assumes one square per
texture set:

* `mesh::uv_raster::wrap_to_tile` translates every UV triangle into the 0..1 square that holds its
  centroid and throws the integer offset away. A mesh with UDIM UVs is therefore **folded onto one
  tile**: islands of different tiles overlap and paint over each other. This is the main hazard
  today, even before any UDIM feature exists.
* A texture set is one material slot, one `resolution` and one `TiledImage` per channel
  (`doc::TextureSet`). Compositing, painting, bakes, export file names, the GPU material
  textures and the 2D view all address "the" image of a set.
* Project schema stores one image per layer channel per set.

## Options

1. **Tile = texture set (recommended first step).** Treat each UDIM tile that holds triangles as
   its own texture set of the material slot (`Body.1001`, `Body.1002`). Everything that already
   works per set (layers, masks, painting, bakes, composites, export) keeps working because a tile
   is just another set with the same triangles filtered by tile. Needed changes:
   * mesh/geometry: keep the tile offset when rasterizing (`wrap_to_tile` returns it already) and
     select triangles per (slot, tile); `TextureSetGeometry` gains a tile key.
   * doc: `TextureSet` gets `udim_tile: Option<[i32; 2]>`; creating a session from a mesh splits
     slots whose UVs cross 0..1 into sets (schema migration: old projects have no tile, behave as
     today).
   * render: the viewport shader needs the tile of each triangle range (draw ranges per set, a UV
     offset per range); this is the one non-trivial GPU change.
   * export: file names carry the UDIM number (`Mesh_Body.1002_BaseColor.png`), plus an option to
     name by plain suffix for engines that want it.
   * UI: Texture Sets lists tiles, the 2D view shows one tile at a time with a tile picker, Layers
     are per set so a smart material applies to the selected tile (with an "apply to all tiles"
     command).
2. **One virtual wide image.** Store a single `TiledImage` of N x M tiles per channel. Fewer
   concepts but every consumer (painting wrap logic, bakes, GPU textures, export, mip chains,
   filters at tile borders) has to learn a bigger coordinate space; filters would bleed across tile
   borders. Rejected: more risk, and much wasted memory for sparse layouts.
3. **Do nothing but warn.** Detect UVs outside 0..1 on import and tell the user their islands
   overlap. Cheap, and worth doing regardless (it fixes the silent folding).

## Proposed plan if approved

1. (Small, safe, first) Import warning: detect UV centroids outside 0..1 and report "this mesh uses
   multiple UV tiles; they are currently folded onto one" in the status bar and the Texture Sets
   panel. Tests on a two-tile plane.
2. Option 1 behind the schema bump (13): tile key in geometry and texture sets, per-tile
   rasterization, painting/bake/export per tile, tests with a two-tile and a four-tile mesh
   (seams, overlap isolation, export names, save/reopen, old-project migration).
3. GPU: per-tile draw ranges with a UV offset uniform; numeric render test showing two tiles
   with different colours on one mesh.
4. UI: tile list, 2D view tile picker, "apply to all tiles" for smart materials.

## Open questions for the owner

* Is per-tile texture sets (option 1) the right mental model for users, or must a layer stack be
  shared across all tiles of a material (Painter-style)? Sharing is possible later by letting a
  layer apply to every set of a slot, but it changes how layers are stored.
* Per-tile resolution: one resolution for all tiles of a slot (simplest) or per tile?
* Which UDIM numbering in export names by default (1001-style) and should a single combined
  ZIP/folder per mesh be produced?
* Ship the warning (step 1) now, regardless of the rest?

## Consequences

No code changes yet. Until implemented, UDIM meshes paint onto one folded tile; the documentation
and this ADR say so.
