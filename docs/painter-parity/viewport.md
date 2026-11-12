# Painter parity: Viewport (3D view)

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/viewport, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/3d-view, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/camera-management, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/camera-settings, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/viewport-settings, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/properties, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/geometry-mask, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/paint-brush, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/clone-tool, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/quick-mask, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/polygon-fill, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/path, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/straight-line, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/lazy-mouse, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/symmetry, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/mirror-symmetry, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/radial-symmetry, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/fill-projections, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/tri-planar-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/planar-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/spherical-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/cylindrical-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/warp-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/spacemouse-by-3dconnexion, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/importing-assets/import-drag-and-drop, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-10-0, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-11-0, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-12-2, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2019-3, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-7-4
Checked against: origin/main 297dcb3 (2026-11-11)

## Viewport structure

Layout notes: Painter's viewport is one central area with four parts stacked top to bottom: the Contextual toolbar (fixed, cannot be moved), the 3D view and 2D view side by side (3D left by default, splitter between them), and a thin progress bar at the bottom. Each view carries its own overlays: camera dropdown and display-mode dropdown top right, axis indicator bottom right, colour-profile text top left, grid unit bottom left. SubCraft puts the 3D view and 2D view in egui_dock tabs (tabbed together by default, 3D in front).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Contextual toolbar strip above the views | Viewport, top | always shown | partial: strip exists above the 3D view only, own control set (see toolbar.md) | crates/ui/src/viewport_bar.rs:185 | - |
| 3D view | Viewport, left | shown | done | crates/ui/src/viewport.rs:57 | - |
| 2D view | Viewport, right | shown | partial: a dock tab behind the 3D view by default, not side by side | crates/ui/src/app.rs:189 | - |
| Splitter between 3D and 2D views | Viewport, between views | 50/50 (unverified) | partial: only after View > Side-by-side 3D / 2D | crates/ui/src/app.rs:232 | - |
| Progress bar for engine computations | Viewport, bottom edge | hidden when idle | missing | - | - |
| Colour-profile name text | 3D view, top left | shown when a profile is loaded | missing | - | - |
| Axis indicator (X/Y/Z orientation) | 3D view, bottom right | shown | missing | - | - |
| Grid unit read-out | 3D view, bottom left | shown when the grid is on | missing | - | - |
| Warning when painting on a non-active Texture Set | 3D view overlay (12.1) | not documented | missing | - | - |

## Camera navigation (mouse)

Layout notes: all navigation is a held modifier plus a mouse drag; the same combinations drive both views. Orbit pivots around the point under the cursor.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Camera rotate (orbit) | Alt + LMB drag | editable shortcut | done | crates/ui/src/orbit.rs (orbit_around) | #163 |
| Camera snap rotate | Alt + Shift + LMB drag | snaps to orthogonal views (every 90 degrees) | done | crates/ui/src/nav.rs (NavMode::OrbitSnap) | #163 |
| Camera translate (pan) | Alt + MMB drag | editable shortcut | done | crates/ui/src/nav.rs:82 | - |
| Camera translate (alternative) | Ctrl + Alt + LMB drag | editable shortcut | done | crates/ui/src/nav.rs:82 | - |
| Camera zoom (dolly) | Alt + RMB drag | editable shortcut | done | crates/ui/src/nav.rs:80 | - |
| Camera zoom with the mouse wheel | Wheel over the view | (unverified) | done | crates/ui/src/viewport.rs:105 | - |
| Rotate environment | Shift + RMB drag (horizontal) | editable shortcut | done | crates/ui/src/nav.rs:76 | - |
| Auto-focus (depth of field focus point) | Ctrl + MMB click on the mesh | needs Post effects + DOF on | partial: Ctrl+MMB works and turns depth of field on; Post effects is not a separate switch | crates/ui/src/viewport.rs (Painter clicks) | #163 |
| Texture set selection | Ctrl + Alt + RMB click on the mesh | editable shortcut | missing | - | - |
| Texture Set highlight under cursor | Hold Ctrl + Alt over the model (12.2) | hidden while LMB is down or panning | missing | - | - |
| Texture Set / UV Tile highlight from the list | Hover a row in the Texture Set list (12.2) | not documented | missing | - | - |
| Tablet scroll | Ctrl + Alt + click drag scrolls panels (2019.3) | not documented | missing | - | - |

## Camera commands and projection

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Center the whole mesh | F | focuses only the view under the cursor; both when the cursor is outside | partial: F / Home reset the 3D camera to the default three-quarter framing; 2D view not affected | crates/ui/src/app.rs:950 | - |
| Focus on the selected path | F while editing a path (11.0) | not documented | missing | - | - |
| Camera Perspective | F5 | default mode | done | crates/app/src/actions.rs:210 | - |
| Camera Orthographic | F6 | off | done | crates/app/src/actions.rs:211 | - |
| Free rotation (roll allowed) | Contextual toolbar > Camera Rotation Mode | off | missing | - | - |
| Constrained rotation (no roll) | Contextual toolbar > Camera Rotation Mode | default | done | crates/ui/src/orbit.rs:114 | - |
| Light the mesh from underneath | Orbit below the horizon (7.x) | not documented | partial: pitch clamped to about +/-89 degrees | crates/ui/src/orbit.rs:116 | - |

## Camera management

Layout notes: imported cameras come from the mesh file (FBX / Alembic) at project creation or re-import. They appear in a dropdown at the top right of the 3D view (hidden when only the default camera exists). Their attributes live in Display settings > Camera, with a Preset dropdown, a lock icon and a Restore button.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Import Cameras checkbox | New project window | on (unverified) | done: New project > Cameras, on by default; glTF, GLB and FBX files carry cameras | crates/ui/src/new_project.rs, crates/app/src/session.rs (import_mesh_cameras) | 217 |
| Import cameras on mesh re-import | Edit > Project configuration | off unless toggled | done: "Import cameras" in File > Reload mesh, off by default | crates/ui/src/mesh_reload.rs | 217 |
| Imported data: name, transform, FOV, aspect ratio | from FBX / ABC mesh file | - | done: from glTF / GLB and FBX mesh files (and separate camera files); saved with the project; roll is dropped by the orbit camera | crates/mesh/src/camera.rs (cameras_in_mesh_file), crates/doc/src/camera.rs | 217 |
| Camera selection dropdown | 3D view, top right | "Default camera"; hidden if no other camera | done: 3D view, top right, hidden when the project has no imported cameras; also Display Settings > Camera | crates/ui/src/view_extras.rs (camera_dropdown) | 217 |
| Default camera entry | Camera dropdown | selected, perspective | done: first entry; choosing it brings back the view from before | crates/ui/src/view_extras.rs (select_camera) | 217 |
| Navigating while an imported camera is active switches to Default camera | 3D view | always | done: unless the camera is locked | crates/ui/src/view_extras.rs (follow_camera) | 217 |
| Select next camera shortcut | Settings > Shortcuts | keys not documented | done: Alt+] (Painter documents no key) | crates/app/src/actions.rs (Action::NextCamera) | 217 |
| Select previous camera shortcut | Settings > Shortcuts | keys not documented | done: Alt+[ (Painter documents no key) | crates/app/src/actions.rs (Action::PrevCamera) | 217 |
| Camera Preset dropdown | Display settings > Camera | active camera | done | crates/ui/src/panels/display_settings.rs (camera) | 217 |
| Field of view | Display settings > Camera | degrees; range not documented | done: 5-120 degrees in Display Settings > Camera | crates/ui/src/panels/display_settings.rs (camera) | 165 |
| Focal length | Display settings > Camera | mm; range not documented | done: full-frame equivalent readout | crates/ui/src/panels/display_settings.rs (camera) | 165 |
| Focus distance | Display settings > Camera | disabled unless Post effects + DOF | partial: "Focus" as a multiple of orbit distance, 0.05-4 | crates/ui/src/viewport_bar.rs:137 | - |
| Aperture | Display settings > Camera | disabled unless Post effects + DOF | partial: DOF amount 0-1 instead of an aperture | crates/ui/src/viewport_bar.rs:136 | - |
| Display ratio | Display settings > Camera | "Unspecified" when absent | done: the imported camera's aspect, "Unspecified" when the file has none | crates/ui/src/panels/display_settings.rs (camera) | 217 |
| Lock camera | Display settings > Camera, lock icon | unlocked | done: checkbox in the 3D view corner and Display Settings > Camera | crates/ui/src/view_extras.rs (follow_camera) | 217 |
| Restore (revert modified attributes) | Display settings > Camera | - | done: puts the field of view and projection back | crates/ui/src/view_extras.rs (restore_camera) | 217 |
| Modified-camera marker (italic name with *) | Camera dropdown | - | done | crates/ui/src/view_extras.rs (camera_label) | 217 |
| Camera frame toggle | Display settings > Viewport settings > Camera frame | off (unverified) | done: Display Settings > Visualization > Camera frame | crates/ui/src/viewport.rs (visualization_overlays) | 181 |
| Gate mask opacity | Display settings > Viewport settings > Camera frame | not documented | done | crates/ui/src/viewport.rs (visualization_overlays) | 181 |

## Viewport layouts

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Viewport layout 3D/2D | Contextual toolbar > Viewport Mode; F1 | default | partial: F1 and Viewport menu; no contextual-toolbar dropdown | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Viewport layout 3D only | Contextual toolbar > Viewport Mode; F2 | - | partial: F2 (Painter key set) and Viewport menu; no contextual-toolbar dropdown | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Viewport layout 2D only | Contextual toolbar > Viewport Mode; F3 | - | partial: F3 and Viewport menu; no contextual-toolbar dropdown | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Swap 2D / 3D view | Contextual toolbar > Viewport Mode; F4 | - | partial: F4 and Viewport menu; no contextual-toolbar dropdown | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Undock 3D view to its own window | View menu (7.4) | docked | partial: Window > Undock 3D view floats it in a window inside the main window (not a separate OS window) | crates/ui/src/docks.rs (toggle_undock) | 217 |
| Undock 2D view to its own window | View menu (7.4) | docked | partial: Window > Undock 2D view floats it inside the main window | crates/ui/src/docks.rs (toggle_undock) | 217 |
| Swap while one view is undocked | View menu (7.4) | - | missing | - | - |
| Hide UI (maximise the viewport) | Tab | - | done | crates/ui/src/app.rs (views_only) | #164 |

## Display modes

Layout notes: the dropdown at the top right of each view switches between Material (lit) and solo modes: one entry per channel of the active Texture Set and one per mesh map. C / B cycle the solo lists and M returns to Material.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Display mode dropdown | 3D view, top right | Material | partial: "View" combo sits in the top strip, left side | crates/ui/src/viewport_bar.rs:361 | - |
| Material (lit) mode | Display mode dropdown | default | done | crates/render/src/lib.rs:138 | - |
| Single channel (solo) mode, one entry per Texture Set channel | Display mode dropdown | - | partial: fixed list (Base Color, Normal, Roughness, Metallic, Height, AO); no Emissive, Opacity, Specular or user channels | crates/render/src/lib.rs:153 | - |
| Composited normal (normal + height + bake) solo entry | Display mode dropdown | - | partial: Normal view and a "Baked normal" shading mode, no combined entry named as such | crates/render/src/lib.rs:146 | - |
| Mesh map solo entries | Display mode dropdown | - | done | crates/ui/src/viewport_bar.rs:377 | - |
| Display next channel | C | - | partial: cycles the fixed view-mode list (includes Material and baked-normal modes) | crates/ui/src/app.rs:959 | - |
| Display previous channel | Shift + C | - | partial: same fixed list | crates/ui/src/app.rs:959 | - |
| Display material | M | - | done | crates/ui/src/app.rs:964 | - |
| Display next mesh map | B | - | partial: B in the Painter key set; cycles every map kind, baked or not | crates/app/src/actions.rs (Action::NextMeshMap) | #164 |
| Display previous mesh map | Shift + B | - | missing | - | - |
| Display mask in viewport | Alt + LMB on a mask thumbnail in the layer stack | - | done | crates/render/src/lib.rs:658 | - |
| Baking visualisation mode (low / high poly, cage) | Baking mode viewport panel | - | partial: bake overlay colours shown while the Bake window is open | crates/ui/src/viewport.rs:522 | - |
| Live preview of the map being baked | Settings > General | off (unverified) | missing | - | - |
| Rendering (Iray) mode | F10; Contextual toolbar | off | partial: path-traced Rendering mode, toggle button at the 3D view's bottom right | crates/ui/src/viewport.rs:590 | - |
| Painting mode | F9 | default mode | partial: F9 closes the Bake window and leaves Rendering mode | crates/app/src/actions.rs (Action::PaintingMode) | #164 |
| Baking mode | F8 | - | partial: F8 opens the Bake window rather than a mode | crates/app/src/actions.rs:221 | - |

## Right-click behaviour

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Context menu: floating Properties of the current tool / layer / effect at the cursor | RMB click in the 3D view | editable shortcut "Context menu" | partial: RMB opens a part menu (target kind, part actions, Show all parts, Eyedropper), not Properties | crates/ui/src/viewport.rs:162 | #131 |
| Floating Properties closes on click outside | 3D view | - | missing | - | #131 |
| Extra Polygon Fill blend modes in the right-click panel | RMB in the viewport (12.2) | - | missing | - | - |

## Painting interactions in the viewport

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Use tool / paint | LMB drag | editable shortcut | done | crates/ui/src/viewport.rs:290 | - |
| Draw straight line from the last stroke point | Shift + LMB click | editable shortcut | done | crates/ui/src/viewport.rs:326 | - |
| Dotted preview of the straight line while Shift is held | 3D view overlay | - | missing | - | - |
| Draw straight line with snapping | Ctrl + Shift + LMB | every 5 degrees | partial: snaps every 5 degrees; no dotted preview line yet | crates/ui/src/nav.rs (snap_line_end) | #163 |
| Change tool size (horizontal drag) | Ctrl + RMB drag | editable shortcut | done | crates/ui/src/viewport.rs:147 | - |
| Change tool hardness (vertical drag) | Ctrl + RMB drag | needs an alpha exposing "hardness" | done | crates/ui/src/viewport.rs:148 | - |
| Change tool flow (horizontal drag) | Ctrl + LMB drag | editable shortcut | done | crates/ui/src/viewport.rs:150 | - |
| Change tool rotation (vertical drag) | Ctrl + LMB drag | editable shortcut | done | crates/ui/src/viewport.rs:151 | - |
| Increase tool size | ] | - | done | crates/app/src/actions.rs:206 | - |
| Decrease tool size | [ | - | done | crates/app/src/session.rs:1586 | - |
| Resume previous stroke | hold A | - | missing | - | - |
| Invert grayscale tool value (on masks) | X | - | partial: X in the Painter key set | crates/app/src/actions.rs (Action::InvertMaskGray) | #164 |
| Pick stroke material | P, then click the mesh | returns to the previous tool after picking | partial: P only in the Painter keymap; Eyedropper stays active after the pick; Shift+click picks the material | crates/ui/src/viewport.rs:294 | - |
| Set Clone tool source location | V + LMB | - | done | crates/ui/src/viewport.rs:300 | - |
| Pause engine computation | Shift + Escape | - | missing | - | - |
| Toggle animation (particles) | Space | - | missing | - | - |
| Polygon Fill: rectangular marquee drag | LMB drag with Polygon Fill | - | missing | - | #131 |
| Polygon Fill: extend loop on the other side | Alt while dragging (loop modes) | - | missing | - | - |
| Polygon Fill: primary loop only | Shift (partial loop mode) | - | missing | - | - |
| Polygon Fill: temporary Polygon loop fill | Shift + A | - | missing | - | - |
| Polygon Fill: combine with previous result | A while dragging | - | missing | - | - |
| Polygon Fill: wireframe of the active Texture Set shown while the tool is active | 3D view overlay | - | missing | - | - |
| Polygon Fill: tooltip with polygon counts and flow arrows | 3D view overlay (12.2) | - | missing | - | - |

## Tool cursors and overlays

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Brush cursor: Full preview (brush alpha / material projected) | 3D view; chosen in Contextual toolbar | default (unverified) | missing | - | - |
| Brush cursor: Brush outline | 3D view; chosen in Contextual toolbar | - | partial: screen-space circle of the brush radius, the only style | crates/ui/src/viewport.rs:491 | - |
| Brush cursor: Crosshair | 3D view; chosen in Contextual toolbar | - | missing | - | - |
| Lazy mouse ring around the cursor | 3D view when Lazy mouse is on | grey circle | missing | - | - |
| Clone source marker | 3D view with the Clone tool | - | done | crates/ui/src/viewport.rs:499 | - |
| Fill layer cursor (no brush) | 3D view with a fill layer selected | - | missing | - | - |
| Projection tool: material tiled over the viewport | 3D view with Projection tool | preview channel set in Display settings | missing | - | - |
| Projection preview channel | Display settings > Tool display | not documented | missing | - | - |
| Mesh wireframe overlay | Display settings > Mesh wireframe > Show mesh wireframe | off | done | crates/ui/src/view_extras.rs:112 | - |
| Wireframe color | Display settings > Mesh wireframe | not documented | done | crates/ui/src/view_extras.rs:104 | - |
| Wireframe opacity | Display settings > Mesh wireframe | not documented | partial: range 0.05-1 | crates/ui/src/view_extras.rs:117 | - |
| Show grid | Display settings > Grid | off (unverified) | partial: default on; toggle in the top strip | crates/ui/src/viewport_bar.rs:310 | - |
| Grid axis | Display settings > Grid | Y | partial: per-axis guide checkboxes (X, Y, Z) instead of a plane axis | crates/ui/src/viewport_bar.rs:329 | - |
| Grid color | Display settings > Grid | not documented | missing | - | - |
| Grid opacity | Display settings > Grid | not documented | missing | - | - |

## Stencil manipulation

Layout notes: the stencil is drawn over the whole viewport in screen space. Holding S switches the mouse to stencil editing and shows a Reset button at the top right of the view.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Stencil overlay drawn over the view | 3D view when a stencil is loaded | opacity from Display settings | done | crates/ui/src/viewport.rs:654 | - |
| Stencil rotate | S + LMB drag | - | done | crates/ui/src/viewport.rs:84 | - |
| Stencil snap rotate | Shift + S + LMB drag | every 90 degrees | done | crates/ui/src/viewport.rs (stencil_drag_rotation) | - |
| Stencil translation | S + MMB drag | - | done | crates/ui/src/viewport.rs:88 | - |
| Stencil zoom | S + RMB drag | - | partial: scale clamped to 0.02-8 | crates/ui/src/viewport.rs:92 | - |
| Reset stencil transform button | 3D view, top right while S is held | - | done | crates/ui/src/viewport.rs (reset_stencil) | - |
| Ignore stencil mask | hold N | - | done | crates/ui/src/viewport.rs:79 | - |
| Hide stencil when painting | Display settings > Tool display | not documented | partial: the option sits in the Properties Stencil block, not in Display settings | crates/ui/src/viewport.rs (stencil_overlay_alpha) | - |
| Stencil display opacity | Display settings > Tool display | not documented | partial: the slider sits in the Properties Stencil block, not in Display settings | crates/ui/src/viewport.rs (stencil_overlay_alpha) | - |
| Projection tool transform: rotate | S + LMB drag | - | missing | - | - |
| Projection tool transform: snap rotate | S + Shift + LMB drag | - | missing | - | - |
| Projection tool transform: zoom | S + RMB drag | - | missing | - | - |
| Projection tool transform: translate | S + MMB drag | - | missing | - | - |

## Symmetry in the viewport

Layout notes: display options (plane, intersection, cursor, manipulator) are only reachable from the Symmetry settings button in the Contextual toolbar; the rows for that menu are in toolbar.md. This table covers what is drawn and dragged in the 3D view.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Symmetry on/off | L | off | partial: L toggles mirror X only | crates/app/src/session.rs:1588 | - |
| Mirror plane (transparent) | 3D view (Show Plane) | not documented | partial: guide lines drawn, no translucent plane | crates/ui/src/viewport.rs:375 | - |
| Mirror intersection line on the mesh | 3D view (Show Intersection) | not documented | missing | - | - |
| Mirrored second cursor | 3D view (Show Cursor) | not documented | missing | - | - |
| Hide second cursor while painting | 3D view (Hide While Painting) | not documented | missing | - | - |
| Mirror axis-position manipulator (drag handles) | 3D view | off until enabled | missing | - | - |
| Radial axis line | 3D view (Show Axis) | not documented | partial: drawn as part of the symmetry guides | crates/ui/src/viewport.rs:379 | - |
| Radial axis / mesh intersection point | 3D view (Show Intersection) | not documented | missing | - | - |
| Radial axis-position manipulator | 3D view (Show Axis position manipulator) | off | missing | - | - |

## Quick mask

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Toggle quick mask edition | U | off | missing | - | - |
| Clear quick mask | Y | - | missing | - | - |
| Invert quick mask | I | - | missing | - | - |
| Quick mask overlay tint in the view | 3D view while editing | not documented | missing | - | - |

## Geometry hide / isolate

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Hide / ignore excluded geometry | Alt + H; Contextual toolbar button | off; disabled when nothing is excluded | partial: H over the model hides the part under the cursor, not tied to a layer geometry mask | crates/ui/src/viewport.rs:183 | - |
| Texture Set isolate | Alt + Q | off | partial: I over the model isolates a part; no Texture Set solo key | crates/ui/src/viewport.rs:183 | - |
| Geometry mask editing: click mesh parts in the view to include / exclude | 3D view while the geometry mask icon is active | - | partial: "pick parts" mode toggles the layer per mesh part | crates/ui/src/viewport.rs:264 | - |
| UV Tile hover highlight during geometry mask edit | 3D and 2D views | - | missing | - | - |

## Drag and drop onto the mesh

Layout notes: while dragging over the 3D view Painter highlights the Texture Set (or, with Ctrl, the part) that will receive the drop.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Drop material from Assets onto the mesh: new fill layer on the Texture Set under the cursor | Assets > 3D view | - | done | crates/ui/src/viewport.rs:190 | - |
| Drop smart material onto the mesh | Assets > 3D view | - | done | crates/ui/src/panels/shelf.rs:63 | - |
| Ctrl + drop material: restrict to the mesh part under the cursor (geometry mask) | Assets > 3D view | - | missing | - | - |
| Highlight of the target mesh while dragging | 3D view | - | partial: text label of the action and target near the cursor, no mesh highlight | crates/ui/src/viewport.rs:513 | - |
| Drop alpha / procedural / texture: warp-projected decal layer with channel chooser | Assets > 3D view | menu on release | partial: texture becomes a new UV fill layer; alpha becomes brush tip; no warp decal or channel menu | crates/ui/src/panels/shelf.rs:65 | - |
| Alt + drop material: decal with warp projection | Assets > 3D view | - | partial: Alt replaces the selected fill layer's material instead | crates/ui/src/panels/shelf.rs:60 | - |
| Drop smart mask onto the mesh | Assets > 3D view | - | partial: applied to the selected layer, not as a new layer | crates/ui/src/panels/shelf.rs:64 | - |
| Drop generator or filter onto the mesh | Assets > 3D view (10.x) | - | missing | - | - |
| Drop external material file onto the mesh | OS file manager > 3D view | - | partial: window-level drop imports .spsm smart materials, models and projects only | crates/ui/src/app.rs:1717 | - |
| Drop external SVG onto the mesh: warp layer with Graphic to Material | OS file manager > 3D view | - | missing | - | - |

## Fill projection manipulators (3D)

Layout notes: selecting a fill layer or fill effect with a 3D projection shows a transform gizmo in the 3D view and swaps the Contextual toolbar to manipulator controls (listed in toolbar.md). Tri-planar, Planar, Spherical and Cylindrical share the same gizmo; Warp adds a grid of points.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide manipulator | Q | shown | missing | - | - |
| Increase the manipulator size | + | - | missing | - | - |
| Decrease the manipulator size | - | - | missing | - | - |
| Cycle through manipulation spaces (local / world) | T | - | missing | - | - |
| Translate tool | W | - | missing | - | - |
| Rotate tool | E | - | missing | - | - |
| Scale tool | R | - | missing | - | - |
| Surface tool (Planar and Warp only) | Shift + W | - | missing | - | - |
| Translation: drag one axis | LMB drag on an axis | - | missing | - | - |
| Translation: drag a two-axis plane | LMB drag on a plane handle | - | missing | - | - |
| Translation: drag in camera plane | LMB drag on the centre | - | missing | - | - |
| Translation constrained (grid steps) | Shift + LMB drag | step from Manipulator settings | missing | - | - |
| Rotation around one axis | LMB drag on a ring | - | missing | - | - |
| Free rotation on all axes | LMB drag between the rings | - | missing | - | - |
| Rotation constrained (angle steps) | Shift + LMB drag | step from Manipulator settings | missing | - | - |
| Scale along one axis | LMB drag on an axis handle | - | missing | - | - |
| Scale constrained (steps) | Shift + LMB drag | same step as translation | missing | - | - |
| Surface manipulator: drag to snap onto the mesh | LMB drag (Planar, Warp) | - | missing | - | - |
| Precision mode | Ctrl while dragging a manipulator (10.0) | - | missing | - | - |
| Projection volume / plane preview in the view | 3D view | - | missing | - | - |
| Toggle warp edition mode (Transform warp / Edit vertices) | Shift + V | Transform warp | missing | - | - |
| Warp: select a grid point | LMB click on a point | - | missing | - | - |
| Warp: add to selection | Shift + LMB click | - | missing | - | - |
| Warp: remove from selection | Ctrl + LMB click | - | missing | - | - |
| Warp: rectangle selection | LMB drag on empty space | - | missing | - | - |
| Warp: rectangle add / remove | Shift / Ctrl + LMB drag | - | missing | - | - |
| Warp: deselect | LMB click away | - | missing | - | - |
| Warp: quick-move a point on the surface | LMB drag a point | - | missing | - | - |
| Warp: quick-move a point in camera space | Ctrl + LMB drag a point | - | missing | - | - |
| Warp: edit tangents of selected points | Rotation / Scale manipulator on a point | - | missing | - | - |
| Warp: projection-depth arrows per grid point | 3D view | green arrows | missing | - | - |
| Warp: cancel a split tool | Escape | - | missing | - | - |

## Path tool in the viewport

Layout notes: the Path tool works only in the 3D view. A floating Paths panel inside the viewport lists the paths of the selected paint layer / effect.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add a point on the mesh surface | LMB click with a Path tool | needs 2+ points | partial: Pen tool anchors are edited in UV space (2D view) and projected | crates/ui/src/viewport.rs:281 | - |
| Insert a point on an existing path | LMB click on the curve | - | missing | - | - |
| Move a point | LMB drag a point | - | partial: drag on the surface in 3D | crates/ui/src/viewport.rs:286 | - |
| Exit path edition | Escape or Enter | - | done | crates/ui/src/pen.rs:52 | - |
| Start a new path | LMB click on blank surface after exiting | - | done | crates/ui/src/viewport.rs:281 | - |
| Select an existing path | LMB click on a path in the view | - | missing | - | - |
| Toggle corner / smooth on a point | Double-click a point | - | missing | - | - |
| Break tangents while moving a handle | Alt | - | missing | - | - |
| Scale both tangents together | Ctrl | - | missing | - | - |
| Straight segment between points | Shift (11.0) | - | missing | - | - |
| Angle snapping when adding a point | Ctrl (11.0) | angle set in the path settings menu | missing | - | - |
| Auto-close by clicking the first point | LMB on first vertex with the last selected (11.0) | Ctrl selects instead | missing | - | - |
| Rectangle selection of points | LMB drag | - | missing | - | - |
| Select all points | Ctrl + A | - | missing | - | - |
| Delete selected points | Delete | - | done | crates/ui/src/pen.rs:60 | - |
| Delete path | Backspace (11.0) | - | partial: Backspace deletes the selected anchor only | crates/ui/src/pen.rs:60 | - |
| Path preview (dotted next segment) | Shift + P toggles (11.0) | on | missing | - | - |
| Manipulators on path points | W, S, D toggle them (11.0) | - | missing | - | - |
| Paths panel: list of paths | Floating panel in the viewport | shown with a Path tool | missing | - | - |
| Paths panel: rename | Double-click a path | - | missing | - | - |
| Paths panel: delete | Delete key | - | missing | - | - |
| Paths panel: copy | Ctrl + C | - | missing | - | - |
| Paths panel: paste | Ctrl + V | - | missing | - | - |
| Paths panel: duplicate | Ctrl + D | - | missing | - | - |
| Paths panel: show / hide path | Eye icon | - | missing | - | - |
| Paths panel: right-click menu with the same actions | RMB on a path | - | missing | - | - |
| Paths panel: copy / paste properties | RMB menu | same tool type only | missing | - | - |
| Paths panel: copy / paste position (all vertices) | RMB menu | - | missing | - | - |
| Hover feedback from the Paths panel in the view | 3D view | - | missing | - | - |
| Message when using a Path tool in the 2D view | 2D view | - | missing | - | - |

## Animation playback

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Toggle animation (pause / resume particle simulation) | Space | playing | missing | - | - |

## SpaceMouse (3Dconnexion)

Layout notes: all settings below live in the 3Dconnexion driver panel, not in Painter; Painter supplies the default profile and a pivot icon drawn in the view.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| SpaceMouse navigation in the 3D view (rotate, pan, zoom) | 3D view (7.4.2+) | plug and play | missing | - | - |
| SpaceMouse navigation in the 2D view | 2D view | - | missing | - | - |
| Speed slider | 3Dconnexion settings | not documented | missing | - | - |
| Object Mode | 3Dconnexion > Advanced > Navigation modes | default | missing | - | - |
| Camera Mode | 3Dconnexion > Advanced > Navigation modes | - | missing | - | - |
| Target Camera Mode | 3Dconnexion > Advanced > Navigation modes | - | missing | - | - |
| Helicopter Mode | 3Dconnexion > Advanced > Navigation modes | - | missing | - | - |
| Lock Horizon | 3Dconnexion > Advanced > Navigation modes | on | missing | - | - |
| Rotation center: Auto | 3Dconnexion > Advanced > Rotation center | not documented | missing | - | - |
| Rotation center: Always Show | 3Dconnexion > Advanced > Rotation center | - | missing | - | - |
| Rotation center: Show on Motion | 3Dconnexion > Advanced > Rotation center | default | missing | - | - |
| Rotation center: Hide | 3Dconnexion > Advanced > Rotation center | - | missing | - | - |
| Button assignment (commands, macros, radial menus) | 3Dconnexion > Buttons | - | missing | - | - |
| Radial menu / Menu button opens properties | Device | - | missing | - | - |

## SubCraft-only viewport features (no Painter counterpart, not counted)

Layout notes: these exist in SubCraft and have no row above because Painter has nothing equivalent: camera bookmarks (crates/ui/src/view_extras.rs:131), the bottom-left hints list (crates/ui/src/viewport_hints.rs:72), the top-right symmetry status badge (crates/ui/src/viewport.rs:545), Esc to show all hidden parts (crates/ui/src/viewport.rs:186), on-model handles for text layers (crates/ui/src/viewport.rs:791), a Blender navigation scheme (crates/ui/src/nav.rs:16), Shift+MMB to turn only the key light (crates/ui/src/nav.rs:78), and a render-resolution scale (crates/ui/src/viewport_bar.rs:347).
