# Missing features compared with Substance 3D Painter

First written 2026-10-22. Checked again against the code on 2026-11-23: every item below was
searched for in the source, and items that now exist moved to "Done since the first list". It is
a planning list, not a promise.

## Still missing

### Interop
- Export `.spp`: not planned (unsupported and fragile).
- Editable `.sbsar` graphs: SubCraft reads what `.spp` / `.spsm` containers hold, but does not run
  Substance graphs.

### Painting
- Liquify / push brush that moves existing paint.
- Heal (clone with seam-free blending into the target).
- Shape tools beyond polygon fill and paths (rectangle, ellipse).
- Stroke stabilizer extras: post-smoothing, speed-adaptive smoothing, smoothing preset buttons.

### Layers and masks
- Layer links across texture sets (one layer driving several sets).
- 2D UDIM tile picker.

### Baking
- Bake setting presets (save a configuration and reuse it in other projects).
- GPU path for Bent normal, Height and the high-poly projection (AO, Thickness and Curvature
  already run on the GPU).

### Viewport and rendering (owned by the viewport thread)
- Scene lights (point / spot / area).

### Workflow
- Version history beyond undo (saved versions you can browse and restore).
- Mesh editing such as UV edits.
- Point clouds, FBX animation.

### Platform and distribution
- Code-signed installers (needs an enrolled signing identity).
- AppImage / Flatpak packaging.

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
- Turntable as GIF, camera bookmarks, environment rotation and blur.
- Shelf tags and saved searches; weave, rivet, brick and fingerprint content.
- ARM and macOS builds through the release workflow.
