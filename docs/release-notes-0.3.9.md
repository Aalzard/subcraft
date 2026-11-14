SubCraft 0.3.9.

- **MCP server**: AI agents (Claude Code, Claude Desktop, any MCP client) can texture models with SubCraft's engine without the app's window open. Thanks to @himsssss for starting it (#92).
- **Themes**: Dark, Light and a Painter-style grey, in Settings > General.
- **Linux pen tablets**: native Wayland tablet input (clicks and pressure) and X11 pressure (#82).
- **Painting by part, UV island or object**: excluded geometry is greyed out in 3D, the hovered part is highlighted, and Polygon Fill previews in the 3D view.
- **Fill projection manipulator**: move, rotate and scale fill projections directly in the 3D view.
- **Brush**: Stamps Blending, hold A to continue the previous stroke, brush preview styles and pen pressure curves.
- **Paths**: the pen tool in the 3D view, and erase and smudge along a path.
- **Saved with the project**: bake settings, the viewport camera and the export folder (#320, #321, #322).
- **Baking**: the bake preview follows the Average normals toggle (#307) and comes back when you re-enter Baking mode (#323); ID colour picking shows the ID map (#333); a choice of GLB vertex colour sets for the ID map (#366).
- **Faster**: large brushes on 4K projects no longer stall.
- Plus Blur Slope, an environment map mini-shelf, Shift+Esc to pause engine computation, and many fixes.

Full details: [CHANGELOG.md](https://github.com/Aalzard/subcraft/blob/main/CHANGELOG.md).

**Windows build is not code-signed.** SmartScreen may warn on first run: choose More info, then Run anyway. Check the download against `SHA256SUMS.txt`.

**Files:** Windows installer (`SubCraft-0.3.9-setup.exe`), portable zip, Linux x86_64 tarball (built by CI, not run by hand), and checksums.
