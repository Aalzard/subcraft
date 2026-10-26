# Discord post draft (SubCraft 0.1.8)

Fits Discord's 2000-character limit. Screenshots to attach are in `docs/screenshots/`:
`v018-overview.png`, `v018-shelf-environments.png`, `v018-new-project.png`, `v018-export-window.png`,
`tool-clone.png`, `viewport-quality-glow-glare.png`. Nothing here claims more than the release notes do.

---

**SubCraft 0.1.8 is out!** Free, open-source 3D texture painter in the spirit of Substance Painter (Windows).

**Painting**
- New **Clone** and **Smudge** tools, plus "project from the view" for brush images
- **80 normal maps + 80 brush alphas** (CC0) in the starter pack, 50 + 50 of them new

**Materials**
- **15 new smart materials** (20 total) and **16 new smart masks** (21 total), plus new Dirt, Cracks, Streaks, Speckles and Pores generators
- **10 built-in HDRI environments** (interiors, forest, alpine lake, neon arcade, space atrium...). Full-size versions are an optional download on the release page

**Projects and export**
- New **New Project window**: see triangles, materials, UV tiles and warnings before you create anything; choose template, resolution, split rule and normal format
- **UDIM** support: one texture set per tile, `.1001` names or per-tile folders
- **Export checkboxes** for every map, Select all / None / Invert, Alt+click to export just one
- **Unity URP** and **Godot 4** presets, plus Height and Bent Normal bakes

**Look and feel**
- Tone mapping, bloom, ambient occlusion, emission glow and glare; render stills and PNG turntable frames
- Refreshed charcoal UI: Shelf on the left, effect stage thumbnails, drag-to-reorder effects, channel cards
- Edge wear and filters (blur, sharpen, bevel, highpass...) reworked

Heads-up: this build isn't code-signed yet, so Windows SmartScreen may warn (More info > Run anyway). Turntable export is PNG frames only (no GIF/MP4), and there's no UDIM tile picker yet. The browser version is gone; SubCraft is desktop only now.

Download: https://github.com/Aalzard/subcraft/releases/latest
What would stop you using it for a real project?
