# Painter parity: Toolbars and sliders

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/toolbars, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/interface, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/miscellaneous/sliders, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/viewport, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/geometry-mask, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/painting, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/tool-list, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/paint-brush, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/eraser, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/polygon-fill, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/smudge-tool, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/clone-tool, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/quick-mask, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/path, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/ribbon-tool, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/filled-path, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/lazy-mouse, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/symmetry, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/mirror-symmetry, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/radial-symmetry, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/tri-planar-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/planar-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/spherical-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/cylindrical-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/warp-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/uv-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-8-3, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-11-0, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-12-0, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2019-3
Checked against: origin/main 297dcb3 (2026-11-11)

## Tools toolbar

Layout notes: a narrow vertical strip, top left of the main window by default; it can only be docked vertically on the left or right edge. Tools are listed top to bottom in the order below and are usable only while a paint layer (or paint effect / mask) is selected. Tools with a second "Physical" mode expose it from the same button (and through Ctrl + the tool's number); picking a particle brush preset in Assets also switches to it. The Path button opens a small menu of path types. SubCraft has a fixed left rail (crates/ui/src/app.rs:1056) with Brush, Eraser, Polygon fill, Clone, Smudge, Eyedropper and Pen, then a mask-target toggle, a reset-camera button and a shortcuts button; these last three have no Painter counterpart on this toolbar.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Paint | Tools toolbar; 1 | selected by default | done | crates/ui/src/app.rs:1063 | - |
| Physical paint (particles) | Tools toolbar, Paint second mode; Ctrl + 1 | - | missing | - | - |
| Eraser | Tools toolbar; 2 | - | done | crates/ui/src/app.rs:1064 | - |
| Physical eraser (particles) | Tools toolbar, Eraser second mode; Ctrl + 2 | - | missing | - | - |
| Path tools button (opens the path type menu) | Tools toolbar, third icon | no default shortcut ("Select paint along path tool" is unbound) | partial: single Pen tool, last in the rail; P by default | crates/ui/src/app.rs:1069 | - |
| Paint along path | Path tools menu | - | partial: Pen paths are edited in UV space with their own stroke style, not the brush engine on the 3D surface | crates/app/src/pen.rs:1 | - |
| Ribbon path | Path tools menu | - | partial: Path type dropdown in the pen Properties, no Path tools menu | crates/ui/src/pen.rs | - |
| Filled path | Path tools menu | - | missing | - | - |
| Erase along path | Path tools menu | - | missing | - | - |
| Smudge along path | Path tools menu | - | missing | - | - |
| Projection | Tools toolbar; 3 | - | missing | - | - |
| Physical projection (particles) | Tools toolbar, Projection second mode; Ctrl + 3 | - | missing | - | - |
| Polygon Fill | Tools toolbar; 4 | - | done | crates/ui/src/app.rs:1065 | - |
| Smudge | Tools toolbar; 5 | - | partial: placed after Clone in the rail | crates/ui/src/app.rs:1067 | - |
| Clone (relative source) | Tools toolbar; 6 | source follows after each stroke | partial: one clone mode only | crates/ui/src/app.rs:1066 | - |
| Clone (absolute source) | Tools toolbar, Clone second mode; Ctrl + 6 | - | missing | - | - |
| Material picker (temporary tool, returns to the previous tool) | Tools toolbar; P | - | partial: Eyedropper stays selected after the pick; P only in the Painter keymap | crates/ui/src/app.rs:1068 | - |
| Quick mask | Tool list entry; U | off | done: selection button on the left rail | crates/ui/src/app.rs | - |
| Tools disabled unless a paint layer / paint effect is selected | Tools toolbar | - | partial: tools stay enabled and a paint layer is created on the first stroke | crates/ui/src/viewport.rs:318 | - |
| Dock the toolbar on the left or right edge | Drag the toolbar | left | partial: fixed on the left | crates/ui/src/app.rs:1056 | - |
| Send to (Stager / Photoshop) icon | Tools toolbar, below the tools | - | n/a | - | - |

## Docks toolbar

Layout notes: a narrow vertical strip at the top right of the main window, dockable only on the left or right edge. It shows one button per dock window that is currently closed. Clicking a button pops the dock out next to its button, floating over the interface; clicking again closes it. If the popped dock is dragged away it becomes a normal floating window that can be docked, and its button leaves the toolbar until the window is closed again. SubCraft has no such toolbar; every panel is a permanent egui_dock tab (crates/ui/src/app.rs:189).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Docks toolbar strip | Main window, top right | shown | done: header bar, right side, beside Bake, in windows at least 1400 px wide (narrower ones reopen panels from Window > Views) | crates/ui/src/docks.rs (toolbar) | 217 |
| Button per closed dock window | Docks toolbar | - | done: Display settings, History, Properties, Layers, Texture Set List and Settings, Assets, Parts | crates/ui/src/docks.rs (DOCKABLE) | 217 |
| Click a button: dock floats next to the button | Docks toolbar | - | done | crates/ui/src/docks.rs (float) | 217 |
| Click the button again: closes the dock | Docks toolbar | - | done | crates/ui/src/docks.rs (toolbar) | 217 |
| Drag the popped dock away: becomes a regular floating / dockable window | Docks toolbar | - | done: drag its tab into the layout | crates/ui/src/docks.rs | 217 |
| Closing a window returns its button to the toolbar | Docks toolbar | - | done: every panel but the 3D and 2D views has a close button | crates/ui/src/docks.rs (closed) | 217 |
| Display settings button | Docks toolbar | present by default (unverified) | done | crates/ui/src/docks.rs | 217 |
| Shader settings button | Docks toolbar | present by default (unverified) | done | crates/ui/src/docks.rs | 217 |
| History button | Docks toolbar | present by default (unverified) | done | crates/ui/src/docks.rs | 217 |
| Log button | Docks toolbar | present by default (unverified) | missing | - | - |
| Properties reachable from the Docks toolbar | Docks toolbar | when the Properties dock is closed | done | crates/ui/src/docks.rs | 217 |
| Dock the toolbar on the left or right edge | Drag the toolbar | right | missing | - | - |

## Plugins toolbar

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Plugins toolbar strip | Main window | shown when a plugin adds a button | missing | - | - |
| One button per installed and enabled scripting plugin | Plugins toolbar | - | missing | - | - |

## Contextual toolbar: painting tools (left side)

Layout notes: the Contextual toolbar is fixed above the viewports and cannot be moved. Its left part changes with the selected tool; with Paint, Eraser, Projection, Smudge and Clone it shows, left to right, the numeric brush shortcuts (Size, Flow, Stroke opacity, Spacing), then the Lazy mouse button with its Distance, the Symmetry button with its settings button, and tool-display helpers. SubCraft's strip (crates/ui/src/viewport_bar.rs:185) starts with view controls (Resolution, Anti-aliasing, Ortho, View, grid, wireframe, camera) and then shows a combined Symmetry and Lazy Mouse button, Size, Flow, Opacity and Hardness, with a second row of per-channel paint switches; Resolution, Anti-aliasing, the grid and wireframe buttons, Hardness and the channel switches have no counterpart on Painter's toolbar.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Size | Contextual toolbar, painting tools | not documented | partial: percent of the model diagonal, 0.1-100; scrub field without a slider | crates/ui/src/brush_bar.rs:189 | - |
| Flow | Contextual toolbar, painting tools | not documented | done | crates/ui/src/brush_bar.rs:44 | - |
| Stroke opacity | Contextual toolbar, painting tools | not documented | partial: labelled "Opacity" | crates/ui/src/brush_bar.rs:46 | - |
| Spacing | Contextual toolbar, painting tools | not documented | partial: on the bar where there is room (own row from 900 pt, beside the view controls from 1400 pt), else in the Brush settings menu and Properties; shown as % of the diameter | crates/ui/src/brush_bar.rs (spacing) | - |
| Pen-pressure toggle beside a value | Contextual toolbar / Properties | not documented | partial: pressure amounts for Size and Flow inside the stroke-options popup | crates/ui/src/brush_bar.rs:95 | - |
| Lazy mouse button | Contextual toolbar; D | off | done | crates/ui/src/brush_bar.rs (stroke_options, toggle_lazy) | - |
| Lazy mouse Distance | Contextual toolbar, next to the Lazy mouse button | not documented | partial: on the bar where there is room, always in the symmetry settings menu; 1-100 px | crates/ui/src/brush_bar.rs (distance) | - |
| Symmetry button (on / off) | Contextual toolbar; L | off | done: the button and L toggle and restore the last axes (each remembers its own last axes) | crates/ui/src/brush_bar.rs (toggle_symmetry), crates/app/src/session.rs (ToggleSymmetry) | #363 |
| Symmetry settings button (opens the settings menu) | Contextual toolbar, next to Symmetry | - | done | crates/ui/src/brush_bar.rs (stroke_options) | - |
| Brush preview: Full preview | Contextual toolbar (2019.3) | not documented | missing | - | - |
| Brush preview: Brush outline | Contextual toolbar (2019.3) | not documented | partial: the only cursor style, not selectable | crates/ui/src/viewport.rs:491 | - |
| Brush preview: Crosshair | Contextual toolbar (2019.3) | not documented | missing | - | - |
| Pressure curve: linear | Contextual toolbar (2019.3) | not documented | missing | - | - |
| Pressure curve: easy-in | Contextual toolbar (2019.3) | not documented | missing | - | - |
| Pressure curve: easy-in-out | Contextual toolbar (2019.3) | not documented | missing | - | - |
| Hide / ignore excluded geometry button | Contextual toolbar; Alt + H | off; disabled when nothing is excluded | done | crates/ui/src/viewport_bar.rs | - |
| Ignore masked UV Tiles while painting button | Contextual toolbar (UV Tile projects) | not documented | missing | - | - |
| Clone source behaviour button (source follows / returns) | Clone tool (location not documented) | follows | missing | - | - |
| Polygon Fill options on the toolbar | Contextual toolbar with Polygon Fill | not documented | partial: "Fill by" (Triangle, Part, UV island) only in Properties | crates/ui/src/panels/brush.rs:23 | #131 |
| Blending mode shortcut (cage painting in Baking mode) | Contextual toolbar (12.2) | - | missing | - | - |

## Contextual toolbar: Symmetry settings menu (Mirror)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Symmetry type: Mirror | Symmetry settings menu | Mirror | done | crates/ui/src/brush_bar.rs:68 | - |
| Axis | Symmetry settings menu | X (unverified) | partial: X, Y and Z are independent toggles (several at once) | crates/ui/src/brush_bar.rs:70 | - |
| Axis position | Symmetry settings menu | 0 (unverified) | done: per enabled axis, plus Reset; the manipulator drags it | crates/ui/src/brush_bar.rs (mirror_positions) | - |
| Show Plane | Symmetry settings menu > display | not documented | done (under the Show symmetry guides switch) | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Show Intersection | Symmetry settings menu > display | not documented | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Show Cursor | Symmetry settings menu > display | not documented | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Hide While Painting | Symmetry settings menu > display | not documented | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Enable manipulator | Symmetry settings menu > display | off | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Manipulator Size | Symmetry settings menu > display | not documented | done: 0.25 to 4 | crates/ui/src/brush_bar.rs (symmetry_display) | - |

## Contextual toolbar: Symmetry settings menu (Radial)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Symmetry type: Radial | Symmetry settings menu | off | done | crates/ui/src/brush_bar.rs:106 | - |
| Axis | Symmetry settings menu | not documented | done | crates/ui/src/brush_bar.rs:112 | - |
| Count (number of copies) | Symmetry settings menu | (unverified) | done | crates/ui/src/brush_bar.rs:124 | - |
| Flip copy | Symmetry settings menu | off | done: every other copy mirrored across the plane through the axis and the first copy | crates/paint/src/settings.rs (copy_cameras) | - |
| Axis position X | Symmetry settings menu | 0 (unverified) | done | crates/ui/src/brush_bar.rs:129 | - |
| Axis position Y | Symmetry settings menu | 0 (unverified) | done | crates/ui/src/brush_bar.rs:129 | - |
| Axis position Z | Symmetry settings menu | 0 (unverified) | done | crates/ui/src/brush_bar.rs:129 | - |
| Show Axis | Symmetry settings menu > display | not documented | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Show Intersection | Symmetry settings menu > display | not documented | partial: the Intersection switch covers mirror planes; the radial axis / mesh point is not drawn | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Show Cursor (brush tool) | Symmetry settings menu > display | not documented | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Hide While Painting (brush tool) | Symmetry settings menu > display | not documented | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Show Axis position manipulator | Symmetry settings menu > display | off | done: drags the radial pivot | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Manipulator Size | Symmetry settings menu > display | not documented | done | crates/ui/src/brush_bar.rs (symmetry_display) | - |
| Symmetry button greyed out for unsupported fill projections | Contextual toolbar with a fill layer | - | missing | - | - |

## Contextual toolbar: Path tools

Layout notes: with a Path tool active the left side shows path controls in the order below; values of the selected points (such as pressure) are edited from a vertex control on the same toolbar.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show / hide viewport interface | Contextual toolbar, Path tools | shown | missing | - | - |
| Display settings menu | Contextual toolbar, Path tools | - | missing | - | - |
| Handle size | Path display settings | not documented | missing | - | - |
| Path width | Path display settings | not documented | missing | - | - |
| Path color | Path display settings | not documented | missing | - | - |
| Unselected path color | Path display settings | not documented | missing | - | - |
| Normals | Path display settings | off (unverified) | missing | - | - |
| Tangents | Path display settings | not documented | missing | - | - |
| Path direction | Path display settings | not documented | missing | - | - |
| Path preview | Path settings menu; Shift + P (11.0) | on | missing | - | - |
| Angle snapping settings | Path settings menu (11.0) | not documented | missing | - | - |
| Snap to vertices | Path settings menu, magnet (11.0) | off | missing | - | - |
| Snap to edges | Path settings menu, magnet (11.0) | off | missing | - | - |
| Snap to center of edges | Path settings menu, magnet (11.0) | off | missing | - | - |
| Path type button (switch Paint / Ribbon / Filled / Erase / Smudge) | Contextual toolbar, Path tools | - | partial: Paint, Filled and Ribbon in the pen Properties' Path type dropdown | crates/ui/src/pen.rs | - |
| Reverse path direction | Contextual toolbar, Path tools | - | missing | - | - |
| Toggle corner / smooth | Contextual toolbar, Path tools | shortcut unbound | missing | - | - |
| Custom tangents | Contextual toolbar, Path tools | off | missing | - | - |
| Open / close path | Contextual toolbar, Path tools | open | partial: "Closed path" checkbox in the pen panel | crates/ui/src/pen.rs:98 | - |
| Delete vertex | Contextual toolbar, Path tools | - | partial: Delete / Backspace key only | crates/ui/src/pen.rs:60 | - |
| Vertex control: pressure of the selected points | Contextual toolbar, Path tools | not documented | missing | - | - |
| Translate manipulator toggle | Contextual toolbar, Path tools; W (11.0) | - | missing | - | - |
| Rotate manipulator toggle (several points) | Contextual toolbar, Path tools (11.0) | - | missing | - | - |
| Scale manipulator toggle (several points) | Contextual toolbar, Path tools (11.0) | - | missing | - | - |
| Symmetry | Contextual toolbar, Path tools | off | missing | - | - |
| Hide / ignore excluded geometry | Contextual toolbar, Path tools | off | done: button at the start of the channel row | crates/ui/src/viewport_bar.rs | - |

## Contextual toolbar: fill projection manipulators (3D projections)

Layout notes: shown when a fill layer or fill effect uses Tri-planar, Planar, Spherical or Cylindrical projection. The viewport side of these controls (dragging the gizmo, keys Q W E R T) is in viewport.md. SubCraft has UV, Tri-planar and Planar projections edited only by sliders in Properties (crates/ui/src/panels/properties.rs:965); there is no gizmo or toolbar set.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide manipulator | Contextual toolbar; Q | shown | missing | - | - |
| Manipulator settings menu | Contextual toolbar | - | missing | - | - |
| Manipulator size | Manipulator settings; + / - | not documented | missing | - | - |
| Grid steps | Manipulator settings | not documented | missing | - | - |
| Angle steps | Manipulator settings | not documented | missing | - | - |
| Translation manipulator | Contextual toolbar; W | selected (unverified) | missing | - | - |
| Rotation manipulator | Contextual toolbar; E | - | missing | - | - |
| Scale manipulator | Contextual toolbar; R | - | missing | - | - |
| Surface manipulator (Planar and Warp only) | Contextual toolbar; Shift + W | - | missing | - | - |
| Manipulator space: Local space | Contextual toolbar; T cycles | not documented | missing | - | - |
| Manipulator space: World space | Contextual toolbar; T cycles | not documented | missing | - | - |
| Mirror on X | Contextual toolbar | - | missing | - | - |
| Mirror on Y | Contextual toolbar | - | missing | - | - |
| Mirror on Z | Contextual toolbar | - | missing | - | - |
| Reset transformation | Contextual toolbar | - | missing | - | - |
| Symmetry button for fill layers | Contextual toolbar (11.1) | off | missing | - | - |

## Contextual toolbar: Warp projection extras

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Warp edition menu | Contextual toolbar, Warp | - | partial: the Edit row in Properties, not on the contextual toolbar | crates/ui/src/warp_view.rs | - |
| Transform warp | Warp edition menu; Shift + V toggles | default | done: Shift+V toggles | crates/ui/src/warp_view.rs | - |
| Edit vertices | Warp edition menu; Shift + V toggles | - | done: Shift+V toggles | crates/ui/src/warp_view.rs | - |
| Split warp crosswise | Warp edition menu | - | partial: through the middle cells, from Properties | crates/ui/src/panels/properties.rs (warp_rows) | - |
| Split warp horizontally | Warp edition menu | - | partial: through the middle cells, from Properties | crates/ui/src/panels/properties.rs (warp_rows) | - |
| Split warp vertically | Warp edition menu | - | partial: through the middle cells, from Properties | crates/ui/src/panels/properties.rs (warp_rows) | - |
| Warp projection settings menu | Contextual toolbar, Warp | - | partial: rows in Properties | crates/ui/src/panels/properties.rs (warp_rows) | - |
| Rows and columns | Warp projection settings | editable only before any point is moved | done: locked once a point moved | crates/ui/src/panels/properties.rs (warp_rows) | - |
| Handle size | Warp projection settings | not documented | done | crates/ui/src/warp_view.rs | - |
| Grid color | Warp projection settings | not documented | done | crates/ui/src/warp_view.rs | - |
| Automatic tangents | Contextual toolbar, Warp | not documented | missing | - | - |
| Warp to geometry | Contextual toolbar, Warp (12.0) | not documented | missing | - | - |
| Reset menu | Contextual toolbar, Warp | - | partial: Reset grid button in Properties | crates/ui/src/panels/properties.rs (warp_rows) | - |
| Restore the global transformation | Reset menu | - | missing | - | - |
| Reset all vertices | Reset menu | - | missing | - | - |
| Reset selected vertices | Reset menu | - | missing | - | - |

## Contextual toolbar: UV projection (2D manipulator)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide manipulator | Contextual toolbar; Q | shown | missing | - | - |
| Manipulator handles size menu | Contextual toolbar | - | missing | - | - |
| Handles size: Small | Manipulator handles size | not documented | missing | - | - |
| Handles size: Medium | Manipulator handles size | not documented | missing | - | - |
| Handles size: Large | Manipulator handles size | not documented | missing | - | - |
| Mirror on X | Contextual toolbar | - | missing | - | - |
| Mirror on Y | Contextual toolbar | - | missing | - | - |
| Reset pivot point | Contextual toolbar | - | missing | - | - |
| Reset transformation | Contextual toolbar | - | missing | - | - |

## Contextual toolbar: fixed right side

Layout notes: the right end never changes with the tool. It holds the mode switches (Painting, Baking, Rendering) and the view layout / camera buttons. The camera selector and the display-mode dropdown sit inside each view just under this strip, at its top right; they are listed in viewport.md and view-2d.md. In SubCraft the equivalents are at the left end of the strip.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Viewport Mode button (layout menu) | Contextual toolbar, right | 3D/2D | missing | - | - |
| Viewport Mode: 3D/2D | Viewport Mode menu; F1 | default | partial: View > Side-by-side 3D / 2D in the main menu | crates/ui/src/app.rs:1246 | - |
| Viewport Mode: 3D only | Viewport Mode menu; F2 | - | partial: dock tab click only | crates/ui/src/app.rs:189 | - |
| Viewport Mode: 2D only | Viewport Mode menu; F3 | - | partial: dock tab click only | crates/ui/src/app.rs:189 | - |
| Viewport Mode: Swap 3D/2D | Viewport Mode menu; F4 | - | partial: manual dock drag only | crates/ui/src/app.rs:232 | - |
| Perspective Mode: Perspective view | Contextual toolbar, right; F5 | default | partial: "Ortho" checkbox off, at the left of the strip | crates/ui/src/viewport_bar.rs:264 | - |
| Perspective Mode: Orthographic view | Contextual toolbar, right; F6 | - | partial: "Ortho" checkbox on, at the left of the strip | crates/ui/src/viewport_bar.rs:264 | - |
| Camera Rotation Mode: Free rotation | Contextual toolbar, right | - | done: in the camera menu of the top strip (and Display Settings > Camera) | crates/ui/src/orbit.rs (free_orbit) | 258 |
| Camera Rotation Mode: Constrained rotation | Contextual toolbar, right | default | done: the default; levels a roll left by free rotation | crates/ui/src/orbit.rs (orbit) | 258 |
| Rendering Mode (Iray) button | Contextual toolbar, right; F10 | off | partial: "Render" toggle at the 3D view's bottom right (path tracing) | crates/ui/src/viewport.rs:590 | - |
| Baking mode button | Contextual toolbar, right (8.3); F8 | - | partial: F8 / Ctrl+B open the Bake window, no toolbar button | crates/app/src/actions.rs:221 | - |
| Painting mode button | Contextual toolbar, right; F9 | active | missing | - | - |
| Toolbar cannot be moved | Contextual toolbar | fixed | done | crates/ui/src/viewport_bar.rs:185 | - |

## Sliders

Layout notes: a Painter slider is a bar with its numeric field above it at the right. All five behaviours apply to every slider in the application. SubCraft uses egui sliders with the value box at the right end of the bar (crates/ui/src/widgets.rs:192) and frameless scrub fields on the top strip (crates/ui/src/brush_bar.rs:162).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Drag | Any slider: click and drag on the bar | - | done | crates/ui/src/widgets.rs:192 | - |
| Precision drag | Any slider: hold Shift while dragging | - | partial: Shift slows only the scrub number fields, not slider bars | crates/ui/src/brush_bar.rs:186 | - |
| Jump | Any slider: click anywhere on the bar | - | done | crates/ui/src/widgets.rs:192 | - |
| Up / Down | Any slider: click to focus, then Up / Down arrow keys step the value | - | partial: egui steps a slider with arrow keys only when it has keyboard focus (Tab); focus on click not confirmed | crates/ui/src/widgets.rs:192 | - |
| Value edition | Any slider: click the text field above the bar and type | - | partial: value box sits at the right end of the bar, not above | crates/ui/src/widgets.rs:192 | - |
