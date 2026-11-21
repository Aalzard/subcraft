SubCraft 0.4.6.

- **Paint across UDIM tiles**: materials, textures, filters, smart materials and duplicated layers dropped on a UDIM model now reach every tile. Before, such a layer landed on one tile only and could vanish from the Layers panel while it still rendered (issue #581).
- **Display Settings reset**: Reset all at the top of the panel puts every display setting back, keeping the environment map and colour LUT you picked. Ctrl+click a setting, or right-click its name, to reset just that one; a dot marks the settings you changed (issue #607).
- **Studio lighting turns**: the studio presets (Studio, Soft Studio and the other chrome-ball environments) now rotate with Shift + right drag and the Rotation slider, like HDR environments (issue #610).
- **Painting normals over other layers**: Properties > Paint now says when the layer's Normal blend lets the bumps of the layers below show through your strokes, with a one-click "Cover the layers below" button (issue #611).
- **Better crash reports on macOS**: the crash window and bug report now name the actual error and where it happened, not just "panic in a function that cannot unwind" (issue #612).
- **Join the SubCraft Discord**: Help > Discord server opens the invite, and an occasional card offers it too, until you join or choose "Don't show again".

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.4.6-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
