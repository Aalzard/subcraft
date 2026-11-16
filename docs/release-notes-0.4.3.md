SubCraft 0.4.3.

- **Stitch brushes**: Stitch Straight, Cross, Zigzag and Needle Holes tips that join into a line of stitches along a brush stroke or a pen path, raised in Height. Thanks to @ConcretJJ for asking (#553).
- **Eyedropper everywhere**: every colour swatch has the eyedropper, and it samples anywhere in the SubCraft window. This also fixes the eyedropper on a fill layer's Base Color. Thanks to @slimane-d3v (#575).
- **Paint across UDIM tiles**: an option in Texture Set Settings so strokes cross tile borders and materials and new layers go to every tile. Off by default. Thanks to @Lwanmtr (#581).
- **Pen tool**: snapping to the model's vertices and edges, a snap angle setting, multi-point selection, Reverse path direction, path display settings, and Delete / Ctrl+C / Ctrl+V in the Paths panel.
- **Projection manipulator**: Q / W / E / R keys, plane handles, free rotation, Local / World space and a Surface tool for planar projections; Mirror on X / Y / Z for fill projections and a Symmetry button for fill layers.
- **2D view UV frame manipulator**: move, rotate and scale a fill layer's UV transform in the 2D view, with Mirror X / Y.
- **Blender navigation for pens**: Alt+LMB acts as the middle button, so a pen can orbit, pan and zoom. Thanks to @hced (#583).
- **ID map baking**: a vertex colour set for each additional high-poly file. Thanks to @HFutabaH (#366).
- **Fixed**: GLB files with custom vertex attributes now import (thanks to @HFutabaH, #582); on Wayland the lower pen barrel button pans as on X11 (thanks to @ivxvm, #82); no progress bar during brush strokes.
- Also: ribbon path tiling and alignment options, Warp projection reset and split tools, automatic ID mask on Ctrl+drop, Grayscale custom weights, GIF and WBMP export, and a Shelf auto-update menu.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.4.3-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
