# Painter parity: 2D view

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/2d-view, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/viewport, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/viewport-settings, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/geometry-mask, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-tiles/uv-tiles, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-tiles/image-sequence, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-reprojection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/post-processing/post-processing, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/spacemouse-by-3dconnexion, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/getting-started/project-creation, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/symmetry, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/straight-line, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/paint-brush, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/path, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/uv-projection, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/fill-match-per-uv-tile, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-presets/predefined-presets/predefined-presets, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2018-3, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2019-3, https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-7-4
Checked against: origin/main 297dcb3 (2026-11-11)

## View structure and overlays

Layout notes: the 2D view sits to the right of the 3D view inside the viewport and shows the UV islands of the currently selected Texture Set with the layer stack result. It has no toolbar of its own: brush controls are on the shared Contextual toolbar above. Overlays: display-mode dropdown top right, the UV Tile information button beside it (UV Tile projects only), and a U/V axis indicator bottom right. SubCraft's 2D View is a dock tab (crates/ui/src/panels/uv_view.rs:28) with its own header row: a "Show" source combo on the left, a paint-only-this-channel button, a UV-lines toggle, a Fit button, then a copy of the brush controls and channel switches. The paint-only-this-channel button, Fit button and the duplicated brush row have no Painter counterpart inside the 2D view.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV islands of the selected Texture Set with the layer stack result | 2D view | shown | done | crates/ui/src/panels/uv_view.rs:101 | - |
| View follows the Texture Set selection | 2D view | - | done | crates/ui/src/panels/uv_view.rs:29 | - |
| Display mode dropdown | 2D view, top right | Material | partial: "Show" combo at the top left of the tab | crates/ui/src/panels/uv_view.rs:43 | - |
| Material (lit) display in UV space | Display mode dropdown | default | missing | - | - |
| Single channel display, one entry per Texture Set channel | Display mode dropdown | - | done | crates/ui/src/panels/uv_view.rs:31 | - |
| Mesh map display | Display mode dropdown | - | done | crates/ui/src/panels/uv_view.rs:33 | - |
| Axis information (U and V directions) | 2D view, bottom right | shown | done | crates/ui/src/panels/uv_view.rs | - |
| UV Tile information button (show / hide UDIM labels) | 2D view, next to Display mode | UV Tile projects only | missing | - | - |
| Light gray background marking the paintable 0-1 UV range | 2D view, regular projects | shown | partial: dark panel with a 1 px border around the 0-1 square | crates/ui/src/panels/uv_view.rs:119 | - |
| UV islands outside 0-1 shown but not interactive | 2D view, regular projects | shown | missing | - | - |
| Post effects are not applied in the 2D view | 2D view | always | done | crates/ui/src/panels/uv_view.rs:102 | - |
| Separate colour-management display transform for the 2D view | 2D view (7.4) | not documented | missing | - | - |
| Undock the 2D view to its own window | View menu (7.4) | docked | missing | - | - |
| Live preview of the map being baked | 2D view; Settings > General | off (unverified) | missing | - | - |

## Display shortcuts

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Display next channel | C | - | done: C switches both views | crates/ui/src/app.rs | - |
| Display previous channel | Shift + C | - | done | crates/ui/src/app.rs | - |
| Display material | M | - | partial: M returns the 2D view to the base colour (no lit material in UV space) | crates/ui/src/app.rs | - |
| Display next mesh map | B | - | done: both views; unbaked maps fall back to the base colour in 2D | crates/ui/src/app.rs | - |
| Display previous mesh map | Shift + B | - | done | crates/ui/src/app.rs | - |
| Viewport layout 2D only | F3 | - | done; also a 2D view button on the left tool rail | crates/ui/src/app.rs | - |
| Swap 2D / 3D view | F4 | - | done | crates/ui/src/app.rs | - |

## Mesh wireframe and channel display options

Layout notes: these options are in Display settings > Viewport settings and act on both views. The complete Display settings checklist belongs to the display-settings area; only the rows that change what the 2D view draws are repeated here.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show mesh wireframe (UV wireframe in the 2D view) | Display settings > Mesh wireframe | off | partial: grid toggle in the 2D tab header (on by default), separate from the 3D wireframe switch | crates/ui/src/panels/uv_view.rs | - |
| Wireframe color | Display settings > Mesh wireframe | not documented | done: the 2D UV lines use the mesh wireframe colour; right-click the grid toggle to change it | crates/ui/src/panels/uv_view.rs | - |
| Wireframe opacity | Display settings > Mesh wireframe | not documented | done: shared with the mesh wireframe opacity; right-click the grid toggle | crates/ui/src/panels/uv_view.rs | - |
| Scale HDR values | Display settings > Channel display | not documented | missing | - | - |
| Use +/- color for HDR values | Display settings > Channel display | not documented | missing | - | - |
| Color channels: RGBA | Display settings > Channel display | default on colour channels | missing | - | - |
| Color channels: Grayscale+Alpha | Display settings > Channel display | default on grayscale channels | missing | - | - |
| Color channels: R | Display settings > Channel display | - | missing | - | - |
| Color channels: G | Display settings > Channel display | - | missing | - | - |
| Color channels: B | Display settings > Channel display | - | missing | - | - |
| Color channels: Alpha | Display settings > Channel display | - | missing | - | - |
| Selected color channel name shown in the view | 2D view overlay | - | missing | - | - |

## Navigation

Layout notes: the 2D view uses exactly the same combinations as the 3D view; orbit becomes a rotation of the UV space around the cursor.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Rotate the UV space around the cursor | Alt + LMB drag | - | done: about the point where the drag began; Fit to view and F reset it | crates/ui/src/panels/uv_view.rs | - |
| Snap rotation to orthogonal angles | Alt + Shift + LMB drag | every 90 degrees | done | crates/ui/src/panels/uv_view.rs | - |
| Pan | Alt + MMB drag | - | done | crates/ui/src/panels/uv_view.rs:84 | - |
| Pan (alternative) | Ctrl + Alt + LMB drag | - | done | crates/ui/src/panels/uv_view.rs | - |
| Zoom | Alt + RMB drag | - | done: zooms on the point where the drag began | crates/ui/src/panels/uv_view.rs | - |
| Zoom with the mouse wheel | Wheel over the view | (unverified) | done: zoom range 0.2-20, centred on the cursor | crates/ui/src/panels/uv_view.rs | - |
| Center the UVs in the 2D view | F with the cursor over the 2D view (2019.3) | - | done | crates/ui/src/app.rs | - |
| F with the cursor outside both views focuses both | F (2019.3) | - | done | crates/ui/src/app.rs | - |
| SpaceMouse navigation in the 2D view | 3Dconnexion device (7.4.2) | - | missing | - | - |

## Painting in UV space

Layout notes: every paint tool works in the 2D view except the Path tools, which show a message instead. Strokes made here are stored in UV space, so they cannot be re-projected after a mesh re-import that changes the UVs.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Paint with the Paint tool | LMB drag in the 2D view | - | done | crates/ui/src/panels/uv_view.rs:211 | - |
| Erase with the Eraser | LMB drag | - | done | crates/ui/src/panels/uv_view.rs:207 | - |
| Projection tool in UV space | LMB drag | - | missing | - | - |
| Polygon Fill in UV space | LMB click | - | done | crates/ui/src/panels/uv_view.rs:198 | - |
| Polygon Fill rectangular marquee in UV space | LMB drag | - | done | crates/ui/src/panels/uv_view.rs | #131 |
| Polygon Fill: element under the cursor highlighted | 2D view overlay | - | done: follows the fill mode | crates/ui/src/panels/uv_view.rs | - |
| Smudge in UV space | LMB drag | - | done | crates/paint/src/stroke.rs | - |
| Clone in UV space (V + LMB sets the source) | LMB drag | - | done: Ctrl+click or V+click sets the source; the source is shared with the 3D view | crates/app/src/session.rs | - |
| Clone brush preview in the 2D view | 2D view overlay (2018.3) | - | partial: source cross and offset circle, no image preview | crates/ui/src/panels/uv_view.rs | - |
| Material picker in the 2D view | P, then LMB | - | done: Eyedropper tool, Shift+click picks the material | crates/ui/src/panels/uv_view.rs | - |
| Painting limited to the 0-1 range in regular projects | 2D view | - | done | crates/ui/src/panels/uv_view.rs:177 | - |
| Brush cursor in the 2D view | 2D view overlay | - | partial: outline circle for brush, eraser, smudge and clone | crates/ui/src/panels/uv_view.rs | - |
| Draw straight lines | Shift + LMB | - | done | crates/ui/src/panels/uv_view.rs:214 | - |
| Draw straight lines with snapping | Ctrl + Shift + LMB | every 5 degrees | done | crates/ui/src/panels/uv_view.rs | - |
| Dotted straight-line preview | 2D view overlay while Shift is held | - | done | crates/ui/src/panels/uv_view.rs | - |
| Change tool size / hardness | Ctrl + RMB drag | - | done (shared with the 3D view) | crates/ui/src/viewport.rs | - |
| Change tool flow / rotation | Ctrl + LMB drag | - | done (shared with the 3D view) | crates/ui/src/viewport.rs | - |
| Increase / decrease tool size | ] and [ | - | done | crates/app/src/session.rs:1586 | - |
| Lazy mouse in the 2D view | D; Contextual toolbar | - | done | crates/app/src/session.rs:1725 | - |
| Symmetry while painting in the 2D view | L; Contextual toolbar | not supported for projection / stencil | done: each dab is mirrored through the 3D model to the matching texture position | crates/paint/src/stroke.rs | - |
| Stencil shown and usable in the 2D view | 2D view with a stencil loaded | - | done: laid over the 2D view and masks UV strokes there | crates/paint/src/kernel.rs | - |
| Stencil rotate | S + LMB drag | - | done | crates/ui/src/viewport.rs | - |
| Stencil snap rotate | Shift + S + LMB drag | - | done | crates/ui/src/viewport.rs | - |
| Stencil translation | S + MMB drag | - | done | crates/ui/src/viewport.rs | - |
| Stencil zoom | S + RMB drag | - | done | crates/ui/src/viewport.rs | - |
| Ignore stencil mask | hold N | - | done | crates/ui/src/viewport.rs | - |
| Quick mask editing in the 2D view | U / Y / I | - | done: red tint over the protected areas | crates/ui/src/panels/uv_view.rs | - |
| Brush Size Space "Texture" follows the 2D zoom | Properties > Brush > Size Space | Object | missing | - | - |
| Right-click: floating Properties at the cursor | RMB click in the 2D view | - | done | crates/ui/src/panels/uv_view.rs | #131 |
| Path tools show a "not supported in 2D" message | 2D view with a Path tool | - | partial: the reverse, Pen paths are drawn and edited in the 2D view | crates/ui/src/panels/uv_view.rs:130 | - |

## UV Tiles (UDIM)

Layout notes: in a UV Tile project the 2D view draws a grid, one cell per UV range, each labelled with its UDIM number; all tiles of the Texture Set are visible side by side and paintable, and a stroke can cross tile borders. SubCraft imports each UDIM tile as its own texture set (crates/app/src/new_project.rs:312), so the 2D view only ever shows one 0-1 tile.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV Tiles displayed as a grid | 2D view, UV Tile projects | shown | missing | - | - |
| UDIM number label per tile | 2D view, UV Tile projects | shown; toggled by the UV Tile information button | missing | - | - |
| All tiles of the Texture Set visible side by side | 2D view | - | partial: one tile per texture set; switch texture sets to see another tile | crates/app/src/new_project.rs:312 | - |
| Paint on any tile | 2D view | - | partial: only the selected texture set's tile | crates/ui/src/panels/uv_view.rs:172 | - |
| Paint across tile borders in one stroke | 2D and 3D views | - | missing | - | - |
| UV Tile hover highlight | 2D and 3D views, geometry mask edit | - | missing | - | - |
| Include / exclude a UV Tile in the layer's UV Tile mask | LMB click on a tile | - | missing | - | - |
| Include / exclude several UV Tiles | Ctrl + LMB click on tiles | - | missing | - | - |
| Ignore masked UV Tiles while painting | Contextual toolbar button | not documented | missing | - | - |
| Tile highlight when hovering the Texture Set list | Texture Set list (12.2) | - | missing | - | - |
| Fill (match per UV Tile) result shown per tile | 2D view | - | missing | - | - |
| Painting tools default to single channel in UV Tile projects | Properties | on (release note) | missing | - | - |

## UV projection manipulator

Layout notes: selecting a fill layer or fill effect that uses UV projection draws a rectangular transform frame with handles and a pivot in the 2D view only. Its toolbar buttons (show / hide, handle size, mirror, reset) are in toolbar.md.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide manipulator | Q | shown | missing | - | - |
| Translate | LMB drag inside the frame | - | missing | - | - |
| Translate constrained to one axis | Shift + LMB drag inside | horizontal or vertical by drag direction | missing | - | - |
| Rotation | LMB drag outside the frame | - | missing | - | - |
| Move the pivot (rotation origin) | LMB drag the pivot | centre | missing | - | - |
| Rotation constrained | Shift + LMB drag outside | every 45 degrees | missing | - | - |
| Scale / deform | LMB drag a handle | - | missing | - | - |
| Scale constrained (keep ratio) | Shift + LMB drag a handle | - | missing | - | - |
| Scale mirrored around the pivot | Ctrl + LMB drag a handle | - | missing | - | - |
| Scale mirrored and constrained | Shift + Ctrl + LMB drag a handle | - | missing | - | - |
| Tiled preview of the image inside the frame | 2D view | follows UV Wrap | missing | - | - |

## Export of the 2D view

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| "2D View" export preset | Export textures window > Output template | - | partial: reached through File > Export 2D view, which opens the export window in 2D-only mode | crates/ui/src/app.rs:1140 | - |
| Exports the current 2D view state as one texture per Texture Set with the regular export settings | Export textures window | - | done | crates/export/src/template.rs:216 | - |
| "2D View" converted map as an input for custom presets | Export window > Output templates > Converted maps | - | partial: Source::View2d exists in the template model; not offered in a preset editor on main | crates/export/src/template.rs:20 | #124 |
