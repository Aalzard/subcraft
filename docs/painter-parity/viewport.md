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
| Progress bar for engine computations | Viewport, bottom edge | hidden when idle | done: a thin moving bar along the top of the 3D view while layers are computed | crates/ui/src/viewport.rs (computing_bar) | 249 |
| Colour-profile name text | 3D view, top left | shown when a profile is loaded | missing | - | - |
| Grid unit read-out | 3D view, bottom left | shown when the grid is on | done: bottom left whenever the grid is on, just above the viewport hints when they show | crates/ui/src/viewport.rs (grid_readout_anchor) | 391 |
| Axis indicator (X/Y/Z orientation) | 3D view, bottom right | shown | done: red X, green Y, blue Z above the Render toggle, far axes dimmed; Display Settings > Viewport > Axis indicator | crates/ui/src/viewport.rs (axis_indicator) | 261 |
| Warning when painting on a non-active Texture Set | 3D view overlay (12.1) | not documented | done: a status message naming the set the stroke started on, with how to select it | crates/ui/src/viewport.rs | 258 |

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
| Auto-focus (depth of field focus point) | Ctrl + MMB click on the mesh | needs Post effects + DOF on | done: Display Settings > Camera > Auto-focus keeps the focus on the centre of the view; Ctrl+MMB picks a point | crates/ui/src/viewport.rs | 249 |
| Texture set selection | Ctrl + Alt + RMB click on the mesh | editable shortcut | done: Ctrl+Alt+right-click selects the texture set of the part under the cursor (Painter navigation) | crates/ui/src/viewport.rs (pick_set) | 163 |
| Texture Set highlight under cursor | Hold Ctrl + Alt over the model (12.2) | hidden while LMB is down or panning | done: while Ctrl+Alt is held, not while painting or navigating; a warm tint | crates/ui/src/viewport.rs, crates/render/src/viewport.wgsl | 258 |
| Texture Set / UV Tile highlight from the list | Hover a row in the Texture Set list (12.2) | not documented | partial: hovering a Texture Set List row highlights its set; no UV tile highlight | crates/ui/src/panels/texture_sets.rs (set_rows) | 258 |
| Tablet scroll | Ctrl + Alt + click drag scrolls panels (2019.3) | not documented | missing | - | - |

## Camera commands and projection

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Center the whole mesh | F | focuses only the view under the cursor; both when the cursor is outside | done: F frames the view under the cursor, both views when it is elsewhere | crates/ui/src/app.rs (frame_targets) | 249 |
| Focus on the selected path | F while editing a path (11.0) | not documented | done: with the Pen tool, F centres the 3D view on the path (same direction, closer) and frames its UV rectangle in the 2D view | crates/ui/src/orbit.rs (focus_on), crates/ui/src/panels/uv_view.rs (uv_frame) | 265 |
| Camera Perspective | F5 | default mode | done | crates/app/src/actions.rs:210 | - |
| Camera Orthographic | F6 | off | done | crates/app/src/actions.rs:211 | - |
| Free rotation (roll allowed) | Contextual toolbar > Camera Rotation Mode | off | done: Camera menu or Display Settings > Camera > Camera rotation: Free orbits about the view's own axes (rolls, goes over the top); Constrained levels the horizon again | crates/ui/src/orbit.rs (free_orbit) | 258 |
| Constrained rotation (no roll) | Contextual toolbar > Camera Rotation Mode | default | done | crates/ui/src/orbit.rs:114 | - |
| Light the mesh from underneath | Orbit below the horizon (7.x) | not documented | done: Shift+MMB drag up / down tilts the key light, down to below the horizon | crates/ui/src/viewport.rs (lit_direction) | 249 |

## Camera management

Layout notes: imported cameras come from the mesh file (FBX / Alembic) at project creation or re-import. They appear in a dropdown at the top right of the 3D view (hidden when only the default camera exists). Their attributes live in Display settings > Camera, with a Preset dropdown, a lock icon and a Restore button.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Import Cameras checkbox | New project window | on (unverified) | done: New project > Cameras, on by default; glTF, GLB and FBX files carry cameras | crates/ui/src/new_project.rs, crates/app/src/session.rs (import_mesh_cameras) | 217 |
| Import cameras on mesh re-import | Edit > Project configuration | off unless toggled | done: "Import cameras" in File > Reload mesh, off by default | crates/ui/src/mesh_reload.rs | 217 |
| Imported data: name, transform, FOV, aspect ratio | from FBX / ABC mesh file | - | done: from glTF / GLB and FBX mesh files (and separate camera files); saved with the project; the roll comes through too (#412) | crates/mesh/src/camera.rs (cameras_in_mesh_file), crates/doc/src/camera.rs | 217 |
| Camera selection dropdown | 3D view, top right | "Default camera"; hidden if no other camera | done: 3D view, top right, hidden when the project has no imported cameras; also Display Settings > Camera | crates/ui/src/view_extras.rs (camera_dropdown) | 217 |
| Default camera entry | Camera dropdown | selected, perspective | done: first entry; choosing it brings back the view from before | crates/ui/src/view_extras.rs (select_camera) | 217 |
| Navigating while an imported camera is active switches to Default camera | 3D view | always | done: unless the camera is locked | crates/ui/src/view_extras.rs (follow_camera) | 217 |
| Select next camera shortcut | Settings > Shortcuts | keys not documented | done: Alt+] (Painter documents no key) | crates/app/src/actions.rs (Action::NextCamera) | 217 |
| Select previous camera shortcut | Settings > Shortcuts | keys not documented | done: Alt+[ (Painter documents no key) | crates/app/src/actions.rs (Action::PrevCamera) | 217 |
| Camera Preset dropdown | Display settings > Camera | active camera | done | crates/ui/src/panels/display_settings.rs (camera) | 217 |
| Field of view | Display settings > Camera | degrees; range not documented | done: 5-120 degrees in Display Settings > Camera | crates/ui/src/panels/display_settings.rs (camera) | 165 |
| Focal length | Display settings > Camera | mm; range not documented | done: full-frame equivalent readout | crates/ui/src/panels/display_settings.rs (camera) | 165 |
| Focus distance | Display settings > Camera | disabled unless Post effects + DOF | done: a distance from the camera in scene units that stays put while zooming; set by the slider, Ctrl + middle click, focus picking and auto-focus | crates/doc/src/display.rs (focus_distance) | 397 |
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
| Viewport layout 3D/2D | Contextual toolbar > Viewport Mode; F1 | default | done: F1, Viewport menu and the contextual toolbar's Viewport Mode dropdown | crates/ui/src/viewport_bar.rs (viewport_mode) | 365 |
| Viewport layout 3D only | Contextual toolbar > Viewport Mode; F2 | - | done: F2 (Painter key set), Viewport menu and the Viewport Mode dropdown | crates/ui/src/viewport_bar.rs (viewport_mode) | 365 |
| Viewport layout 2D only | Contextual toolbar > Viewport Mode; F3 | - | done: F3, Viewport menu and the Viewport Mode dropdown | crates/ui/src/viewport_bar.rs (viewport_mode) | 365 |
| Swap 2D / 3D view | Contextual toolbar > Viewport Mode; F4 | - | done: F4, Viewport menu and the Viewport Mode dropdown | crates/ui/src/viewport_bar.rs (viewport_mode) | 365 |
| Undock 3D view to its own window | View menu (7.4) | docked | partial: Window > Undock 3D view floats it in a window inside the main window (not a separate OS window) | crates/ui/src/docks.rs (toggle_undock) | 217 |
| Undock 2D view to its own window | View menu (7.4) | docked | partial: Window > Undock 2D view floats it inside the main window | crates/ui/src/docks.rs (toggle_undock) | 217 |
| Swap while one view is undocked | View menu (7.4) | - | done: F4, the Viewport menu or the Viewport Mode dropdown puts the other view in the floating window; other panels stay where they are | crates/ui/src/app.rs (apply_view_layout) | 392 |
| Hide UI (maximise the viewport) | Tab | - | done | crates/ui/src/app.rs (views_only) | #164 |

## Display modes

Layout notes: the dropdown at the top right of each view switches between Material (lit) and solo modes: one entry per channel of the active Texture Set and one per mesh map. C / B cycle the solo lists and M returns to Material.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Display mode dropdown | 3D view, top right | Material | done: at the 3D view top right (above the camera dropdown); also in Display Settings | crates/ui/src/viewport.rs (display_corner) | 365 |
| Material (lit) mode | Display mode dropdown | default | done | crates/render/src/lib.rs:138 | - |
| Single channel (solo) mode, one entry per Texture Set channel | Display mode dropdown | - | done: every channel of the texture set (#181) | crates/ui/src/viewport_bar.rs (view_mode) | 181 |
| Composited normal (normal + height + bake) solo entry | Display mode dropdown | - | partial: "Normal + Height" shows the tangent-space normal the shading uses (Normal channel with the Height bump); the baked mesh normal is not layered in, as SubCraft's material shading does not use it | crates/render/src/viewport.wgsl (combined_normal) | 374 |
| Mesh map solo entries | Display mode dropdown | - | done | crates/ui/src/viewport_bar.rs:377 | - |
| Display next channel | C | - | done: C cycles the material and each channel of the selected texture set (#181) | crates/ui/src/viewport_bar.rs (cycle_channel) | 181 |
| Display previous channel | Shift + C | - | done: Shift+C (#181) | crates/ui/src/viewport_bar.rs (cycle_channel) | 181 |
| Display material | M | - | done | crates/ui/src/app.rs:964 | - |
| Display next mesh map | B | - | partial: B in the Painter key set; cycles every map kind, baked or not | crates/app/src/actions.rs (Action::NextMeshMap) | #164 |
| Display previous mesh map | Shift + B | - | done: Shift+B in the Painter key set | crates/ui/src/viewport_bar.rs (cycle_mesh_map_back) | - |
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
| Dotted preview of the straight line while Shift is held | 3D view overlay | - | done: from the last stroke point, snapped with Ctrl exactly as the click draws it | crates/ui/src/viewport.rs (straight_line_end) | 261 |
| Draw straight line with snapping | Ctrl + Shift + LMB | every 5 degrees | done: snaps every 5 degrees, with the dotted preview | crates/ui/src/nav.rs (snap_line_end) | #163, 261 |
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
| Pause engine computation | Shift + Escape | - | done: layer computation waits and the 3D view says so until Shift+Esc again; export and save still compute | crates/app/src/session.rs (set_engine_paused) | 395 |
| Toggle animation (particles) | Space | - | missing | - | - |
| Polygon Fill: rectangular marquee drag | LMB drag with Polygon Fill | - | done | crates/ui/src/viewport.rs | #131 |
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
| Brush cursor: Brush outline | 3D view; chosen in Contextual toolbar | - | done: screen-space circle of the brush radius (default); Display Settings > Viewport > Brush cursor | crates/ui/src/viewport.rs (brush_mark) | 265 |
| Brush cursor: Crosshair | 3D view; chosen in Contextual toolbar | - | done: Display Settings > Viewport > Brush cursor > Crosshair (also for the symmetry cursors); not in the top strip | crates/ui/src/viewport.rs (brush_mark) | 265 |
| Lazy mouse ring around the cursor | 3D view when Lazy mouse is on | grey circle | done: grey ring of the lazy distance around the cursor | crates/ui/src/viewport.rs (overlays) | 261 |
| Clone source marker | 3D view with the Clone tool | - | done | crates/ui/src/viewport.rs:499 | - |
| Fill layer cursor (no brush) | 3D view with a fill layer selected | - | missing | - | - |
| Projection tool: material tiled over the viewport | 3D view with Projection tool | preview channel set in Display settings | missing | - | - |
| Projection preview channel | Display settings > Tool display | not documented | missing | - | - |
| Mesh wireframe overlay | Display settings > Mesh wireframe > Show mesh wireframe | off | done | crates/ui/src/view_extras.rs:112 | - |
| Wireframe color | Display settings > Mesh wireframe | not documented | done | crates/ui/src/view_extras.rs:104 | - |
| Wireframe opacity | Display settings > Mesh wireframe | not documented | partial: range 0.05-1 | crates/ui/src/view_extras.rs:117 | - |
| Show grid | Display settings > Grid | off (unverified) | partial: default on; toggle in the top strip | crates/ui/src/viewport_bar.rs:310 | - |
| Grid axis | Display settings > Grid | Y | done: Display Settings > Grid > Axis picks the plane the grid lies in (Y is the floor) | crates/ui/src/panels/display_settings.rs (grid_plane) | 181 |
| Grid color | Display settings > Grid | not documented | done | crates/ui/src/panels/display_settings.rs (grid_color) | 181 |
| Grid opacity | Display settings > Grid | not documented | done | crates/ui/src/panels/display_settings.rs (grid_opacity) | 181 |

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
| Hide stencil when painting | Display settings > Tool display | not documented | done: Display Settings > Tool display (the same setting as the Properties stencil block) | crates/ui/src/panels/display_settings.rs (Tool display) | 181 |
| Stencil display opacity | Display settings > Tool display | not documented | done: Display Settings > Tool display (the same setting as the Properties stencil block) | crates/ui/src/panels/display_settings.rs (Tool display) | 181 |
| Projection tool transform: rotate | S + LMB drag | - | done: turns the pattern about the view centre | crates/ui/src/viewport.rs (projection_rotate) | PRNUM |
| Projection tool transform: snap rotate | S + Shift + LMB drag | - | done: quarter turns, as the stencil | crates/ui/src/viewport.rs (projection_input) | PRNUM |
| Projection tool transform: zoom | S + RMB drag | - | done: scales the pattern about the view centre (1/50 to 50 tiles) | crates/ui/src/viewport.rs (projection_scale) | PRNUM |
| Projection tool transform: translate | S + MMB drag | - | done: the pattern follows the mouse | crates/ui/src/viewport.rs (projection_move) | PRNUM |

## Symmetry in the viewport

Layout notes: display options (plane, intersection, cursor, manipulator) are only reachable from the Symmetry settings button in the Contextual toolbar; the rows for that menu are in toolbar.md. This table covers what is drawn and dragged in the 3D view.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Symmetry on/off | L | off | done: L turns symmetry off and back on with the last axes (Action::ToggleSymmetry); Shift+X keeps the X-mirror toggle in the Painter key set | crates/app/src/actions.rs (ToggleSymmetry) | 363 |
| Mirror plane (transparent) | 3D view (Show Plane) | not documented | done: each active mirror plane as a translucent sheet in its axis colour, 10 % past the model, at the axis position; Display Settings > Viewport > Mirror planes, on by default | crates/ui/src/viewport.rs (mirror_plane_corners) | 264 |
| Mirror intersection line on the mesh | 3D view (Show Intersection) | not documented | done: the symmetry guides draw where each mirror plane cuts the mesh, at the axis position (Display Settings > Viewport > Symmetry guides) | crates/app/src/guides.rs (symmetry_sections) | 264 |
| Mirrored second cursor | 3D view (Show Cursor) | not documented | done: one per mirror / radial copy (up to 16), faint when the copy is behind the model, following moved planes and flipped copies; Display Settings > Viewport > Symmetry cursors, on by default | crates/ui/src/viewport.rs (symmetry_cursors) | 262 |
| Hide second cursor while painting | 3D view (Hide While Painting) | not documented | done: Display Settings > Viewport > Hide while painting, off by default | crates/ui/src/viewport_bar.rs (symmetry_cursor_hide_painting) | 262 |
| Mirror axis-position manipulator (drag handles) | 3D view | off until enabled | done: a handle per mirror plane moves it along its axis; the radial pivot moves across its plane | crates/ui/src/symmetry_view.rs (manipulate) | - |
| Radial axis line | 3D view (Show Axis) | not documented | done: own Show Axis toggle | crates/app/src/guides.rs (radial_guides) | - |
| Radial axis / mesh intersection point | 3D view (Show Intersection) | not documented | missing | - | - |
| Radial axis-position manipulator | 3D view (Show Axis position manipulator) | off | missing | - | - |

## Quick mask

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Toggle quick mask edition | U | off | done: also the selection button on the tool rail | crates/app/src/session.rs | - |
| Clear quick mask | Y | - | done | crates/app/src/session.rs | - |
| Invert quick mask | I | - | done (without a quick mask, I still isolates the part under the cursor) | crates/app/src/session.rs | - |
| Quick mask overlay tint in the view | 3D view while editing | not documented | done: while editing, Base Color is uploaded tinted red where the mask protects, so the 3D and 2D views both show it (tile by tile); the 2D view keeps its light overlay once applied | crates/app/src/quick_mask_tint.rs, crates/ui/src/state.rs (sync_gpu) | 260 |

## Geometry hide / isolate

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Hide / ignore excluded geometry | Alt + H; Contextual toolbar button | off; disabled when nothing is excluded | done | crates/app/src/part_workflow.rs | - |
| Texture Set isolate | Alt + Q | off | done: Alt+Q hides the other texture sets' geometry, again shows everything (Action::IsolateTextureSet); I still isolates the part under the cursor | crates/app/src/actions.rs (IsolateTextureSet) | 164 |
| Geometry mask editing: click mesh parts in the view to include / exclude | 3D view while the geometry mask icon is active | - | done: click toggles, a box adds, Ctrl+box removes, in the 3D and 2D views | crates/ui/src/viewport.rs | - |
| UV Tile hover highlight during geometry mask edit | 3D and 2D views | - | missing | - | - |

## Drag and drop onto the mesh

Layout notes: while dragging over the 3D view Painter highlights the Texture Set (or, with Ctrl, the part) that will receive the drop.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Drop material from Assets onto the mesh: new fill layer on the Texture Set under the cursor | Assets > 3D view | - | done | crates/ui/src/viewport.rs:190 | - |
| Drop smart material onto the mesh | Assets > 3D view | - | done | crates/ui/src/panels/shelf.rs:63 | - |
| Ctrl + drop material: restrict to the mesh part under the cursor (geometry mask) | Assets > 3D view | - | done: uses the part kind chosen in Parts | crates/ui/src/viewport.rs | - |
| Highlight of the target mesh while dragging | 3D view | - | done: the texture set under the cursor is highlighted (renderer slot highlight); with Ctrl, the part is outlined, hidden edges left out | crates/ui/src/viewport.rs (drop_outline), crates/app/src/region_outline.rs | 260 |
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
| Toggle warp edition mode (Transform warp / Edit vertices) | Shift + V | Transform warp | done: Shift+V over the 3D view | crates/ui/src/warp_view.rs | - |
| Warp: select a grid point | LMB click on a point | - | done | crates/ui/src/warp_view.rs | - |
| Warp: add to selection | Shift + LMB click | - | done | crates/ui/src/warp_view.rs | - |
| Warp: remove from selection | Ctrl + LMB click | - | done | crates/ui/src/warp_view.rs | - |
| Warp: rectangle selection | LMB drag on empty space | - | done | crates/ui/src/warp_view.rs | - |
| Warp: rectangle add / remove | Shift / Ctrl + LMB drag | - | done | crates/ui/src/warp_view.rs | - |
| Warp: deselect | LMB click away | - | done | crates/ui/src/warp_view.rs | - |
| Warp: quick-move a point on the surface | LMB drag a point | - | done | crates/ui/src/warp_view.rs | - |
| Warp: quick-move a point in camera space | Ctrl + LMB drag a point | - | done: Ctrl+drag, in the view plane | crates/ui/src/warp_view.rs | - |
| Warp: edit tangents of selected points | Rotation / Scale manipulator on a point | - | missing | - | - |
| Warp: projection-depth arrows per grid point | 3D view | green arrows | missing | - | - |
| Warp: cancel a split tool | Escape | - | missing | - | - |

## Path tool in the viewport

Layout notes: the Path tool works only in the 3D view. A floating Paths panel inside the viewport lists the paths of the selected paint layer / effect.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add a point on the mesh surface | LMB click with a Path tool | needs 2+ points | partial: Pen tool anchors are edited in UV space (2D view) and projected | crates/ui/src/viewport.rs:281 | - |
| Insert a point on an existing path | LMB click on the curve | - | done: click on the curve; the curve keeps its shape (de Casteljau split) | crates/doc/src/pen.rs (split_segment), crates/ui/src/viewport.rs (Pen tool) | - |
| Move a point | LMB drag a point | - | partial: drag on the surface in 3D | crates/ui/src/viewport.rs:286 | - |
| Exit path edition | Escape or Enter | - | done | crates/ui/src/pen.rs:52 | - |
| Start a new path | LMB click on blank surface after exiting | - | done | crates/ui/src/viewport.rs:281 | - |
| Select an existing path | LMB click on a path in the view | - | done: click on another path's curve selects its layer | crates/ui/src/viewport.rs (Pen tool) | - |
| Toggle corner / smooth on a point | Double-click a point | - | done | crates/ui/src/viewport.rs (Pen tool) | - |
| Break tangents while moving a handle | Alt | - | partial: Shift (Alt navigates the camera in SubCraft's 3D view) | crates/ui/src/viewport.rs (Pen tool) | - |
| Scale both tangents together | Ctrl | - | done: Ctrl while dragging a handle; the other tangent keeps its direction | crates/app/src/pen.rs (drag_to_scaled), crates/ui/src/viewport.rs (Pen tool) | - |
| Straight segment between points | Shift (11.0) | - | done: Shift+click adds a point without tangents | crates/app/src/pen.rs (keep_new_point_straight), crates/ui/src/viewport.rs (Pen tool) | - |
| Angle snapping when adding a point | Ctrl (11.0) | angle set in the path settings menu | done: Ctrl, 15 degree steps on screen (no setting yet) | crates/ui/src/viewport.rs (Pen tool) (snap_angle) | - |
| Auto-close by clicking the first point | LMB on first vertex with the last selected (11.0) | Ctrl selects instead | done: with the last point selected and 3+ points; Ctrl skips it | crates/app/src/pen.rs (close_path), crates/ui/src/viewport.rs (Pen tool) | - |
| Rectangle selection of points | LMB drag | - | missing | - | - |
| Select all points | Ctrl + A | - | missing | - | - |
| Delete selected points | Delete | - | done | crates/ui/src/pen.rs:60 | - |
| Delete path | Backspace (11.0) | - | done: Backspace deletes the selected path's layer; Delete removes the selected point | crates/ui/src/pen.rs (keyboard) | - |
| Path preview (dotted next segment) | Shift + P toggles (11.0) | on | done: Shift+P toggles it; follows the Ctrl snap | crates/ui/src/viewport.rs (Pen tool) | - |
| Manipulators on path points | W, S, D toggle them (11.0) | - | missing | - | - |
| Paths panel: list of paths | Floating panel in the viewport | shown with a Path tool | done: floating at the top left of the 3D view with the Pen tool; each path is its own layer, so it lists the texture set's pen-path layers | crates/ui/src/paths_panel.rs | - |
| Paths panel: rename | Double-click a path | - | done | crates/ui/src/paths_panel.rs | - |
| Paths panel: delete | Delete key | - | partial: right-click > Delete (the Delete key removes the selected point) | crates/ui/src/paths_panel.rs | - |
| Paths panel: copy | Ctrl + C | - | partial: right-click > Copy properties / Copy position, no Ctrl+C | crates/ui/src/paths_panel.rs | - |
| Paths panel: paste | Ctrl + V | - | partial: right-click > Paste properties / Paste position, no Ctrl+V | crates/ui/src/paths_panel.rs | - |
| Paths panel: duplicate | Ctrl + D | - | done: right-click > Duplicate; Ctrl+D duplicates the selected path layer | crates/ui/src/paths_panel.rs | - |
| Paths panel: show / hide path | Eye icon | - | done | crates/ui/src/paths_panel.rs | - |
| Paths panel: right-click menu with the same actions | RMB on a path | - | done | crates/ui/src/paths_panel.rs | - |
| Paths panel: copy / paste properties | RMB menu | same tool type only | done: width, hardness, opacity, values, type and stamp; the points stay | crates/ui/src/paths_panel.rs | - |
| Paths panel: copy / paste position (all vertices) | RMB menu | - | done: the points (fresh ids) and open / closed; the style stays | crates/ui/src/paths_panel.rs | - |
| Hover feedback from the Paths panel in the view | 3D view | - | done: the hovered row's path is drawn over the view | crates/ui/src/paths_panel.rs (hovered), crates/ui/src/viewport.rs (Pen tool) | - |
| Message when using a Path tool in the 2D view | 2D view | - | n/a: SubCraft's Pen tool also edits paths in the 2D view | crates/ui/src/pen.rs (uv_input) | - |

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
