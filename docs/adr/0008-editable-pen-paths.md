# ADR-0008 — Editable pen paths

Status: Accepted (2026-10-15)

## Decision

An editable pen layer is paint content with an optional `PenPath` source and derived channel
rasters. A path has UUID anchors, relative incoming/outgoing Bézier handles, open/closed state,
UV radius, hardness, opacity and linear color/encoded data-channel values. Coordinates use
ADR-0005's top-left UV convention. Surface picking applies the triangle's UV tile offset.

The path source stays in `doc`; `project` maps it explicitly to schema 4. Schemas 1–3 load with
no path source. Cached `TiledImage` rasters keep the compositor and export independent of the
editor and allow immediate display on load. Editing regenerates the cache from parameters,
never by repainting over the old result. Ordinary brush strokes reject path content; mask
painting and other layer operations retain their existing behavior. Duplication gives anchors
fresh UUIDs while sharing unchanged raster tiles.

`app::pen` owns editing drafts, handle semantics, queued raster work, cancellation and history.
UI code maps pointer hits/coordinates to these operations and draws overlays. A released
gesture queues a separate undoable edit. Raster work runs in bounded steps; the frontend spends
16,384 segment-distance evaluations per frame. Tile bins, anchor/subdivision counts and queued
edits are capped. Completed results check scene identity, resolution and expected path source;
they preserve current names, masks, folder locations and selection. Unchanged cache tiles are
shared with the previous image. Save, export, project changes and update restart wait for
pending work; the browser's unsaved indicator includes uncommitted pen work.

Raster coverage is the closest distance to the complete subdivided curve, with round caps,
falloff and a one-texel edge band. Segment overlap does not accumulate opacity. Temporary texel
buffers are tile-sized; a layer never requires a dense RGBA canvas.

## Limits

These paths follow texture UVs. Width is a percentage of texture width. Separate paths are
needed across discontinuous UV islands; overlapping UVs share texels. The 3D view places points
on visible surfaces and edits controls that can be mapped back to the surface. The 2D view
supports precise editing, including handles outside UV islands. Brush alpha tips and world
symmetry are separate brush features. Surface-space paths across seams can build on this model
later, with an explicit schema migration if their source data changes.
