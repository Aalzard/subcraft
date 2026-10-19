# Changelog

What changed in SubCraft, newest first. Download the latest build from the
[Releases page](https://github.com/Aalzard/subcraft/releases/latest).

## Unreleased

### UI
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

### New
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
  supporting schema 8.

### Fixed
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
