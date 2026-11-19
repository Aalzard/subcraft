SubCraft 0.4.4.

- **Image tweaks**: Balance, Contrast and Invert on an image used in a fill layer channel, and Balance and Contrast on a mask's Image effect, like a procedural pattern has. Thanks to @slimane-d3v (#591).
- **Assets panel**: preview thumbnails for generators, a Grunges category with the built-in Dirt Grunge map, and alphas can be dropped on a fill layer's channel row. A texture dropped on a mask thumbnail now goes into the mask. Thanks to @slimane-d3v (#591).
- **Faster generators on large texture sets**: mesh maps baked below the set's resolution (1024 maps on a 4096 set) are read 7 to 9 times faster, with the same result.
- **Fixed**: Paint across UDIM tiles kept working only until a material went onto one tile alone; layers are now paired between tiles by name. Thanks to @Lwanmtr (#581).

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.4.4-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
