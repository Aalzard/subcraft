SubCraft 0.4.2.

- **Smart material pack defaults**: pack materials set their own normal strength, height strength and UV tiling, applied automatically (normal 0.15, height 0.07, tiling 3 by default); the Assets panel downloads the materials-v2 pack.
- **Painter (pen) workspace layout**: a Pen panel with a colour wheel, big shortcut buttons and the brush settings, from Window > Layout.
- **Per-channel brush blending**: a blending mode for each channel of the brush material.
- **Material Preview**: the brush material on a lit sphere; drag to turn the light.
- **Custom brush test stroke**: drag in the brush preview to try the brush along your own stroke.
- **Anchors navigation**: jump from a reader to its layer, and a go-to button beside every anchor picker.
- **Quick setup**: a texture dropped on the model asks which channel it fills.
- **Faster**: automatic UV unwrap up to 7.5x, the 2D view Material display about 10x, and the ID colours panel no longer slows painting.
- Also: smooth Ambient Occlusion generators by default, a history steps setting, a Help menu, Ctrl+Alt+drag panel scrolling, tip footprint and blocked-stroke cursors, new command-line flags, and several bug fixes.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.4.2-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
