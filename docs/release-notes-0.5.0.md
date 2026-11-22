SubCraft 0.5.0.

- **Use Micro Details on generators**: Dirt, Edge wear, AO and Curvature generators can read the Normal and Height of the layers below them, so wear catches the small raised details of a material and dirt settles into its grooves, as in Substance Painter.
- **Better Edge wear**: a thin edge line at low Wear that grows with Wear and disappears at 0, a much wider Spread, wear on rounded edges and a chipped-paint breakup.
- **Much lower memory on big projects**: an 11-set 2048² project went from 6.0 GB to 1.4 GB peak, and its first full composite is about twice as fast (issue #628).
- **Less desktop lag**: SubCraft uses far less GPU and processor time while idle or compositing, so other programs stay smooth (issue #628).
- **Faster exports and layer edits**: exports about 3.6 times faster, and layer edits under smart materials up to 3 times faster, with identical results.
- **Reset to default everywhere**: Ctrl+click a setting, or right-click its name, to put its default back in the brush settings, layer and fill properties, generators and the other panels, and a dot marks the settings you changed (issue #609).
- **Eyedropper**: one pick from a painting tool goes back to the brush, and a pick samples every channel the brush paints (issues #619, #620).
- **Fixes**: Shelf brushes no longer switch your brush channels back on (issue #618); UDIM paint-across opacity drags and multi-layer actions; brush preset values checked.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.5.0-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
