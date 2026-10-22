# ADR-0021 — Bake geometry inspection and live map previews

Status: Accepted (2026-10-19)

## Decision

The Bake dialog owns transient viewing controls. `app::BakePreviewJob` imports the high-poly
mesh on a worker through the UV-optional baking importer. Both meshes retain their original
coordinates (ADR-0005/0019); preview never registers, recenters or rescales one mesh against
the other. The loaded immutable high mesh is reused by the bake job, which builds its BVH.
Obsolete path/scene results are discarded. Cached source reuse checks path, length and modified
time; a changed file is reimported for baking. Reload high poly refreshes the inspection mesh.
No document fields or schema migration are needed.

`render::bake_preview` caches separate high/low surface and cage buffers. A depth comparison
shows the high surface in blue and exposed low surfaces in red. Both cage bounds use shading
normals and the same max(low diagonal, high diagonal) scale as the baker. Cage distances update
uniforms without rebuilding geometry. Orbit reveals surfaces on other sides. Colors are a
surface visibility comparison, **not** a signed-volume or complete ray-miss classification:
a high surface beyond the front cage can still appear blue. Custom cages, mesh-name matching,
automatic alignment and a full projection-error heatmap remain future work.

This deliberately implements the owner's exposed-low/high-covering-low visualization first.
[Painter's visualization settings](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/baking-visualization-settings)
also distinguish matching errors outside the cage; its red cage intersections are a more
extensive diagnostic than this initial depth comparison.

`bake_streamed` publishes each **complete, padded** map in requested order. `bake` wraps the
same implementation and returns exactly the same full results synchronously. No synthetic
reveal animation, sleep or partially valid mesh map is introduced. Cancellation preserves
already completed maps and never publishes the unfinished one. Progress includes the map
currently computing. Import, projection and image evaluation stay outside the UI thread.

The app job installs one completed map per poll, preserving intermediate UI observations even
when the worker finishes several maps quickly. Maps share immutable TiledImage storage with
the worker. High-poly normals create/reuse the existing live normal layer (ADR-0019) as soon
as Normal completes; other authored channels and selection remain unchanged. Completed maps
remain saved/exportable after cancellation, following existing non-history mesh-map storage.
Dropping a job cancels it; a changed scene rejects queued results before installation.

The viewport draws a selected raw mesh map on low-poly UVs with cached independent bind
groups. Scalars display grayscale; RGB data displays stored values without color conversion.
Pending sets use neutral map values. Material textures, samplers and the user's viewport mode
are not overwritten. Follow bake selects the newest completed map; manual map/Material choice
pins the view. Closing the dialog restores the regular material view; painting is suppressed
in the 3D bake view while navigation remains available. Live maps can be switched off.

Progress is **map-by-map**, not an intra-map tile wipe. The seven existing maps are supported;
there is no Height mesh-map baker yet. Cached preview geometry is retained for rebakes during
the UI session. There is no claim of bounded loading cost for arbitrarily large sculpts.

## Validation

Numeric tests compare streamed and synchronous maps exactly, including padding and flat-normal
values; cancellation stops publication between maps. App tests cover independent installation,
authored-channel isolation, scene rejection and UV-less unaligned import. GPU readbacks cover
blue/red depth ordering, cage visibility, raw RGB/scalar values, MSAA and material restoration.
UI tests cover preview invalidation; existing button/error and bake/export tests remain active.
Native screenshots and interactive/performance limits are recorded in progress.md.

`--bake-high path --bake-preview --screenshot path` captures the actual pre-bake geometry.
`--bake-preview-map normal_base` with `--screenshot` captures that completed map after GPU
upload, potentially during a running bake, then quits. It intentionally does not wait for all
other maps. Without a pinned map, screenshots wait for full bake/composite completion.
