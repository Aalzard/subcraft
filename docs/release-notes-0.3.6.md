SubCraft 0.3.6.

- **No more driver resets at start-up on integrated GPUs** (#119). GPU work is split into short, timed pieces so it never trips the Windows graphics watchdog, integrated GPUs get a smaller memory budget and no MSAA, and a lost graphics device no longer crashes the app.
- **Orbit around the point under the cursor** (#115), plus Painter's mouse gestures: 90 degree orbit snapping, Ctrl+Shift+click straight lines, Ctrl+MMB focus, Ctrl+Alt+RMB to pick a texture set.
- **Painter's menu bar**: File, Edit, Mode, Window and Viewport menus in Painter's layout, with view layouts (3D | 2D, 3D only, 2D only), F8 / F9 / F10 modes and Painter's display keys.
- **Faster edits in big layer stacks**: an edit near the bottom of a 40-layer 4K stack takes about half the time.
- Fixes: Clone without a source no longer adds an empty layer, shortcuts no longer act behind dialogs, the Transform filter on a flat layer, tri-planar blending at high Hardness, brush values kept as stored, stencil tiling and preset fixes.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.6-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
