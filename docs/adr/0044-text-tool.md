# ADR 0044: Text tool (decals, brush tips, projected stamps)

## Status
Accepted (branch `cloud/text-tool`).

## Context
Parity with TextCraft's text tool: type text, pick any font (bundled, installed, or a `.ttf` / `.otf`
file), set size and spacing, and either stamp the text along strokes or place it as a decal.
Arabic / Hebrew and other complex scripts must shape correctly. Project files keep text editable.
`CLAUDE.md` forbids egui / wgpu below `render` / `ui`, so shaping and rasterization live in a
plain crate.

## Decision
* **New crate `subcraft-text`** (depends only on `core`): `rustybuzz` (HarfBuzz port, MIT) for
  shaping, `unicode-bidi` (MIT/Apache) for mixed-direction runs, `ab_glyph_rasterizer`
  (Apache-2.0) for analytic area coverage, `fontdb` (MIT) for installed fonts. All pure Rust.
  `layout()` produces glyph positions in em units; `rasterize()` renders the outlines under any
  affine transform at the target resolution (no intermediate bitmap, so decals are exactly as
  sharp at 8192² as at 512²). Edges are clipped by us before they reach the rasterizer, because
  `ab_glyph_rasterizer` mis-indexes (panics in debug) on geometry left of / above the buffer.
* **Fonts are identified by `FontRef`**: `Bundled { name }`, `System { family, bold, italic }` or
  `Embedded { resource }`. A font file picked by the user is embedded in the project as a
  `ResourceKind::Font` resource (original bytes, UUID identity, never the path), like images.
  A missing system font falls back to the default bundled font; a missing embedded font is an error.
  Bundled: DejaVu Sans / Sans Bold / Serif / Sans Mono (Latin, Greek, Cyrillic, Arabic, Hebrew;
  Bitstream Vera licence, shipped in `crates/text/assets/fonts`).
* **A text decal is an editable source on a paint layer** (`PaintContent::text`, same pattern as the
  pen path): parameters in resolution-independent units (UV position, size as a fraction of the
  texture width, rotation); the channel rasters are a cache rendered from the outlines. Changing the
  texture-set resolution re-renders text layers instead of resampling them.
* **Brush use**: the text is rendered to a square alpha tip (`paint::text_decal::text_tip`) and
  goes through the existing stamp kernel, so symmetry, spacing, jitter, culling and stencils all
  apply. A *projected decal* is one camera-aligned dab of that tip: ordinary (undoable) paint,
  not editable afterwards.
* **Schema 16**: `text` on paint content (`TextLayerV16`) plus `kind: "font"` resources.
  Schemas 1-15 load unchanged.
* **Undo**: every edit is `InsertLayer` / `ReplaceLayer`, one history entry each.

## Consequences
* UV-space decals cannot cross UV seams (each island is rendered separately by the compositor
  sampling the UV raster); use the projected stamp for text that must wrap over an edge.
* No font fallback chain: characters missing from the chosen font render as that font's `.notdef`
  glyph (the layout reports the count in `TextLayout::missing`).
* Letter spacing is not applied inside cursive scripts (it would break joins).
* Mesh reload converts text layers to plain paint layers (their UV position is only valid for the
  old layout), like pen paths.
