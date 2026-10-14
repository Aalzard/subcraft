# Changelog

What changed in SubCraft, newest first. Download the latest build from the
[Releases page](https://github.com/Aalzard/subcraft/releases/latest).

## Unreleased

### New
- **Web version**: use SubCraft in the browser at https://aalzard.github.io/subcraft/ (needs
  WebGPU). Open uploads files, Save and Export download them. FBX is desktop only, and big
  operations are slower than in the desktop app.

### Changed
- Shelf thumbnails no longer pause the app while they render.

## 0.1.2

### New
- **Parts**: show a layer only on some pieces of the model, like an axe's blade. Properties →
  Parts → *Pick parts in the 3D view*, then click pieces to show or hide the layer there (the piece
  under the cursor lights up). Painting, fills, smart materials and export all respect it, and it
  is saved in the project. Parts are the separate (unconnected) pieces of the mesh.

### Changed
- **New app icon**: an engraved hand on orange, used for the program, the window and taskbar, the
  installer and the Linux menu entry.

## 0.1.1

### New
- **Textures in the shelf**: Shelf → Textures lists every image you imported (or put in
  `Library\textures`), sorted into Base Color / Normal / Roughness… by file name. Drag one onto a
  channel row in Properties to use it there, or onto the model / a layer for a new fill layer.
- **Tiling X / Y and offset**: fill projections tile and shift on each axis separately (Link keeps
  them equal), with a reset button.
- **Channel strength sliders**: every fill channel has a power slider — Normal and Height
  strength, Roughness / Metallic / AO strength, Base Color brightness, Emissive intensity. Saved in
  projects (older projects open at full strength).

### Fixed
- SubCraft crashed at startup on some PCs (older Intel graphics drivers, `igvk64.dll`). On
  Windows it now runs on Direct3D 12 instead of Vulkan; set `WGPU_BACKEND=vulkan` to go back.

## 0.1.0

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
