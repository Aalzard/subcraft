SubCraft 0.6.0.

- **Procedural smart materials with sliders**: 28 procedural, 17 weathered (Wear, Age and Dirt) and 6 stylized hand-painted materials. Select the folder and change colours, wear, rust, dirt and scale in Properties, or add your own sliders. One slider can drive several settings through a formula.
- **Material filters**, as Painter's MatFinish and MatFX: 17 filters such as Galvanized, Brushed, Edge Damages, Rust Weathering, Peeling Paint, Water Drops, HBAO and Baked Lighting Stylized, plus PBR Validate, Non-uniform Blur and Color Match.
- **New generators and smart masks**: Dripping Rust, Fiber Glass Edge Wear, the Mask Builder, and 19 multi-effect smart masks (leaks, dust, moss, drips, frayed edges and more).
- **Better 3D view**: clear coat, sheen, anisotropy, subsurface, translucency and glass; a displacement preview; brighter rough metals, soft shadows, contact shadows and screen-space reflections. The path tracer renders all of these too, with depth of field.
- **Painting**: Heal for the Clone tool, stabilizer presets and post-smoothing, a Shape fill tool, Liquify Push, particle brushes (rain streaks, drips, burns), pen tilt, image decals that wrap over edges, and stencils from the Shelf.
- **Stitches**: draw stitches with the Pen, 14 thread stitches that cross UV seams, an Auto Stitcher for UV seams and hems, and your own stitch tiles.
- **Models and baking**: multiple UV sets, Edit UV islands, bake presets, and Bent normal and high-poly projection on the GPU.
- **Workflow**: version history (File > Version history), Live link to Unity, Unreal and Godot, and Rhai scripts in a Plugins menu.
- **Linux**: an AppImage and a Flatpak bundle.
- **Fixes**: one material now controls every UDIM tile (issue #581), normal maps on projected and tri-planar fills, smart masks keep their blend modes, a Heal hang and a rare compositor freeze.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.6.0-setup.exe`), portable zip, Linux x86_64 tarball, AppImage and Flatpak (Linux builds are run by CI with `--version`, not opened by hand), and checksums.
