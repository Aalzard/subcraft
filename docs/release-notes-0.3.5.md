SubCraft 0.3.5.

- **Slider boxes keep what you type** (#140): typing `.25` no longer turns into `0.52`. The text stays until Enter or clicking away, Escape reverts, and a decimal comma works.
- **Brush colour picker** (#141): hue ring, saturation/value square, RGB and HSV sliders, a hex field, old/new swatches and your last 12 colours.
- **Closer to Substance Painter:**
  - Brush block in Properties: Size and Flow with pen pressure, jitters, Size Space and an Alpha slot.
  - Stencil tiling, snap rotation and reset; the eraser erases only the channels you enable; clone source Relative or Absolute.
  - Tool, brush and material presets from Properties, with rename, duplicate and favourites in the Shelf.
  - Fill projection options: Filtering, UV Wrap, Shape crop and tri-planar Hardness.
  - More filters: Smoothstep, Threshold modes, Blur wrap, HSL, Color Correct, Transform, Mirror and more.
  - Mask Editor generator, combining textures and baked maps in one mask.
  - Display Settings panel (View > Display settings), saved with the project.
  - Environment and camera: backdrop opacity and blur, exposure in EV, field of view, click to set focus, F9 back to painting.
  - Post effects: per-effect switches, more tone mapping, glare shapes, lens flare, aberration, vignette, sharpen, grain and colour correction.
  - Viewport: solo view of any channel, grid plane and colour, texture filtering and a camera frame.
- **Export preset editor:** make your own export presets with file name patterns, formats and bit depths, channel packing and per-map overrides. TIFF export, and Substance Painter export presets can be imported.
- **Many crash and bug fixes**: GPU device loss saves your work instead of crashing, corrupt `.spp` files and unusual meshes no longer crash the import, mesh reload keeps paint, exports can't overwrite each other, strokes no longer land on text layers, and more.
- Linux: a clear message when window libraries are missing or there is no display, instead of "update your graphics driver".

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** Free code signing through SignPath Foundation is being set up, but this build is still unsigned. SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.5-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
