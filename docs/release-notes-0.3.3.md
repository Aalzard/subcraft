SubCraft 0.3.3 is a hotfix.

- If your graphics driver resets while SubCraft runs (the screen goes dark for a moment), the next start now opens in safe mode and turns GPU compositing and GPU baking off, so it doesn't repeat on every start. Turn them back on in Preferences after a driver update (#119).

Nothing else changed since 0.3.2. Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.3-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums (`SHA256SUMS.txt` for Windows, `SHA256SUMS-linux-x86_64.txt` for Linux).
