## SubCraft 0.3.0

Generators work more like Painter's, 4K sets are a lot more usable, and the app is faster and has a new window.

**What's new**
- **Generators like Painter.** Dirt and Edge wear use a real grunge texture with Painter's defaults and control names, plus an Ambient Occlusion generator. Old projects and the built-in smart materials look the same as before.
- **4K generator sliders** update while you drag, and the noise cache no longer recomputes everything on each step.
- **Brush preview.** Pick a tip, preset, alpha, normal map or height map in the Shelf and it becomes the brush right away, with a preview card in Properties. Normal-map brushes switch instantly.
- **Faster:** compositing, noise, image fills and brush strokes are roughly 1.5x to 3x faster depending on the stack, with identical output.
- **Frameless window** with its own minimize / maximize / close buttons (`SUBCRAFT_NATIVE_TITLE_BAR=1` brings back the OS frame).
- **Texture set size** can be changed from the Texture Set List. Shift+right-drag turns the environment, like Painter.
- New startup defaults: the Shelf opens on Smart Materials and the environment is Glasshouse Morning.
- Fixed the language picker for Korean and Traditional Chinese.

**Known issues**
- Not checked by hand with a pen or a mouse: the brush preview card, the frameless window edges and the new generator defaults were covered by automated tests and scripted captures.
- Layout is close to, not identical to, Painter.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.0-setup.exe`), portable zip, Linux x86_64 tarball if listed, and `SHA256SUMS.txt`.
