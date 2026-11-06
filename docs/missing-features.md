# Missing features compared with Substance 3D Painter

Written 2026-10-22 from SubCraft 0.1.8's own limits (see `docs/progress.md`) and the Painter feature set
as generally known. It is a planning list, not a promise. "Known" means we already recorded the gap.

## Interop (the .spp goal)
- Read Substance Painter `.spp` projects: mesh, baked maps, flattened per-channel images first;
  editable layers only if the container turns out to allow it. Needs sample files (investigation not started).
- Export `.spp`: not planned, unsupported and fragile.
- Import `.sbsar` Substance materials / `.sbs` graphs (procedural inputs): not started.

## Painting
- Tablet pressure / tilt mapping (not verified with a real tablet).
- Per-stamp camera projection; Clone with rotate/scale and heal/seam blending (known).
- Physics/"paint with dynamics", liquify. (Done in session 109: scatter/particle brush with fall, smudge-with-material.) Smart/decal brushes still missing.
- Paint through UV-space projection tools, Fill with symmetric tri-planar per-island controls.
- Stroke stabilizer: weighted-average smoothing + rope (lazy) + catch-up exist (session 109). Missing: post-smoothing, speed-adaptive smoothing, Shift-drag line lock/snapping, smoothing preset buttons. Paint-through-mask tool variants (partly there: stencils).
- Anchor points / Bezier editing exists; no shape tools beyond polygon fill.

## Layers, masks, filters
- Anchor point system and layer links across texture sets; shared layer stacks across UDIM tiles (known).
- Apply-smart-material-to-all-tiles command and 2D UDIM tile picker (known).
- Layer instances / linked layers, adjustment layers beyond Levels/Filter folders, per-channel blend mixing.
- More filters: Non-uniform blur, Slope blur, Color match (Gradient Map, Histogram Scan / Range, Median and Warp were added in session 108). Warp has no image-driven intensity map yet; Gradient Map has no gradient-driven (curve) editor or import from Substance gradients.
- Generators: more bake-driven generators (Ambient Occlusion cavity variants, Thickness, World-space normal usage
  in more presets), Dirt/curvature presets beyond the new five.
- Smart material export/import compatibility with Substance `.spsm` / `.sbsar` (not started).
- Built-in content is now 26 smart materials (13 were removed on remove-13-materials), 36 smart masks and 13 brush presets. Still missing: a real brick-course
  pattern, smart masks that combine several effects with blend modes (a recipe is
  one effect), fingerprint / fabric-weave / rivet patterns, and shelf tags beyond one category per smart asset.

## Baking
- GPU baking for Bent normal, Height and the high-poly projection (AO, Thickness and Curvature run on the GPU since session 109; the rest and the UV rasterisation stay on the CPU), Ray-traced AO with multiple samples/denoise, Bent normal on production meshes (not verified),
  Position/World normal from high poly with multiple UV sets, bake by mesh name / match "_high/_low", multiple high polys,
  texture-set-wide baking queue with presets.

## Viewport and rendering
- Real path-traced / Iray-style rendering and render settings; depth of field, motion blur, SSS, refraction,
  anisotropy, clear coat and sheen layers in the viewport shader.
- Environment: custom environment blur/rotation lock, background modes and lights (point/spot/area), shadows.
- Turntable as GIF/MP4 (PNG frames only), scene camera bookmarks.
- Displacement / tessellation preview (height shown as bump).

## Materials and workflow
- Texture-set settings parity: per-channel bit depth (16/32-bit), custom channel formats, channel packing UI beyond presets.
- Export presets: Substance-compatible preset import, custom preset editor, bit depth/dilation per map, sbsar publish.
- Resource shelf: online library, shelf "Share"/sync, tags beyond category, saved-search.
- Plugins/scripting API (Painter has JS + Python); SubCraft has none.
- Collaboration: version history beyond undo, project merge, auto-save versioning.
- Multi-mesh scenes: several meshes in one project with per-mesh sets; mesh editing such as UV edits.
- Mesh formats: USD, Alembic, FBX animation, point clouds (OBJ/glTF/FBX static meshes only).

## Platform and distribution
- Code-signed installers (needs an enrolled signing identity), macOS build, ARM builds.
- Auto-update for Linux packages beyond executable replace; Flatpak/AppImage packaging.
