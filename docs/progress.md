# SubCraft — implementation progress

Last updated: 2026-10-13 (session 4: height bumps, project save/load, texture export)

## Decisions

* Stack: Rust 2024 + wgpu 30 + egui 0.36/eframe + egui_dock 0.21 — see `adr/0001-technology-stack.md`.
* Texture engine: CPU-authoritative copy-on-write tiled images (64² tiles, f16), per-tile
  compositor with dirty-tile tracking, projection brush — `adr/0003-texture-engine.md`.
* Conventions: right-handed, +Y up, CCW front faces, UV origin top-left (glTF), MikkTSpace
  tangents → OpenGL (+Y) normal maps, wgpu camera conventions — `adr/0005-coordinate-conventions.md`.
* Undo: history of structurally shared document snapshots + merge keys; strokes push the
  post-stroke document — `adr/0006-undo.md`.

## Done (all tests passing — 127 tests, `cargo test --workspace`)

| Crate | Contents | Tests |
|---|---|---|
| `core` | typed UUID ids, `ChannelKind`, `MeshMapKind`, sRGB/HSV math, `IRect` | 7 |
| `image` | `TiledImage` (COW tiles, diff by pointer, value `PartialEq`), `Bitmap` (u8/u16/f32, bilinear), PNG/TGA/JPEG/EXR I/O, dilation (jump-flood infinite + width) | 12 |
| `mesh` | canonical `Mesh` + `MeshBuilder`, OBJ/glTF/FBX import, OBJ writer, SAH BVH, topology, versioned binary encoding, UV rasterizer with conservative rim + **region rasterization** (`rasterize_surface_region`), seam-heavy primitives | 22 |
| `doc` | 32 blend modes, fills/projections, masks, paint/fill/folder layers, `TextureSet` tree ops, `Document`, `History`, `ResourceStore`; `same_parameters` now treats a lazily created paint channel as a raster-only change | 9 |
| `materials` | noise + patterns; **`sample`**: lazily built per-tile `TileGeometry` (normalized position, normal, coverage) cached per slot + resolution, `MeshMaps`, `eval_fill_tile` for Uniform / Bitmap / Procedural / MeshMap through UV / Planar / Triplanar | 10 |
| `compositor` | W3C separable compositing of all 32 modes (whiteout / RNM / HSV / signed height), isolated + passthrough folders, mask stack (paint, fill, generators, levels, invert), rayon per-tile evaluation, `CompositeCache` dirty-tile diff | 18 (6 unit + 12 acceptance) |
| `paint` | `BrushSettings` / `BrushMaterial` / `Camera`, path resampling (spacing, lazy mouse, straight line, jitter), symmetry via mirrored cameras, CPU depth map + backface culling, tile-parallel UV-raster stamp kernel (Tangent\|Wrap, Tangent\|Planar, Camera, UV), stroke-opacity-capped coverage written from the pre-stroke snapshot, eraser, mask painting, polygon fill (triangle / element / UV island) | 21 (10 unit + 11 acceptance) |
| `app` | **new**: `Session` (scene + BVH/geometry/depth caches, document + `History`, selection, brush state, active stroke, per-texture-set `CompositeCache`), undoable `Command`s (layers, visibility, rename, blend mode/opacity with coalesced drags, fill channels/projection, masks + effects), stroke lifecycle (one history entry per stroke, cancel), picking, toolkit-independent `Action` + shortcut table; project save / open with unsaved-changes tracking; texture export | 11 |
| `render` | **new**: wgpu offscreen viewport: metal/roughness GGX + key light + procedural sky (analytic irradiance + split-sum), ACES, 4x MSAA to an sRGB texture; composite channels uploaded per dirty tile as f16; channel preview pass for the 2D view; Height shown as bump (tangent-space normal from height, whiteout with the normal map); headless device + readback | 3 (render on any adapter; llvmpipe here) |
| `ui` | **new**: eframe + egui_dock workspace: 3D view (orbit/pan/zoom, painting, Shift+click lines, brush cursor), 2D view (composite + UV wireframe), Layers, Properties (fill channels, procedural patterns, projection, masks, generators/levels/invert), Brush (tools, settings, jitter, symmetry, lazy mouse, material channels), Texture Sets, History (click to jump), menus, status bar, shortcuts, dark theme, layout persistence | 2 |
| `project` | **new**: `.subcraft` zip container: `manifest.json` (format + schema version gate), `mesh.bin`, `document.json` (explicit schema v1 for texture sets / layer tree / masks / effects; small value types pinned by a golden JSON test), sparse raster blobs (`SCTILE`, only allocated tiles), original resource bytes; atomic save (temp file + rename); validation on load (resolutions, component counts, truncation) | 7 |
| `export` | **new**: templates (PBR Metal/Rough, Unreal packed ORM + DirectX normal, Unity HDRP mask map, glTF ORM), per-component packing (invert for smoothness, constants), sRGB only for color channels in integer formats, height baked into exported normals (same math as the viewport), height as mid-gray-centered PNG16 / raw EXR, UV padding (infinite / N texels / none), file-name plan | 5 |
| `apps/subcraft` | desktop binary: `subcraft [project.subcraft | mesh] [--demo] [--view-2d] [--screenshot out.png]` | - |

Acceptance tests from the brief, all passing (`crates/compositor/tests/compositor.rs`,
`crates/paint/tests/paint.rs`, `crates/materials/src/sample/tests.rs`):

* sample: uniform exact; UV procedural == `pattern::eval` at `transform_2d(uv)`; triplanar on each
  cube face == the matching planar sample; bitmap sRGB decode only for color; mesh-map fallback.
* compositor: reorder two paint layers; hidden parent folder hides children; channel opacity
  isolated to its channel; black mask hides, white mask paint reveals locally (1 dirty tile);
  fill roughness change keeps paint above; passthrough (0.25) vs isolated (0.5) under Multiply;
  single-tile edit → exactly that dirty tile (also for a lazily created channel); undo via
  `History` → earlier composite bit-exact, re-evaluating only the stroke's tile.
* paint: cube seam continuity (max coverage jump 0.04 across the edge); thin slab back island
  untouched (with culling, depth-only, Camera and Planar alignment); disabled channels share
  storage and are byte-identical; identical footprints/determinism; overlap ≤ stroke opacity;
  base color + roughness in one stroke; History undo/redo exact (and pre-stroke tiles untouched);
  X symmetry mirrored (coverage mass 374.6 vs 372.0, mirrored centroids < 0.02); UV-island polygon
  fill touches no other island.
* Mutation check: forcing `DepthMap::visible` to `true` makes the slab test fail (1554 texels
  painted through) — the visibility test is load-bearing.

Measured (release, 4-core cloud container, 2048² Body set of the sample crate):
full composite with a 6-octave Clouds procedural fill 650 ms (805 ms incl. first geometry build);
first dab incl. 1920×1080 depth map 23 ms; 200 cursor samples (95 stamps) 75 ms; incremental
recomposite after that stroke 12 ms (15 tiles).

Run the app: `cargo run --release` (opens the sample crate; `cargo run --release -- --demo` adds
a wood + moss layer stack with painted strokes; `cargo run --release -- path/to/mesh.fbx` opens a mesh).

Tests: `cargo test --workspace` (render tests skip themselves if no GPU adapter exists).

### First visual milestone, verified by running the real app (session 3)

In the cloud container (no GPU) the app ran on lavapipe/llvmpipe (Vulkan) under Xvfb:

* `--demo --screenshot`: window, dock layout, procedural wood fill, moss fill masked by a
  Directional generator (top faces only), painted strokes, 2D view with UV wireframe:
  `docs/screenshots/demo-3d-view.png`, `demo-2d-view.png`.
* Real input via `xdotool`: LMB drag paints (the stroke continues across the corner seam onto
  the side island), `]` enlarges the brush, Ctrl+Z removes the last stroke, Ctrl+Y restores it,
  RMB drag orbits: `docs/screenshots/mouse-painting.png`.
* Bugs found and fixed by running it: egui samples textures as gamma-encoded, so the viewport is
  shown through an `Rgba8Unorm` view of the sRGB target (and the 2D view through a linear-to-sRGB
  preview pass); icon glyphs missing from the default fonts replaced with text; scripted
  screenshot runs no longer overwrite the saved layout. (Xvfb has no window manager: keyboard
  tests need `xdotool windowfocus`.)
* Viewport render is about 40 ms/frame at 920x860 on llvmpipe (CPU); a real GPU is far faster.

### Session 4 — verified in the running app

* Height painting now shows as bumps in the 3D view (View menu: Height bump strength);
  headless test proves height and metallic/roughness visibly change the render.
* File menu: New, New from mesh, Open project (Ctrl+O), Save (Ctrl+S), Save as (Ctrl+Shift+S),
  Export textures (Ctrl+Shift+E). Window title shows the project name and `*` when modified.
* Driven with xdotool under Xvfb: Export window lists the planned files and clicking Export wrote
  all 10 maps of both texture sets; File > Quit with unsaved changes shows the Save / Don't save /
  Cancel prompt; Cancel keeps the app open, Don't save closes it.
* Not testable in the container: the native file dialogs (rfd needs a desktop portal on Linux).
  The save/open code paths behind them are covered by `app` and `project` tests; the dialogs
  themselves need a check on Windows.

CPU engine tests only: `cargo test -p subcraft-core -p subcraft-image -p subcraft-mesh -p subcraft-doc -p subcraft-materials -p subcraft-compositor -p subcraft-paint`

### Deviations from the brief (sound-engineering choices)

* **Branch**: work was pushed to `claude/practical-curie-lk9f9s` (the cloud session only allows
  that branch) instead of `engine/compositor-paint`; never to main.
* **Texel write** is source-over in premultiplied space rather than a lerp of straight values:
  lerping straight value toward the brush color from a transparent texel would blend in the
  image's fill value and produce gray halos at soft edges. Still recomputed from the pre-stroke
  snapshot by `cov × stroke_opacity`, so the opacity cap holds.
* **`roundness`** added to `BrushSettings` so `angle` / `follow_path` / angle jitter have a visible
  effect without brush-alpha bitmaps.
* **Composite 4th component** of rgb channels is the accumulated alpha (1 at the root), giving the
  renderer an RGBA16F upload.
* **Mask eraser** returns the base raster toward its fill value (masks have no alpha to reduce).
* **Stamps may center on another texture set's surface**; only the active set's triangles are
  painted (a brush straddling a material border paints its part).
* **UV alignment** skips 3D visibility (paints the UV disc, as in the 2D view).
* **Signed Addition**: neutral value adds nothing (0.5 for 0..1 channels, 0 for Height).
* **Generators**: Directional uses the low-poly interpolated normal (always available) rather
  than a baked world-normal map; Position Gradient runs along +Y within the mesh bounds.

## Next steps (in order)

1. `bake` (normal from high poly / world normal / position / ID / AO / curvature / thickness into
   `MeshMaps`) so Edges / Cavity generators and mesh-map fills get real data; Bake window;
   AO + curvature included in exports.
2. `assets`: brush alphas + stencils, material / smart-material presets, environment HDRIs
   (real IBL with prefiltered maps), shelf panel.
3. Autosave + recent files; export presets saved per project; custom export templates editor.
4. UI follow-ups: drag-and-drop layer reordering, inline rename, eyedropper, painting in the
   2D view, texture-set resolution change, camera FOV / ortho toggle, texture mipmaps.
5. Engine follow-ups: incremental dirty tracking inside mask paint effects; near-plane
   clipping in the depth rasterizer; f16 tile geometry; renormalize normals after opacity
   lerp; GPU compositor behind the same interface.

Remaining stub crates (empty `lib.rs`): bake, assets.
