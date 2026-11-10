SubCraft 0.3.1 is a hotfix.

- Fixes a crash a few seconds after start-up on some Windows PCs with Vulkan overlays installed (for example GOG Galaxy) (#98). GPU compositing and baking now use the same graphics API and adapter as the window.

Nothing else changed since 0.3.0. Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.1-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums (`SHA256SUMS.txt` for Windows, `SHA256SUMS-linux-x86_64.txt` for Linux).
