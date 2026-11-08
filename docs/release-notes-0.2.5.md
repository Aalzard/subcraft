## SubCraft 0.2.5

Big interface release: a Painter-style layout, a new default model, two new languages, and a faster pen.

**Highlights**
- Painter-style layout: Assets shelf, Texture Set List, Layers, Texture Set Settings and a context-sensitive Properties panel; side-by-side 3D | 2D view.
- Starts on the bundled **SubCraft_base** model with the **Dusty Plastic** smart material; 2048 default resolution.
- Korean and Traditional Chinese interface (machine-drafted, native review welcome).
- **Normal-map brush** (paint normal / height stamps from the shelf) and a **Micro effect** on generators that also writes relief into Normal / Height.
- **18 new PBR materials** (basalt, travertine, terrazzo, cobblestone, asphalt, bark, cork, bamboo, linen, leather, rubber, aluminum, copper, galvanized steel, hex armor and more) as materials and smart materials.
- **High-poly alignment** (offset, scale, rotation, auto-fit, blue overlay) with a redesigned Bake window; smoother AO and sharper mesh maps.
- Text tool rework (composer window, on-model handle), depth of field, new generator controls, world-space blur, Painter keymap preset.
- New logo and warmer charcoal theme.
- Faster pen strokes via a live preview that thins stamps and flushes exactly on release.
- Safer on odd hardware: safe mode, GPU capability checks, diagnostics copy.

**Known issues**
- Very large brushes at 4096 are faster but not 60 fps.
- Some Texture Set Settings fields are read-only (description, shader instance, channel format/remove, non-square sizes).
- Layout is close to, not identical to, Painter.
- Linux build not included in this candidate unless listed in the assets.
- The new brush, Micro effect, bake alignment and bake-quality changes were checked by automated tests and scripted captures, not by hand with a mouse or pen.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Files:** Windows installer (`SubCraft-0.2.5-setup.exe`), portable zip, and `SHA256SUMS.txt`.
