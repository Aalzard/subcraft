# ADR-0023 — Channel controls, bake refresh and symmetry guides

Status: Accepted (2026-10-19)

Channel buttons emit UI events. App commands own independent toggling and Alt-click solo for
fill-layer and Fill-effect sources, preserving disabled sources/strengths and committing the
whole solo operation as one history entry. Brush channel toggles change the existing tool
material, retaining its values/images; they do not change historical paint on a layer. Disabled
rows are omitted, including Fill-effect blending rows. This is not a new per-paint-layer channel
visibility model. The latter remains separate from the brush's enabled painting channels.

Albedo filtering remains document data but its control is exclusively in Texture Sets. The
existing app command, project field, CPU reconstruction and per-material GPU sampler remain
unchanged (ADR-0020); this supersedes that decision's duplicate toolbar placement. Projection
Rotation already exists and is moved directly beneath Tiling in the shared editor used by
layers, Fill effects, brush projection and masks. No new transformation math or schema change.

The Bake view checks loaded high-poly path/length/modified time at 250 ms intervals and starts
the existing app import worker when the source changes. Selecting a different path starts
loading on the next frame. Cached geometry remains visible while replacement loading runs;
obsolete results are still dropped and successful imports invalidate the GPU display cache.
Read/import errors remain visible and retry automatically. An active bake keeps its immutable
source; changing its source file does not silently restart or mutate the running bake.
Successful baking resets the raw-map selector and viewport mode to Material every time,
superseding ADR-0021's completion behavior. Completed maps remain manually selectable.

`app::guides` intersects mesh triangles with X/Y/Z planes through the whole-mesh bounds center,
the same frame mirrored painting uses. It caches all axes once per scene, avoiding per-frame
triangle work. Small surrounding rectangles identify the planes beyond the surface, including
flat meshes. UI uploads segments only when scene/axes change. A render-owned line pass draws
them in red with surface depth testing and a small clip-depth offset; LineList pipeline depth
bias is unsupported by wgpu. Texture data is never touched.
Disabled axes hide immediately. Show symmetry guides in the popup is a persisted Viewport
preference (default true for old preference files), separate from the stroke's symmetry state.
No painting coordinates or brush/project schema changed for the guide toggle.

These changes leave the generator surface-distance/micro-normal dependency and emission
post-processing contracts open; see material-workflow-roadmap.md rather than inferring those
features from existing Cavity/Emissive enums or adding no-op controls.
