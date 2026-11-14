SubCraft 0.4.0.

- **Painter-style default layout**: 3D and 2D views side by side, with the 2D view narrower.
- **Projection tool** like Painter's: project a material over the view and move it with S+drag.
- **Colour profiles**: a viewport LUT, white point and profile name; LUTs can be imported as resources.
- **Smart materials library**: load a materials pack folder, browse thumbnails, and apply a material as a fill layer.
- **Save as copy**, and Project configuration with Reimport mesh.
- **Camera clipping fixed** (#446) and **black lines on baked ID maps fixed** (#466).
- Export: Dilation + transparent padding. Baking: File ID source. MCP server: ten robustness fixes.
- Many smaller fixes: themes, culling, Warp, the pen tool, brush presets and more.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.4.0-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
