SubCraft 0.3.4.

- **Anchor points**, like Substance Painter: capture a layer's content or mask at a point and read it from layers above (fill source, Anchor Reference mask effect, generator input). `.spp` import keeps Painter's anchors live. Projects use schema 17; older projects open unchanged, older SubCraft refuses the new files instead of losing anchors.
- Linux: `--x11` option for pen tablets on Wayland compositors without tablet emulation (#82).
- OBJ and glTF import no longer crash on partial UVs or normals, and glTF import ignores missing texture files.
- A quick pen tap paints with its own pressure instead of full pressure.
- Saving refuses a project that is too large to open again; start-up survives non-UTF-8 command-line arguments.

**Known issues**
- Not in this release: the export preset editor and a few more crash fixes are still being finished.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.4-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums (`SHA256SUMS.txt` for Windows, `SHA256SUMS-linux-x86_64.txt` for Linux).
