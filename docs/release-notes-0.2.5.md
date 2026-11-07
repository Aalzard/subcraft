## SubCraft 0.2.5

Big interface release: a Painter-style layout, a new default model, two new languages, and a faster pen.

**Highlights**
- Painter-style layout: Assets shelf, Texture Set List, Layers, Texture Set Settings and a context-sensitive Properties panel; side-by-side 3D | 2D view.
- Starts on the bundled **SubCraft_base** model with the **Dusty Plastic** smart material; 2048 default resolution.
- Korean and Traditional Chinese interface (machine-drafted, native review welcome).
- Text tool rework (composer window, on-model handle), depth of field, new generator controls, world-space blur, Painter keymap preset.
- New logo and warmer charcoal theme.
- Faster pen strokes via a live preview that thins stamps and flushes exactly on release.
- Safer on odd hardware: safe mode, GPU capability checks, diagnostics copy.

**Known issues**
- Very large brushes at 4096 are faster but not 60 fps.
- Some Texture Set Settings fields are read-only (description, shader instance, channel format/remove, non-square sizes).
- Layout is close to, not identical to, Painter.
- Linux build not included in this candidate unless listed in the assets.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Files:** Windows installer (`SubCraft-0.2.5-setup.exe`), portable zip, and `SHA256SUMS.txt`.
