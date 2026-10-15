# SubCraft — implementation progress

Last updated: 2026-10-15 (session 43: removed object hover tint)

## Decisions

* Stack: Rust 2024 + wgpu 30 + egui 0.36/eframe + egui_dock 0.21 — see `adr/0001-technology-stack.md`.
* Texture engine: CPU-authoritative copy-on-write tiled images (64² tiles, f16), per-tile
  compositor with dirty-tile tracking, projection brush — `adr/0003-texture-engine.md`.
* Conventions: right-handed, +Y up, CCW front faces, UV origin top-left (glTF), MikkTSpace
  tangents → OpenGL (+Y) normal maps, wgpu camera conventions — `adr/0005-coordinate-conventions.md`.
* Undo: history of structurally shared document snapshots + merge keys; strokes push the
  post-stroke document — `adr/0006-undo.md`.

## Implemented subsystems (latest test counts are in the session entries)

| Crate | Contents | Tests |
|---|---|---|
| `core` | typed UUID ids, `ChannelKind`, `MeshMapKind`, sRGB/HSV math, `IRect` | 7 |
| `image` | `TiledImage` (COW tiles, diff by pointer, value `PartialEq`), `Bitmap` (u8/u16/f32, bilinear), PNG/TGA/JPEG/EXR I/O, dilation (jump-flood infinite + width) | 12 |
| `mesh` | canonical `Mesh` + `MeshBuilder`, OBJ/glTF/FBX import, OBJ writer, SAH BVH, topology, versioned binary encoding, UV rasterizer with conservative rim + **region rasterization** (`rasterize_surface_region`), seam-heavy primitives | 22 |
| `doc` | 32 blend modes, fills/projections, masks, paint/fill/folder layers, `TextureSet` tree ops, `Document`, `History`, `ResourceStore`; `same_parameters` now treats a lazily created paint channel as a raster-only change | 9 |
| `materials` | noise + patterns; **`sample`**: lazily built per-tile `TileGeometry` (normalized position, normal, coverage) cached per slot + resolution, `MeshMaps`, `eval_fill_tile` for Uniform / Bitmap / Procedural / MeshMap through UV / Planar / Triplanar | 10 |
| `compositor` | W3C separable compositing of all 32 modes (whiteout / RNM / HSV / signed height), isolated + passthrough folders, mask stack (paint, fill, generators, levels, invert), rayon per-tile evaluation, `CompositeCache` dirty-tile diff | 18 (6 unit + 12 acceptance) |
| `paint` | `BrushSettings` / `BrushMaterial` / `Camera`, path resampling (spacing, lazy mouse, straight line, jitter), symmetry via mirrored cameras, CPU depth map + backface culling, tile-parallel UV-raster stamp kernel (Tangent\|Wrap, Tangent\|Planar, Camera, UV), stroke-opacity-capped coverage written from the pre-stroke snapshot, eraser, mask painting, polygon fill (triangle / element / UV island), brush alpha tips (`Stroke::with_alpha`) | 22 (10 unit + 12 acceptance) |
| `app` | **new**: `Session` (scene + BVH/geometry/depth caches, document + `History`, selection, brush state, active stroke, per-texture-set `CompositeCache`), undoable `Command`s (layers, visibility, rename, blend mode/opacity with coalesced drags, fill channels/projection, masks + effects), stroke lifecycle (one history entry per stroke, cancel), picking, toolkit-independent `Action` + shortcut table; project save / open with unsaved-changes tracking; texture export; background `BakeJob` feeding mesh maps to generators / export / project; shelf actions (insert preset / smart material layers, smart masks, brush alpha) and offscreen thumbnail scenes | 14 |
| `render` | **new**: wgpu offscreen viewport: metal/roughness GGX + key light + procedural sky (analytic irradiance + split-sum), ACES, 4x MSAA to an sRGB texture; composite channels uploaded per dirty tile as f16; channel preview pass for the 2D view; Height shown as bump; studio reflections (ring of panels + overhead + key softbox, roughness-dependent blur); baked / painted AO occludes ambient light; gray previews for scalar channels and baked maps; environment parameters per frame + HDRI upload (prefiltered mips sampled by roughness, SH irradiance, rotation); headless device + readback | 7 (render on any adapter; llvmpipe here) |
| `ui` | **new**: eframe + egui_dock workspace: 3D view (orbit/pan/zoom, painting, Shift+click lines, brush cursor), 2D view (composite + UV wireframe), Layers, Properties (fill channels, procedural patterns, projection, masks, generators/levels/invert), Brush (tools, settings, jitter, symmetry, lazy mouse, material channels), Texture Sets, History (click to jump), menus, status bar, shortcuts, dark theme, layout persistence | 2 |
| `bake` | **new**: CPU baker: tangent-space normal (low poly MikkTSpace frame; optional high poly through a front/rear cage), world normal, position, ID (per mesh element), AO (cosine-weighted BVH rays), curvature (seam-independent 3D neighbourhood convexity, 0.5 flat), thickness; gutter padding; progress + cancel | 7 |
| `assets` | **new**: built-in brush tips (procedural) + image import; 16 material presets (measured metal reflectances, plastics, paints, procedural concrete / wood / rust / leather); 5 smart materials (folders of presets masked by Edges / Cavity / Directional generators + grunge); 5 smart masks; 5 procedural environments + HDRI import (CPU prefiltered mip chain, SH9 irradiance, sun direction) | 5 |
| `project` | **new**: `.subcraft` zip container: `manifest.json` (format + schema version gate), `mesh.bin`, `document.json` (explicit schema v1 for texture sets / layer tree / masks / effects; small value types pinned by a golden JSON test), sparse raster blobs (`SCTILE`, only allocated tiles), baked mesh maps per texture set, original resource bytes; atomic save (temp file + rename); validation on load (resolutions, component counts, truncation) | 8 |
| `export` | **new**: templates (PBR Metal/Rough, Unreal packed ORM + DirectX normal, Unity HDRP mask map, glTF ORM), per-component packing (invert for smoothness, constants), sRGB only for color channels in integer formats, height baked into exported normals (same math as the viewport), height as mid-gray-centered PNG16 / raw EXR, UV padding (infinite / N texels / none), file-name plan; baked AO fills a missing Ambient Occlusion channel | 6 |
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
* User report "material channels don't work": painting Roughness changed the texture but at most
  1/255 on screen. Cause: the smooth sky gradient gave dielectrics nothing to reflect, and the
  scene was overexposed. Fix: studio environment in reflections (ring of light panels around the
  horizon, overhead light, key softbox; edges blur and dim with roughness), rebalanced key /
  ambient so mid-gray reads mid-gray. New end-to-end test (`ui/tests/material_channels.rs`):
  Session strokes of metallic / roughness / height each change 5k-12k viewport pixels.
* Not testable in the container: the native file dialogs (rfd needs a desktop portal on Linux).
  The save/open code paths behind them are covered by `app` and `project` tests; the dialogs
  themselves need a check on Windows.

### Session 5 — baking, verified in the running app

* File > Bake mesh maps (Ctrl+B) or Texture Sets > Bake mesh maps: maps, size, AO samples /
  distance, curvature radius, optional high-poly file + cage. Runs on a background thread with a
  progress bar and Cancel; results install into the session, re-evaluate composites (Edges /
  Cavity generators and mesh-map fills now use real data), shade the viewport (AO) and show in
  the 2D view ("Baked …" entries). Saved in projects; baked AO fills AO slots in exports.
* Running it in the app found a crash: 3-component maps (world normal, position, ID) were
  uploaded as RGB into RGBA16F textures (wgpu validation error). Fixed by padding to RGBA; new
  render test uploads and previews every mesh-map layout.
* Sample crate, all 7 maps at 512²: ~3 s per texture set (release, 4 cores); 1024² ~4x that.
* Screenshot: `docs/screenshots/baked-ao-2d-view.png`.

### Session 6 — assets, verified in the running app

* New **Shelf** panel under the 3D view: Materials, Smart Materials, Smart Masks, Brushes,
  Environments, each with rendered thumbnails (materials on a sphere; smart materials / masks on
  a cube with baked curvature + AO, so wear and dirt show; environments on a chrome sphere;
  brush tips as images). Thumbnails render one per frame on a separate offscreen renderer.
* Click to apply: material preset = fill layer, smart material = folder of masked layers (one
  undo step), smart mask = black mask + generators on the selected layer, brush tip = next
  strokes, environment = viewport lighting (with rotation / intensity). Import alpha / HDRI.
* Driven in the app: bake (Ctrl+B) then Worn Painted Metal on the crate body gave paint with
  dirt in the trim contact areas and worn edges; Sunset environment re-lit the scene;
  screenshots `docs/screenshots/smart-material-worn-metal.png`, `shelf-materials.png`.
* Not saved in projects yet: the chosen environment and imported alphas / HDRIs (viewport and
  tool settings, like Painter's shelf; next step is a user library folder).

### Session 7 — edge wear followed UV seams (fixed, verified in the running app)

* User report: edge wear / curvature masks looked "applied by UV, not by the object's edges".
* Cause: generator masks (Edges, Cavity, Directional, Position) break up their threshold with
  noise, and that noise was projected in **UV**. Each island got an unrelated patch of noise, so
  wear appeared on one side of an edge and stopped dead at the seam. The baked curvature itself
  was already seam-free (checked: sphere / torus show no seam lines; both faces of a box edge
  bake identical values at every distance).
* Fix: the breakup noise is now triplanar on object position (`compositor/src/mask.rs`).
  Test `generator_noise_breakup_is_continuous_across_uv_seams` (flat quad split into two far-apart,
  mirrored islands): average mask jump across the seam was 0.45 before, < 0.05 after.
* Driven in the app: bake + Worn Painted Metal on the crate, before/after in
  `docs/screenshots/edge-wear-seam-fix.png` (left: one-sided band on the lid's top face cut at
  the seam; right: broken-up wear following the edge).

### Session 8 — UI redesign, verified in the running app

* User request: sleeker, modern UI with sharp edges, more breathing room, clearly separated sections.
* New theme (`ui/src/theme.rs`): layered charcoal surfaces, square corners everywhere, 1px hairlines,
  one orange accent, larger spacing / control heights. Embedded fonts (`ui/assets/fonts`, subset):
  Inter (OFL) for text, Phosphor (MIT) for icons, usable inline in any label (`ui/src/icons.rs`).
* Shell: header bar (brand, menus with icons, centered project name, undo / redo / save, Bake and an
  accent Export button), left tool rail (brush, eraser, fill, X symmetry, paint-mask toggle, reset
  camera, shortcuts), slim status bar. Dock tabs carry icons, the active one an accent top line;
  3px gaps between panels; leaf collapse arrows removed. Default layout gives Properties more room
  (dock layout key bumped to v3, so the new default shows once).
* Shared widgets (`ui/src/widgets.rs`): collapsible sections, label/editor property rows with a fixed
  label column and full-width sliders, icon buttons, segmented controls, list rows with accent
  selection bar, badges, empty states. All panels and the Bake / Export / unsaved dialogs use them.
* Tests: `crates/ui/tests/theme.rs` checks every icon and the punctuation used are in the embedded
  fonts (and that a dropped glyph is really absent). Screenshot `docs/screenshots/ui-redesign.png`;
  also clicked through fill-layer properties with mask effects, Brush tab, 2D view, Bake and Export.

### Session 9 — ready to go public

* License: dual MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, workspace `license` field),
  replacing the proprietary marker. Bundled fonts keep their OFL / MIT licenses.
* Added `README.md`, `CONTRIBUTING.md` (setup, architecture rules, PR checklist),
  `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), issue / PR templates and CI
  (`.github/workflows/ci.yml`: Linux full test suite on lavapipe; Windows build + engine tests).
* Checked before publishing: no keys / tokens in the files or the git history.
* Making the repository public is a GitHub setting the owner flips (Settings → General →
  Danger Zone → Change visibility).

### Session 10 — Windows .exe and local development guide

* `scripts/build-windows.ps1` (Windows PowerShell 5.1 / 7): builds with the new `dist` profile
  (release without debug info, stripped: 28 MB exe instead of 192 MB) and the `windows-gui` feature
  (no console window), then packages `dist/SubCraft-<version>-windows-x64.zip` with the README and
  licenses. Tested here with PowerShell 7 (10.6 MB zip; the packaged binary starts and renders).
* The exe embeds the brand icon (`apps/subcraft/assets/icon.ico` via `winresource` in `build.rs`;
  a missing resource compiler only warns). The window / taskbar icon is drawn in code
  (`theme::app_icon`, tested).
* `.github/workflows/release.yml`: publishing a GitHub Release builds the zip on windows-latest and
  attaches it; PRs touching packaging build it as an artifact.
* `docs/development.md`: Windows setup, daily branch / PR loop with the protected `main`, building
  the exe, publishing a release, troubleshooting.
* Not verifiable from Linux: the Windows-only code paths (resource embedding, GUI subsystem) and
  the C part of the FBX importer don't cross-compile here; the Windows CI jobs build them.

### Session 11 — Windows installer, verified on a Windows 11 PC (RTX 4060)

* Whole suite on Windows: `cargo test --workspace` → 153 passed, 0 failed (GPU render tests ran on
  the RTX 4060). `scripts/build-windows.ps1` → `SubCraft-0.1.0-windows-x64.zip` (8.8 MB).
* New `installer/subcraft.iss` (Inno Setup 6) + `scripts/build-installer.ps1`: builds the exe and
  zip, then `dist/SubCraft-<version>-setup.exe` (7.9 MB). Version read from the root `Cargo.toml`
  (numeric file version derived, so `0.2.0-beta.1` works). Program Files by default, "install for
  me only" offered; Start Menu shortcut, optional desktop shortcut, uninstaller in Apps, MIT
  license page, optional `.subcraft` association, fixed AppId so upgrades replace older versions.
* Verified here (per-user silent install, `/CURRENTUSER /VERYSILENT /TASKS="desktopicon,associate"`):
  files installed, both shortcuts, Apps entry "SubCraft 0.1.0", association registered; launching
  the **Start Menu shortcut** opened the installed app and it rendered the sample crate;
  opening a `.subcraft` file through the shell association opened it in SubCraft (title showed the
  file); uninstall removed files, shortcuts, Apps entry and registry keys. First run left an empty
  `.subcraft` key — fixed with `uninsdeletekeyifempty`, re-tested clean.
* Not verified locally: the all-users (Program Files) install, which needs a UAC prompt; it uses
  the same script with `{autopf}` / HKLM. The release workflow builds the installer on
  `windows-latest` (Inno Setup 6.7.1 is preinstalled on the Windows Server 2025 image; the job
  installs it with Chocolatey if missing) and attaches the setup.exe and the zip to the release.
* README: Download button (links to the latest release) + live version badge + install steps
  incl. SmartScreen; `docs/development.md`: building the installer and publishing a release.

### Session 12 — your own image textures in fill layers (roadmap 1, part 1)

* Fill-layer channels have a new **Image** source (Properties → Channels → source dropdown):
  a file dialog picks PNG / JPG / TGA / TIF / BMP / EXR / HDR; the channel shows a thumbnail,
  file name, size and **Replace…**. The layer's projection (UV / tri-planar / planar, scale,
  rotation, offset) applies. Choosing an image is one undoable step.
* `Session::import_image` (`crates/app/src/images.rs`): the original file bytes go into the
  `ResourceStore` (UUID identity) and are embedded in the project, so it stays portable; importing
  the same file again reuses the resource; unreadable files and images over 16384 px report errors.
* Color rules unchanged and now tested end to end: Base Color decodes sRGB → linear, data channels
  (roughness etc.) use the raw values.
* Fixed while testing: projects with TGA images could not be reopened (TGA has no file signature,
  decoding from memory failed) and re-saving a loaded project renamed its images `*.bin`.
  `image::io::decode_bitmap` / `guess_extension` now fall back to the file name.
* Tests (6 new, 159 total): `crates/app/tests/images.rs` (sRGB vs raw, image orientation, save →
  delete source → reopen bit-exact for PNG / TGA / EXR, second save, dedup, undo / redo, bad files),
  `image::io` extension + in-memory decode for every format.
* Verified in the real app on Windows: a project with a brick image on the crate's Body, reopened
  after deleting the source PNG — renders with 2x tiling, Properties shows the image row
  (`docs/screenshots/image-fill-layer.png`). Not clicked through by hand: the native file dialog
  itself (same `rfd` call as the shelf's existing Import alpha).

### Session 13 — import a texture set as one material (roadmap 1, part 2)

* **Import texture set…** (Layers toolbar image icon, Layer menu, Shelf → Materials): pick a
  texture's maps in one file dialog; they become **one fill layer** named after the files (one
  undo step) on the selected texture set.
* Matching by file name (`crates/assets/src/texture_maps.rs`), scanning words from the end, with
  camelCase / `_` / `-` / space splitting: base color (basecolor, base_color, albedo, diffuse, diff,
  color, col, bc), roughness (rough), **glossiness / smoothness → inverted to roughness**, metallic
  (metalness, metal), normal (nrm, nor, normalGL, nor_gl; **DirectX `normalDX` / `nor_dx` → green
  flipped to OpenGL**; an OpenGL normal wins if both exist), height (displacement, disp, bump), AO,
  emissive, opacity. One file per channel. Skipped with a reason: non-images, unrecognized names
  (previews), packed ARM / ORM maps (not supported yet), duplicates, channels the texture set lacks
  (e.g. AO in the default template). Skips are logged; the status bar summarizes.
* Converted maps (gloss, DirectX normal) are stored converted, losslessly (PNG 8 / 16-bit, EXR for
  float), so no project-format change was needed. `Bitmap::map_color`, `image::encode_bitmap`.
* Tests (9 new, 168 total): 23 real-world names (Poly Haven, ambientCG, Unreal, camelCase), plan
  selection / skip reasons / layer naming, `map_color`, in-memory encode round trip for 8 / 16-bit /
  float, end-to-end import (gloss 0.2 → roughness 0.8, DX green 0.3 → GL 0.7, sRGB base color,
  composite, single undo), "nothing recognized" leaves the document unchanged.
* Verified in the real app on Windows, through the real file dialog (driven with simulated
  input): `docs/screenshots/import-texture-set-dialog.png`, `import-texture-set.png`.
* Found, not fixed here: in a short Layers panel, the second layer row overlaps the Blend / Opacity
  rows (pre-existing layout issue; next UI fix).

### Session 14 — images as masks (roadmap 1, part 3)

* New mask effect **Image** (Properties → Mask → **+ Image**): pick an image; the effect shows its
  thumbnail, **Replace…**, **Invert** and its own projection (UV / tri-planar / planar, scale,
  rotation, axis). Images with transparency (a logo) stamp white where opaque; images without
  use their brightness (raw values, no sRGB decode). A missing image contributes nothing.
* Project **schema 2** (`MaskEffectKindV1::Image`); schema-1 projects load unchanged (test), older
  SubCraft builds refuse schema-2 files with the "saved by a newer SubCraft" message. Mask images
  are embedded in the project like fill images.
* The fill layer's projection controls became a shared `projection_editor` (fill + image mask).
* Tests (3 new, 171 total): compositor (alpha logo, inverted, grayscale raw values, missing image),
  project (schema-1 file loads, image mask keeps its image through save / load).
* Verified in the real app on Windows through the real dialogs: Plastic Glossy Red from the shelf,
  black mask, + Image → star logo PNG, Planar projection → a red star on the crate's front
  (`docs/screenshots/image-mask-star.png`).
* Brush stencils from images are a separate item (roadmap step 3, stencils / projection painting).

### Session 15 — user asset library on disk (roadmap 2, part 1)

* Library folder `Documents/SubCraft/Library` (or `$SUBCRAFT_LIBRARY`), with `alphas/` and
  `environments/`. **Import alpha…** / **Import HDRI…** now copy the file there, so imported tips
  and HDRIs are back after a restart; re-importing the same file selects it instead of duplicating;
  a different file with the same name becomes `name (2).ext`. Files dropped into the folders by
  hand also appear.
* Startup: library alphas load immediately, HDRIs are prefiltered on a background thread and pop
  into the shelf when ready (no startup delay). Unreadable files are logged and skipped.
* Shelf: **Library folder** button (Brushes, Environments) opens the folder; right-click a library
  item → **Show in folder** / **Remove from library** (deletes the library copy only; refuses
  paths outside the library).
* `crates/assets/src/user_library.rs` (`UserLibrary`, `load_alpha`, `load_environment`),
  `user_file` on `BrushAlpha` / `Environment`.
* Tests (5 new, 176 total): import → restart → loaded with a stable id (source file deleted),
  dedup + numbered names, broken files reported, remove only inside the library, HDRI from the
  library, root resolution (override / Documents / empty value).
* Verified in the real app on Windows (temporary library via `SUBCRAFT_LIBRARY`): pre-seeded alpha
  + panorama appear at startup, Import alpha through the real dialog copies the file, right-click
  Remove deletes the panorama, and after a restart the imported alpha is still there and the
  removed environment is gone (`docs/screenshots/user-library-shelf.png`, `user-library-menu.png`).

### Session 16 — save your own smart materials and smart masks (roadmap 2, part 2)

* Right-click a layer row → **Save as smart material** (any layer or folder; the whole subtree)
  or **Save mask as smart mask** (the mask's effect stack). Files go to
  `Library/smart-materials/*.scsmart` and `Library/smart-masks/*.scmask`, named after the layer
  (`name (2)` if taken, unsafe characters replaced), appear in the shelf at once and at every
  start; the shelf name is the file name, so renaming the file renames the asset. Right-click a
  saved item → Show in folder / Remove from library. The row menu also has Duplicate / Delete.
* Format (`crates/project/src/fragment.rs`): the project's own layer / mask-effect schema (v2),
  raster blobs and embedded images in a small zip, so smart materials evolve with projects.
  The project writer/loader were factored (`mask_effect`, `pack_resources`, `unpack_resources`,
  `write_payload`) so both containers share one implementation.
* Applying: fresh ids, one undo step, the images the smart material / mask uses are added to
  the project; painted rasters are resampled when the target texture set has another resolution
  (`TiledImage::resampled`, `Layer::map_rasters`).
* Tests (8 new, 184 total): fragment round trip (folder + paint raster + mask with image +
  generator, only referenced images stored), smart mask round trip, project files rejected as
  fragments, resampling, unique / safe file names, save → load → apply identical composite in
  a fresh session, 128 → 512 resample keeps painted values, smart mask brings its image,
  broken library files skipped.
* Verified in the real app on Windows (temporary library): applied Worn Painted Metal, renamed
  it "My Blue Metal", right-click → Save as smart material (1.6 KB file), restarted, the saved
  item showed in the shelf with a thumbnail and applied to the Trim texture set
  (`docs/screenshots/save-smart-material-menu.png`, `saved-smart-material.png`). The smart-mask
  path is covered by tests; not clicked through by hand.

### Session 17 — shelf search and categories (roadmap 2, part 3)

* Every shelf tab has a **Search…** box and filter chips: **All**, **Mine** (items from the user
  library) and, on Materials, one chip per category (Metal, Plastic, Paint, Stone, Wood, Fabric).
  Every search word must appear in the name or category (case-insensitive); clicking an active
  chip turns it off; chips reset when switching tabs; an empty result says so.
* Logic in `crates/ui/src/panels/shelf_filter.rs` (pure, 3 unit tests); 187 tests total.
* The bundled icon font is a subset without a magnifier glyph — the font-coverage test caught it,
  so the box uses a "Search…" hint instead of an icon.
* Verified in the real app: "gol" → Gold only, Wood chip → Wood, Smart Materials + Mine → the
  saved "My Blue Metal" only (`docs/screenshots/shelf-search.png`).

### Session 18 — brush presets (roadmap 2, part 4)

* Brush panel → **Presets**: type a name (or leave it empty to use the tip's name) and
  **Save preset**: size, flow, opacity, spacing, hardness, angle, roundness, jitter, alignment,
  culling, lazy mouse, symmetry, the material channel values and the tip go to
  `Library/brush-presets/<name>.scbrush` (JSON). Presets show first in Shelf → Brushes with the
  tip's thumbnail; click to load, right-click → Show in folder / Remove from library. Presets are
  found by search ("preset") and the Mine chip.
* The tip is referenced by name (built-in or library alpha), not embedded; a missing tip falls
  back to Round with a status message. `BrushSettings`, `BrushMaterial`, `StrokeOptions` now use
  `#[serde(default)]`, so presets written by older versions load after new fields are added.
* Tests (3 new, 190 total): full round trip through the library, missing tip → Round + message,
  minimal / foreign / broken files.
* Verified in the real app: Splatter at 18 % saved as "Big Splatter", switched to Round 5 %,
  clicking the preset restored Splatter 18 % (`docs/screenshots/brush-presets.png`).

### Session 19 — drag and drop from the shelf (roadmap 2, part 5)

* Materials, smart materials and smart masks can be **dragged** from the shelf (a floating label
  follows the cursor). Drop targets highlight while hovered:
  * **3D view**: applies to the texture set under the cursor (dropping on the crate's posts
    switches to Trim first); a drop on the background uses the selected texture set.
  * **Layer row**: materials / smart materials land above that layer; smart masks go onto it.
  Clicks still work as before; both go through one `shelf::apply_shelf_item`.
* `UiState::new` no longer reads the user's Documents library itself; the app calls
  `with_user_library` at startup (keeps UI tests independent of the machine's files).
* Tests (3 new, 193 total): drop/click routing for materials, smart materials, smart masks and
  out-of-range items (`crates/ui/tests/shelf_drop.rs`).
* Verified in the real app with real mouse drags: Plastic Glossy Red dropped on a post →
  applied to the Trim set (`docs/screenshots/shelf-drag.png`, `shelf-drop-trim.png`); Edge Wear
  dropped on the Layer 1 row → masked that layer, one history entry (`shelf-drop-layer.png`).

### Session 20 — Layers panel overlap fix

* Bug (found in session 13): when the Layers panel is short (e.g. a 1280×720 window), layer rows
  were drawn over the Blend / Opacity controls. Cause: egui's `ScrollArea` has a 64 px minimum
  height, so the list grew past the space left above the pinned blend controls.
* Fix: the list is limited to the remaining height (`min_scrolled_height(0)`, `max_height`) and
  scrolls; below 200 px of panel height, Blend and Opacity collapse into one compact row (blend
  dropdown + opacity %), so more rows stay visible. Normal sizes are unchanged.
* Verified in the real app at 1280×720 and maximized
  (`docs/screenshots/layers-overlap-before.png`, `layers-overlap-after.png`). Layout only, no new
  unit test (covered by the screenshots).

### Session 21 — stencils (roadmap 3, part 1)

* Brush panel → **Stencil**: Choose image… / Remove, Scale, Rotation, Tile. The image lies over
  the 3D view (semi-transparent); white lets paint through, black blocks it (brightness × alpha,
  sampled in the main view's screen space, also for symmetry strokes). In the 3D view hold
  **S** + drag: left = rotate, middle = move, right = scale; hold **N** to paint without it.
* `crates/paint/src/stencil.rs` (`Stencil`), kernel factor per texel, `Stroke::with_stencil`,
  `BrushState::{stencil, stencil_bypass}`. Not saved in projects (tool setting, like Painter).
* Tests (5 new): stencil transform / sampling / corners, paint acceptance (white half paints,
  black half blocks: 0 texels), session (black blocks, N bypass paints).
* Verified in the real app: star logo stencil, two strokes → paint has the star's sharp edges
  (`docs/screenshots/stencil.png`). **Not yet verified by hand:** the hold-S drag controls (the
  automated attempt was invalidated by remembered panel state); the sliders cover the same
  transform and work.

### Session 22 — tiling X / Y and offset

* Properties → Projection: Tiling X / Y with a **Link** toggle (on when X = Y), Offset X / Y and a
  reset button. Uses the existing `Projection::scale` / `offset` fields, so no schema change.
* Screenshot runs use a temp `persistence_path`, so scripted shots no longer inherit the user's
  scroll / panel state. Test: `transform_2d` scales X and Y separately before offset and rotation.
* Verified in the real app (`docs/screenshots/projection-tiling.png`).

### Session 23 — channel strength sliders

* `FillChannel::strength` (default 1) applied after the fill is evaluated
  (`compositor/src/strength.rs`): Normal scales the tangent-space XY and renormalizes, Height is a
  signed multiply, Base Color a clamped brightness, Emissive unclamped, other scalars clamped 0..1.
* Saved as `FillChannelV1::strength` (`serde(default = 1)`), so older projects load unchanged.
* Tests: strength kernels (3), compositor normal / height, project round trip.
* Verified in the real app (`docs/screenshots/channel-strength.png`).

### Session 24 — textures in the shelf, drag onto channels

* Shelf → **Textures** (first tab, opens by default once there are textures): every image in the
  library's `textures/` folder, with thumbnails decoded on a background thread and filter chips
  per detected channel. "Import textures…" copies files there; images picked for a channel or a
  texture set are remembered too.
* Drag a texture onto a **channel row** in Properties (highlighted while hovering):
  `Session::set_fill_channel_image` imports it, converts gloss / DirectX normal only when the name
  names that same channel, and keeps the channel's strength — one undo step. Drop on the model or
  a layer (or click): `Session::add_texture_layer` makes a fill layer in the channel its name
  suggests, Base Color otherwise.
* Tests (4 new): channel drop (conversion, kept strength, paint layer refused), texture as layer,
  library categories, copy-once + reload.
* Verified in the real app with a CeramicTiles set (base color, glossiness, height, DirectX
  normal): Textures tab with Base Color / Roughness / Height / Normal chips and thumbnails; the
  fill layer built from those files renders tiles with relief (`docs/screenshots/textures-shelf.png`).
  **Not yet verified by hand:** the mouse drag itself onto a channel row (the drop handler is the
  same `dnd_release_payload` path as the existing shelf drops; the session call is tested).

### Session 25 — Direct3D 12 on Windows (startup crash fix)

* A user's v0.1.0 crashed at startup with an access violation in `igvk64.dll` (Intel's Vulkan
  driver, 31.0.101.2127). wgpu offered Vulkan + DX12 and picked Vulkan. `ui::wgpu_options` now
  restricts Windows to DX12 (the Vulkan driver is never loaded), falling back to Vulkan + GL only
  when no DX12 adapter exists; `WGPU_BACKEND` still overrides.
* Verified: release build logs "Direct3D 12 (NVIDIA GeForce RTX 4060)" and the demo renders
  identically; `WGPU_BACKEND=vulkan` skips the probe and runs. Not reproduced on Intel hardware
  (none here).

### Session 26 — part masks ("select only the blade")

* A user asked how to select one piece of a divided mesh. New per-layer `Layer::parts:
  Option<PartMask>` (enabled + set of part ids), independent of the paintable mask and multiplied
  into it in the compositor; a layer with an enabled, empty set is skipped. Parts are
  `topology::mesh_elements` (connected pieces), cached per `TextureSetGeometry`, with the part of
  every texel stored in `TileGeometry` (from the rasterized triangle).
* Saved as `LayerV1::parts` (optional; older readers ignore it, so no schema bump).
* App: `Command::SetParts`, `Session::{mesh_parts, part_at, toggle_layer_part}` (one undo step
  per click). UI: Properties → **Parts** (near the top; open when the layer uses parts) with
  *Pick parts in the 3D view*, All / None / Invert and a checkbox per part; while picking, clicks in
  the 3D view toggle the hovered part (highlighted in orange, x-ray), Esc or *Done* ends it, and a
  pill in the view says what a click will do. One-piece meshes get a hint to split the model.
* Tests (3 new): separate pieces are separate parts and clicks find them (gap = none), layer only
  on picked parts with toggle / undo / disable / empty set, save + reopen.
* Verified in the real app with a three-piece OBJ: the fill shows only on the blade and Parts
  lists 1 of 3 visible (`docs/screenshots/parts.png`). **Not yet verified by hand:** the clicks
  and hover highlight in the 3D view (no mouse driving while the user is at the PC).

### Session 27 — web version

* The app builds for `wasm32-unknown-unknown` and runs on WebGPU (`web/index.html`,
  `scripts/build-web.sh`, `.github/workflows/web.yml` → GitHub Pages).
* `core::vfs` (read / write / create_dir_all / exists): `std::fs` on desktop; in the browser an
  in-memory store for uploads plus a write sink that downloads the file. Image load/save
  (`encode_image`), OBJ (`tobj::load_obj_buf`, `.mtl` next to the OBJ), glTF (`gltf_from_vfs`
  on web: GLB or `.gltf` + uploaded `.bin`), project / fragment save + load and export go
  through it. FBX (`ufbx`, C) is desktop only.
* `core::task::spawn`: a thread on desktop, inline on the web (bake job, texture thumbnails,
  HDRI loading). `doc::History` and the viewport timer use `web-time`.
* `ui::files`: pickers by `Purpose` (open mesh / project, high poly, stencil, texture set,
  shelf textures, alpha, HDRI, fill channel / mask images), answered via `poll` each frame;
  native rfd dialogs answer at once, browser uploads later. Save on the web downloads
  `<name>.subcraft`; Export downloads every file.
* `RenderTarget::read_rgba8_async` + `PendingReadback`: shelf thumbnails no longer block (the
  browser can't wait on the GPU; desktop benefits too).
* Verified in the in-app browser (Chromium, WebGPU) on a local server: UI, 3D view, a paint
  stroke, shelf thumbnails, applying Rusty Iron (a few seconds, single thread), no console
  errors. **Not yet verified:** upload / download (needs the browser's file chooser), baking,
  other browsers, the Pages deployment.

### Session 28 — autosave + crash recovery

* `app::recovery`: every running SubCraft claims `<data dir>/recovery/<uuid>/` (data dir =
  `$SUBCRAFT_DATA` or `%LOCALAPPDATA%\SubCraft`) with `session.json` (version, project name, the
  user's project path) and `running.lock`, held with an OS file lock (`File::try_lock`) for the
  life of the process. A clean exit (`eframe::App::on_exit`) deletes the folder; a folder whose
  lock nobody holds is a crash. Works with several SubCraft windows at once (each has its own
  lock) and needs no PID checks.
* `Autosaver`: copies the last committed document (`Session::snapshot`, structurally shared, so
  instant) and writes `autosave-NNNN.subcraft` on a background thread via the normal atomic
  project writer; keeps the newest N (default 3, `rotate` by sequence number). Due when the
  project is modified **and** changed since the last copy (document or baked maps), every
  interval (default 5 min, Edit → Preferences, 0 = off) or soon after a bake / shelf apply
  (30 s minimum gap). Copies only ever go into the session folder (checked before writing).
* Start-up: crashed sessions with copies → modal "SubCraft didn't close properly. Recover your last
  session?" (project, file, last autosave) with Recover / Discard / Show folder; ones without
  copies are deleted silently, `*.saving` leftovers removed. Recover opens the newest copy, points
  the session back at the user's file (Save writes there) and keeps it marked unsaved; the copies
  move into the new session's folder until it exits cleanly. `--recover` recovers without asking,
  `--autosave-interval <s>` overrides the interval (testing).
* Status bar: "Autosaving…" / "Autosaved N min ago" (hover: folder). Preferences persist (eframe
  storage, `subcraft_prefs_v1`).
* Web: no data folder, so no autosave. Instead a "Unsaved work" window offers **Download backup**
  every interval while there are unsaved changes, and `web/index.html` asks before closing the
  tab (`window.subcraftUnsaved`, set by the app).
* Tests (8 new, 219 total): rotation by number, next path, clean exit vs crash marker, live
  sessions never reported, empty crashed sessions cleaned, adopt; end to end on a real session
  (`app/tests/recovery.rs`): 3 timed copies → 2 kept, user's file bytes + mtime unchanged, "crash"
  → recover → identical layers, Save goes to the user's file; operation-triggered copies respect
  the gap, baking counts as a change.
* Verified in the real app on Windows: opened a saved project with `--demo --autosave-interval 2`,
  autosave written after 2 s, killed the process (`Stop-Process -Force`): the project file's
  hash was unchanged; the next start showed the prompt (`docs/screenshots/recovery-prompt.png`);
  `--recover` restored Wood / Moss / Layer 1 / Saved Fill with "Recovered … Save it to keep it"
  (`recovery-restored.png`); the recovery folder was empty after that clean exit. Web build in the
  in-app browser: Edit → Preferences (1 min, persisted across reload), a stroke sets the
  close-tab flag, the reminder shows after a minute and Later dismisses it
  (`web-backup-reminder.jpg`). **Not verified by hand:** clicking Recover / Discard / Show folder
  in the desktop prompt (same code path as `--recover` / `CrashedSession::discard`, both tested),
  the web Download backup button (it is the normal web Save) and the browser's close-tab dialog.

### Session 29 — report a bug + crash logs

* `app::diagnostics`: logger (replaces the stderr-only one in `main.rs`): stderr (non-packaged
  builds), the last 300 lines in memory, and `<data dir>/logs/subcraft.log`; older runs rotate to
  `subcraft.1.log` … (5 kept), and a run over 4 MB rotates mid-run. SubCraft messages from Info up,
  other crates (wgpu, eframe) Warn and up. Times are UTC (logged in the header line).
  First lines: version + OS (`cmd /c ver` on Windows without a console window, `/etc/os-release`
  on Linux), GPU / backend / driver (`bug_report::record_gpu` from the wgpu adapter). Every history
  entry is logged as `action: <label>` (slider drags once per merged entry) and status-bar messages
  as `status: …`, so the log shows recent actions and user-visible errors.
* Panic hook (`install_panic_hook`): writes `<data dir>/crashes/crash-<UTC time>-NN.txt` with
  version, OS, GPU, message, location + thread, full backtrace and the last log lines (10 kept),
  then runs the default hook. It writes to the log file with `try_lock` so a panic inside the
  logger can't deadlock.
* Next start: unseen reports → "SubCraft crashed last time" window (message, report path) with
  Report a bug… / Show report / Close; either button marks them seen (`.seen` marker). It waits
  until the recovery prompt is answered.
* Help → **Report a bug…**: shows version / OS / GPU and the log file to attach, **Open GitHub
  issue** opens `issues/new?template=bug_report.yml&version=…&os=…&gpu=…&logs=…` (plus title and
  description for crashes); Help → Show log files. eframe's `links` feature enabled for opening
  URLs on desktop. Nothing is sent automatically.
* `.github/ISSUE_TEMPLATE/bug_report.yml` + `feature_request.yml` (issue forms; field ids
  `version`, `os`, `gpu`, `logs`, `what-happened` match the link's query) replace the Markdown
  templates. Discussions are not turned on (waiting for the owner's OK).
* `--test-crash` panics after 30 frames (testing).
* Tests (4 new, 223 total): issue link carries version / OS / GPU / log path, everything
  percent-encoded, crash title; log rotation shifts and drops the oldest; crash reports written
  with unique sortable names, shown once, pruned to 10 with their markers; UTC date formatting.
* Verified in the real app on Windows (`SUBCRAFT_DATA` temp folder): a `--demo` run logged the
  OS (Windows 10.0.26200.9457), GPU (RTX 4060, Dx12, driver 32.0.16.1062) and every demo action;
  `--test-crash` exited with code 101 and wrote a 13 KB crash report with backtrace and log tail;
  the next start showed the crash window (`docs/screenshots/crash-report-window.png`), and the
  logs had rotated to `subcraft.1.log` / `.2.log`. Web build: Help → Report a bug window
  (`report-a-bug-web.png`; the browser hides the GPU name, so it says "unknown GPU
  (BrowserWebGpu)"), and Open GitHub issue built the right link and tried to open a tab.
  **Not verified:** that GitHub prefills the form fields from the link (the `bug_report.yml`
  form only exists on GitHub after this PR is merged), the desktop Open GitHub issue / Show
  report / Show log files clicks (no mouse driving on the desktop), and macOS.

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

### Session 30 — right-click texturing and Parts

* Reviewed and independently tested diagnostics PR #33 (workspace tests, clippy, desktop test
  panic/restart and WebGPU bug-report dialog). Merged after Test (Linux), Build + engine tests
  (Windows), Build (web), and both packaging jobs succeeded.
* Reviewed CalangoTech's Studio fork, commit `bd6c064`. Adapted its unlit eyedropper approach,
  adding bilinear repeat sampling and GPU refresh regression coverage. Layer-move, UV-free bake,
  supersampling and mask-filter implementations are candidates for separate later PRs. None of
  the fork's recovery code was copied.
* Parts panel: cached catalogues for connected pieces, UV islands, named objects and material
  IDs. Import preserves OBJ object / glTF node / FBX node names with UUID object identity;
  mesh blob v2 and project schema 3 retain them; old mesh v1 / project schemas still load.
* 3D context menu: fill, paint only here, mask selected layer, four generator masks, color/material
  picking, isolate/hide; same actions on Parts rows. I/H act on the hovered piece, Esc/Show all
  restores visibility. Hover highlights the target. RMB dragging still orbits.
* Editing logic stays in app. A fill/paint part spanning material IDs creates layers in all its
  texture sets in one undo step; subsequent strokes select the matching paint layer. Geometry
  masks restrict actual stroke and polygon-fill writes. Layer menu adds paint/fill layers,
  white/black masks, generators and mask fill/paint effects alongside duplicate/delete/library save.
* Isolate/hide uses a temporary mesh with degenerate hidden triangles, preserving triangle
  numbering. Picking, painting and depth use that mesh; project saving, masks, bakes and exports
  keep original geometry. Browser testing caught lost GPU materials on visibility changes; fixed
  full re-upload, including baked AO, and covered it with GPU image comparisons.
* Verification: 239 workspace tests passed (including GPU comparisons), clippy completed with
  existing warnings; desktop release and web builds passed. Desktop screenshot from a throwaway example that
  exports/imports a seven-object OBJ, fills one object and saves/reopens a project; WebGPU context
  menu, targeted fill, isolate, H/Esc, layer generator menu, no browser errors. Screenshots:
  `screenshots/parts-workflow.png`, `parts-context-web.png`, `layer-context-web.png`.
  Desktop menu clicks were not driven (no desktop input); their app actions and browser UI were
  exercised. Browser file upload/download and large-model responsiveness remain later tasks.
  Browser policy subsequently blocked reconnecting to localhost after the environment changed;
  the final catalogue-cache/help-text adjustments were verified by the workspace/GPU tests and
  builds, but were not re-clicked in the browser after that block.

### Session 31 — welcome screen and quick start

* PR #34 merged after the owner approved it and all three required checks passed.
* Welcome hub offers New from a model, Open project, Try the sample, desktop recent files,
  Quick start and What's new. Startup display is optional, migrates older preferences to on,
  and can be changed in the hub or Edit → Preferences. Help reopens all three windows.
* Recent navigation history lives in desktop settings, keeps twelve deduplicated model/project
  locations, refreshes availability on startup/open/save, and marks missing files as removable.
  Successful opens/saves update it; failed opens retain the current document. Recent opens use
  the existing unsaved-changes guard. Browser uploads are not recorded as persistent locations.
* Six-step guide: model → bake → smart material → paint/Parts → save → export. Its buttons use
  existing actions/dialogs, and Back/Next/Finish work without editing the project. Launcher data
  and guide text live in app; widgets only dispatch actions. Added short welcome tooltips and a
  full-changelog link. Screenshot flags: `--welcome`, `--quick-start`, `--guide-step 1..6`.
* Added `docs/getting-started.md` with real app screenshots and linked it from README.
  `screenshots/welcome.png` and `quick-start-parts.png` were captured by the release executable
  from the throwaway seven-object OBJ/project fixture, using isolated data/library folders.
* Verification: 246 workspace tests passed, including GPU tests, preference migration, recent
  open/failure, unsaved guard, every guide page and synthetic egui button/navigation input.
  Clippy completed with existing warnings. Desktop release and wasm/dist + wasm-bindgen builds
  passed; both release-app screenshot runs exited successfully. No desktop input was driven.
* Outstanding: browser welcome/guide interaction could not be verified because browser policy
  rejected reconnecting to the local preview. No workaround was attempted; requested that the
  owner reopen/tag the tab. This feature remains draft until that check is possible; starter-pack
  implementation has not begun, preserving the requested feature order.
* Starter-pack inventory only: all 60 source PNGs decoded/verified (30 RGB normal maps and 30
  grayscale alphas, all 1024²). Normals are 12,311,802 bytes, alphas 1,583,193; gzip totals
  13,589,367 bytes, only 2.2% smaller. Plan on-demand web loading rather than embedding those
  images in the wasm. Baseline after this feature: release exe 26,303,488 bytes; web wasm
  16,112,859 bytes. Source assets in the owner's primary checkout remain untouched.

### Session 32 — v0.1.3 release (2026-10-15)

* Owner requested that public downloads include the latest updates. Prepared v0.1.3 from main:
  autosave/recovery, diagnostics/bug reports, and the merged Parts/eyedropper workflow. Welcome
  work remains in draft PR #35 pending browser interaction verification; starter assets are not
  included yet. Version bump and Cargo.lock go through a release PR before publishing the tag.
* Fixed Windows packaging to read the same `CARGO_TARGET_DIR` that Cargo builds into. The old
  script could copy a stale `target/dist/subcraft.exe` after building somewhere else. Cleanup
  paths are checked inside dist before replacement. Added black-box PowerShell tests for
  default/relative/absolute/missing custom targets, staged/zip payloads and font licenses.
* Version changes and packaging-test changes trigger Windows/Linux package CI; the Windows
  job runs packaging-path regressions before compiling the executable and Inno installer.
  README and CHANGELOG describe v0.1.3; development notes document the target-dir tests.
* Verification: 239 workspace tests passed (including GPU tests), clippy completed with existing
  warnings, all four packaging cases passed. Built actual Windows GUI/dist executable, zip
  and installer with the E: target directory; web/dist + wasm-bindgen also passed. Installer
  is 9,268,331 bytes (8.84 MiB), zip 10,554,397 bytes (10.07 MiB), executable 26,236,416 bytes.
* Ran silent per-user install of a 0.1.2 payload, upgrade to the packaged 0.1.3 payload, launch
  the installed executable on the throwaway seven-object project and capture a screenshot,
  then silent uninstall. Verified executable hashes, embedded version and removal of the
  temporary uninstall entry. Only the test installer AppId/name were changed for isolation;
  app payload and installation script behavior match the shipping package. User's installed
  v0.1.2 binary hash and registration stayed unchanged. Data/library were isolated on E:;
  no desktop mouse/keyboard input was used. Evidence lives in the E: build cache under
  `subcraft-release-013-smoke` (installed-parts.png, install/upgrade/uninstall logs).
* Not tested: manual installer wizard, admin/all-users installation, upgrade of the owner's
  existing installation, or browser interaction with this version bump. The browser policy
  block still applies; the merged Parts workflow had prior WebGPU verification in session 30.
* PR #36 passed all five checks and merged after owner approval. Published v0.1.3 from merge
  commit `d4802d8`, with the three checked CI packages attached before publication. Confirmed
  anonymous downloads and that releases/latest redirects to v0.1.3; web build and Pages deploy
  passed. Public installer is 9,311,778 bytes (8.88 MiB), Windows zip 10,571,257 bytes, Linux
  archive 12,073,788 bytes. Ran the CI-packaged executable on the fixture too; the model region
  matched the local installed-build screenshot exactly. Shipping code excludes draft #35.
* Merged release main into the welcome branch, retaining the welcome entry under Unreleased
  and both session records. Browser interaction remains the outstanding check for PR #35.
  Re-ran all 246 tests, clippy, native release and web builds on the 0.1.3 base; native welcome
  and guide screenshot runs passed and the documentation images were refreshed.

### Session 33 — in-app desktop updates and v0.1.4 preparation (2026-10-15)

* Published v0.1.3 after PR #36's checks passed and owner approval. Installer, Windows zip and
  Linux archive are attached to the public latest release; release/package and Pages workflows
  passed. Anonymous download checks passed. The owner's existing v0.1.2 installation is intact.
* Owner prioritised a green update button before continuing the roadmap. The top bar now checks
  the latest stable GitHub release, downloads the package for the current install type, verifies
  size/SHA-256 and restarts after installation. Help also offers a check-only action. Native HTTP,
  archive and OS dependencies stay out of wasm; all project/checkpoint logic stays in app.
* Windows installed copies preserve per-user/all-users mode; all-users uses Windows elevation.
  Windows/Linux x64 portable copies replace only the executable with rollback. A copied helper
  validates the job and official release before allowing the parent to close, then rechecks the
  payload. Project checkpoints preserve unsaved document changes, original save path and UUID
  selection. Bake completion is awaited; undo history and camera state reset on restart.
  Large completed payloads are removed on restart; checkpoints are retained in the data folder.
* Tested the complete native UI/check/download/helper/install/restart path with isolated real
  v0.1.3 and v0.1.4 executables, a local release fixture and the explicit test-only update-fixture
  feature. Both portable and silent per-user installer paths passed, including install paths
  with spaces. Installer tests changed only AppId/name to protect the production registration.
  Before/after 3D screenshot crops are pixel-identical (RGB mean absolute difference 0).
  Original project hashes and unsaved flags were preserved; uninstall and original installation
  hash/registration checks passed. No desktop input was driven.
  A further portable cycle restored 12 allocated paint tiles from unsaved demo strokes, while
  the original project remained blank; its model screenshot also matched exactly after restart.
* Installer testing caught Inno rejecting Windows canonical device-path prefixes; fixed and
  added a regression. The failed attempt restarted the old app and restored its checkpoint.
  Production transport separately checked the actual public v0.1.3 metadata and downloaded
  its 9,307,434-byte installer, verifying the official SHA-256 digest over HTTPS.
* Local verification: 248 workspace tests passed including GPU comparisons, clippy completed
  with existing warnings and no updater warnings, shipping Windows GUI/zip/installer and wasm
  builds passed. The v0.1.4 installer is 10,091,781 bytes (9.62 MiB; +0.75 MiB), Windows zip
  11,669,622 bytes, executable 28,641,280 bytes. The wasm output is 16,102,302 bytes versus
  16,102,337 for v0.1.3 (-35 bytes);
  no native updater dependencies appear in the wasm dependency tree. Screenshot:
  `screenshots/update-restart.png` (real per-user update fixture). README, changelog, development
  notes and ADR 0007 describe the update contract and one-time bootstrap download.
* Not tested: all-users/UAC interaction, a Linux GUI restart cycle, or browser interaction.
  Browser reconnection remains policy-blocked. Automatic approval review also blocked an extra
  launch of the shipping GUI for a check-only screenshot with the reason "blocked by policy";
  the successful real fixture-app restart screenshots and production HTTPS probe were retained.
  v0.1.4 is prepared on the updater PR; publication requires owner merge/release approval.

### Session 34 — welcome integration after the updater release (2026-10-15)

* Owner approved PR #37 merge/publication. All three required checks and both packaging checks
  passed; merged at `49b1bdc` and published v0.1.4 with all three checked CI packages attached.
  Public latest resolves to v0.1.4 and anonymous downloads passed size/signature range checks.
  The release workflow and main's Linux/Windows/web checks passed; Pages deployment passed.
* Resumed draft PR #35 and merged v0.1.4 into its branch, retaining the updater, welcome hub,
  recents and guide. Update restarts now bypass the welcome hub even for an unmodified restored
  project; added a startup regression. What's new also describes the desktop update button.
* Final verification: 258 workspace tests passed including GPU checks; clippy completed with
  existing warnings. Desktop and web dist builds passed (16,110,595-byte wasm, +8,293 bytes
  versus v0.1.4). Both shipping GUI screenshot runs opened the generated multi-part project
  with isolated data/library and exited successfully, without desktop input or update checks.
* Browser verification on port 8773: walked all six guide steps, opened bake/export settings,
  downloaded a 9,429-byte project and reopened it through the welcome hub, imported the
  generated named OBJ, tried the sample, opened What's new, and checked the startup preference
  survives a reload while Help can still reopen the hub. Existing eframe viewport warnings
  appeared; no runtime errors were observed. Actual FBX/GLB bake-to-export runs remain next.
* Browser testing found overlapping project titles and a clipped welcome hub in narrow panes.
  Titles now stay inside the gap between controls; the hub fits and wraps actions. Added real
  egui rendering regressions and verified the final rebuilt browser UI. Updated screenshot
  documentation against v0.1.4. The owner authorized merging #35 once required checks pass.
* Starter asset inventory for the next feature: 30 normal PNGs and 30 alpha PNGs, all 1024²,
  under the original repository's output/textures and output/alphas. Total PNG sizes are
  12,311,802 bytes for normals and 1,583,193 for alphas. These are not in v0.1.4 yet.

### Session 35 — layer tree editing (2026-10-15)

* Welcome PR #35 merged as c152e97 after its required checks passed. The owner then moved
  fundamental editing ahead of the starter pack: layer dragging/folders first, then an
  editable pen/Bézier path tool in its own PR.
* Reviewed CalangoTech's Studio fork, including bd6c064's layer tree. Adapted its folder drop
  zones and inline rename, retaining SubCraft's Parts/mask/generator menus. Rows/names drag
  directly; app-level destination rules handle bottom-to-top indices, nested moves, cycles,
  stale UUIDs/texture sets and no-op drops. Separate top/bottom root zones let layers leave
  folders. A move selects the layer and opens its destination folder in one history entry.
* Added folder disclosure buttons, correct open/closed icons, inline double-click/F2 rename,
  Enter/focus-loss commit, Esc cancellation and blank-name rejection. The context menu also
  offers Group, Move into folder and Move to top level. Existing schema 3 already stores folder
  expansion and the tree; no schema bump is needed. Shelf payloads keep their existing route.
* Validation: 270 workspace tests and clippy passed (existing warnings); 12 new app/UI regressions
  exercise real egui pointer/key events, long-list edge scrolling, nested/cyclic/stale
  destinations, numeric composite colors, undo/redo, folder opening and save/load.
* Desktop and web dist builds passed. Shipping GUI launch opened the generated seven-object,
  1,008-triangle model/project with isolated data/library, saved `docs/screenshots/layer-tree.png`
  and exited successfully on D3D12. No desktop input was sent. Browser checks on port 8773
  verified dragging names above/below folders, collapsing, dropping into a collapsed folder,
  leaving it via the root target, undo/redo, double-click rename and Enter. The edited 9,888-byte
  project downloaded and reopened with its order, hierarchy and renamed layer intact. Existing
  eframe viewport warnings appeared; no runtime errors were observed.
* Web wasm: 16,161,410 bytes, +50,815 (+0.32%) versus the welcome build. Native executable:
  28,775,424 bytes. README includes the real desktop screenshot and fork credit. Linux/native
  mouse gestures were not run locally; Linux checks run in CI, and direct egui input plus browser
  gestures cover the shared interaction code. The PR still needs the owner's merge approval;
  these edits are not in public desktop v0.1.4 yet.

### Session 36 — editable pen/Bézier paths (2026-10-15)

* The owner authorized merging #38 once its three required checks passed. Linux, Windows and
  web all passed; #38 merged as fbe780f. Development continued on `codex/bezier-pen` from main.
* Added UUID pen anchors, relative cubic handles and style as optional paint-layer source data,
  with explicit project schema 4 and legacy defaulting for schemas 1–3. A derived tiled cache
  feeds existing compositing/export; duplication regenerates anchor UUIDs and shares rasters.
  ADR-0008 records the data, job and UV conventions. Ordinary brushing cannot overwrite the
  derived path cache; pen layers keep masks, Parts, blending and tree operations.
* App controller owns drafts, mirror/broken-handle edits, Corner/Smooth/Delete, queued gestures,
  per-gesture undo and stale-job checks. Bounded raster work runs across frames, with capped
  anchors/subdivision/tile references/queue length and unchanged-tile sharing. Surface picking
  uses canonical UV tile wrapping. Paths support PBR channels and subpixel edge coverage.
* Added Pen (P), 2D and 3D pointer controls/overlays, parameter controls, closed paths, New path,
  Enter/Esc/Delete and Shift handles. Save/Open/Export, close and update restart wait for pending
  path work; browser close protection includes it. UV width/seams/overlaps and 2D editing outside
  islands are documented. Brush alphas/symmetry remain brush features.
* Verification: 296 workspace tests passed (26 new tests), including GPU checks, numeric
  raster coverage, budget equivalence, duplicate identity, parameter validation, project/schema
  round trips, queued undo, preservation of concurrent names/masks/folders, direct egui 2D/3D
  input, pan/zoom, separate handles and pending Save/Open/update handling. A throwaway example
  opened the generated seven-part project, placed/shaped a curve and exported ten maps. Clippy
  passed with existing warnings. Shipping GUI screenshot runs in both 3D and 2D exited 0 on
  D3D12 with isolated data/library; README includes the actual 3D screenshot.
* Final browser build on port 8773 verified automatic Pen selection on load, creating a second
  path directly on the 3D model, moving a handle in 2D, undo/redo, closing the curve, downloading
  a 61,780-byte project and reopening it with the curve/closed state and editable handles intact.
  BaseColor exported as a 512px PNG and was visually checked. The browser saved only that first
  map although the existing exporter reported ten files; record this multi-download problem for
  the web workflow follow-up rather than claiming all ten browser maps passed. Only existing
  eframe Title/SetTheme warnings appeared; no browser runtime errors were observed.
* Final wasm: 16,332,539 bytes, +171,129 (+1.06%) over the layer-tree build. Native executable:
  28,997,120 bytes. Native pointer/tablet interaction and Linux execution were not run locally;
  shared egui input tests/browser gestures cover editing, and Linux runs in CI. No installer
  release was made; the public desktop remains v0.1.4. The pen PR needs owner merge approval.

### Session 37 — CC0 starter images and shelf preview (2026-10-15)

* Owner approved #39 after all required checks. Linux, Windows and web passed; Pen merged as
  4c711f7. Owner requested adding the starter images before the next desktop release.
* Added all 30 normal maps and 30 alphas from the owner's output folder, losslessly optimized
  from 13,894,995 to 12,928,272 bytes. Thumbnail PNGs total 340,359 bytes. CC0 credits and a
  manifest accompany the full-size assets; desktop embeds the full images for portable/updater
  compatibility, while the wasm excludes them and fetches individual PNGs from static hosting.
* Added shelf categories/tileable filter, normal drag/drop and an image/alpha preview with
  repeated tiles, lit sphere/sample stroke, strength, green flip, invert, brightness/contrast,
  scale, rotation, addressing modes and edge blending. Assets owns image math; app owns captured
  destinations, resource UUIDs, portability and undo. Preview/apply pixel jobs have frame budgets.
  Save/Open/Export/close and update restart wait for pending image application.
* Added tests for every catalog image, normal/vector math, alpha coverage, address modes,
  seam equality, job-budget equivalence, lit/stroke previews, undo/redo, save/load, stale targets,
  real egui controls, apply/cancel, filter metadata and pending Save/update guards. Windows
  packaging regression includes starter credits. Final workspace run: 314 passed (18 new);
  clippy passed with existing warnings. The throwaway example applied a 1024px adjusted normal
  and alpha stroke to the seven-part model, exported ten maps and reopened its portable project.
  Latest Windows GUI screenshot run exited 0 with isolated data/library and no desktop input.
* Browser verification found a missing final repaint after asynchronous application: the saved
  data was correct but the viewport stayed stale until another input. Fixed the final repaint,
  added application/loading progress and visible errors, and added a regression for the frame
  that displays a completed job. Final browser normal-row replacement now updates immediately.
  Tiling, normal strength/green flip, alpha contrast/rotation/invert, wrap menus, lit sphere and
  sample stroke were exercised. Adjusted alpha painting and a final project download/reopen
  passed; the stroke and embedded normal maps persisted, with no browser runtime errors.
* Final feature wasm is 16,781,729 bytes, +449,190 (+2.75%) over Pen. Its initial shelf requests
  no full pack PNGs; selecting a normal fetches one image. Full source pixels match all 60 owner
  PNGs exactly. The first optimized native build was 42,384,384 bytes. Shipping installer/zip
  sizes and final release smoke tests will be recorded with the version/release PR. Native
  pointer/tablet gestures and Linux execution were not run locally; shared egui/browser
  interaction tests and CI cover those routes. No release has been published yet.

### Session 38 — complete browser export downloads (2026-10-15)

* Starter pack #40 merged as 4a16359 after Linux/Windows/web checks and both packaging jobs
  passed. The owner requested including the starter pack before publishing v0.1.5. Its web
  assets/CC0 credits are deployed; desktop release preparation follows this export fix.
* Browser export previously sent every map separately through the virtual-file download sink;
  only the first map reached disk in verification. Added a single in-memory ZIP, rendered through
  the same engine callback as folder export. PNG/JPEG/EXR payloads are stored without redundant
  compression. Archives reject unsafe/duplicate names, and the app owns export orchestration.
  Browser export makes no per-map virtual-file writes. Project Save keeps its existing download.
* Validation: 318 workspace tests passed (four new), clippy passed with existing warnings,
  native/web builds passed and native GUI screenshot launch exited 0 with the generated project.
  Tests compare all ten archived files byte-for-byte with folder export, exercise packed/16-bit
  output, reject name collisions/unsafe entries and compare app archive contents with its plan.
* Actual browser export downloaded a 277,217-byte ZIP with all ten 512px PNGs. CRC/decode checks
  passed and every decoded pixel matched the native output exactly. Project Save also downloaded
  successfully. No browser runtime errors appeared. Final wasm: 16,881,451 bytes (+99,722 / 0.59%
  over starter previews). Screenshot is in the guide. Large-operation frame splitting and real
  multi-part FBX/GLB workflow polish remain separate roadmap work; no Linux GUI/tablet input was
  run locally. The release has not been published yet.

### Session 39 — v0.1.5 release preparation (2026-10-15)

* Browser export #41 merged as dd722bf after all three required checks passed. The owner asked
  for the starter pack first, then the release; preparation continues on `codex/release-0-1-5`.
* Bumped workspace packages to 0.1.5, grouped the changelog, updated README/guide availability,
  and made What's new show the build version, starter images, Pen, layer editing, welcome and
  browser ZIP export. Added a real egui What's new button/heading/content regression.
* Candidate builds, full checks and isolated installer/upgrade/launch verification are in
  progress. Release assets will be attached to a draft before publishing so latest downloads
  never point at an empty release. The production installation and user data stay separate
  from the temporary test installation.

### Session 40 — v0.1.5 published with the starter pack (2026-10-15)

* Release PR #42 merged as fbe6b7e after the three required checks and both packaging jobs passed.
  Its tree matches tested candidate c4db1a3 exactly. Published tag/release v0.1.5 with the Windows
  installer, Windows portable ZIP and Linux archive attached before publication. Asset sizes and
  SHA-256 digests were checked against the downloaded CI candidates. The stable latest endpoint
  now exposes v0.1.5 for the existing v0.1.4 update button.
* All 319 local workspace tests passed; clippy passed with existing warnings. Ran the exact CI
  Windows payload after an isolated per-user v0.1.4 install and upgrade to v0.1.5. Payload hash,
  embedded version, bundled CC0 credits, native screenshot launch and silent uninstall passed.
  The production executable and installation registration were unchanged. The CI web candidate
  displayed the current What's new and reopened the schema-4 starter project without console
  errors. Earlier browser preview/painting/save/reopen/export results remain in sessions 37–38.
* Final candidate sizes: installer 23,222,859 B, Windows ZIP 24,867,813 B, Linux archive
  26,388,176 B and web wasm 16,740,045 B. Full starter PNGs total 12,928,272 B, with 340,359 B of
  thumbnails. Desktop embeds the full pack; web fetches selected full images separately. README
  includes the image overview, preview screenshot and download-size table.
* Verified the public site after deployment: What's new reports 0.1.5, a selected 1024px normal
  loaded with its lit preview, and no console errors appeared. The public wasm SHA-256 matches
  the tested CI candidate. One fresh compressed HTTPS module transfer was 5,600,744 B in 2.999 s;
  this measures network transfer, with module compilation/GPU startup taking additional time.
* Linux GUI, physical tablet input and the manual all-users installer wizard were not run
  locally. Automatic approval review blocked Cargo incremental-cache cleanup without a specific
  reason; the cache was left untouched and CI packages were used for release smoke tests.

### Session 41 — graphics API preferences (2026-10-15)

* The owner confirmed that the green button successfully updated the app to v0.1.5. Requested
  desktop API selection, an ArtCraft independence notice, removal of object hover tint and
  painting when only the edge of the brush overlaps the model. These are separate branches/PRs.
* Added Edit → Preferences → Graphics: Automatic, DirectX 12 on Windows and OpenGL on
  Windows/Linux, with the actual running API shown. A version-1 app-data settings file is read
  before GPU creation and saved atomically on selection. Changes apply next launch; UI asks
  users to save/restart. Existing `WGPU_BACKEND` overrides remain effective and are explained.
  Returned explicit-API startup errors retry Automatic once and open Preferences. Corrupt/future
  settings fall back safely. Browser Preferences reports its API without desktop-only choices.
* The first real OpenGL launch caught unsupported alternate texture view formats. Added a
  display-copy fallback for both viewport and channel previews that preserves sRGB-encoded
  egui input, leaving the normal fast path on other APIs. Numeric GPU tests roundtrip all 256
  encoded levels and check orientation within one byte. Kept linear shading, MSAA and readback.
* Validation: 325 workspace tests passed, clippy passed with existing warnings, native/web
  builds passed. Actual DirectX 12 and OpenGL GUI screenshot launches exited 0 with the generated
  seven-part schema-4 starter project; OpenGL 2D preview also launched. Re-ran the six renderer
  image tests and the new display test on actual OpenGL; all passed. Browser Preferences reports
  WebGPU and renders the workspace without runtime errors. Screenshot is linked from README.
  Linux/macOS GUI and physically missing/failed drivers were not exercised locally.
* The first local link ran out of E: disk space. NTFS compression preserved the entire inactive
  incremental cache while reducing 26,148,288,818 B to 15,507,901,087 B. No files were deleted.
  Subsequent builds keep the requested E: target and disable incremental compilation.

### Session 42 — ArtCraft independence notice (2026-10-15)

* Graphics PR #44 passed Linux tests, Windows build/engine tests, web build and both package
  jobs. It awaits the owner's merge approval. `codex/artcraft-independence` builds on that
  tested branch so combined behavior is covered while retaining one feature per PR.
* Added the explicit statement that SubCraft is independent and is not officially developed
  by, affiliated with, or endorsed by ArtCraft. It is prominent near the README introduction,
  repeated on the welcome screen, and available in Help → About SubCraft with the version,
  license and repository link. The app uses one shared wording for both windows.
* Validation: 326 workspace tests passed. The new UI regression checks the complete notice
  in welcome/About at the minimum desktop size and in a narrow window, with project content
  unchanged. Native welcome screenshot launch exited 0 with isolated data/library and the
  generated project. Web build passed; actual browser welcome and Help → About displayed the
  full statement with no runtime errors. No new graphics/paint behavior is introduced here.

### Session 43 — remove object hover tint (2026-10-15)

* Removed the orange triangle overlay entirely, including explicit part picking and hovering
  the Parts list. Removed its shared hover state and unnecessary ordinary-hover surface query.
  Parts list names retain their tooltips/context menus; right-click targeting, layer part-mask
  picking and I/H/Esc actions remain available. README no longer promises hover highlighting.
* Validation: 328 workspace tests passed. New real egui pointer/key tests verify no geometry
  tint in ordinary hover or part-picking mode, no hover document edits, functioning H/Esc and
  working part-mask clicks. Native generated-project screenshot launch exited 0 with isolated
  data/library. Web build passed; actual browser part context menu still targets the model.
  Model RGB was exactly identical before/after hovering: 229,710 channels compared, zero
  differences, with no browser runtime errors. This branch builds on #45 for combined testing.

## Next steps (in order)

Current priorities requested by the owner:
* Current follow-up: brush overlap at model edges. Graphics selection, the independence notice
  and hover-tint removal are implemented in sessions 41–43 and await feature PR merges.
1. ~~Right-click texturing and Parts~~ — session 30; PR #34 merged.
2. ~~Welcome screen, recent files, quick-start guide and screenshot documentation~~ — sessions
   31/34; PR #35 merged. Included in published desktop v0.1.5.
3. ~~Layer dragging, folder controls and inline rename~~ — session 35; #38 merged after all
   required checks passed and the owner's approval.
4. ~~Editable pen/Bézier paths~~ — session 36; #39 merged after required checks and owner approval.
5. ~~Starter pack and image previews~~ — session 37; #40 merged. Browser ZIP #41 merged too.
   Both are included in published v0.1.5 with the welcome screen, layer editing and Pen tool.
6. Real multi-part FBX/GLB bake → smart material → paint → export.
7. Browser import/save/export verification and work split across frames. Multi-download export
   is fixed in session 38; remaining work includes large operations and real GLB/OBJ import checks.
8. Projection painting, clone, smudge/blur, followed by the remaining engine/library follow-ups.

Earlier roadmap details:

1. **Your own image textures** (users ask for this: "add our own textures", SBSAR materials).
   - ~~Fill layer channels: an "Image" source with a file picker~~ — done (session 12).
   - ~~"Import texture set as material"~~ — done (session 13). Follow-up: packed ARM / ORM maps
     (needs a per-channel component selector on image sources).
   - ~~Images as masks~~ — done (session 14). Brush stencils: see step 3.
   - Imported images are stored in the project (`ResourceStore`, UUID identity, versioned schema;
     bump the schema if needed) so projects stay portable; optionally added to the shelf.
   - Color rules: base color is sRGB → linear on import; data maps (roughness, metallic, normal,
     height, AO) are never color-transformed.
   - SBSAR can't be read directly (Adobe's closed Substance engine); the documented route is to
     export maps with the free Adobe Substance 3D Player and import them as a texture set.
2. User asset library on disk: ~~persist imported alphas + HDRIs~~ (session 15); ~~save custom smart
   materials / masks~~ (session 16); ~~shelf search and categories~~ (session 17);
   ~~brush presets~~ (session 18), ~~drag-and-drop from shelf~~ (session 19).
3. Stencils / projection painting with an image; clone tool; smudge / blur filter layers.
4. Bake follow-ups: per-material high-poly matching by name, cage mesh support, anti-aliased
   (supersampled) bakes, GPU baking for large meshes.
5. ~~Autosave~~ (session 28) + recent files; export presets saved per project; custom export templates editor.
6. UI follow-ups: drag-and-drop layer reordering, inline rename, eyedropper, painting in the
   2D view, texture-set resolution change, camera FOV / ortho toggle, texture mipmaps.
7. Engine follow-ups: incremental dirty tracking inside mask paint effects; near-plane
   clipping in the depth rasterizer; f16 tile geometry; renormalize normals after opacity
   lerp; GPU compositor behind the same interface.

All workspace crates now have real implementations.
