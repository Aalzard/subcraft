SubCraft 0.7.0.

- **Baking**: additional ID maps (ID 2 to ID 9), skew correction with edge protection, paintable skew and cage push maps, an Opacity map, curvature auto tonemapping, 8192 bakes, bake settings per texture set, AO Ignore backface, and baking on several graphics cards at once.
- **Painting and layers**: Brush stamp mapping (brush material images follow each dab), Depth culling for planar fills, Clear layer content, Instantiate across texture sets (Ctrl+Shift+D), and R/G/B channel picks in the Mesh Map generator.
- **Substance archives**: .sbsar layers read your baked mesh maps through their image inputs.
- **Viewport and workspace**: axis views from the axis indicator or Alt+1/3/7, texture set mixing shown in the 3D view, roughness-aware screen-space reflections, and panel tabs that wrap onto more rows.
- **Plugins**: shareable plugin folders with scripts and assets, a Manage plugins window, and two example plugins.
- **Faster**: smart materials and project opening (procedural noise about 40% faster, blends about 2.8x faster), several patterns 1.6 to 2.6x faster, blur filters about 30% faster.
- **Fixes**: exported baked normals follow the project's DirectX / OpenGL setting; a quick bake no longer re-bakes the extra ID maps.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.7.0-setup.exe`), portable zip, Linux x86_64 tarball, AppImage and Flatpak (Linux builds are run by CI with `--version`, not opened by hand), and checksums.
