SubCraft 0.3.2 is a hotfix.

- Fixes a crash ("map accumulators: BufferAsyncError") when the GPU fails during a bake; SubCraft now finishes on the processor instead (#119).

Nothing else changed since 0.3.1. Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.2-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums (`SHA256SUMS.txt` for Windows, `SHA256SUMS-linux-x86_64.txt` for Linux).
