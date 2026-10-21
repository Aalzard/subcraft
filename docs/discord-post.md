# Discord post draft

Every claim below was seen in the app or covered by a test in this repository. The captures are
in `docs/screenshots/showcase-*.png`. Not covered here: the parts right-click menu and the pen
path (they need mouse input, so use the older `layer-tree.png` / `pen-path.png` or record them
live), and a workflow video, which has to be recorded by hand. Check the release notes before
promising that anything below is in the downloadable build, since this work is on
`feature/brush-roadmap` and has not been released.

---

Hey everyone! I've been building SubCraft, a free 3D texture painter in the spirit of Substance Painter, and I'd love for you to try it and tell me what's missing.

A few things that are new or recently reworked:

- **Edge wear that behaves.** The amount (Level) and how far it creeps in from the edge (Spread) now work together, so you can get a thin chipped rim or heavy worn paint just by moving two sliders. It only starts at edges you can actually see, and the boundary is chipped, not a straight stripe. (`showcase-edge-wear.png`, `showcase-edge-wear-sheet.png`)
- **Smart materials.** Old Wood and Worn Painted Metal are on the crate in `showcase-finished-crate.png`, both from the built-in Shelf. One drag each.
- **Starter pack.** 30 normal maps and 30 brush tips come with it, free to use. Drag a normal onto a brush slot or a layer channel. (`showcase-starter-pack-shelf.png`)
- **Brushes you can pick in one click.** Click a brush on the Shelf and it's ready to paint, with a small "Brush: name" note in the corner of the 3D view. Double-click to edit it. (`showcase-brush-notice.png`)
- **Layers with folders, masks and 32 blend modes.** (`showcase-layer-tree.png`)
- **Filters on layers and masks:** blur, sharpen, directional blur, highpass, bevel and mask outline.

Try it:
- In your browser (needs a recent Chrome/Edge/Firefox/Safari): https://aalzard.github.io/subcraft/
- Windows desktop download: https://github.com/Aalzard/subcraft/releases/latest

It's still early, so please tell me what's missing or broken. What would stop you using it for a real project?
