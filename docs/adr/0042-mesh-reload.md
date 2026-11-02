# ADR-0042 — Mesh reload

Status: Accepted (2026-10-30)

## Context

Artists change the model after they start painting: a fixed edge loop, a new part, a re-unwrap.
Until now the only path was a new project, which loses every layer. Painter-class tools offer
"reload mesh" that keeps the layer stack and, when the UVs changed, moves the paint onto the new
layout.

## Decision

File > Reload mesh… runs in two background steps, each with progress and Cancel
(`crates/app/src/mesh_reload.rs`).

1. **Check.** Import the file and group its triangles into texture sets by material, by object or
   as one set, whichever gives the most name matches with the current sets (UDIM tiles get their
   own sets, as in New Project). Sets are matched by name. For each matched pair a 64² probe of
   the new layout is mapped onto the old surface (below); if more than 2% of the texels land on a
   different old UV, the set is reported as moved and **Reproject** is suggested, otherwise
   **Keep UVs**.
2. **Transfer.**
   * Keep UVs: the new document keeps the old layer objects (structurally shared, no copy).
   * Reproject: for every texel of the new layout, take its 3D point, find the closest point on
     the old surface restricted to the triangles of the matched old set (new BVH query with a
     triangle filter, `mesh::bvh`), and read the old UV there (`mesh::transfer`). Every raster —
     paint channels, paint effects, mask paint, mask paint effects — and the baked mesh maps are
     resampled through that table. Sampling skips old texels outside the old islands so seams do
     not pick up gutter colour, and only tiles that receive paint are allocated (`TiledImage`
     stays sparse). Pen paths are flattened into paint layers.
   * New materials get an empty set. Unmatched old sets are kept on an extra material slot with no
     triangles and listed in the report, so nothing is silently lost.
3. **Finish** on the UI thread swaps mesh, BVH, document and maps. It is refused if the project
   changed since the reload was prepared.

The mesh is not part of undo history, so a reload clears the history (the dialog says so) and marks
the project modified. Reprojected baked maps are flagged as stale (`ProjectSettings.stale_mesh_maps`,
cleared by a new bake), and the model path is stored (`ProjectSettings.mesh_source`) so the next
reload defaults to it. Both fields are optional, so the project schema was not bumped. Hidden or
isolated parts are reset because part numbering can change.

Scripted use: `subcraft project.subcraft --reload-mesh v2.obj --reload-mode keep|reproject
--screenshot out.png`.

## Consequences

* Painting survives re-unwraps and topology edits within a material. Paint on surface that no
  longer exists is dropped; paint on new surface comes from the closest old point.
* Painted Normal-channel values are copied as they are; tangent-space detail is not re-oriented
  when an island rotates. Layers using part visibility are counted in the report because part
  ids may now point at different geometry.
* Reproject is lossy (one bilinear resample); Keep UVs is bit-exact.
* Undo across a reload is not possible. Putting the mesh into history would be a new ADR.
