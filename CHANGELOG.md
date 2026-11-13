# Changelog

What changed in SubCraft, newest first. Download the latest build from the
[Releases page](https://github.com/Aalzard/subcraft/releases/latest).

## Unreleased

## 0.3.7 - 2026-11-13

### Added
- **Paint or fill a part, a UV island or the whole object**, like Painter: Polygon Fill on fill layers (triangle, polygon, mesh, UV island, box selection), the geometry mask and Alt+H to hide excluded geometry (#200).
- **Paint directly in the 2D view**: every paint tool works there like in 3D, including smudge, clone, picker, stencil, symmetry and the mouse gestures (#199).
- **Transparent textures** (issue #145): an Opacity channel, transparent texture sets, alpha in the viewport, and RGBA Base Color plus an Opacity map on export (#198).
- **Right-click Properties popup** in the viewport, curvature modes and detail sizes, and box selection for polygon fill (#131).
- **Baked map import**: bring in your own baked mesh maps, curvature from the normal map, a ray-miss heatmap and an ID colour editor (#137). ID map colours can come from vertex colour, material colour, mesh ID or submesh (#230, issue #117).
- **Painter .spp projects in File > Open**, as a reconstruction: the layers are rebuilt, the original file is not changed (#191).
- **Editable shortcut editor** (Settings > Shortcuts) (#209).
- **Shader Settings panel** with shader instances (#207), and Rendering mode settings: min samples, time limit, firefly switch, resolution override, save render (#193).
- **Cameras**: import cameras from the mesh file, a camera menu with the default camera, free camera rotation, and a highlight on the texture set under the cursor (#217, #258). 3D view overlays: axis indicator, straight-line preview, lazy mouse ring (#261). Symmetry cursors (#262).
- **Texture sets**: channel rows and sixteen user channels (#197).
- **Layers**: mask actions and the mask view (#220), multi-channel filters and Passthrough layers (#208), flatten and layer export (#219), Ctrl-drop with a mask, smart masks as layers, pasted layers renamed (#229), layer row and blending polish (#177).
- **Generators and filters**: generators on layer content and a Random seed button (#204), custom grunge images and Gradient Dynamic (#205), Compare Mask, Levels with histogram, Color Selection (#201), 3D Distance, 3D Linear Gradient, UV Random Color, Grayscale Conversion (#203), Height Adjust, FXAA, Drop Shadow, Glow, Bevel and Mask Outline options (#202).
- **Fill projections**: spherical and cylindrical, and symmetry on fills (#228). Symmetry and Lazy mouse buttons and Spacing on the brush bar (#227).
- **More export file types and bit depths**, like Painter: BMP, Radiance HDR, lossless WebP and 16-bit
  (half) float EXR. File type and bit depth are now two menus; the bit depth menu only lists what the
  file type stores and offers **8-bit + dithering**, which turns banding in smooth gradients into fine
  noise.
- **Export padding modes** Dilation + background color (the gutter beyond the dilation distance gets
  each map's default: flat normal, white occlusion...) and Dilation + diffusion (a smooth blend of the
  surrounding islands). The dilation distance defaults to 16 and goes up to 256.
- **Size menu** in Export Textures: write every map at 128 to 8192, or at each texture set's size.
- **Converted maps** in the preset editor, like Painter's: Diffuse, Specular, Reflection, Glossiness,
  Glossiness squared, Mixed AO, f0 and 1/ior, computed from the metal / roughness channels. Painter
  presets that use them import without losing those maps.
- **Export window tabs**, like Painter: Settings, Output templates (the preset list and editor) and List of
  exports (files written per texture set, and the messages; Export switches to it). Settings lists Global
  settings and each texture set: a texture set can have its own file type, bit depth, size and padding.
  The bottom row has Reset all, Cancel (closes without keeping the changes), Save settings (stores the
  export settings in the project) and Export.
- **About 25 more built-in export templates** with Painter's names: Blender, Arnold, Corona, CryEngine 3,
  Amazon Lumberyard, Keyshot, Lens Studio, Maxwell, Redshift, Renderman, Shade 3D, Spark AR, V-Ray, Unity
  HDRP / URP Specular, PBR Specular Glossiness (converted), Roblox MaterialVariant, glTF + displacement,
  Mesh Maps and Document channels + Normal + AO.

### Changed
- **Faster painting on big projects**: brush stamping runs on its own pool (4-5x faster on a 4K project with a 300 px brush, stalls gone) (#256); catching up after a stroke is about 2x faster (#259).

### Fixed
- Menu bar readable with a light system theme (issue #206, #221).
- Export: the alpha of a colour channel is no longer sRGB-encoded (#238); packed HDR maps keep their format (#239); 16-bit float TIFF and half-float EXR fixes (#240); gray+alpha in JPEG and TIFF (#231); a preset with problems writes nothing (#253); OBJ/MTL material slots with the same name stay separate (#226).
- Atomic file writes with retries on Windows (#233). Deeply nested USD files are refused instead of crashing (#224).
- Painting under a Passthrough filter refreshes every tile it reaches (#243); Color Correct keeps values above 1 (#250); duplicated mask effects and anchor references fixed (#236, #246, #252); Mask Outline follows the resolution scale (#254).
- Device loss during start-up stops drawing instead of crashing (#195); hidden texture sets no longer take picks; Shift+Insert pastes layers (#196); PhotoCraft no longer writes into text and path layers (#225); shortcut and session fixes (#242, #251, #255, #241, #237, #244).

## 0.3.6 - 2026-11-12

### Added
- **Orbit around the point under the cursor** (issue #115). Alt+LMB / RMB drag orbits around the point of
  the model under the cursor (Preferences: "Orbit around the point under the cursor", on by default; `F`
  forgets the point). Also Painter's mouse gestures: Alt+Shift+LMB orbits in 90 degree steps,
  Ctrl+Shift+click draws a straight line from the last stroke point snapped to 5 degrees, Ctrl+MMB sets the
  depth of field focus, Ctrl+Alt+RMB selects the texture set under the cursor, V + click sets the clone
  source.
- **Painter's menu bar**: File, Edit, Mode, Window, Viewport, Plugins and Help in Painter's order, with view
  layouts (3D | 2D, 3D only, 2D only, Swap), Painting / Bake / Rendering modes (F9 / F8 / F10) and Painter's
  display keys.

### Changed
- **Faster edits low in large layer stacks**: the per-tile field cache keeps every procedural field of a big
  stack (an edit at the bottom of a 40-layer 4K stack: about 15 s to 7 s).

### Fixed
- **Graphics driver resets on integrated GPUs at start-up** (issue #119). GPU compositing is split into short,
  timed submissions so one never runs long enough to trip the Windows watchdog. Integrated GPUs get a smaller
  memory budget (no 4x MSAA, a 128 MB compositing tile pool, environment maps up to 1024 px); set
  `SUBCRAFT_FULL_GPU_MEMORY=1` to turn this off. A lost graphics device no longer crashes the app.
- **Clone** with no source set no longer leaves an empty layer behind; Clone and Smudge in the 2D view no
  longer paint plain strokes; Delete, Ctrl+Z and other shortcuts no longer act behind modal windows.
- **Transform filter** on a flat layer is computed per tile; tri-planar weights no longer pop at high Hardness.
- **Brush block** no longer rewrites stored values (jitter, spacing) just by being shown; a stencil tiled on
  one axis no longer bleeds the opposite edge; a tiny stencil scale in a preset no longer hangs; renaming a
  preset no longer adds a number.

## 0.3.5 - 2026-11-12

### Added
- **Export preset editor.** Export Textures has a preset menu with New, Duplicate, Delete, Import and
  Export, and a Preset editor: per output map a file name pattern (`$mesh`, `$textureSet`, `$udim`,
  `$suffix`), format (PNG, TGA, EXR, TIFF, JPEG) and bit depth (8 / 16 / 32-bit float where the format
  allows), channel packing per R/G/B/A slot (any channel and component, grayscale, normal, height,
  baked mesh map or a constant, with invert), and padding, size and normal-format overrides per map
  or per preset. Presets are saved in the user library (`export-presets`) and can be exported and
  imported as JSON; Substance 3D Painter export presets (`.spexp` / export JSON) can be imported.
  Built-in templates stay read-only and write exactly the same files as before.
- **TIFF export** at 8, 16 and 32-bit float.
- **Brush colour picker** (issue #141). Clicking the brush's Base Color or Emissive swatch opens a picker with a
  hue ring around a saturation/value square, R/G/B and H/S/V sliders, a hex field (`#RRGGBB`, `RRGGBB` or `#RGB`;
  pasting a code applies it), old/new swatches (click Old to go back) and the last 12 colours picked, kept in
  the preferences. Changes apply to the brush at once. Colours stay linear; HSV, RGB numbers and hex are sRGB.
- **Brush block in Properties** (Painter parity). Paint tool Properties follow Painter's order: Brush, Alpha,
  Stencil, Material. Size and Flow have pen pressure toggles (with a Minimum row), plus Stroke opacity,
  Spacing, Angle, Follow Path, four 0-1 jitters with pressure toggles, Alignment, Backface Culling and
  Size Space (Object / Viewport / Texture). The Alpha slot has a thumbnail, a searchable mini browser and
  accepts alphas dropped from the Shelf.
- **Stencil, eraser and clone options.** Stencil tiling (horizontal, vertical, both), Shift + S + drag snaps
  the rotation to 90 degrees, a Reset button, stencil opacity and Hide stencil when painting. The eraser
  erases only the enabled channels. Clone has a Relative / Absolute source mode.
- **Tool, brush and material presets** from a right-click in Properties or the Presets block. A tool preset
  switches to its tool. The Shelf preset menu has Update from current tool, Rename, Duplicate, Favorites
  (listed first with a star) and Delete. Older presets still load.
- **Fill projection options**: Filtering (Bilinear HQ, Bilinear Sharp, Nearest), UV Wrap, Shape crop for 3D
  projections, and a 0-1 tri-planar Hardness in place of Sharpness (old projects keep their look).
- **More Standard filters**: Smoothstep, Threshold modes, Blur and Histogram Shift wrap, HSL Perceptive mode,
  Contrast Luminosity, Color Correct (shadows, midtones, highlights), Transform and Mirror.
- **Mask Editor generator** with global Invert, Blur, Balance and Contrast, and Texture, Texture 2, Ambient
  Occlusion, Curvature, World Space Normal, Position Gradient and Thickness groups, each with its own opacity.
- **Display Settings panel** (View > Display settings): Environment, Camera, Post effects, Render and
  Visualization in Painter's order, saved with the project. Opening a project never marks it modified.
- **Environment and camera**: backdrop opacity and blur, exposure in EV, World or Local alignment, drag an
  environment onto the 3D view, field of view with a focal-length readout, Ctrl+MMB sets the focus
  distance, F9 returns from rendering to painting.
- **Post effects**: master and per-effect switches, more tone mapping functions with Gamma and Mapping
  Factor, bloom falloff, Painter's 13 glare shapes, lens flare, chromatic aberration, vignette, sharpen,
  film grain and colour correction.
- **Viewport visualisation**: solo view of any channel, channel display options (no lighting, HDR scale,
  signed colours, components), anisotropic filtering and MipMap bias, grid plane, colour and opacity, and
  a camera frame with aspect presets.

### Fixed
- **Typing into a slider's number box** (issue #140). The box no longer rewrites the number while you type
  (typing `.25` used to give `0.52`). The text stays as typed until Enter or clicking away; Escape reverts.
  `.25`, `-.5`, `25`, `1e-2` and a decimal comma (`0,25`) are understood. Applies to all property sliders and
  the brush bar's number boxes.
- **Many crash and bug fixes found by testing:**
  - A lost GPU device copies unsaved work to the recovery folder and stops drawing instead of crashing
    (issue #119); older recovery copies are no longer offered for a newer file.
  - Render export, the GPU compositor and extra GPU devices no longer panic on GPU failures or a poisoned lock.
  - Corrupt Painter `.spp` files give an error instead of a crash.
  - Import: non-ASCII texture set names, deeply nested USD, STL / PLY edge cases, huge UVs and out-of-range
    submeshes no longer crash; mesh reload no longer strands or erases paint.
  - Export: texture sets can no longer overwrite each other's files, nothing is exported into the working
    directory, the JPEG override keeps packed alpha, and USD material slots with the same name stay separate.
  - Painting: brush strokes no longer land on a text layer, nested passthrough folders, Edge Detect, mirror
    symmetry and pen-pressure spacing are fixed, and a polygon fill with a normal-stamp brush fills a flat normal.
  - Saving keeps resources that only exist as pixels; projects with folder anchors or NaN values open again.
  - Ctrl+V pastes layers even when the clipboard held no text; Ctrl+click deselect no longer sets up Delete on the
    wrong layer; the content effect Name field accepts typing; panels follow the open session.
  - One unknown preference no longer resets all preferences; no validation error every frame with MSAA off.
  - PhotoCraft: a save during a brush stroke waits for the stroke to end.
- **Clear start-up message on Linux** when window libraries are missing or there is no display, instead of
  "update your graphics driver".

## 0.3.4

### Added
- **Anchor points**, like Substance Painter. Add › Anchor Point on a layer's content or mask captures that
  stack at that point. Layers above can read it: a fill channel's source (Anchor), an Anchor Reference mask
  effect (with optional Levels), or a generator's input image in place of its baked map. Pick the anchor and,
  for content anchors, the channel. References only read anchors below them; a missing, misplaced or
  circular reference is shown with a warning on the layer row and in Properties and changes nothing.
  Duplicates and pasted masks read their own copied anchors. Works with GPU compositing. Projects use schema
  17 (older projects open unchanged; older SubCraft refuses the new files instead of losing anchors).
- **`.spp` import keeps anchors live**: Painter anchor points become anchor points, and mask and fill
  references stay linked to them, instead of being baked into fixed rasters.

### Fixed
- **Pen tablet clicks on Linux Wayland** (issue #82). On Wayland compositors that do not emulate a
  mouse for tablet tools (Niri and some wlroots compositors), a pen moved the cursor but SubCraft
  received no clicks at all. The new `--x11` option (or `SUBCRAFT_X11=1`) runs SubCraft under
  XWayland, where the pen works as a pointer. It is only used when an X server is reachable, and the
  start-up log now says which display backend is in use.
- **OBJ and glTF import no longer crash** on files where only some corners have UVs or normals (for example DAZ Studio OBJs).
- **glTF import ignores texture files.** A `.gltf` shared without its textures, or with differently-cased file names on Linux and macOS, now imports its geometry instead of failing.
- **A quick pen tap** paints with the tap's pressure instead of full pressure, which could leave a big spot.
- **Saving refuses a project that is too large to open again** (over 1 GiB per entry) and says so, instead of writing a file that then fails to open.
- **Start-up no longer aborts** when a command-line argument is not valid UTF-8, and the user library works on systems without a Documents folder.

## 0.3.3

### Fixed
- **Repeated graphics resets on some integrated GPUs** (issue #119). When the graphics card stops
  responding (a driver reset: the screen goes dark for a moment), the next start now opens in safe
  mode and turns GPU compositing and GPU baking off, so it doesn't happen again on every start. Turn
  them back on in Preferences after a driver update.

## 0.3.2

### Fixed
- **Crash "map accumulators: BufferAsyncError" while baking** (issue #119). When the GPU failed to
  hand back bake, composite or path-tracer results (for example after a driver reset), SubCraft
  stopped with an error. It now logs the failure and finishes the bake and the layer composite on
  the processor instead.

## 0.3.1

### Fixed
- **Crash a few seconds after start-up on some Windows machines** (issue #98). GPU compositing and
  GPU baking created their own graphics instance with every API enabled, so a window running on DX12
  still started the Vulkan loader and its implicit layers (for example the GOG Galaxy overlay). They
  now use the window's API and adapter only.

## 0.3.0

### Added
- **Brush preview.** Picking a brush tip, preset, starter alpha, normal map or height map in the Shelf
  makes it the active brush right away and shows a BRUSH card in Properties with the name, tip and a
  preview stamp and stroke. The chosen tile gets a highlight.
- **Ambient Occlusion generator**: smooth by default (Balance 0.53, Contrast 0, no breakup, Pattern size 12.5%). Raw mesh-map
  generators also start smooth.
- **Frameless window.** SubCraft draws its own minimize, maximize and close buttons next to Export; drag
  the empty header to move the window, double-click to maximize, drag the edges to resize. Set
  `SUBCRAFT_NATIVE_TITLE_BAR=1` for the normal OS frame.
- **Texture set size** can be changed from the Texture Set List (128 to 4096, undoable).
- **Shift+right-drag** rotates the environment, as in Painter. Turning only the key light is now
  Shift+middle-drag.

### Changed
- **Generators like Substance Painter.** New Dirt and Edge wear generators break up with a real grayscale
  grunge texture (bundled, seamless, CC0) instead of stylized speckles, with Painter's defaults and control
  names (Dirt Level / Contrast, Wear Level / Contrast, Grunge Amount, Edges Masking, Ambient Occlusion
  Masking, Curvature Weight, Edges Smoothness). Edge wear keeps the edge itself worn and chips the band
  behind it. Older projects keep their exact look.
- **Smart-material dirt.** Every Dirt generator on the smart-material shelf, and every newly added Dirt
  generator, uses Painter-style settings (Dirt Level 0.75, Dirt Contrast 0.39, Grunge Amount 0.52, Pattern
  size 16%, grunge map on). Smart materials with a Dirt or Grime layer get an "Ambient Occlusion" layer just
  below it: a dark, slightly rougher fill at 60% layer opacity, masked by an Ambient Occlusion generator.
- **Generator sliders on 4K sets** update while you drag instead of waiting for you to stop, and the
  procedural noise cache no longer thrashes (it was recomputing the noise on every step).
- **Normal-map brushes** switch instantly: the thumbnail is the stamp at once and the full-size image
  follows a moment later.
- **Faster compositing, noise, image fills and brush strokes**, roughly 1.5x to 3x depending on the
  stack, with identical output (checked texel for texel against the old code).
- **Startup defaults**: the Shelf opens on Smart Materials, the environment is Glasshouse Morning with the
  sun in front of the camera, the Assets column is narrower, panel greys are closer to Painter's, and the
  brush, eraser, fill, clone and smudge icons are no longer repeated in the 3D view toolbar.
- Fixed the language picker for Korean and Traditional Chinese.

## 0.2.5 — release candidate

Test status for this candidate is recorded in `docs/progress.md`; items marked "untested" were not
run by hand.

### Added
- **Painter-style layout** (default): Assets shelf with category icons, search and a bottom toolbar on
  the left; 3D view in the centre; Texture Set List, Layers, Texture Set Settings and a
  context-sensitive Properties panel on the right. Painter Fill / Material properties, mesh-map
  capture flags. View menu: side-by-side 3D | 2D split.
- **Default model SubCraft_base** (bundled FBX, imported from memory) with the **Dusty Plastic** smart
  material already applied; the default texture resolution is now **2048**.
- **Languages**: Korean and Traditional Chinese, a language preference and CJK font fallback.
- **Text tool**: Aa entry point, composer window (Ctrl+Shift+T), on-model move / scale / rotate handle,
  drag coalesced into one undo step; shelf drops highlight and explain; Alt applies shelf items in place.
- **Depth of field** in the 3D view (off by default; display and renders only, never exported textures).
- **Generators**: Painter-style edges masking, AO masking, curvature weight, smoothness, grunge scale,
  fine grain control and breakup noise offset; new Dirt and Edges generators start with chipped /
  speckled defaults (older projects keep their look). Contact sheets in `docs/generators/`.
- **World-space blur** for paint layers.
- **Painter keymap preset**, shortcut cheat sheet, mouse gestures and a viewport hint overlay.
- **Compatibility**: safe mode, GPU capability checks, device-lost handling, atomic exports, copyable
  diagnostics and fatal-error dialogs (`docs/compatibility.md`).
- **Bake log** with per-map timing and warnings, copyable from the Bake window.
- **Content**: 30 more normal maps and 4 alpha brushes in the starter pack; a separate Normal Maps shelf tab.
- **Normal-map brush**: paint a normal map or height texture from the shelf as a stamp brush (whiteout
  blend, per-stamp slope accumulation, optional tiled mode) with a NORMAL MAP BRUSH panel and live
  preview. Not tried with a real pen or tablet.
- **Micro effect on generators**: a generator's mask can also write relief into the Normal / Height
  channels of the layers below (amount, Raise / Carve direction, optional "grain follows generator").
  Saved in the project; older projects are unchanged.
- **Smoother AO, sharper mesh maps**: AO and thickness bakes get a 3x3 smoothing of covered texels,
  mesh maps below the texture set resolution are sampled bilinearly, and generator grain now fades in
  with Breakup (Breakup 0 is exactly smooth).
- **High-poly alignment and a redesigned Bake window**: offset, scale, rotation, centring and auto-fit
  for the high-poly source, drawn as a blue transparent overlay on the low poly; the Bake window is
  now organised in cards (Models, placement, maps) with per-map live progress, a collapsed details
  section and a size that defaults to the texture set resolution.
- **Support link**: a donate button in the README and a donate link in the repository funding file.
- **18-material PBR pack** (Black Basalt, Travertine, Terrazzo, Cobblestone, Cracked Asphalt, Dry Mud,
  Desert Sand, Mossy Stone, Tree Bark, Cork, Bamboo Flooring, Woven Linen, Worn Leather, Industrial
  Rubber, Brushed Aluminum, Oxidized Copper, Galvanized Steel, Hex Armor): textured color / roughness /
  metallic / normal / height / AO materials, each also a smart material with wear and dirt layers.
  Where a name was already taken the smart material is called "... PBR".
- **Shelf**: a Filters tab (click adds a filter to the selected layer or swaps the selected filter);
  clicking a material swaps the selected fill layer's material (Alt+click replaces in place, drag adds
  a layer); import Painter smart materials (`.spsm`); model files can be dropped anywhere on the window.
- **Layers and tools**: Ctrl+A selects all layers, Delete / Ctrl+D act on all selected layers; fill layers
  show a map slot per channel and accept dropped image files; the brush always paints (it adds a paint
  layer above a selected fill layer or folder); the path tool rasterizes the whole path at once;
  View > Mesh maps shows any baked map raw on the model; Shift+RMB / Shift+MMB drag turns the light /
  environment.
- Commands to remove a texture-set channel and change its format exist in the app; the Texture Set
  Settings panel does not use them yet.

### Changed
- **New logo** (white hexagonal S ribbon with a four-point star on orange) and a slightly warmer
  charcoal interface (lightness unchanged).
- **Closer to Substance Painter's look**: Painter's grey levels (panel bodies lighter than the #2b2b2b
  strips, brighter text) with the warm tint kept, hairlines between list rows and a narrower Assets column.
- The language picker in Preferences shows 한국어 and 繁體中文 correctly while the interface is in English
  (the CJK fonts load while Preferences is open, not only for the active language).
- **Frameless window**: SubCraft draws its own title bar with minimize, maximize / restore and close
  beside Export; drag the top bar to move (double-click maximizes), resize from any edge or corner.
  `SUBCRAFT_NATIVE_TITLE_BAR=1` brings back the OS frame.
- **Startup**: the Glasshouse Morning environment, turned so the key light comes from the front and
  above; the Assets shelf always opens on Smart Materials.
- **Shift+RMB drag rotates the environment** (as in Painter; listed in the hint overlay, shortcuts window
  and keymap). Turning only the key light moved to Shift+MMB.
- The Brush / Eraser / Fill / Clone / Smudge icons left the 3D view toolbar (they are in the tool rail).
- The **texture set size in the Texture Set List** is a dropdown (128 to 4096) that resizes the set, with undo.
- Shelf previews render with a preview-only tiling boost, at higher resolution, off the UI thread.
- 13 built-in smart materials were removed, and later also Muddy Leather, Dirty Rubber, Sandstone, Worn Leather and Old Wood; "Mossy Bricks" is now "Bricks" without moss and "Worn Brown Leather" is "Brown Leather".
- README rewritten with new screenshots; a Support link was added.

### Fixed
- Pen strokes: stamps along every pointer position of a frame (fewer gaps on fast strokes).
- Directional / cavity / edge generator breakup is continuous across UV seams.
- Text composer window layout (value boxes fully visible); stale or untranslated UI strings.

### Performance
- **Pen lag**: expensive strokes (large brush, large texture) use a live preview that applies every
  2nd-3rd stamp and the rest exactly on pen lift; per-stroke raster cache and unchanged-tile skip.
  At 4096 the work while drawing dropped about 2.5-3x with a 0.15-0.25 s pause on release.
- Generators: the slow 12-16 s recomposite at 2048 seen in an example harness was the harness using the
  system heap; the app uses mimalloc and takes roughly 0.13-0.5 s (single PC, indicative).

### Known issues
- Pen strokes with very large brushes (e.g. 300 px) at 4096 are faster but still not 60 fps
  (about 17 ms per stamp on the processor); tablet-driver behaviour was not reproduced.
- Generator timing figures come from one machine; stacks dominated by blur-type filters or procedural
  noise are not helped by the GPU compositor, and a 4096 cold composite takes over a second.
- Texture Set Settings has read-only fields: description, shader instance, channel format and channel
  remove, and non-square sizes. Filtering and UV Wrap on Fill layers are shown disabled.
- The Korean and Traditional Chinese translations are machine-drafted and need native-speaker review.
- The layout is a close but not exact match of Substance Painter; the lock toggle uses a check/x glyph.
- Linux and macOS builds of this candidate were not produced or tested on the release machine.
- The normal-map brush, Micro effect, smoother AO / sharper mesh maps and high-poly alignment were
  verified by unit / integration tests and scripted captures; none were exercised by hand with a mouse
  or pen, and the Bake window redesign was not compared against real production models.

## 0.2.1 — 2026-11-04

### New
- **Blender-style 3D navigation** (Preferences, "3D navigation"): orbit, pan and zoom like
  Blender, with continuous mouse takeover while navigating. The Painter scheme stays the default.
- **Export the current 2D view** as a texture ("2D View" export template).
- **Scene export** (OBJ + MTL, glTF, GLB, USDA), Roblox and Sketchfab export presets, a render
  size multiplier and a transparent-background option.
- **Text tool**: text layers with bidirectional text and shaping, embedded fonts and
  resolution-independent rasterization; mesh reload keeps them.
- **Automatic UV unwrapping** (File, Auto UV unwrap) for models without UVs, and **PLY and STL
  mesh import**.
- **GPU layer compositing** (Preferences, "Use GPU for compositing"): on by default for texture
  sets of 2048 x 2048 and larger. On a 4096 x 4096 paint stack a layer toggle took 0.5 s instead
  of 1.8 s and an opacity edit 0.5 s instead of 1.1 s (RTX 4060). Smaller sets, sets with
  blur-type filters, procedural-heavy stacks and small edits stay on the processor, which is also
  the fallback if the graphics card fails.
- **3D view**: wireframe overlay with color and opacity, baked normal view modes, camera import
  and bookmarks.
- **Layers**: copy and paste layers and effects, multi-select, texture set rename and hide,
  a per-layer result cache for faster edits, and a safer unsaved-changes prompt.
- **Eleven more filters** (Grayscale, Invert, Posterize, Histogram Shift, Color Balance, Replace
  Color, Normal Invert, Edge Detect, Glow, Directional Warp, Mosaic), a procedural scratch
  generator, and four more smart masks.
- **Smart materials**: import and export `.scsmart` files and PBR texture folders.

### Fixed
- Start-up on machines with more than one graphics card: SubCraft now probes adapters in a
  separate process, remembers what worked, falls back to another adapter and writes a start-up log
  (issue #83; not yet confirmed by the reporter).
- Pen and touch contact is mapped to a primary press before reaching the interface, with
  `SUBCRAFT_INPUT_LOG=1` for diagnostics (issue #82; tablet clicks on Linux Wayland not yet
  confirmed on real hardware).
- The top bar no longer overflows in narrow windows.
- Painter import pads solved textures outside the UV islands from the nearest island texel.

## 0.2.0 — 2026-11-02

### New
- **Open Substance Painter `.spp` projects**: model, baked maps and channel textures, and the
  layer stack rebuilt as SubCraft layers behind a quality gate (background job with progress and
  cancel). Roughness micro grain, normals, masks, placed images and 2D-view strokes are rebuilt.
  `.sbsar` archives can be read. Text labels and stencil sheets are not rebuilt (stencil
  sheets are readable but off by default).
- **USD import** (`.usd`, `.usda`, `.usdc`, `.usdz`) and **Alembic import** (`.abc`, Ogawa).
- **Path-traced Rendering mode** in the 3D view (GPU), plus `--path-trace [samples]`.
- **GPU baking** (ambient occlusion, thickness, curvature) behind a "Use GPU for baking"
  preference, and much faster big projects: parallel save, compositor and bake speedups.
- **Brush upgrades**: stroke smoothing with catch-up, scatter/particle mode, smudge strength,
  smear and material, 22 built-in brush presets.
- **New filters**: Gradient Map, Histogram Scan, Histogram Range, Median, Warp.
- **14 more smart materials and 15 more smart masks**, with shelf categories and descriptions.
- **Community pull requests**: painting in the 2D view with per-channel brush switching, folder
  hierarchy in the Layers panel, radial symmetry (2-64 copies), mesh reload (keep UVs or
  reproject paint), ID Color Mask, Edit in PhotoCraft (PSD round trip), pen pressure.
- **Platform builds**: macOS universal, Linux aarch64 and Windows arm64 packaging, an
  architecture-aware updater and Cmd shortcut hints on macOS (not yet tested on that hardware).

### Fixed
- Viewport render targets are clamped to the device texture limit (crash on very large windows
  with Mesa/RADV).
- Bake Match by Name: rays from a low part only hit its own high part.
- Long lists in New Project wrap; saved window layout is validated; start-up falls back to an
  error window instead of closing.
- Painting a stroke across a hard edge stays gap-free and keeps its width.

## 0.1.8 — 2026-10-22

### New
- **Clone and Smudge tools**, and **Project from the view** for brush images (camera projection).
  Clone: Ctrl+click sets the source, later strokes keep the offset. Smudge drags paint along
  with Flow as strength. Both work on paint layers and masks.
- **New Project window** (File, Welcome screen): pick a model and see triangles, materials,
  objects, UV range, UDIM tiles and warnings before creating anything. Choose template,
  resolution, texture-set split (by material / object / one set), UDIM option, normal format
  and an optional high-poly model. Your last choices are remembered.
- **UDIM**: a model with several UV tiles becomes one texture set per tile, each with its own
  layers; export as `.1001` file names or one folder per tile.
- **Export checkboxes**: every output map is listed per texture set with checkboxes, Select
  all / None / Invert and a live file count. Alt+click a map to export only that one. The
  selection and the project's normal-map format are saved in the project and now really apply
  to exported normal maps.
- **Engine presets** for Unity URP and Godot 4; the Unreal preset is renamed "ORM packed".
- **Height and Bent Normal bakes** (Bake window and 2D view; both also work as generator sources).
- **Viewport rendering**: tone mapping (ACES, Filmic, Reinhard, Linear), exposure, bloom,
  ambient occlusion, and emission glow and glare streaks (Render quality window). File >
  Render image saves a still (1024-4096 px, optional 2x supersampling); Turntable saves
  24-120 numbered PNG frames.
- **Emission** can be switched on from Properties (+ emiss chip), starting white at intensity 3.
- **Shelf content**: 15 new smart materials (20 in total), 16 new smart masks (21 in total) and
  five new procedural generators (Dirt, Cracks, Streaks, Speckles, Pores).
- **50 new normal maps and 50 new brush alphas** in the CC0 starter pack: now 80 + 80, all CC0.
- **10 built-in HDRI environments** (Paint Booth, Ceramic Gallery, Library Hall, Atelier
  Windows, Redwood Creek, Alpine Lake Blue Hour, Salt Flats Twilight, Glasshouse Morning, Neon
  Arcade, Orbital Atrium) next to the five studio/sky presets. They are AI-generated
  panoramas with synthesized HDR lighting, CC0 (see `assets/environments/CREDITS-CC0.txt`).
  The full-size 2048x1024 originals are an optional extra download on the release page.
- Material filtering, effect reordering and stage thumbnails, Bevel / Mask Outline / Blur /
  Sharpen / Highpass / Directional Blur filters and Natural Edge Wear controls (details below).

### Changed
- Viewport tone mapping, bloom, AO and glow are display-only; exported textures are never affected.
- Package size grows: the starter pack is about 60 MB instead of 13 MB and the bundled
  environments add about 16 MB, because all images ship inside the program (see the release notes
  for installer sizes).
- Project files use schema 13; older projects open unchanged. Saving with this version
  needs this version or newer to reopen.

### Fixed
- Natural and Exposed Edge Wear connectivity, curvature at hard corners and UV borders,
  maximum Spread at low Level, high-poly bakes without UVs, live baked-normal layers and bake
  error reporting (details below).
- Brush slots accept drops; wrap and filter audits from the 0.1.8 development cycle.
- UVs outside the 0..1 tile range are no longer silently merged into tile 1001.

### Removed
- The browser version (WebAssembly build and its GitHub Pages site). SubCraft is a desktop
  app for Windows and Linux; nothing changes for desktop users (ADR-0038).

### Limitations
- Turntable export writes PNG frames only: there is no GIF or MP4 encoder.
- UDIM has no 2D tile picker and no "apply to all tiles" command; layer stacks are per tile.
- Clone copies from a point (no heal or seam blending); camera projection projects once per
  stroke, not per stamp.
- Windows installers and packages are not code-signed yet, so SmartScreen may warn.

### Details of earlier work in this release

#### Rendering
- Filter minified material textures with incremental GPU mipmaps, reducing fine PBR grain
  and shimmer at distance. Preserve sparse uploads, original/exported texels and the Nearest
  albedo pixel-art mode. Verified with native 4K wood/metal imports and numeric GPU tests.

#### Distribution
- Prepare signed release checks: pinned tools/actions, locked dependencies, app/installer
  signatures, Defender reports, checksums and GitHub provenance. Publishing requires an
  enrolled signing identity. Add an offline Sandbox smoke-test harness; hosted signing
  and guest execution remain unverified on this development PC.

#### UI
- Drag effect names/rows/preview squares to reorder within their content or mask stack. Amber
  insertion guides, edge scrolling and Escape cancellation; the moved effect stays selected
  and each drop is one undo step. Existing layer/shelf dragging is preserved.
- Effect rows show live stage thumbnails: compare the result through each Paint, Fill, Generator,
  Levels or Filter, click a square to edit, and resize with the existing Layers Preview size
  control. Passthrough folder previews now include the correct lower-stack input.
- Shelf is now on the left by default. Parts, Texture Sets and History share bottom-center
  tabs. Wide Texture Sets panels arrange set selection, resolution/filter and bake/mesh
  controls horizontally; narrow sidebars retain the vertical layout.
- Shelf assets wrap within narrow panels and scroll vertically; compact category dropdowns
  replace clipped tabs. Shelf and Layers have independent, remembered preview-size sliders
  beneath their lists, with adaptive layer rows and single-line asset names.
- Softer neutral charcoal theme, warm gray text, restrained amber accents and thin sliders
  with circular handles. Near-square buttons and interface scaling are retained.
- Layers show larger content/mask previews, type/effect counts and a quiet blend/opacity
  column. Compact nested effects have connecting lines and collapsible domain headers.

#### New
- Non-destructive Bevel and Mask Outline on content, masks and adjustment folders. Radius,
  Threshold, relief Amount and Inner/Outer/Both borders; consistent UV contour distances,
  scaled thumbnails, undo and exact export. Schema 12 preserves older stacks.
- Directional Blur with UV angle/radius and Highpass with detail scale/amount on content,
  masks and adjustment folders. Existing undo, worker previews and exact export are retained;
  schema 11 preserves older projects and smart resources.
- Non-destructive Gaussian Blur and Sharpen effects for layer content, masks and adjustment
  folders. Radius/Amount controls, continuous cross-tile sampling, soft paint coverage without
  dark fringes, scaled thumbnails and exact undo/save/export. Schema 10 opens older projects.
- Non-destructive Contrast, HSL, Threshold and Clamp content filters; Contrast/Threshold/Clamp
  on masks. Select an effect for its own parameters, channel and blending. Filters preserve
  paint coverage, support undo and save in projects/Smart Materials/Masks (schema 9).
- Folder Filter/Levels adjustments: process children in Normal mode or the lower accumulation
  in Passthrough mode, limited by the folder mask and opacity.
- Natural Edge Wear: Spread and Variation controls grow irregular wear from connected convex
  edges, including at Breakup 0. Spread is independent of Level. Existing masks can opt in through
  Natural edge wear; new Edges generators enable it by default. Schema 8 preserves legacy
  generators and saves the new controls in projects/smart recipes.
- Generator Pattern size, Seed/Next and Reduce repetition controls. New Edges/Cavity effects
  start at 0.3 breakup; organic volume noise varies across faces while remaining UV-seam continuous.
- Compact fill/Fill-effect/brush channel toggles, Alt-click solo and hidden disabled rows.
- Red symmetry-plane surface guides with a Show symmetry guides switch in the mirror popup.
- High-poly preview automatically refreshes when its source file changes. Successful bakes
  return to Material. Albedo filtering is now only in Texture Sets; Rotation sits below Tiling.
- Project schema 7 preserves legacy generator breakup when reading earlier projects and recipes.
- Bake inspection: blue high-poly/red exposed low-poly surfaces and adjustable front/rear
  cage wireframe before baking. Geometry retains its exported alignment.
- Live bake maps: completed maps appear on the low-poly model while remaining maps compute;
  follow automatically or inspect a finished map, then return to the material view.
- Albedo filtering: choose Bilinear or Nearest beneath Resolution in the Texture Sets panel.
  Nearest preserves imported pixel art through projection, painting, working-size
  changes, preview and exported maps. The choice is undoable and saved per texture set.
- Older projects open with Bilinear. Current saves require this build or a newer reader
  supporting schema 9.

#### Fixed
- Baked convex corners beside separate parts no longer lose wear when Natural edge wear is
  off. Rebake Curvature to update stored maps; flat contact creases retain their concavity.
- Maximum Spread now reaches its full distance at low Level values such as 0.065. Existing
  Natural masks retain settings but may change width at Level values other than 0.5.
- Natural wear stays connected at the default crate's body/post joins where contact curvature
  previously erased edge coverage. Geometric seam matching tolerates float-rounding splits.
- Curvature no longer weakens at exact hard corners and UV-border vertices due to triangle
  centroid direction, preventing holes in high-threshold Edge Wear. Rebake Curvature in existing
  projects to update stored maps.
- High-poly bake sources can import without UVs; UVs are required only on the painting model.
- High-poly normal bakes appear through an editable live Baked normal layer and export with
  the material. Existing mesh-map references are reused on rebake.
- Bake errors/completion stay visible in the dialog. Misaligned/out-of-cage sources report
  failure, partial ID projection misses no longer crash, and partial job failures are reported.

## 0.1.7 — 2026-10-18

### Faster
- Desktop viewport compositing now runs in the background. Cold procedural tiles no longer
  block the UI thread; the newest edit wins and export waits for the exact complete result.
  Large texture changes appear progressively while the controls remain available.
- Generator, material, opacity, blend and Levels edits reuse cached work instead of rebuilding
  every procedural field. The 4K demo measures roughly 14–15 ms median editing frames on the
  development PC; initial graphics setup can still cause a startup pause.
- Procedural patterns are computed once and reused; only the pattern you are editing is
  recomputed. Generating them is also about twice as fast, with exactly the same look.
- Only the parts of a texture that actually changed are sent to the graphics card.
- New GPU textures clear to their fill value and upload only allocated tiles, reducing the
  cost of opening a sparse 4K stack.

### New
- Paint texture images into individual brush channels, including normal maps, or paint a whole
  imported material. UV, planar and triplanar projection share scale/rotation/offset controls;
  normals support Replace and renormalized detail blending. Brush presets embed the images.
- Layer rows show real thumbnails of each layer's content (for the channel picked above the
  Layers list) and, beside it, its mask. Click the content or the mask thumbnail to choose what
  you edit and paint; the selected one has an orange outline.
- Mask Properties: enable/disable, invert (as an editable effect, nothing is lost), copy/paste
  a mask between layers, remove, a grayscale UV preview, and the mask's effects with move,
  duplicate, blend and opacity controls. Shift+click a mask thumbnail to disable it, Alt+click
  to preview it. Adding a white or black mask selects it right away.
- Effects on layer content: each paint or fill layer now has its own effect stack (Paint,
  Fill, Levels), separate from its mask's stack. The selected layer lists the effects of the
  content or mask (whichever thumbnail you clicked) under its row: click one to edit it in
  Properties, use the eye to hide it, right-click to reorder, duplicate or delete.
- Paint effects: select one and the brush paints into it, so you can paint on top of fill
  layers and erase that paint later without touching the fill. Mask Paint effects too.
- Projects now use format 5. Older projects open unchanged; projects with content effects
  need this version to open.
- A compact brush toolbar above the 3D View, grid/axis controls, a symmetry and lazy-mouse
  popover, adjustable UI size, calmer controls, and alternating rows in Layers, Parts and History.

### Fixed
- Properties follows layer/content/mask/effect selection instead of staying on the brush.
- Starting background compositing preserves existing synchronous results. Exporting or
  sampling during a refresh no longer leaves the viewport with stale tiles.

### Still in development
- General filters such as Blur, Sharpen and Bevel, Compare Mask, and Anchor Points are not
  included yet. Available mask effects are Paint, Fill, image, Generator, Levels and Invert;
  paint/fill layer content supports Paint, Fill and Levels.

## 0.1.6

### Fixed
- Importing a texture set with an ambient occlusion map adds an Ambient Occlusion channel to
  the texture set instead of skipping the map, so the texture's AO reaches the viewport and the
  Unreal / Unity / glTF exports. Texture sets without their own AO still use baked AO.
- Shelf → Textures → Import textures… now also turns the picked maps into one fill-layer
  material (base color, roughness, AO…) on the selected texture set, besides keeping them on
  the shelf. Images whose names don't say which map they are only go to the shelf.
- Saved smart materials that use images showed a blank white thumbnail; the preview now has
  their images. Smart material and smart mask thumbnails are shown on a sphere.
- The brush keeps painting where it overlaps the model when its center moves just past the
  model's edge, instead of stopping.
- Removed the orange object/part hover overlay entirely, including part-picking mode and
  hovering the Parts list. Right-click actions, part-mask picking and I/H/Esc shortcuts remain
  available without changing the model's displayed colors.

### New
- 3D View toolbar: render resolution (100 / 75 / 50 / 25 % for speed), anti-aliasing on/off,
  orthographic camera, and a View mode that shows the full material or one channel (base
  color, normal, roughness, metallic, height, AO) on the model. Remembered between launches.
- Texture Sets → Resolution: change a texture set's working resolution (128 to 4096) at any
  time, or apply it to every texture set. Painted strokes and masks are resampled; one undo
  step brings the old ones back. Lower is faster to paint and fill.
- Update notice: when SubCraft starts it checks GitHub for a newer release in the background;
  if there is one, a small window points at the green Update button (Update now / Later).
  Turn it off in Edit → Preferences → "Check for updates when SubCraft starts".
- Blend mode and opacity are now on the right of every layer row (for the channel picked at
  the top of the Layers panel) instead of in a separate block under the list.
- Clear ArtCraft independence notice in the README, on the welcome screen and in the new
  Help → About SubCraft window: SubCraft is not officially developed by, affiliated with,
  or endorsed by ArtCraft.
- Desktop graphics selection in Edit → Preferences: Automatic, DirectX 12 on Windows and
  OpenGL on Windows/Linux, with the active API shown. The setting persists immediately and
  applies at next launch; save your project before restarting. Failed explicit API startup
  retries Automatic and opens Preferences with an explanation. `WGPU_BACKEND` still overrides
  the preference. Browser settings report the browser's active API.

## 0.1.5

Published desktop packages: Windows installer 23.22 MB, Windows ZIP 24.87 MB and Linux x64
archive 26.39 MB. The browser module is 16.74 MB; full starter PNGs load on demand.

### New
- **CC0 texture starter pack**: 30 tileable 1024px normal maps and 30 brush alphas supplied by
  Aalzard, with a Starter pack shelf category, thumbnails and a Tileable filter. Normal maps drag
  onto Normal or create a normal-only fill layer. Full images are bundled offline on desktop and
  fetched on demand in the browser; lossless optimization reduced 13,894,995 bytes to 12,928,272.
  Shelf thumbnails total 340,359 bytes. Credits ship with desktop and web packages.
- **Texture/alpha preview**: 1×1, 2×2 and 3×3 tile views, lit normal sphere and sample alpha stroke;
  normal strength, flip-green (OpenGL/DirectX), bump inversion, alpha brightness/contrast, scale,
  rotation, repeat/mirror/clamp and seamless edge blend. Changes apply to a copy and adjusted
  normal maps embed in saved projects. Pixel work is split across frames; Save/Export and update
  restart wait for pending application.
- **Editable pen/Bézier paths**: select Pen (P), click points in the 3D or 2D view and drag to
  create curves. Move anchors and handles, break mirrored handles with Shift, close/open paths,
  choose Corner/Smooth points, delete points, and adjust width, hardness, opacity and PBR channel
  values. Paths live on their own editable layers, support undo/redo, and remain editable after
  save/load or duplication. Raster work is split across frames; saving/exporting and update
  restart wait for pending path edits. Project schema 4 stores path parameters and their tiled
  raster cache; schemas 1–3 still load. Width follows UVs; use separate paths across UV seams and
  the 2D view for handles outside UV islands.
- **Layer tree editing**: drag rows or names above/below layers, into folders, and out to either
  end of the top level, with visible drop guides and scrolling at the list edges. Folder arrows
  expand/collapse; dropping inside opens the folder. Double-click/F2 renames inline, Enter accepts,
  Esc cancels. Right-click also offers grouping, moving into folders and moving to the top level.
  Moves preserve UUIDs, masks and children, are undoable, and survive save/load. Adapted from
  CalangoTech's Studio fork, with destination validation moved into the app controller.
- **Welcome screen and quick start**: New from a model, Open project, desktop recent files,
  Try the sample, What's new and a six-step in-app guide. Help reopens them; turn off the
  startup screen in Edit → Preferences. An illustrated `docs/getting-started.md` is linked
  from the README. Missing recent files are marked and removable. Update restarts open the
  restored workspace directly.

### Fixed
- **Browser export downloads every map in one ZIP** instead of triggering separate downloads
  that browsers could silently drop after the first PNG. The archive uses the desktop renderer,
  encoding, padding and templates; project Save remains a separate `.subcraft` download.
- Long project names no longer overlap top-bar menus or actions in a narrow window.
- The welcome hub fits narrow windows and wraps its actions to keep them visible.

## 0.1.4

### New
- **In-app desktop updates**: a green **Check / update** button in the top bar checks the latest
  stable GitHub release, downloads the matching installer or portable package, verifies its
  SHA-256 digest, installs it and restarts. A private project checkpoint restores unsaved work
  and the original save path. Failed installation returns to the previous app and workspace.
  Windows installations keep their per-user/all-users mode; all-users updates ask for Windows
  permission. Windows and Linux x64 portable updates replace only the executable.

## 0.1.3

### New
- **Right-click texturing and Parts**: fill or paint only the clicked piece/UV island, mask a
  layer, add edge-wear/dirt/directional/position generator masks, pick color/material and
  isolate/hide pieces (I/H; Esc restores). Parts lists connected pieces, UV islands, material
  IDs and imported OBJ/FBX/glTF object names. Named objects and mask domains use project schema
  3; older projects still open. The layer menu now includes paint/fill layers, masks and effects.
- **Eyedropper**: unlit linear color/material sampling, adapted from CalangoTech's Studio fork.
  Part masks now prevent paint writes outside their selection, even if the mask is later removed.
- **Report a bug** (Help menu): opens a new GitHub issue in your browser with your SubCraft
  version, OS and GPU already filled in, and tells you which log file to attach. Nothing is sent
  automatically.
- **Log files and crash reports**: SubCraft writes a log (`%LOCALAPPDATA%\SubCraft\logs`, last 5
  runs, Help → Show log files) with the version, OS, GPU, recent actions and errors. If it
  crashes, a crash report is saved in `%LOCALAPPDATA%\SubCraft\crashes` and shown on the next
  start, with a button to report it.
- New GitHub issue forms for bug reports and feature requests.
- **Autosave and crash recovery**: SubCraft saves a recovery copy of the open project in the
  background every 5 minutes (change it in Edit → Preferences) and after big operations like
  baking or applying a smart material. Copies go to `%LOCALAPPDATA%\SubCraft\recovery` (the last 3
  are kept), never over your own `.subcraft` file. If SubCraft crashes or gets killed, the next
  start asks *"SubCraft didn't close properly. Recover your last session?"* with Recover / Discard /
  Show folder. In the browser it reminds you to download a backup instead, and closing the tab
  with unsaved work asks first.
- **Web version**: use SubCraft in the browser at https://aalzard.github.io/subcraft/ (needs
  WebGPU). Open uploads files, Save and Export download them. FBX is desktop only, and big
  operations are slower than in the desktop app.

### Changed
- Windows packaging respects `CARGO_TARGET_DIR`, so custom build folders cannot accidentally
  package an older executable from `target/`. Packaging-path regressions run in CI, and version
  bump pull requests also build both desktop packages.
- Shelf thumbnails no longer pause the app while they render.

## 0.1.2

### New
- **Parts**: show a layer only on some pieces of the model, like an axe's blade. Properties →
  Parts → *Pick parts in the 3D view*, then click pieces to show or hide the layer there (the piece
  under the cursor lights up). Painting, fills, smart materials and export all respect it, and it
  is saved in the project. Parts are the separate (unconnected) pieces of the mesh.

### Changed
- **New app icon**: an engraved hand on orange, used for the program, the window and taskbar, the
  installer and the Linux menu entry.

## 0.1.1

### New
- **Textures in the shelf**: Shelf → Textures lists every image you imported (or put in
  `Library\textures`), sorted into Base Color / Normal / Roughness… by file name. Drag one onto a
  channel row in Properties to use it there, or onto the model / a layer for a new fill layer.
- **Tiling X / Y and offset**: fill projections tile and shift on each axis separately (Link keeps
  them equal), with a reset button.
- **Channel strength sliders**: every fill channel has a power slider — Normal and Height
  strength, Roughness / Metallic / AO strength, Base Color brightness, Emissive intensity. Saved in
  projects (older projects open at full strength).

### Fixed
- SubCraft crashed at startup on some PCs (older Intel graphics drivers, `igvk64.dll`). On
  Windows it now runs on Direct3D 12 instead of Vulkan; set `WGPU_BACKEND=vulkan` to go back.

## 0.1.0

### New
- **Linux build**: `SubCraft-<version>-linux-x86_64.tar.gz` with an `install.sh` that adds SubCraft
  to your application menu (no root needed).
- **Windows installer**: `SubCraft-<version>-setup.exe` with Start Menu shortcut, optional desktop
  shortcut, uninstaller in Settings → Apps and `.subcraft` file association. A portable zip is
  still available. (#10)
- **Use your own images**: any PNG / JPG / TGA / TIF / EXR in a fill layer channel, with UV,
  tri-planar or planar projection, tiling and rotation. Images are saved inside the project. (#11, #13)
- **Import texture set**: select a texture's maps (base color, roughness, normal, height, metallic,
  AO…) and get one fill layer. Files are matched by name; glossiness and DirectX normal maps are
  converted automatically. Find it in the Layers toolbar, the Layer menu and Shelf → Materials. (#12, #13)
- **Image masks**: use a black & white image or a logo with transparency as a mask effect. (#13)
- **Your library** in `Documents\SubCraft\Library`: imported brush tips and HDRIs stay in the shelf
  after a restart. Right-click library items to show or remove them. (#14)
- **Save smart materials and smart masks**: right-click a layer → *Save as smart material* or
  *Save mask as smart mask*, then reuse them in any project from the shelf. (#15)
- **Shelf search and filters**: search box plus *All*, *Mine* and category chips. (#16)
- **Brush presets**: save size, flow, tip, jitter, symmetry and material from the Brush panel;
  presets appear first in Shelf → Brushes. (#17)
- **Drag and drop**: drag materials, smart materials and smart masks onto the model (applies to
  the part under the cursor) or onto a layer. (#18)
- **Stencils**: paint through an image laid over the 3D view. Hold S and drag to move, rotate or
  scale it; hold N to paint without it. (#20)

### Fixed
- Projects containing TGA images could not be reopened. (#11)
- Edge wear and other generator masks followed UV seams instead of the model's edges. (#6)
- Layer rows overlapped the Blend / Opacity controls in a short Layers panel. (#19)

### Changed
- Project file schema 2 (image masks). Older projects open unchanged; older SubCraft versions
  can't open projects saved with this version. (#13)
