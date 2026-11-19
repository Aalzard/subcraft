SubCraft 0.4.5.

- **Reassign Texture Sets on reimport**: a material renamed in your modelling program can keep its old texture set (layers and baked maps) instead of starting empty. Project configuration has a Texture set choice for each material of the reimported model.
- **Layer search**: a Search layers field filters the layer stack by name and opens the folders that hold matches.
- **GIF turntable**: turntable renders also save a looping 640 px `turntable.gif`, ready to share.
- **File > Save as template**: save a project's channels, resolution, normal format, export preset and UDIM setting under a name, then pick it in New Project under Your templates.
- **Faster under load**: after dragging a generator slider on a large texture set, the exact result arrives about twice as fast while other programs keep the processor busy.
- **Fixed**: compositing could hang for good the first time a Curvature generator with Medium or Coarse detail was evaluated. Help > Shortcuts no longer lists the projection manipulator keys as missing.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.4.5-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
