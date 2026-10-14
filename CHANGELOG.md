# Changelog

What changed in SubCraft, newest first. Download the latest build from the
[Releases page](https://github.com/Aalzard/subcraft/releases/latest).

## Unreleased (coming in 0.1.0)

### New
- **Linux build**: `SubCraft-<version>-linux-x86_64.tar.gz` with an `install.sh` that adds SubCraft
  to your application menu (no root needed).
- **Windows installer**: `SubCraft-<version>-setup.exe` with Start Menu shortcut, optional desktop
  shortcut, uninstaller in Settings → Apps and `.subcraft` file association. A portable zip is
  still available. (#10)
- **Use your own images**: any PNG / JPG / TGA / TIF / EXR in a fill layer channel, with UV,
  tri-planar or planar projection, tiling and rotation. Images are saved inside the project. (#11, #13)
- **Import texture set**: select a texture's maps (base color, roughness, normal, height, metallic,
  AO…) and get one fill layer. Files are matched by name; glossiness and DirectX normal maps are
  converted automatically. Find it in the Layers toolbar, the Layer menu and Shelf → Materials. (#12, #13)
- **Image masks**: use a black & white image or a logo with transparency as a mask effect. (#13)
- **Your library** in `Documents\SubCraft\Library`: imported brush tips and HDRIs stay in the shelf
  after a restart. Right-click library items to show or remove them. (#14)
- **Save smart materials and smart masks**: right-click a layer → *Save as smart material* or
  *Save mask as smart mask*, then reuse them in any project from the shelf. (#15)
- **Shelf search and filters**: search box plus *All*, *Mine* and category chips. (#16)
- **Brush presets**: save size, flow, tip, jitter, symmetry and material from the Brush panel;
  presets appear first in Shelf → Brushes. (#17)
- **Drag and drop**: drag materials, smart materials and smart masks onto the model (applies to
  the part under the cursor) or onto a layer. (#18)
- **Stencils**: paint through an image laid over the 3D view. Hold S and drag to move, rotate or
  scale it; hold N to paint without it. (#20)

### Fixed
- Projects containing TGA images could not be reopened. (#11)
- Edge wear and other generator masks followed UV seams instead of the model's edges. (#6)
- Layer rows overlapped the Blend / Opacity controls in a short Layers panel. (#19)

### Changed
- Project file schema 2 (image masks). Older projects open unchanged; older SubCraft versions
  can't open projects saved with this version. (#13)
