# Missing features compared with Substance 3D Painter

First written 2026-10-22. Checked again against the code on 2026-11-23: every item below was
searched for in the source, and items that now exist moved to "Done since the first list". It is
a planning list, not a promise.

## Still missing

### Interop
- Export `.spp`: not planned (unsupported and fragile).
- Editable `.sbsar` graphs: SubCraft reads what `.spp` / `.spsm` containers hold, but does not run
  Substance graphs.

### Baking
- The high-poly projection runs on the CPU when parts are matched by name (the GPU pass cannot
  filter hits by part).

### Viewport and rendering (owned by the viewport thread)
- Scene lights (point / spot / area).

### Workflow
- Mesh editing such as UV edits.
- Point clouds, FBX animation.

### Platform and distribution
- Code-signed installers (needs an enrolled signing identity).
- Flatpak packaging.

## Done since the first list
- Read Painter `.spp` projects and `.spsm` smart materials.
- Pen pressure and tilt, Physical Size brushes, particle (scatter) brushes, smudge with material.
- Shift + click straight lines with 15 degree snapping, stroke stabilizer (average, rope, catch-up).
- Clone and camera projection painting.
- Copy and paste layers and effects between projects (the in-app clipboard outlives the open project).
- Anchor points (content and mask anchors, references from fills, masks and generators).
- Color / ID selection masks with tolerance and softness.
- Fill projections: UV, tri-planar, planar, spherical, cylindrical.
- Filters: Gradient Map (with editor), Histogram Scan, Median, Warp, MatFinish / MatFX, PBR Validate.
  Blur Slope (noise or previous input; Blur / Min / Max), Non-uniform Blur and Color Match.
  They read imported images or anchor points (blur map, custom noise, source / target colours).
- Per-channel format (8-bit, 16-bit, 16F, 32F).
- Export presets: SubCraft presets library and import of Painter export presets.
- Baking: several high polys, match by name (`_low` / `_high`), Thickness, Bent normal,
  multi-sample AO with denoise.
- Multiple UV sets per model.
- Scripting: Rhai scripts over the tool catalog (Plugins menu, `subcraft-mcp --script`).
- Live link to Unity, Unreal and Godot.
- Mesh formats: OBJ, glTF, FBX, USD, Alembic.
- Liquify (Push mode of the Smudge tool), Heal (Clone tool option), rectangle / ellipse Shape fill tool.
- Bake presets (built-in and saved to the library).
- Stabilizer presets (Off / Light / Medium / Strong / Rope), speed-adaptive smoothing, catch-up toggle,
  post-smoothing (the finished stroke is painted again along a smoothed path).
- Layer links across texture sets: "Instantiate across texture sets" and the instance list (this was
  listed as missing by mistake).
- Version history: each Save keeps the file it replaces; File > Version history restores one.
- Turntable as GIF, camera bookmarks, environment rotation and blur.
- Shelf tags and saved searches; weave, rivet, brick and fingerprint content.
- 2D UDIM tile picker (tiles laid out as in UV space, previous / next).
- ARM and macOS builds through the release workflow.
- Linux AppImage (x86_64), built and checked by the release workflow.
- GPU Bent normal bake (same rays as AO, checked against the CPU bake).
- GPU high-poly projection (cage rays for Normal, Height, ID, Position ...), checked against the CPU.
