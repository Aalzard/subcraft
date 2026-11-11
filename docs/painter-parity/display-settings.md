# Painter parity: Display Settings

Sources:
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/display-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/environment-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/camera-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/viewport-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/post-processing
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/depth-of-field
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/bloom
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/glare
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/lens-flare
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/lateral-aberration
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/vignette
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/sharpen
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/film-grain
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/tone-mapping
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/color-correction
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/lens-distortion
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/color-profile
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/iray-renderer/iray-renderer
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/iray-renderer/iray-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/iray-renderer/viewer-and-mdl-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/subsurface-scattering/subsurface-parameters
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/subsurface-scattering/enabling-subsurface-in-a-project
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management-with-opencolorio
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management-with-adobe-ace-icc
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/camera-management
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/mode-menu
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/viewport-menu
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-12-0
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-12-2
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes

Checked against: origin/main 297dcb3 (2026-11-11)

General note on status: SubCraft has no Display Settings panel. Its equivalents are spread over the 3D view toolbar
(`crates/ui/src/viewport_bar.rs`), the View > "Render quality" window (`crates/ui/src/render_export.rs`), the Shelf's
Environments category (`crates/ui/src/panels/shelf.rs`) and the Render button popup. Every control that exists but lives in
one of those places is therefore `partial` (different place), even when the function is the same. Painter's docs give
almost no numeric defaults or ranges for this panel; "not documented" means exactly that.

## Window and view mode

Layout notes: one dockable window. Since 12.2 the content is grouped as Environment settings, Camera settings, Post effects,
Render settings and Visualization settings, with a row of jump buttons at the top. The older doc pages (still the only
per-setting reference) describe three groups: Environment, Camera, Viewport; the sections below follow those pages and
place the 12.2 additions where the release notes put them. The settings are stored with the project.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Display Settings window | Dock toolbar / Window menu | closed dock icon | missing | - | - |
| Group jump buttons (Environment, Camera, Post effects, Render, Visualization) | Display Settings, top row (12.2) | not documented | missing | - | - |
| Settings saved with the project | Display Settings (whole window) | per project | partial: stored in per-user preferences, not in the project | crates/ui/src/viewport_bar.rs:17 (ViewportPrefs) | - |
| View mode dropdown | Display Settings top; also viewport corner dropdown | Material | partial: only in the 3D view toolbar | crates/ui/src/viewport_bar.rs:361 | - |
| View mode: Lighting section (Material) | View mode dropdown | default | partial: toolbar combo entry "Material" | crates/render/src/lib.rs:135 | - |
| View mode: Single channel section (one entry per Texture Set channel) | View mode dropdown | not documented | partial: 6 fixed channels (Base Color, Normal, Roughness, Metallic, Height, AO); no Emissive, Opacity, Specular, User | crates/render/src/lib.rs:146 | - |
| View mode: Mesh maps section (one entry per baked map) | View mode dropdown | not documented | partial: toolbar combo "Mesh maps (baked)" list | crates/ui/src/viewport_bar.rs:376 | - |
| Display material | Viewport menu; shortcut M | M | done | crates/app/src/actions.rs:214 | - |
| Display next channel | Viewport menu; shortcut C | C | partial: cycles the fixed view-mode list (includes Material and baked-normal modes) | crates/ui/src/app.rs:959 | - |
| Display previous channel | Viewport menu; shortcut Shift+C | Shift+C | partial: same as above | crates/app/src/actions.rs:213 | - |
| Display next mesh map | Viewport menu; shortcut B | B | missing: `cycle_mesh_map` exists but nothing calls it (B is the Brush tool) | crates/ui/src/viewport_bar.rs:387 | - |
| Display previous mesh map | Viewport menu; shortcut Shift+B | Shift+B | missing | - | - |

## Environment settings: Environment

Layout notes: a thumbnail button for the map (opens a mini-shelf filtered to environments), then sliders in the order below.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Environment Map (button opening a mini-shelf) | Display Settings > Environment | not documented | partial: picked by clicking a tile in Shelf > Environments, no button/mini-shelf | crates/ui/src/panels/shelf.rs:725 | - |
| Drag an environment map onto the 3D view / onto the button | 3D view, Environment Map button | - | missing: shelf drag payloads have no environment kind | - | - |
| Override environment map color space (checkbox + color space list) | Display Settings > Environment (color-managed projects) | off | missing | - | - |
| Environment Opacity | Display Settings > Environment | not documented | missing: background is a flat color, the map is never drawn behind the model | - | - |
| Environment Exposure (EV) | Display Settings > Environment | 0 EV | partial: "Intensity" multiplier 0.1-4 (default 1), not EV, in Shelf > Environments | crates/ui/src/panels/shelf.rs:842 | - |
| Environment Rotation | Display Settings > Environment | not documented | partial: "Rotation" -180..180 degrees in Shelf > Environments | crates/ui/src/panels/shelf.rs:842 | - |
| Rotate environment by mouse | 3D view, Shift+Right mouse drag | Shift+RMB | done | crates/ui/src/nav.rs:76 | - |
| Environment Blur | Display Settings > Environment | not documented | missing | - | - |
| Environment Alignment | Display Settings > Environment | World; options World, Local | missing | - | - |

## Environment settings: Shadows

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Shadows (enable) | Display Settings > Shadows | off (unverified) | missing: raster view has no shadows (only the path-traced mode casts them) | - | - |
| Computation mode | Display Settings > Shadows | Lightweight; options Intensive, Average, Lightweight | missing | - | - |
| Shadows opacity | Display Settings > Shadows | not documented | missing | - | - |

## Camera settings: Camera

Layout notes: a Preset dropdown chooses which camera's attributes are shown (Default camera plus imported ones), with a
Restore button and a lock icon beside it; then Field of view, Focal length, Focus distance, Aperture. Focus distance and
Aperture are greyed out unless Post effects and Depth of field are on. A modified imported camera is shown in italics with
a trailing asterisk.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Camera Preset dropdown (choose camera to edit) | Display Settings > Camera | Default camera | partial: camera menu lists saved/imported viewpoints as bookmarks, no per-camera attribute editing | crates/ui/src/view_extras.rs:124 | - |
| Active camera dropdown | 3D view top bar | Default camera | partial: same bookmark menu (applies the viewpoint to the one orbit camera) | crates/ui/src/view_extras.rs:140 | - |
| Import cameras with the mesh (FBX / Alembic) | New project, Project configuration: Import Cameras | on (unverified) | partial: separate "Import camera…" from glTF/GLB/JSON only | crates/ui/src/view_extras.rs:186 | - |
| Imported camera is left when navigating (switches to Default camera) | 3D view | - | partial: bookmark is copied into the orbit camera, so navigating simply moves it | crates/ui/src/view_extras.rs:39 | - |
| Restore (reset imported camera attributes) | Display Settings > Camera | - | missing | - | - |
| Lock camera | Display Settings > Camera, lock icon | unlocked | missing | - | - |
| Modified marker (italic name with asterisk) | Camera dropdown | - | missing | - | - |
| Display ratio (read-only; "Unspecified" when absent) | Display Settings > Camera | Unspecified | missing | - | - |
| Field of View (degrees) | Display Settings > Camera | not documented | missing: value exists on the camera (set only by imported cameras), no control | - | - |
| Focal length (mm) | Display Settings > Camera | not documented | missing | - | - |
| Focus Distance | Display Settings > Camera | not documented | partial: "Focus distance" 0.2-3 as a multiple of the orbit distance, in Render quality window | crates/ui/src/render_export.rs:97 | - |
| Set focus distance by clicking the mesh | 3D view, Ctrl+Middle mouse click | Ctrl+MMB | partial: "Pick in 3D view" button then click; no shortcut | crates/ui/src/render_export.rs:103 | - |
| Aperture | Display Settings > Camera | not documented | partial: "Blur amount" 0-1 (default 0.5), not a physical aperture | crates/ui/src/render_export.rs:94 | - |
| Camera Perspective | Shortcut F5 (also viewport projection control) | perspective | done | crates/app/src/actions.rs:210 | - |
| Camera Orthographic | Shortcut F6 | - | done | crates/app/src/actions.rs:211 | - |

## Post effects: stack

Layout notes: since 12.0 each effect is a collapsible entry with its own on/off switch; effects are applied in the listed
order: Depth of field, Bloom, Glare, Lens flare, Lateral aberration, Vignette, Sharpen, Film grain, Tone mapping, Color
correction. Post effects are applied to the 3D view only, never the 2D view, and are shared with the Iray mode. SubCraft
extras with no Painter counterpart in this list: screen-space ambient occlusion (render_export.rs:82), emission-only glow
(render_export.rs:111) and "Reset look" (render_export.rs:133).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Activate Post Effects (master switch) | Display Settings > Post effects | off (unverified) | missing: each SubCraft effect is switched by its own amount | - | - |
| Per-effect on/off switch | Each effect header | off | partial: only Depth of field has a switch; others are off at amount 0 | crates/ui/src/render_export.rs:91 | - |
| Post-effect assets (dedicated asset usage, default assets in the library) | Assets window (12.0) | - | missing | - | - |
| Effects hidden in the 2D view | 2D view | always | done | crates/render/src/post.rs:89 | - |

## Post effects: Depth of field

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Depth of Field (enable; no own parameters, driven by Camera Focus Distance and Aperture) | Display Settings > Post effects | off | partial: "Enable" in Render quality window, plus an extra "Focus range" slider | crates/ui/src/render_export.rs:91 | - |

## Post effects: Bloom

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Amount | Post effects > Bloom | not documented | partial: "Bloom" 0-2, default 0 (off), in Render quality window | crates/ui/src/render_export.rs:76 | - |
| Threshold | Post effects > Bloom | not documented (1.0 = screen white) | partial: "Bloom threshold" 0-1, default 0.8 | crates/ui/src/render_export.rs:79 | - |
| Falloff | Post effects > Bloom | not documented | missing | - | - |

## Post effects: Glare

Layout notes: Luminance, Threshold, Remap Factor, then the Shape dropdown. The doc page predates 12.0; the 12.0 notes say
Glare was reworked, so parameter names may have changed (unverified).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Luminance | Post effects > Glare | not documented; 0 disables, realistic 0.5-4, max about 16 | partial: "Glare" 0-3, streaks from emissive areas only | crates/ui/src/render_export.rs:117 | - |
| Threshold | Post effects > Glare | not documented; 0-1 recommended | missing | - | - |
| Remap Factor | Post effects > Glare | 1.0 neutral | missing | - | - |
| Shape (dropdown) | Post effects > Glare | not documented; 13 options below | partial: "Glare shape" with 4 options plus a "Streak length" slider | crates/ui/src/render_export.rs:119 | - |
| Shape: Bloom | Glare > Shape | - | missing | - | - |
| Shape: Lens Flare | Glare > Shape | - | missing | - | - |
| Shape: Standard | Glare > Shape | - | missing | - | - |
| Shape: Cheap Lens | Glare > Shape | - | missing | - | - |
| Shape: After Image | Glare > Shape | - | missing | - | - |
| Shape: Filter Cross Screen | Glare > Shape | - | partial: named "Horizontal + vertical" | crates/render/src/post.rs:70 | - |
| Shape: Filter Cross Screen Spectral | Glare > Shape | - | missing | - | - |
| Shape: Filter Snow Cross (6 directions) | Glare > Shape | - | missing | - | - |
| Shape: Filter Snow Cross Spectral | Glare > Shape | - | missing | - | - |
| Shape: Filter Sunny Cross (8 directions) | Glare > Shape | - | partial: named "Starburst" | crates/render/src/post.rs:71 | - |
| Shape: Filter Sunny Cross Spectral | Glare > Shape | - | missing | - | - |
| Shape: Horizontal Streak | Glare > Shape | - | partial: named "Horizontal" | crates/render/src/post.rs:68 | - |
| Shape: Vertical Streak | Glare > Shape | - | partial: named "Vertical" | crates/render/src/post.rs:69 | - |

## Post effects: Lens flare

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Resolution | Post effects > Lens flare | not documented | missing | - | - |
| Camera | Post effects > Lens flare | options Panoramic Lens, Telephoto Lens | missing | - | - |
| Amount | Post effects > Lens flare | not documented; may exceed 1.0 | missing | - | - |
| Threshold | Post effects > Lens flare | not documented | missing | - | - |
| Aperture scale | Post effects > Lens flare | not documented | missing | - | - |
| Coat thickness | Post effects > Lens flare | not documented | missing | - | - |
| Coat IOR | Post effects > Lens flare | not documented | missing | - | - |
| Occlusion scale | Post effects > Lens flare | not documented | missing | - | - |
| Occlusion smoothness | Post effects > Lens flare | not documented | missing | - | - |
| Unique ghosts | Post effects > Lens flare | not documented | missing | - | - |
| Ghost position scale | Post effects > Lens flare | not documented | missing | - | - |
| Aperture texture (resource slot) | Post effects > Lens flare | not documented | missing | - | - |

## Post effects: Lateral aberration

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Amount | Post effects > Lateral aberration | not documented | missing | - | - |
| Radius | Post effects > Lateral aberration | not documented | missing | - | - |
| Softness | Post effects > Lateral aberration | not documented | missing | - | - |
| Ratio | Post effects > Lateral aberration | not documented; 1 = circular | missing | - | - |
| Center X | Post effects > Lateral aberration | not documented | missing | - | - |
| Center Y | Post effects > Lateral aberration | not documented | missing | - | - |
| Achromatic aberrations (checkbox) | Post effects > Lateral aberration | off (unverified) | missing | - | - |

## Post effects: Vignette

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Vignette (enable; the doc page lists no parameters) | Display Settings > Post effects | not documented | missing | - | - |

## Post effects: Sharpen

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Amount | Post effects > Sharpen | not documented | missing | - | - |
| Diameter (pixels) | Post effects > Sharpen | not documented | missing | - | - |

## Post effects: Film grain

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Amount | Post effects > Film grain | not documented | missing | - | - |
| Radius (pixels) | Post effects > Film grain | not documented | missing | - | - |
| Type | Post effects > Film grain | options monochromatic, colored | missing | - | - |

## Post effects: Tone mapping

Layout notes: Exposure, Gamma, Function, Mapping Factor. The Camera settings page lists only Linear (default) and ACES for
Function; the Tone Mapping page lists the eight functions below and names Sensitometric as what Auto selects. 12.0 "updated
the tone mapper", so the exact current list is not confirmed by the docs. Mapping Factor is ignored for Linear, LinearSat
and Sensitometric.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Exposure | Post effects > Tone mapping | not documented | partial: "Exposure" multiplier 0.25-4 (default 1) in Render quality window | crates/ui/src/render_export.rs:73 | - |
| Gamma | Post effects > Tone mapping | not documented | missing | - | - |
| Function (dropdown) | Post effects > Tone mapping | Linear (camera page) / Auto (tone-mapping page) | partial: 4 options, default ACES | crates/ui/src/render_export.rs:65 | - |
| Function: Auto | Tone mapping > Function | picks Sensitometric | missing | - | - |
| Function: Linear (unclamped) | Tone mapping > Function | - | partial: "Linear (clip)" clamps, which is LinearSat behaviour | crates/render/src/post.rs:37 | - |
| Function: LinearSat (clamped) | Tone mapping > Function | - | partial: named "Linear (clip)" | crates/render/src/post.rs:37 | - |
| Function: Sensitometric | Tone mapping > Function | - | missing | - | - |
| Function: Reinhard | Tone mapping > Function | - | done | crates/render/src/post.rs:36 | - |
| Function: ReinhardLum | Tone mapping > Function | - | missing | - | - |
| Function: Log | Tone mapping > Function | - | missing | - | - |
| Function: LogLum | Tone mapping > Function | - | missing | - | - |
| Function: ACES | Tone mapping > Function (camera settings page) | - | done | crates/render/src/post.rs:34 | - |
| Mapping Factor | Post effects > Tone mapping | not documented | missing | - | - |

## Post effects: Color correction

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Saturation | Post effects > Color correction | not documented; 0 = grayscale | missing | - | - |
| Contrast | Post effects > Color correction | not documented | missing | - | - |
| Brightness | Post effects > Color correction | not documented | missing | - | - |
| Bias | Post effects > Color correction | not documented | missing | - | - |
| Sepia tone ratio | Post effects > Color correction | not documented | missing | - | - |
| White Balance Temperature (K) | Post effects > Color correction | 6500 K | missing | - | - |

## Post effects: Lens Distortion (pre-12.0 effect, still documented)

Layout notes: this page is still in the help but the effect is not in the 12.0 effect list, so it may have been removed
(unverified).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Power | Post effects > Lens Distortion | not documented | missing | - | - |
| FOV | Post effects > Lens Distortion | not documented | missing | - | - |
| Edge Roundness | Post effects > Lens Distortion | not documented | missing | - | - |
| Edge Smoothness | Post effects > Lens Distortion | not documented | missing | - | - |

## Camera settings: Temporal anti-aliasing

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Temporal Anti-Aliasing (enable) | Display Settings > Camera | not documented | partial: "Anti-aliasing" checkbox is 4x MSAA, not temporal accumulation | crates/ui/src/viewport_bar.rs:262 | - |
| Accumulations | Display Settings > Temporal anti-aliasing | 16 recommended; 64 for high contrast (full option list not documented) | missing | - | - |

## Camera settings: Subsurface scattering

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Subsurface scattering (project-wide enable) | Display Settings, below Post effects | off | missing | - | - |
| Sample Count | Display Settings > Subsurface scattering | not documented; 8, 32, 64 shown as examples | missing | - | - |

## Camera settings: Color profile

Layout notes: a checkbox, a LUT resource slot and a white-point value. With the checkbox on and no LUT, every view is shown
linear/raw. Hidden or superseded in colour-managed (OCIO/ACE) projects (unverified).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Activate Color Profile | Display Settings > Color profile | off | missing | - | - |
| Color profile LUT (resource slot, 3D LUT stored as a 2D float texture) | Display Settings > Color profile | none; shipped LUTs are 2048x128 | missing | - | - |
| White point | Display Settings > Color profile | 1 | missing | - | - |
| Import a LUT with the "colorlut" usage | Import resources window | - | missing | - | - |

## Render settings (12.2)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Order-independent transparency | Display Settings > Render settings | off | missing: the viewport shader does not render opacity at all | - | - |
| Sort capacity | Display Settings > Render settings | High (4x); options Low (1x), Medium (2x), High (4x), Very high (8x), Ultra (16x) | missing | - | - |

## Viewport settings: Texture filtering

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Anisotropic Filtering | Display Settings > Texture filtering | Medium (4 spp); options Disabled, Low (2), Medium (4), High (8), Very High (16) | missing | - | - |
| MipMap bias | Display Settings > Texture filtering | 0 - Soft; options 0 Soft, 1 Medium Soft, 2 Sharp, 3 Very Sharp (bias 0 to -3) | missing | - | - |

## Viewport settings: Camera frame

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show camera frame (toggle; exact label unverified) | Display Settings > Camera frame | off (unverified) | missing | - | - |
| Gate mask opacity | Display Settings > Camera frame | not documented | missing | - | - |

## Viewport settings: Tool display

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Hide stencil when painting | Display Settings > Tool display | not documented | missing: stencil overlay only hides while N (bypass) is held | crates/ui/src/viewport.rs:659 | - |
| Stencil display opacity | Display Settings > Tool display | not documented | missing | - | - |
| Projection preview channel | Display Settings > Tool display | not documented | missing | - | - |

## Viewport settings: Mesh wireframe

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show mesh wireframe | Display Settings > Mesh wireframe | off | partial: toolbar toggle button (and its right-click popup), draws polygon edges with a merge angle | crates/ui/src/view_extras.rs:104 | - |
| Wireframe color | Display Settings > Mesh wireframe | not documented | partial: in the toolbar button's right-click popup | crates/ui/src/view_extras.rs:115 | - |
| Wireframe opacity | Display Settings > Mesh wireframe | not documented | partial: 0.05-1 (default 0.6) in the right-click popup | crates/ui/src/view_extras.rs:117 | - |

## Viewport settings: Channel display

Layout notes: only enabled while a single channel is shown. The selected colour component name is overlaid in the viewport.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Display solo view without lighting (unlit) | Display Settings > Channel display | not documented | partial: single-channel views are always unlit, no toggle and no edge-shaded variant | crates/render/src/viewport.wgsl:201 | - |
| Scale HDR values | Display Settings > Channel display | not documented (shown = channel / scale) | missing | - | - |
| Use +/- color for HDR values (checkbox) | Display Settings > Channel display | off (unverified) | missing | - | - |
| Positive color | Display Settings > Channel display | not documented | missing | - | - |
| Negative color | Display Settings > Channel display | not documented | missing | - | - |
| Color channels | Display Settings > Channel display | RGBA (color) / Grayscale+Alpha (grayscale); options RGBA, Grayscale+Alpha, R, G, B, Alpha | missing | - | - |
| Component name overlay in the viewport | 3D / 2D view | - | missing | - | - |

## Viewport settings: Grid

Layout notes: the grid subdivides automatically with zoom; the current unit is written in the viewport's bottom-left corner.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show grid | Display Settings > Grid | not documented | partial: toolbar toggle button and "Show grid" in its right-click popup; on by default | crates/ui/src/viewport_bar.rs:310 | - |
| Axis | Display Settings > Grid | Y; options X, Y, Z | partial: three checkboxes toggle axis guide lines; the grid plane itself is fixed to the floor | crates/ui/src/viewport_bar.rs:329 | - |
| Grid color | Display Settings > Grid | not documented | missing | - | - |
| Grid opacity | Display Settings > Grid | not documented | missing | - | - |
| Grid unit readout | 3D view, bottom-left | - | missing | - | - |

## Rendering mode (Iray): entering and status

Layout notes: Iray is a separate application mode with its own right-hand panels (Iray settings on top, then the same
Display Settings content, then viewer/MDL settings). SubCraft's equivalent is the path-traced "Rendering mode", which is a
toggle of the 3D view rather than a mode with its own panel layout; its settings are a popup on the Render button and a
copy in Preferences (`crates/ui/src/recovery.rs:327`). SubCraft extras: Bounces 1-16 (viewport_bar.rs:281) and Denoise
(viewport_bar.rs:288).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Rendering (Iray) | Mode menu | - | partial: View > "Rendering mode (path tracing)" (no Mode menu) | crates/ui/src/app.rs:1266 | - |
| Rendering (Iray) shortcut | F10 | F10 | done | crates/app/src/actions.rs:222 | - |
| Painting (leave rendering) shortcut | F9 | F9 | missing: F10 toggles; F9 is unbound | - | - |
| Camera icon | Main toolbar | - | partial: "Render" button in the 3D view's bottom-right corner | crates/ui/src/viewport.rs:608 | - |
| Status (Rendering / Paused / Done) | Iray settings, top | - | partial: status pill in the 3D view (samples per second / paused / done) | crates/ui/src/viewport.rs:557 | - |
| Resolution (readout) | Iray settings, top | viewport size | missing | - | - |
| Scene Size (readout) | Iray settings, top | - | missing | - | - |
| Iterations (done / max) | Iray settings, top | - | partial: "n / max samples" in the status pill | crates/ui/src/viewport.rs:556 | - |
| Rendering time (elapsed / max) | Iray settings, top | - | missing | - | - |
| Pause / resume button | Iray settings | - | partial: Pause/Resume button beside the Render button | crates/ui/src/viewport.rs:603 | - |

## Rendering mode (Iray): settings

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Min Sample | Iray settings | not documented | missing | - | - |
| Max Sample | Iray settings | not documented | partial: "Max samples" 0-8192 (0 = unlimited, default 1024) in the Render button popup | crates/ui/src/viewport_bar.rs:284 | - |
| Max Time | Iray settings | not documented | missing | - | - |
| Max Time unit | Iray settings | options seconds, minutes, hours | missing | - | - |
| Caustic Sampler Enabled | Iray settings | not documented | missing | - | - |
| Firefly Filter Enabled | Iray settings | not documented | missing: a fixed clamp in the shader, no control | - | - |
| Override viewport resolution | Iray settings | off | missing | - | - |
| Width | Iray settings (when overriding) | not documented | missing | - | - |
| Height | Iray settings (when overriding) | not documented | missing | - | - |
| Save Render | Iray settings | - | partial: "Save image…" in the Render quality window renders the raster view (PNG only), not the path-traced accumulation | crates/ui/src/render_export.rs:170 | - |
| Share (upload the render to ArtStation) | Iray settings | - | missing | - | - |

## Rendering mode (Iray): viewer settings

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Environment map (button; drop an HDR on it) | Viewer settings > Environment | shared with viewport | partial: shared with the viewport, chosen in Shelf > Environments | crates/ui/src/panels/shelf.rs:725 | - |
| Environment Exposure | Viewer settings > Environment | shared | partial: shared "Intensity" multiplier | crates/ui/src/panels/shelf.rs:842 | - |
| Environment Rotation | Viewer settings > Environment | shared | partial: shared "Rotation" slider | crates/ui/src/panels/shelf.rs:842 | - |
| Dome type | Viewer settings > Dome | options Infinite Sphere, Sphere, Sphere with ground | partial: "Ground shadow" checkbox adds a gray ground disc; no dome choice | crates/ui/src/viewport_bar.rs:289 | - |
| Radius | Viewer settings > Dome | not documented | missing | - | - |
| Texture Scale | Viewer settings > Dome | not documented | missing | - | - |
| Clear Color (checkbox + color) | Viewer settings > Dome | off | missing | - | - |
| Ground X | Viewer settings > Ground | bottom of the scene bounding box | missing | - | - |
| Ground Y | Viewer settings > Ground | bottom of the scene bounding box | missing | - | - |
| Ground Z | Viewer settings > Ground | bottom of the scene bounding box | missing | - | - |
| Reflectivity (color) | Viewer settings > Ground | not documented | missing | - | - |
| Glossiness | Viewer settings > Ground | not documented | missing | - | - |
| Shadow Intensity | Viewer settings > Ground | not documented | missing | - | - |
| Visible from below | Viewer settings > Ground | not documented | missing | - | - |
| Post effects and camera settings shared with the viewport | Iray mode Display Settings | - | partial: same preferences feed both modes, but there is no panel in the mode | crates/ui/src/viewport_bar.rs:128 | - |
| Depth of field post effect overrides Iray's own DOF | Post effects | - | missing | - | - |

## Rendering mode (Iray): default MDL parameters

Layout notes: listed under the viewer settings in Iray mode, one set per shader instance.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Emissive Intensity | MDL parameters | not documented | missing | - | - |
| Refraction | MDL parameters | not documented | missing | - | - |
| IOR | MDL parameters | not documented (air 1.0, glass 1.5) | missing | - | - |
| Scattering | MDL parameters | not documented | missing | - | - |
| Absorption | MDL parameters | not documented | missing | - | - |
| Absorption Color | MDL parameters | not documented | missing | - | - |

## Colour management view options

Layout notes: in colour-managed projects each viewport gets a toggle button and a display-transform dropdown in its top bar;
2D and 3D views can be set separately. The choice is not saved with the project. Project-level colour management settings
(Legacy / OpenColorIO / Adobe ACE, configuration, import/export colour spaces) belong to the project configuration area and
are listed only as one row here. SubCraft always shows linear data through a fixed sRGB output.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Display transform on/off button | Viewport top bar (3D and 2D) | on | missing | - | - |
| Display transform dropdown | Viewport top bar (3D and 2D) | from the OCIO configuration | missing | - | - |
| Separate display transform for the 2D and the 3D view | Viewport top bars | - | missing | - | - |
| Display transform auto-disabled when soloing a data channel | Viewports in solo mode | automatic | partial: data channels are always shown raw; there is no transform to disable | crates/render/src/viewport.wgsl:201 | - |
| Project color management mode (Legacy, OpenColorIO, Adobe ACE) | New project / Project configuration | Legacy | partial: fixed Legacy-style sRGB/linear handling, no setting | crates/core/src/color.rs | - |
