SubCraft 0.3.7.

- **Paint or fill a part, a UV island or the whole object**, like Painter: Polygon Fill on fill layers (triangle, polygon, mesh, UV island, box), the geometry mask, and Alt+H to hide the rest.
- **Paint directly on the 2D texture**: every paint tool now works in the 2D view, the same as in 3D.
- **Transparent textures** (#145): an Opacity channel, transparency in the viewport, and RGBA / Opacity maps on export.
- **Right-click Properties** in the viewport, an **editable shortcut editor**, a **Shader Settings** panel, cameras imported from the mesh file, and free camera rotation.
- **Baked map import** with an ID colour editor; ID colours from vertex colour, material, mesh ID or submesh (#117).
- **Painter .spp files open from File > Open** as a reconstruction (the layers are rebuilt; the original file is not changed).
- Many more Painter features: mask view and actions, Passthrough layers, flatten, generators on layer content, Compare Mask, Levels, new generators and filters, spherical/cylindrical fills, more export formats (BMP, HDR, WebP, half-float EXR, dithering).
- **Faster painting on big projects**: brush stamping is 4-5x faster on 4K texture sets.
- Fixes: menu bar readable with a light system theme (#206), export alpha and format fixes, safer file writes, and about 25 smaller fixes.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.7-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
