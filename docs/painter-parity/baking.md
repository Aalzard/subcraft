# Painter parity: Baking

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/baking , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/baking-interface , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/how-to-bake-mesh-maps , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/baking-visualization-settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/mesh-map-settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/cage-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/baking/skew-correction , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/baking-panels/mesh-map-bakers , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/baking-panels/common-mesh-map-settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/baking-panels/mesh-map-settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/baking-panels/baking-log , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/mode-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/edit-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/viewport-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/general-preferences , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/mesh-map , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes
Checked against: origin/main 297dcb3 (2026-11-11)

The public pages name the settings but give almost no defaults, ranges or dropdown option lists for the bakers. Values recalled from the application are marked (unverified); the rest say "not documented".

## Entering and leaving Baking mode

Layout notes: Painter has three modes (Bake mesh maps, Painting, Rendering) switched from the Mode menu, a mode button at the top right of the viewport, or F8 / F9 / F10. Baking mode swaps the whole layout: viewport plus four baking panels (Mesh map bakers, Common settings, Mesh map settings, Baking log) and the Texture Set list; Log, Display settings and History stay reachable from the right bar. SubCraft: F8 swaps the painting panels for the baking panels around the views (left: Texture Set list, Mesh map bakers, Mesh map settings; right: Common settings; below: Baking log; Bake and Return over the views), F9 returns.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Baking mode button | Contextual toolbar, top right of viewport | not documented | partial: top bar "Bake" button switches to Baking mode (not at the top right of the viewport) | crates/ui/src/dialogs.rs (bake_mode) | - |
| Mode > Bake mesh maps | Main menu > Mode | not documented | done | crates/ui/src/app.rs (Mode menu) | - |
| Mode > Painting | Main menu > Mode | default mode | done | crates/ui/src/app.rs (Mode menu) | - |
| Edit > Bake Mesh maps | Main menu > Edit | not documented | done | crates/ui/src/app.rs:1149 | - |
| Dedicated baking layout (panel set changes with the mode) | Baking mode | not documented | done: views plus the baking panels; the painting panels come back with F9 | crates/ui/src/dialogs.rs (bake_mode) | - |
| Return to Painting mode button | Bottom of viewport in Baking mode | not documented | done | crates/ui/src/dialogs.rs (bake_mode) | - |
| Mode switch recorded as an undo step | History | not documented | missing | - | - |
| Baking parameter changes recorded in History | History | not documented | missing | - | - |
| Undo across a bake step reopens Baking mode | History | not documented | missing | - | - |
| Texture Set list available in Baking mode | Baking mode | not documented | done | crates/ui/src/dialogs.rs (bake_set_list) | - |
| Log, Display settings, History available in Baking mode | Right bar | not documented | partial: the painting panels are hidden in Baking mode | crates/ui/src/dialogs.rs (bake_mode) | - |

## Baking mode viewport

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Same navigation as Painting mode | Viewport | not documented | done | crates/ui/src/dialogs.rs:566 | - |
| Channel dropdown shows baked maps | Viewport, top right | not documented | partial: Display dropdown in the Bake window and mesh maps in the view-mode dropdown | crates/ui/src/dialogs.rs:706 | - |
| Baking visualization settings panel (collapsible) | Viewport, top left | not documented | partial: a collapsible section in Common settings > Viewport, not over the viewport | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Bake selected textures button | Viewport, bottom | not documented | done | crates/ui/src/dialogs.rs (bake_mode) | - |
| Cancel baking button | Viewport, bottom | not documented | done | crates/ui/src/dialogs.rs (bake_mode) | - |
| Viewport progress bar (mesh loading and baking) | Viewport | not documented | partial: bar beside Cancel over the views while baking, none for mesh loading | crates/ui/src/dialogs.rs (bake_mode) | - |
| Orbit while a bake runs | Viewport | not documented | done | crates/app/src/bake_job.rs:75 | - |
| Neutral material on the project mesh | Viewport in Baking mode | not documented | done: Visualization settings > Neutral material (off by default) | crates/render/src/lib.rs (neutral material) | - |
| Error notice in viewport when the Baking log is hidden | Viewport | not documented | partial: result line and warnings in the Baking log, which cannot be hidden | crates/ui/src/dialogs.rs (bake_log_panel) | - |

## Texture Set list in Baking mode

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Bake checkbox per texture set | Texture Set list (Baking mode) | not documented | done | crates/ui/src/dialogs.rs (bake_set_list) | - |
| Bake checkbox per UV tile | Texture Set list (Baking mode) | not documented | missing | - | - |
| Selection action menu (copy / invert selection) | Texture Set list (Baking mode) | (unverified) | missing | - | - |
| Description shown in baking list | Texture Set list (Baking mode) | not documented | missing | - | - |

## Mesh map bakers panel

Layout notes: a "Common settings" entry at the top (with its own sync button), then one row per mesh map with thumbnail and five controls in this order: check box, visualize, quick bake, auto-rebake, synchronize. A dropdown menu at the top of the panel holds the check and sync actions; right-click on a map gives the per-map action. Selecting a row shows its parameters in the Mesh map settings panel. SubCraft shows the maps as a two-column grid of toggle chips with All / None.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Common settings entry | Mesh map bakers, top | not documented | partial: selects Common settings (always shown at the right), no sync button | crates/ui/src/dialogs.rs (bakers_panel) | - |
| Baker row: Normal | Mesh map bakers | checked (unverified) | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: World space normal | Mesh map bakers | checked (unverified) | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: ID | Mesh map bakers | checked (unverified) | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: Ambient occlusion | Mesh map bakers | checked (unverified) | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: Curvature | Mesh map bakers | checked (unverified) | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: Position | Mesh map bakers | checked (unverified) | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: Thickness | Mesh map bakers | checked (unverified) | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: Height | Mesh map bakers | not documented | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: Bent normals | Mesh map bakers | not documented | done | crates/ui/src/dialogs.rs:647 | - |
| Baker row: Opacity | Mesh map bakers | not documented | missing | - | - |
| Mesh map thumbnail in the row | Mesh map bakers | not documented | partial: icon in the row, thumbnails only in Texture Set Settings | crates/ui/src/dialogs.rs (bakers_panel) | - |
| Check / uncheck a map | Baker row | not documented | done | crates/ui/src/dialogs.rs (bakers_panel) | - |
| Drag across check boxes to toggle several | Mesh map bakers | not documented | missing | - | - |
| Visualize the map in the viewport | Baker row | not documented | partial: eye button per row, only for baked maps | crates/ui/src/dialogs.rs (bakers_panel) | - |
| Quick bake (only this map) | Baker row | not documented | done | crates/ui/src/dialogs.rs (bakers_panel, start_bake) | - |
| Auto-rebake for the map | Baker row | off; turned on for Normal when cage or skew painting starts | partial: per-map toggle; rebakes after any bake setting, placement or matching change (no cage or skew painting yet) | crates/ui/src/dialogs.rs (auto_rebake_due) | - |
| Synchronize this map's settings across texture sets | Baker row | not documented | missing | - | - |
| Synchronize common settings (Common settings sync window) | Mesh map bakers, top | not documented | missing | - | - |
| Per texture set baker selection and common settings | Baking mode | per texture set | missing | - | - |
| Check all | Mesh map bakers dropdown | not documented | done | crates/ui/src/dialogs.rs (bakers_panel) | - |
| Uncheck all | Mesh map bakers dropdown | not documented | done | crates/ui/src/dialogs.rs (bakers_panel) | - |
| Invert checked mesh maps | Mesh map bakers dropdown | not documented | done | crates/ui/src/dialogs.rs (bakers_panel) | - |
| Apply checked to more Texture sets... | Mesh map bakers dropdown | not documented | missing | - | - |
| Apply checked to all Texture sets | Mesh map bakers dropdown | not documented | missing | - | - |
| Sync all settings to more Texture sets... | Mesh map bakers dropdown | not documented | missing | - | - |
| Sync all settings to all Texture Sets | Mesh map bakers dropdown | not documented | missing | - | - |
| Reset parameters to default values | Mesh map bakers dropdown / settings | not documented | done: the selected baker, or the common settings | crates/ui/src/dialogs.rs (reset_parameters) | - |
| Reset cage painting parameters | Mesh map bakers dropdown | (unverified) | missing | - | - |
| Reset cage painting strokes | Mesh map bakers dropdown | (unverified) | missing | - | - |
| Reset all baking settings | Mesh map bakers dropdown | (unverified) | done: the model files stay | crates/ui/src/dialogs.rs (reset_all) | - |
| Apply (mesh map) settings to more Texture sets... | Right-click on a baker row | not documented | missing | - | - |
| Breadcrumbs in Mesh map settings | Mesh map settings panel | not documented | partial: "Mesh map settings > map" title, not clickable | crates/ui/src/dialogs.rs (map_settings_panel) | - |

## Common settings: Output

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Output size (width) | Common settings > Output settings | paint resolution of the project; list not documented | partial: 256 to 4096, square only | crates/ui/src/dialogs.rs:663 | - |
| Output size lock (non-square) | Common settings > Output settings | locked | missing | - | - |
| Output size (height) | Common settings > Output settings | same as width | missing | - | - |
| Dilation width | Common settings > Output settings | 16 (unverified) | done: 0 to 64 texels, default 16 | crates/ui/src/dialogs.rs (common_settings) | - |
| Apply diffusion | Common settings > Output settings | on (unverified) | done | crates/ui/src/dialogs.rs (common_settings) | - |

## Common settings: High-poly parameters

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Use Low poly mesh as high poly mesh | Common settings > High-poly parameters | not documented | partial: implied by leaving the High poly field empty | crates/ui/src/dialogs.rs:608 | - |
| High definition meshes list (several files) | Common settings > High-poly parameters | empty | partial: several files are baked as one high poly; the overlay and ray-miss check show the first only | crates/ui/src/dialogs.rs (common_settings) | - |
| Add a high definition mesh file | High definition meshes | not documented | done | crates/ui/src/dialogs.rs:609 | - |
| Remove / clear high definition meshes | High definition meshes | not documented | done | crates/ui/src/dialogs.rs:612 | - |
| Cage type | Common settings > Cage | Distance-based (default); Automatic (experimental); Custom file | partial: distance-based, or a custom file when one is set; no Automatic | crates/bake/src/lib.rs (Cage) | - |
| Cage file | Common settings > Cage (Custom file) | none | done: matched to the low poly by UV | crates/ui/src/dialogs.rs (common_settings) | - |
| Max frontal distance | Common settings > Cage (Distance-based) | 0.01 (unverified) | done | crates/ui/src/dialogs.rs:678 | - |
| Max rear distance | Common settings > Cage (Distance-based) | 0.01 (unverified) | done | crates/ui/src/dialogs.rs:681 | - |
| Relative to bounding box | Common settings > Cage | on (unverified) | partial: always relative to the model diagonal, no toggle | crates/bake/src/lib.rs:51 | - |
| Average normals | Common settings > Cage | on (unverified) | done: on by default in the panels | crates/bake/src/lib.rs (averaged_normals) | - |
| Paint cage | Common settings > Cage (Distance-based) | not documented | missing | - | - |
| Ignore backface | Common settings > High-poly parameters | on (unverified) | done: on by default in the panels | crates/bake/src/lib.rs (bake_core) | - |
| Match | Common settings > High-poly parameters | Always (default, unverified); By mesh name | done | crates/ui/src/dialogs.rs:814 | - |
| Low poly mesh suffix | Common settings > High-poly parameters | _low (unverified) | done | crates/ui/src/dialogs.rs:820 | - |
| High poly mesh suffix | Common settings > High-poly parameters | _high (unverified) | done | crates/ui/src/dialogs.rs:822 | - |
| Suffix preview for the current texture set | Common settings > High-poly parameters | not documented | missing | - | - |
| Antialiasing | Common settings > High-poly parameters | None (default); Subsampling 2x2; 4x4; 8x8 (all unverified) | done: None, 2x2, 4x4, 8x8 (reduced when the supersampled bake would pass 8192 px) | crates/bake/src/lib.rs (Antialiasing) | - |

## Common settings: Skew correction

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Paint skew correction | Common settings > Skew correction | needs high poly, Distance-based cage, Average normals | missing | - | - |
| Edge protection | Common settings > Skew correction | not documented | missing | - | - |
| Edge distance | Common settings > Skew correction | not documented; relative to mesh size | missing | - | - |
| Edge contrast | Common settings > Skew correction | not documented | missing | - | - |
| Skew base normal mode (mesh or per-triangle) | Common settings > Skew correction | (unverified) | missing | - | - |

## Mesh map settings: Normal and World space normal

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Normal baker (tangent space, from high poly) | Mesh map bakers | no map-specific settings documented | done | crates/bake/src/lib.rs:40 | - |
| Baked normal uses the project Normal Map Format | Project settings | DirectX or OpenGL | partial: always OpenGL | crates/ui/src/new_project.rs:286 | - |
| World space normal baker | Mesh map bakers | no map-specific settings documented | done | crates/core/src/meshmap.rs:10 | - |

## Mesh map settings: ID

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| ID baker | Mesh map bakers | not documented | done | crates/bake/src/lib.rs:310 | - |
| Color source | Mesh map settings > ID | Vertex Color; Material Color; File ID; Mesh ID/Polygroup (default not documented) | partial: Vertex color, Material color, Mesh ID / Polygroup, Submesh; no File ID | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Color Generator | Mesh map settings > ID (File ID or Mesh ID/Polygroup) | Random; Hue shift; Grayscale (default not documented) | done: Random, Hue shift (default), Grayscale | crates/bake/src/lib.rs (IdGenerator) | - |

## Mesh map settings: Ambient occlusion

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Secondary rays | Mesh map settings > Ambient occlusion | 64 (unverified) | done: 4 to 256, default 32 | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Min Occluder distance | Mesh map settings > Ambient occlusion | 0.00001 (unverified) | done: fraction of the diagonal, default 0 | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Max occluder distance | Mesh map settings > Ambient occlusion | 1 (unverified); values above 1 allowed | done: fraction of the diagonal, default 0.25, values above 1 can be typed | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Relative to bounding box | Mesh map settings > Ambient occlusion | on (unverified) | partial: always relative, no toggle | crates/bake/src/lib.rs:45 | - |
| Spread angle | Mesh map settings > Ambient occlusion | 180 (unverified) | done: 1 to 180, default 180 | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Distribution | Mesh map settings > Ambient occlusion | Cosine (default); Uniform (unverified) | done: Cosine (default), Uniform | crates/bake/src/lib.rs (RayDistribution) | - |
| Ignore backface | Mesh map settings > Ambient occlusion | Never; Always; By mesh name (unverified) | missing | - | - |
| Self occlusion | Mesh map settings > Ambient occlusion | Always; Only same mesh name (unverified) | partial: follows the Match mode, no own setting | crates/bake/src/lib.rs:58 | - |
| Attenuation | Mesh map settings > Ambient occlusion | None; Smooth; Linear (unverified) | done: None (default), Smooth, Linear | crates/bake/src/lib.rs (AoAttenuation) | - |
| Ground plane | Mesh map settings > Ambient occlusion | off (unverified) | done | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Ground plane offset | Mesh map settings > Ambient occlusion | not documented | done: fraction of the diagonal under the lowest point | crates/ui/src/dialogs.rs (map_settings_panel) | - |

## Mesh map settings: Curvature

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Method | Mesh map settings > Curvature | options not documented (geometry based and normal-map based, unverified) | partial: geometry method only | crates/bake/src/curvature.rs:55 | #137 |
| Secondary rays | Mesh map settings > Curvature | not documented | missing | - | - |
| Sampling radius | Mesh map settings > Curvature | not documented; values above 0.01 allowed | done | crates/ui/src/dialogs.rs:692 | - |
| Relative to Bounding box | Mesh map settings > Curvature | not documented | partial: always relative, no toggle | crates/bake/src/lib.rs:49 | - |
| Self intersection | Mesh map settings > Curvature | not documented | missing | - | - |
| Auto tonemapping (per UV tile) | Mesh map settings > Curvature | on | missing | - | - |
| Tonemapping Min | Mesh map settings > Curvature (auto off) | not documented | missing | - | - |
| Tonemapping Max | Mesh map settings > Curvature (auto off) | not documented | missing | - | - |

## Mesh map settings: Position

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Position baker | Mesh map bakers | not documented | done | crates/core/src/meshmap.rs:14 | - |
| Mode | Mesh map settings > Position | All axis; Single axis (default not documented) | done: All axis (default), Single axis | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Axis | Mesh map settings > Position (Single axis) | X; Y; Z (unverified) | done | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Normalization type | Mesh map settings > Position | Bounding Box; Bounding sphere; none (default not documented) | partial: Bounding box (default), Bounding sphere; no "none" | crates/bake/src/lib.rs (PositionNormalization) | - |
| Normalization scale | Mesh map settings > Position | not documented | missing | - | - |

## Mesh map settings: Thickness

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Secondary rays | Mesh map settings > Thickness | not documented | done: 4 to 256, default 16 | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Min Occluder distance | Mesh map settings > Thickness | not documented | missing | - | - |
| Max Occluder distance | Mesh map settings > Thickness | not documented | done: fraction of the diagonal, default 0.5 | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Relative to bounding box | Mesh map settings > Thickness | not documented | partial: always relative, no toggle | crates/bake/src/lib.rs:48 | - |
| Spread angle | Mesh map settings > Thickness | not documented | missing | - | - |
| Distribution | Mesh map settings > Thickness | not documented | missing | - | - |
| Self occlusion | Mesh map settings > Thickness | not documented | missing | - | - |
| Normalization | Mesh map settings > Thickness | not documented | missing | - | - |

## Mesh map settings: Height

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Height baker | Mesh map bakers | not documented | done | crates/core/src/meshmap.rs:18 | - |
| Normalization | Mesh map settings > Height | options not documented (includes Manual) | partial: Cage distances (default) or Manual | crates/bake/src/lib.rs (HeightNormalization) | - |
| Scaling divisor | Mesh map settings > Height (Manual) | not documented | partial: "Height range", a distance as a fraction of the diagonal | crates/ui/src/dialogs.rs (map_settings_panel) | - |

## Mesh map settings: Bent normals

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Secondary rays | Mesh map settings > Bent normals | not documented | partial: shares the ambient occlusion value | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Min Occluder distance | Mesh map settings > Bent normals | not documented | partial: shares the ambient occlusion value | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Max Occluder distance | Mesh map settings > Bent normals | not documented | partial: shares the ambient occlusion value | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Relative to bounding box | Mesh map settings > Bent normals | not documented | partial: always relative, no toggle | crates/bake/src/lib.rs:45 | - |
| Spread angle | Mesh map settings > Bent normals | not documented | partial: shares the ambient occlusion value | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Distribution | Mesh map settings > Bent normals | not documented | partial: shares the ambient occlusion value | crates/ui/src/dialogs.rs (map_settings_panel) | - |
| Ignore backface | Mesh map settings > Bent normals | not documented | missing | - | - |
| Self occlusion | Mesh map settings > Bent normals | not documented | missing | - | - |

## Mesh map settings: Opacity

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Opacity baker | Mesh map bakers | settings not documented | missing | - | - |

## Baking log panel

Layout notes: a docked panel with three tabs. SubCraft has a collapsed "Details" log (with Copy log) and per-map status rows at the bottom of the Bake window.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Baking process tab (status of running / last bake) | Baking log | not documented | done | crates/ui/src/dialogs.rs (bake_log_panel) | - |
| High poly parameters tab (details of high poly files, loading state) | Baking log | not documented | partial: path, loading state, triangle and part counts, size of the one file | crates/ui/src/dialogs.rs (bake_log_panel) | - |
| Matching by name tab (how meshes were paired) | Baking log | shown when Match is By mesh name | partial: the mode and the unmatched low parts, not every pair | crates/ui/src/dialogs.rs (bake_log_panel) | - |
| Arrow button on an error: jump to the related baker settings | Baking log | not documented | missing | - | - |
| Individual UV tiles listed in bake progress | Baking log | not documented | missing | - | - |

## Baking visualization settings

Layout notes: a collapsible panel at the top left of the viewport with a "Hide baking meshes" icon and groups: General, High definition mesh (HP), Cage, UV seams, Project mesh (with Neutral material), plus Skew vectors while skew painting. Each mesh toggle has a colour button next to it. SubCraft has four icon toggles (high / low overlay, cage wireframe, live maps, follow bake) with fixed colours.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Hide baking meshes | Visualization panel | off | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Show for selected Texture Set only | Visualization panel > General | not documented | partial: the project mesh and cage; the high poly is always whole | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Mesh (show high poly; off unloads it) | Visualization panel > High definition mesh (HP) | on (unverified) | partial: hides it, the file stays loaded | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Mesh colour | Visualization panel > High definition mesh (HP) | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Matching error (high poly outside the cage) | Visualization panel > High definition mesh (HP) | on; bright red | partial: the project mesh shows through where the high poly does not cover it; ray-miss heatmap for cage misses | crates/ui/src/dialogs.rs (visualization_settings) | #137 |
| Matching error colour | Visualization panel > High definition mesh (HP) | red | partial: the project mesh colour | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Cage surface | Visualization panel > Cage | on while cage painting; transparent beige | partial: a flat see-through colour, off by default (no cage painting yet) | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Cage surface colour | Visualization panel > Cage | beige | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Cage surface opacity | Visualization panel > Cage | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Cage wireframe | Visualization panel > Cage | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Cage wireframe colour | Visualization panel > Cage | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Cage wireframe opacity | Visualization panel > Cage | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Missing seams on hard edges (3D and 2D views) | Visualization panel > UV seams | not documented | partial: lines in the 3D bake overlay with a count; not in the 2D view | crates/bake/src/seam_check.rs | - |
| Missing seams colour | Visualization panel > UV seams | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Project mesh (show low poly) | Visualization panel > Project mesh | on; forced on by Hide baking meshes | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Project mesh colour | Visualization panel > Project mesh | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Neutral material: Quality | Visualization panel > Project mesh | not documented | missing | - | - |
| Neutral material: Roughness | Visualization panel > Project mesh | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Neutral material: Metallic | Visualization panel > Project mesh | not documented | done | crates/ui/src/dialogs.rs (visualization_settings) | - |
| Neutral material: AO intensity | Visualization panel > Project mesh | not documented | partial: the baked AO at full strength | crates/render/src/lib.rs (neutral material) | - |
| Neutral material: Bent normal | Visualization panel > Project mesh | not documented | missing | - | - |
| Neutral material: Bent normal diffuse amount | Visualization panel > Project mesh | not documented | missing | - | - |
| Neutral material: Bent normal specular amount | Visualization panel > Project mesh | not documented | missing | - | - |
| Cage paint value overlay (green positive, blue negative) | Visualization panel (cage painting) | on | missing | - | - |
| Skew vectors (show lines; red none to green full) | Visualization panel > Skew vectors | on while skew painting | missing | - | - |
| Vectors length | Visualization panel > Skew vectors | not documented | missing | - | - |
| Vectors UV density | Visualization panel > Skew vectors | not documented | missing | - | - |
| Vectors opacity | Visualization panel > Skew vectors | not documented | missing | - | - |

## Cage painting

Layout notes: entered with Paint cage in Common settings (needs a high-poly file and the Distance-based cage). The toolbar offers Paintbrush, Eraser and Polygon fill; the top of the viewport shows size, flow, stroke opacity, spacing, Strength, blend mode and the symmetry toggle. The cage mesh is subdivided automatically.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Cage painting mode | Common settings > Cage > Paint cage | off | missing | - | - |
| Paintbrush tool | Cage painting toolbar | painting-mode shortcut | missing | - | - |
| Eraser tool (back to default cage size) | Cage painting toolbar | painting-mode shortcut | missing | - | - |
| Polygon fill tool (incl. Mesh fill mode) | Cage painting toolbar | painting-mode shortcut | missing | - | - |
| Size | Top of viewport | not documented | missing | - | - |
| Flow | Top of viewport | not documented | missing | - | - |
| Stroke opacity | Top of viewport | not documented | missing | - | - |
| Spacing | Top of viewport | not documented | missing | - | - |
| Strength (signed, HDR slider) | Top of viewport | not documented; low values such as 0.05 advised | missing | - | - |
| Cage strength slider for the last polygon fill | Viewport, temporary | not documented | missing | - | - |
| Blend mode | Top of viewport | Linear dodge (add) (default); Normal; Lighten (Max) | missing | - | - |
| Symmetry toggle | Top of viewport | off (unverified) | missing | - | - |
| Automatic cage subdivision | Automatic | always | missing | - | - |
| Auto-rebake Normal while painting | Mesh map bakers | on when first entering | missing | - | - |

## Skew correction painting

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Skew correction painting mode (stored per texture set) | Common settings > Skew correction > Paint skew correction | off | missing | - | - |
| Brush tool | Skew painting toolbar | painting-mode shortcut | missing | - | - |
| Eraser tool | Skew painting toolbar | painting-mode shortcut | missing | - | - |
| Polygon fill tool | Skew painting toolbar | painting-mode shortcut | missing | - | - |
| Size, flow, opacity, spacing | Top of viewport | not documented | missing | - | - |
| Grayscale colour picker for the skew value | Top of viewport | (unverified) | missing | - | - |
| Brush settings panel on right-click (Projection > Alignment: UV) | Viewport | per tool | missing | - | - |
| Auto-rebake limited to the stroke's UV bounding box | Automatic | on for Normal when first entering | missing | - | - |
| Enable / disable skew correction is undoable | History | not documented | missing | - | - |

## Baking preferences

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Save preprocessed scene files | Settings > General > Baking options | not documented | missing | - | - |
| Enable live preview baking process | Settings > General > Baking options | not documented | partial: "Live maps" and "Follow bake" toggles in the Bake window | crates/ui/src/dialogs.rs:703 | - |
| Enable GPU Raytracing | Settings > General > Baking options | on for supported GPUs (unverified) | partial: GPU rays used by default, no switch | crates/bake/src/lib.rs:56 | - |

## Shortcuts

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Baking (switch to Baking mode) | Shortcut | F8 | done | crates/app/src/actions.rs (Action::Bake) | - |
| Painting (switch to Painting mode) | Shortcut | F9 | done | crates/app/src/actions.rs (Action::PaintingMode) | - |
| Bake Mesh Maps (open baking settings) | Shortcut | Ctrl+Shift+B | done | crates/app/src/actions.rs:221 | - |
| Display next mesh map | Shortcut / Viewport menu | B | partial: dropdown only; B is the Brush tool | crates/ui/src/viewport_bar.rs:378 | - |
| Display previous mesh map | Shortcut / Viewport menu | Shift+B | partial: dropdown only | crates/ui/src/viewport_bar.rs:378 | - |
| Show / hide the viewport visualization | Shortcut | unassigned | missing | - | - |
