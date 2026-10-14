# SubCraft — implementation progress

Last updated: 2026-10-14 (session 16: saved smart materials / masks)

## Decisions

* Stack: Rust 2024 + wgpu 30 + egui 0.36/eframe + egui_dock 0.21 — see `adr/0001-technology-stack.md`.
* Texture engine: CPU-authoritative copy-on-write tiled images (64² tiles, f16), per-tile
  compositor with dirty-tile tracking, projection brush — `adr/0003-texture-engine.md`.
* Conventions: right-handed, +Y up, CCW front faces, UV origin top-left (glTF), MikkTSpace
  tangents → OpenGL (+Y) normal maps, wgpu camera conventions — `adr/0005-coordinate-conventions.md`.
* Undo: history of structurally shared document snapshots + merge keys; strokes push the
  post-stroke document — `adr/0006-undo.md`.

## Done (all tests passing — 153 tests, `cargo test --workspace`)

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
   materials / masks~~ (session 16); next: brush presets, shelf search and categories,
   drag-and-drop from shelf.
3. Stencils / projection painting with an image; clone tool; smudge / blur filter layers.
4. Bake follow-ups: per-material high-poly matching by name, cage mesh support, anti-aliased
   (supersampled) bakes, GPU baking for large meshes.
5. Autosave + recent files; export presets saved per project; custom export templates editor.
6. UI follow-ups: drag-and-drop layer reordering, inline rename, eyedropper, painting in the
   2D view, texture-set resolution change, camera FOV / ortho toggle, texture mipmaps.
7. Engine follow-ups: incremental dirty tracking inside mask paint effects; near-plane
   clipping in the depth rasterizer; f16 tile geometry; renormalize normals after opacity
   lerp; GPU compositor behind the same interface.

All workspace crates now have real implementations.
