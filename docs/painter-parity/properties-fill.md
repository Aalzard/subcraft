# Painter parity: Properties panel, fill layer and fill effect

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/properties , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/fill-projections , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/fill-match-per-uv-tile , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/uv-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/tri-planar-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/planar-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/spherical-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/cylindrical-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/warp-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/physical-size , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/fill , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/paint-brush , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/symmetry , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/mirror-symmetry , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/symmetry/radial-symmetry , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/normal-map-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/ambient-occlusion-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/text-resource , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/vector-graphic-svg
Checked against: origin/main 297dcb3 (2026-11-11)

## Panel structure

Layout notes: with a fill layer or fill effect selected the panel has two upper-case blocks: FILL (Projection dropdown, then the settings of that projection, a "UV transformations" sub-group, a "3D projection" sub-group for 3D modes and a "Symmetry" sub-group for eligible modes) and MATERIAL (channel toggle buttons, the Material mode button, then per-channel rows). On a mask's fill effect the second block is GRAYSCALE with one slot. SubCraft mirrors the two blocks with FILL and MATERIAL headers (crates/ui/src/panels/properties.rs:194, :201).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| FILL block (projection settings) | Properties (fill layer) | n/a | done | crates/ui/src/panels/properties.rs:194 | - |
| MATERIAL block | Properties (fill layer) | n/a | done | crates/ui/src/panels/properties.rs:201 | - |
| Same panel for a Fill effect on a layer's content | Properties (fill effect) | n/a | partial: compact editor (segmented mode) with the Filtering / UV Wrap / Shape crop rows, sources limited to Uniform and Procedural | crates/ui/src/panels/effects.rs:367 | chunk 8 |
| Same panel for a Fill effect in a mask (GRAYSCALE block) | Properties (mask fill effect) | n/a | partial: Value slider or built-in procedural, plus the compact projection editor | crates/ui/src/panels/properties.rs:637 | - |
| Right-click in the viewport opens floating Properties for the selected fill | Viewport | n/a | done (earlier): the right-click Properties popup shows the selected layer's properties | crates/ui/src/view_props.rs | #131 |

## Projection dropdown

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Projection (dropdown) | Properties > Fill | UV projection (unverified) | partial: 3 of 7 modes | crates/ui/src/panels/properties.rs:964 | - |
| Fill (match per UV Tile) | Properties > Fill > Projection | option | missing | - | - |
| UV projection | Properties > Fill > Projection | option | done | crates/ui/src/panels/properties.rs:965 | - |
| Tri-planar projection | Properties > Fill > Projection | option | done | crates/ui/src/panels/properties.rs:965 | - |
| Planar projection | Properties > Fill > Projection | option | partial: projects along a chosen object axis (X/Y/Z) only; no free 3D placement, crop or culling | crates/ui/src/panels/properties.rs:1014 | - |
| Spherical projection | Properties > Fill > Projection | option | done: around the centre of the bounding cube, pole on a chosen axis | crates/doc/src/fill.rs:291 | chunk 46 |
| Cylindrical projection | Properties > Fill > Projection | option | done: around a chosen axis through the centre of the bounding cube | crates/doc/src/fill.rs:291 | chunk 46 |
| Warp projection | Properties > Fill > Projection | option | done: planar along an axis through a grid of movable points; untouched it equals Planar; nothing projected outside the grid | crates/doc/src/warp.rs, crates/materials/src/sample/projection.rs | - |

## Fill (match per UV Tile)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Each image of a UDIM sequence assigned to its own UV tile | Properties > Fill | n/a | missing | - | - |
| No transform settings shown in this mode | Properties > Fill | n/a | missing | - | - |

## UV projection

Layout notes: Filtering, UV Wrap, then the UV transformations group (Scale mode, Tiling or Size, Rotation, Offset). The manipulator for this mode exists only in the 2D view.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Filtering (dropdown) | Properties > Fill | Bilinear &#124; HQ | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| Filtering: Bilinear &#124; HQ | Properties > Fill > Filtering | default | done: trilinear over a cached mip chain (box-filtered, in linear light for colour) | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| Filtering: Bilinear &#124; Sharp | Properties > Fill > Filtering | option | done: bilinear on the full-size image, no mip-maps | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| Filtering: Nearest | Properties > Fill > Filtering | option | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| UV Wrap (dropdown) | Properties > Fill | Repeat | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| UV Wrap: None | Properties > Fill > UV Wrap | option | done: empty outside the 0-1 square of the transformed texture | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| UV Wrap: Repeat horizontally | Properties > Fill > UV Wrap | option | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| UV Wrap: Repeat vertically | Properties > Fill > UV Wrap | option | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| UV Wrap: Repeat | Properties > Fill > UV Wrap | default | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| Single image (or first of a sequence) applied to every UV tile | Properties > Fill | n/a | done | crates/ui/src/panels/properties.rs:965 | - |

## UV transformations (all projections)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV transformations group (collapsible) | Properties > Fill | open | done | crates/ui/src/panels/properties.rs:971 | - |
| Scale mode (dropdown) | Properties > Fill > UV transformations > Scale | Tiling | partial: row shown but disabled with one entry | crates/ui/src/panels/properties.rs:973 | - |
| Scale mode: Tiling | UV transformations > Scale | default | done | crates/ui/src/panels/properties.rs:973 | - |
| Scale mode: Physical Size | UV transformations > Scale | option | done: Scale dropdown Tiling / Physical size | crates/ui/src/panels/properties.rs (physical_size_rows) | #666 |
| Tiling (two values, horizontal and vertical) | UV transformations | 1 (unverified); range not documented | partial: sliders 0.01-16, typed values unclamped, default 1 | crates/ui/src/panels/properties.rs:983 | - |
| Tiling lock (link the two values) | UV transformations | linked (unverified) | done | crates/ui/src/panels/properties.rs:976 | - |
| Rotation | UV transformations | 0 (unverified); range not documented | done | crates/ui/src/panels/properties.rs:1001 | - |
| Offset (two values) | UV transformations | 0, 0 = texture centred on the UVs | partial: sliders -1 to 1, typed values unclamped | crates/ui/src/panels/properties.rs:1007 | - |
| Offset hidden for Tri-planar projection | UV transformations | n/a | done | crates/ui/src/panels/properties.rs:991 | chunk 8 |
| Physical Size: automatic scale from the mesh size and the material's embedded size (X and Y only) | UV transformations (Physical Size) | n/a | partial: the tiling follows the model's real size; materials carry no embedded size, so the size is always typed | crates/doc/src/fill.rs (effective_physical_size) | #666 |
| Custom Size (toggle; forced on when no size is embedded or when resources disagree) | UV transformations (Physical Size) | off | partial: always custom (no embedded sizes) | crates/ui/src/panels/properties.rs (physical_size_rows) | #666 |
| Size (cm), two values | UV transformations (Physical Size) | from the asset | done: linked X/Y, click to unlink | crates/ui/src/panels/properties.rs (physical_size_rows) | #666 |
| Rotation (Physical Size mode) | UV transformations (Physical Size) | not documented | done | crates/ui/src/panels/properties.rs | #666 |
| Offset (Physical Size mode) | UV transformations (Physical Size) | not documented | done | crates/ui/src/panels/properties.rs | #666 |
| Physical size available with every projection type | UV transformations | n/a | partial: UV, Planar, Tri-planar and Warp; Spherical and Cylindrical repeat by angle and keep Tiling | crates/doc/src/fill.rs (supports_physical_size) | #666 |

## UV projection: contextual toolbar and 2D manipulator

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide manipulator | Contextual toolbar | not documented | done: the projection manipulator's Show (Q) also hides the 2D frame | crates/ui/src/uv_gizmo.rs | - |
| Manipulator handles size: Small | Contextual toolbar > handles size | option | missing | - | - |
| Manipulator handles size: Medium | Contextual toolbar > handles size | option | missing | - | - |
| Manipulator handles size: Large | Contextual toolbar > handles size | option | missing | - | - |
| Mirror on X | Contextual toolbar | n/a | done: Mirror X in UV transformations (about the tile's centre) | crates/ui/src/uv_gizmo.rs (mirrored), crates/ui/src/panels/properties.rs (UV transformations) | - |
| Mirror on Y | Contextual toolbar | n/a | done: Mirror Y in UV transformations | crates/ui/src/uv_gizmo.rs (mirrored), crates/ui/src/panels/properties.rs (UV transformations) | - |
| Reset pivot point | Contextual toolbar | n/a | n/a: the frame turns and scales about its centre; there is no movable pivot | crates/ui/src/uv_gizmo.rs | - |
| Reset transformation | Contextual toolbar | n/a | partial: "Reset tiling, offset and rotation" button in Properties | crates/ui/src/panels/properties.rs:1017 | - |
| Translate: drag inside the frame | 2D view manipulator | n/a | partial: drag the centre handle (inside the frame the 2D view keeps painting) | crates/ui/src/uv_gizmo.rs | - |
| Translate constrained to one axis: Shift+drag | 2D view manipulator | n/a | done: Shift while dragging the centre | crates/ui/src/uv_gizmo.rs | - |
| Rotate: drag outside the frame (around the movable pivot) | 2D view manipulator | n/a | partial: drag just outside a corner; turns about the frame's centre (no movable pivot) | crates/ui/src/uv_gizmo.rs | - |
| Rotate in 45 degree steps: Shift+drag outside | 2D view manipulator | n/a | done | crates/ui/src/uv_gizmo.rs | - |
| Scale: drag a handle | 2D view manipulator | n/a | done: corner and edge handles; the opposite side stays | crates/ui/src/uv_gizmo.rs | - |
| Scale keeping ratio: Shift+drag a handle | 2D view manipulator | n/a | done | crates/ui/src/uv_gizmo.rs | - |
| Scale mirrored around the pivot: Ctrl+drag a handle | 2D view manipulator | n/a | partial: Ctrl scales about the frame's centre (both sides move) | crates/ui/src/uv_gizmo.rs | - |
| Scale mirrored and constrained: Shift+Ctrl+drag a handle | 2D view manipulator | n/a | partial: Shift+Ctrl, about the centre keeping the ratio | crates/ui/src/uv_gizmo.rs | - |

## Tri-planar projection

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Filtering (same three options as UV projection) | Properties > Fill | Bilinear &#124; HQ | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| Shape crop (dropdown) | Properties > Fill | Projection extends outside shape | done: crops to the projection box, which follows the 3D Offset / Rotation / Scale (the bounding cube until it is moved) | crates/materials/src/sample/bitmap.rs:214 | chunk 9 (part 1) |
| Shape crop: Project cropped to shape | Properties > Fill > Shape crop | option | done: crops to the projection box, which follows the 3D Offset / Rotation / Scale (the bounding cube until it is moved) | crates/materials/src/sample/bitmap.rs:214 | chunk 9 (part 1) |
| Shape crop: Projection extends outside shape | Properties > Fill > Shape crop | default | done: crops to the projection box, which follows the 3D Offset / Rotation / Scale (the bounding cube until it is moved) | crates/materials/src/sample/bitmap.rs:214 | chunk 9 (part 1) |
| Hardness (blend between the three planes; 1.0 = hard cut) | Properties > Fill | not documented; max 1.0 | done: 0-1, 1.0 = hard cut; default 0.875 (the former Sharpness 8) | crates/ui/src/panels/properties.rs:977 | chunk 8 |
| 3D projection: Offset X, Y, Z (scene bounding-box units, 0 = centre) | Properties > Fill > 3D projection | 0, 0, 0 | done: three number fields, bounding-box units, 0 centred; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Rotation X, Y, Z (degrees) | Properties > Fill > 3D projection | 0, 0, 0 (unverified) | done: degrees about X, Y, Z, applied in that order (the axis choice stays as well); no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Scale X, Y, Z | Properties > Fill > 3D projection | 1, 1, 1 (unverified) | done: 0.01..100 per axis; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |

## Planar projection

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Filtering (same three options) | Properties > Fill | Bilinear &#124; HQ | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| UV Wrap (None / Repeat horizontally / Repeat vertically / Repeat) | Properties > Fill | Repeat | done | crates/ui/src/panels/properties.rs:963 | chunk 8 |
| Shape crop (cropped to shape / extends outside shape) | Properties > Fill | Projection extends outside shape | done: crops to the projection box, which follows the 3D Offset / Rotation / Scale (the bounding cube until it is moved) | crates/materials/src/sample/bitmap.rs:214 | chunk 9 (part 1) |
| Depth culling (toggle) | Properties > Fill | not documented | done: planar only; a height field of the whole mesh seen from the projector, so surfaces behind the nearest one along the projection get nothing (bitmap and procedural fills, image masks) | crates/materials/src/sample/depth_cull.rs | this PR |
| Depth culling: Hardness | Properties > Fill > Depth culling | not documented | done: 0..1, default 0.8; below 1 the projection fades over up to 0.25 of the box depth behind the nearest surface | crates/ui/src/panels/properties.rs (backface_rows) | this PR |
| Backface culling (toggle) | Properties > Fill | not documented | done: on bitmap and procedural fills and image masks; planar faces the side its image reads unmirrored from, cylindrical faces outward | crates/ui/src/panels/properties.rs:1649 | chunk 9 (part 2) |
| Backface culling: Angle | Properties > Fill > Backface culling | not documented | done: 0..180 degrees, default 90 (labelled Culling angle) | crates/ui/src/panels/properties.rs:1649 | chunk 9 (part 2) |
| Backface culling: Hardness | Properties > Fill > Backface culling | not documented | done: 0..1, default 0.5; below 1 the projection fades over up to 30 degrees before the angle (labelled Culling hardness) | crates/ui/src/panels/properties.rs:1649 | chunk 9 (part 2) |
| Projection depth follows the transform's Z scale | Properties > Fill > 3D projection | n/a | done with Shape crop on: the box is as deep as its scale along the projection axis; there is no separate depth without the crop | crates/doc/src/fill.rs:211 | chunk 9 (part 1) |
| 3D projection: Offset X, Y, Z | Properties > Fill > 3D projection | 0, 0, 0 | done: three number fields, bounding-box units, 0 centred; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Rotation X, Y, Z | Properties > Fill > 3D projection | not documented | done: degrees about X, Y, Z, applied in that order (the axis choice stays as well); no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Scale X, Y, Z | Properties > Fill > 3D projection | not documented | done: 0.01..100 per axis; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |

## Spherical projection

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Filtering (same three options) | Properties > Fill | Bilinear &#124; HQ | done | crates/ui/src/panels/properties.rs:1218 | chunk 46 |
| UV Wrap (four options) | Properties > Fill | Repeat | done | crates/ui/src/panels/properties.rs:1218 | chunk 46 |
| Shape Crop (two options) | Properties > Fill | Projection extends outside shape | done: crops to the projection box, which follows the 3D Offset / Rotation / Scale (the bounding cube until it is moved) | crates/materials/src/sample/bitmap.rs:214 | chunk 9 (part 1) |
| UV transformation: Scale | Properties > Fill > UV transformations | not documented | done: the shared Tiling / Rotation / Offset rows | crates/ui/src/panels/properties.rs:1302 | chunk 46 |
| UV transformation: Rotation | Properties > Fill > UV transformations | not documented | done: the shared Tiling / Rotation / Offset rows | crates/ui/src/panels/properties.rs:1302 | chunk 46 |
| UV transformation: Offset | Properties > Fill > UV transformations | texture centred | done: the shared Tiling / Rotation / Offset rows | crates/ui/src/panels/properties.rs:1302 | chunk 46 |
| 3D projection: Offset X, Y, Z | Properties > Fill > 3D projection | 0, 0, 0 | done: three number fields, bounding-box units, 0 centred; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Rotation X, Y, Z | Properties > Fill > 3D projection | not documented | done: degrees about X, Y, Z, applied in that order (the axis choice stays as well); no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Scale X, Y, Z | Properties > Fill > 3D projection | not documented | done: 0.01..100 per axis; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |

## Cylindrical projection

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Filtering (same three options) | Properties > Fill | Bilinear &#124; HQ | done | crates/ui/src/panels/properties.rs:1218 | chunk 46 |
| UV wrap (four options) | Properties > Fill | Repeat | done | crates/ui/src/panels/properties.rs:1218 | chunk 46 |
| Shape crop (two options) | Properties > Fill | Projection extends outside shape | done: crops to the projection box, which follows the 3D Offset / Rotation / Scale (the bounding cube until it is moved) | crates/materials/src/sample/bitmap.rs:214 | chunk 9 (part 1) |
| Angle (how much of the perimeter is covered) | Properties > Fill | not documented (360 unverified) | done: 1-360 degrees, default 360 | crates/ui/src/panels/properties.rs:1232 | chunk 46 |
| Backface Culling (toggle) | Properties > Fill | not documented | done: on bitmap and procedural fills and image masks; planar faces the side its image reads unmirrored from, cylindrical faces outward | crates/ui/src/panels/properties.rs:1649 | chunk 9 (part 2) |
| Backface Culling: Hardness | Properties > Fill > Backface Culling | not documented | done: 0..1, default 0.5; below 1 the projection fades over up to 30 degrees before the angle (labelled Culling hardness) | crates/ui/src/panels/properties.rs:1649 | chunk 9 (part 2) |
| 3D projection: Offset X, Y, Z | Properties > Fill > 3D projection | 0, 0, 0 | done: three number fields, bounding-box units, 0 centred; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Rotation X, Y, Z | Properties > Fill > 3D projection | not documented | done: degrees about X, Y, Z, applied in that order (the axis choice stays as well); no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |
| 3D projection: Scale X, Y, Z | Properties > Fill > 3D projection | not documented | done: 0.01..100 per axis; no viewport manipulator yet | crates/ui/src/panels/properties.rs:1586 | chunk 9 (part 1) |

## Warp projection

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Quick setup: drop an Alpha / Procedural / Texture from Assets onto the mesh, then choose the channel in a popup menu | Viewport | n/a | partial: a texture dropped on the model opens a menu of the texture set's channels; the pick adds a fill layer filling that channel (alphas keep their own drop behaviour; no procedurals in the Shelf) | crates/ui/src/viewport.rs | (this PR) |
| Quick setup with a Material: Alt + drop | Viewport | n/a | missing | - | - |
| Filtering (same three options) | Properties > Fill | Bilinear &#124; HQ | done: the shared Filtering row | crates/ui/src/panels/properties.rs (projection_options) | - |
| UV wrap (four options) | Properties > Fill | Repeat | done: inside the grid | crates/ui/src/panels/properties.rs (projection_options) | - |
| Shape crop (two options) | Properties > Fill | Projection extends outside shape | done: the shared Shape crop row | crates/ui/src/panels/properties.rs (projection_options) | - |
| Projection depth | Properties > Fill | not documented | missing | - | - |
| Depth culling (toggle) | Properties > Fill | not documented | missing | - | - |
| Depth culling: Hardness | Properties > Fill > Depth culling | not documented | missing | - | - |
| 3D projection: Offset X, Y, Z | Properties > Fill > 3D projection | 0, 0, 0 | done: 3D projection group in Properties > Fill | crates/doc/src/fill.rs (ProjectionTransform), crates/ui/src/panels/properties.rs | - |
| 3D projection: Rotation X, Y, Z | Properties > Fill > 3D projection | not documented | done: 3D projection group in Properties > Fill | crates/doc/src/fill.rs (ProjectionTransform), crates/ui/src/panels/properties.rs | - |
| 3D projection: Scale X, Y, Z | Properties > Fill > 3D projection | not documented | done: 3D projection group in Properties > Fill | crates/doc/src/fill.rs (ProjectionTransform), crates/ui/src/panels/properties.rs | - |
| Warp edition menu: Transform warp | Contextual toolbar | default (unverified) | done: Properties > Edit row; drag a point to move the whole grid in the view plane | crates/ui/src/warp_view.rs | - |
| Warp edition menu: Edit vertices | Contextual toolbar | option | done: Properties > Edit row | crates/ui/src/warp_view.rs | - |
| Warp edition menu: Split warp crosswise | Contextual toolbar | action | done: Split tool Crosswise in the Warp settings; click a cell in the 3D view (a dashed preview shows the cut); the middle-cell buttons in Properties stay | crates/ui/src/warp_view.rs (SplitKind, cell_at, split_cell) | - |
| Warp edition menu: Split warp horizontally | Contextual toolbar | action | done: Split tool Horizontally; a row through the clicked cell | crates/ui/src/warp_view.rs (SplitKind, cell_at, split_cell) | - |
| Warp edition menu: Split warp vertically | Contextual toolbar | action | done: Split tool Vertically; a column through the clicked cell | crates/ui/src/warp_view.rs (SplitKind, cell_at, split_cell) | - |
| Cancel a split tool with Escape | Viewport | n/a | done: Esc stops the split tool (before it clears a vertex selection) | crates/ui/src/warp_view.rs (SplitKind, cell_at, split_cell) | - |
| Toggle warp edition mode shortcut (Transform warp / Edit vertices) | Shortcut (Shift+V per SubCraft's Painter key table) | n/a | done: Shift+V over the 3D view | crates/ui/src/warp_view.rs | - |
| Warp projection settings: Rows and columns (editable only while no point was moved) | Contextual toolbar > settings | not documented | done: 1-6 each, locked once a point moved or the grid was split | crates/ui/src/panels/properties.rs (warp_rows) | - |
| Warp projection settings: Handle size | Contextual toolbar > settings | not documented | done: Properties > Handles | crates/ui/src/warp_view.rs | - |
| Warp projection settings: Grid color | Contextual toolbar > settings | not documented | done: Properties > Handles | crates/ui/src/warp_view.rs | - |
| Automatic tangents | Contextual toolbar | not documented | missing | - | - |
| Select a grid point: click | Viewport (Edit vertices) | n/a | done | crates/ui/src/warp_view.rs | - |
| Deselect: click away from points | Viewport (Edit vertices) | n/a | done (also Esc) | crates/ui/src/warp_view.rs | - |
| Add to selection: Shift+click | Viewport (Edit vertices) | n/a | done | crates/ui/src/warp_view.rs | - |
| Remove one point from selection: Ctrl+click | Viewport (Edit vertices) | n/a | done: Ctrl+click without moving | crates/ui/src/warp_view.rs | - |
| Rectangle selection: drag | Viewport (Edit vertices) | n/a | done | crates/ui/src/warp_view.rs | - |
| Rectangle add: Shift+drag | Viewport (Edit vertices) | n/a | done | crates/ui/src/warp_view.rs | - |
| Rectangle remove: Ctrl+drag | Viewport (Edit vertices) | n/a | done | crates/ui/src/warp_view.rs | - |
| Drag a point directly: moves it along the mesh surface | Viewport (Edit vertices) | n/a | done: the dragged point follows the surface under the cursor, the selection moves with it; one undo step per drag | crates/ui/src/warp_view.rs | - |
| Ctrl+drag a point: moves it in camera space | Viewport (Edit vertices) | n/a | done: in the view plane through the point | crates/ui/src/warp_view.rs | - |
| Edit a point's tangents with the Rotation / Scale manipulators | Viewport (Edit vertices) | n/a | missing | - | - |
| Green arrows showing projection direction and depth per grid point | Viewport | n/a | missing | - | - |

## 3D projections: contextual toolbar and manipulator

Layout notes: applies to Tri-planar, Planar, Spherical, Cylindrical and Warp. The manipulator exists only in the 3D view. SubCraft has a manipulator for the 3D Offset / Rotation / Scale of tri-planar, planar, spherical and cylindrical fill layers (crates/ui/src/projection_gizmo.rs); its tool and settings are in Properties, and it has no keyboard shortcuts.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide manipulator | Contextual toolbar | not documented | done: Show checkbox in Properties (no contextual toolbar, no Q shortcut) | crates/ui/src/projection_gizmo.rs:67 | chunk 9 (part 3) |
| Manipulator settings: Manipulator size | Contextual toolbar > settings | not documented | done: 0.5..2 in Properties | crates/ui/src/projection_gizmo.rs:67 | chunk 9 (part 3) |
| Manipulator settings: Grid steps (translate / scale snapping step) | Contextual toolbar > settings | not documented | done: default 0.05 | crates/ui/src/projection_gizmo.rs:67 | chunk 9 (part 3) |
| Manipulator settings: Angle steps (rotate snapping step) | Contextual toolbar > settings | not documented | done: default 15 degrees | crates/ui/src/projection_gizmo.rs:67 | chunk 9 (part 3) |
| Translation manipulator | Contextual toolbar | n/a | done: Move, picked in Properties (no W shortcut: W is a tool key) | crates/ui/src/projection_gizmo.rs:67 | chunk 9 (part 3) |
| Rotation manipulator | Contextual toolbar | n/a | done: picked in Properties (no E shortcut: E is the Eraser) | crates/ui/src/projection_gizmo.rs:67 | chunk 9 (part 3) |
| Scale manipulator | Contextual toolbar | n/a | done: picked in Properties (no R shortcut) | crates/ui/src/projection_gizmo.rs:67 | chunk 9 (part 3) |
| Surface manipulator (Planar and Warp only) | Contextual toolbar | n/a | partial: Surface in the manipulator settings (Shift+W), planar only | crates/ui/src/projection_gizmo.rs (surface_snapped) | - |
| Manipulator space: Local space | Contextual toolbar | not documented | done: the handles always follow the projection box (local space) | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Manipulator space: World space | Contextual toolbar | not documented | done: Local / World in the manipulator settings, T switches; Move and Rotate follow the scene axes, Scale keeps the box axes | crates/ui/src/projection_gizmo.rs (GizmoEdit::world, layout_for) | - |
| Mirror on X | Contextual toolbar | n/a | done: Mirror X / Y / Z toggles in Properties > Fill > 3D projection (the image flips; the facing side stays) | crates/doc/src/fill.rs (ProjectionTransform::mirror), crates/ui/src/panels/properties.rs (transform3_group) | - |
| Mirror on Y | Contextual toolbar | n/a | done: Mirror X / Y / Z toggles in Properties > Fill > 3D projection (the image flips; the facing side stays) | crates/doc/src/fill.rs (ProjectionTransform::mirror), crates/ui/src/panels/properties.rs (transform3_group) | - |
| Mirror on Z | Contextual toolbar | n/a | done: Mirror X / Y / Z toggles in Properties > Fill > 3D projection (the image flips; the facing side stays) | crates/doc/src/fill.rs (ProjectionTransform::mirror), crates/ui/src/panels/properties.rs (transform3_group) | - |
| Reset transformation | Contextual toolbar | n/a | partial: Properties buttons reset the UV tiling / offset / rotation and, separately, the 3D transform | crates/ui/src/panels/properties.rs:1017 | - |
| Translate on one axis: drag an axis arrow | 3D view manipulator | n/a | done: axis handles (round ends, not arrows) | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Translate on a plane: drag a two-axis handle | 3D view manipulator | n/a | done: the squares between two axes | crates/ui/src/projection_gizmo.rs (plane_quad) | - |
| Translate in camera plane: drag the centre handle | 3D view manipulator | n/a | done | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Translate in steps: Shift+drag | 3D view manipulator | n/a | done | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Rotate around one axis: drag a ring | 3D view manipulator | n/a | done | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Free rotate: drag between the rings | 3D view manipulator | n/a | done: trackball turn about the axis across the drag (Shift: angle steps, Ctrl: precise) | crates/ui/src/projection_gizmo.rs (free_rotated), crates/doc/src/fill.rs (rotated_about_axis) | - |
| Rotate in steps: Shift+drag | 3D view manipulator | n/a | done | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Scale along one axis: drag an axis handle | 3D view manipulator | n/a | done; the centre handle scales all three | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Scale in steps: Shift+drag | 3D view manipulator | n/a | done | crates/ui/src/projection_gizmo.rs:239 | chunk 9 (part 3) |
| Surface: drag the manipulator over the mesh to snap the projection onto it | 3D view manipulator | n/a | partial: planar projections only (Warp keeps its grid tools); the centre dragged over the model moves the box onto the surface and turns its projection axis to the normal | crates/ui/src/projection_gizmo.rs (surface_snapped) | - |

## Symmetry on fill projections

Layout notes: a Symmetry sub-group in the FILL block with an "Enable symmetry" toggle, available for Tri-planar, Planar, Spherical, Cylindrical and Warp (greyed out for UV modes). The same toggle is mirrored on the contextual toolbar; display options (plane, intersection, manipulator) are only in the toolbar's settings menu.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Enable symmetry | Properties > Fill > Symmetry | off | done | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Enable symmetry unavailable for UV projection and Fill (match per UV Tile) | Properties > Fill > Symmetry | n/a | done: greyed out on UV projection | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Symmetry type: Mirror | Properties > Fill > Symmetry | not documented | done | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Symmetry type: Radial | Properties > Fill > Symmetry | not documented | done | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Mirror: Axis | Properties > Fill > Symmetry | not documented | done | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Mirror: Axis position | Properties > Fill > Symmetry | not documented | done: bounding-cube coordinate, 0.5 = the middle | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Radial: Axis | Properties > Fill > Symmetry | not documented | done | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Radial: copy count | Properties > Fill > Symmetry | (unverified) | done: 1-64, default 4 | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Radial: Flip copy (alternate copies flipped on U or V) | Properties > Fill > Symmetry | not documented | done | crates/ui/src/panels/properties.rs:1243 | chunk 46 |
| Radial: Axis position X, Y, Z | Properties > Fill > Symmetry | not documented | done | crates/ui/src/panels/properties.rs:1243 | chunk 46 |

## Material

Layout notes: channel toggle buttons in Texture Set channel order with short lower-case names (color, metal, rough, nrm, height ...), then the wide Material mode button showing the loaded material or "No Resource Selected", then one row per enabled channel: channel name, a uniform colour swatch with eyedropper (or value slider), and a resource slot button that replaces the uniform value when filled. A loaded Substance lists its parameters (including seed with a Random button) below. SubCraft draws one card per enabled channel with a thumbnail slot, a source dropdown and the source's controls (crates/ui/src/panels/properties.rs:216).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Channel toggle buttons | Properties > Material | all channels on (unverified) | done | crates/ui/src/panels/properties.rs:1024 | - |
| Disabled channel is left untouched by the layer | Properties > Material | n/a | done | crates/ui/src/panels/properties.rs:214 | - |
| Material mode button (one Substance / material preset drives all channels) | Properties > Material | No Resource Selected | missing: inert placeholder box | crates/ui/src/panels/properties.rs:1044 | - |
| Click Material mode to pick a material from a mini browser | Properties > Material | n/a | missing | - | - |
| Drop a material from Assets onto Material mode | Properties > Material | n/a | partial: Shelf materials apply to the selected fill layer from the Shelf, not by dropping on Properties | crates/app/src/shelf_apply.rs:29 | - |
| Remove the material (back to per-channel slots) | Properties > Material | n/a | missing | - | - |
| Substance (.sbsar) material parameters listed under Material mode | Properties > Material | per resource | missing | - | - |
| Substance output size / resolution parameter | Properties > Material | (unverified) | missing | - | - |
| Seed value | Properties > Material > parameters | per resource | partial: Seed number on SubCraft's built-in procedural sources only | crates/ui/src/panels/properties.rs:782 | - |
| Random (new seed) button | Properties > Material > parameters | n/a (unverified) | missing | - | - |
| Per-channel uniform colour (opens the Color Picker) | Properties > Material > channel row | channel dependent | done: SubCraft's colour picker (wheel, sliders, hex, recent colours, eyedropper) on every swatch | crates/ui/src/color_picker.rs (swatch_linear, screen_pick) | #141 |
| Eyedropper next to the uniform colour | Properties > Material > channel row | n/a | partial: "Pick a colour" in every colour picker (every swatch is SubCraft's picker now); the next click on the model takes its base colour, anywhere else in the window the colour shown there (screenshot); not outside the window | crates/ui/src/color_picker.rs (swatch_linear, screen_pick) | #141 |
| Per-channel uniform value slider (grayscale channels) | Properties > Material > channel row | 0-1 | done | crates/ui/src/widgets.rs:60 | - |
| Height uniform value | Properties > Material > Height | -1 to 1 | done | crates/ui/src/widgets.rs:56 | - |
| Per-channel resource slot (bitmap, Substance, text, vector graphic) | Properties > Material > channel row | empty | partial: bitmap images only | crates/ui/src/panels/properties.rs:868 | - |
| Click the slot to choose a resource from a mini asset browser | Properties > Material > channel row | n/a | partial: opens a file dialog | crates/ui/src/panels/properties.rs:221 | - |
| Drag a texture or an alpha from Assets onto a channel slot | Properties > Material > channel row | n/a | done (alphas: every channel but the normal maps) | crates/ui/src/panels/properties.rs:299 | - |
| Remove the resource from a slot (back to the uniform colour) | Properties > Material > channel row | n/a | partial: switch the source dropdown back to "Uniform" | crates/ui/src/panels/properties.rs:232 | - |
| Per-slot Substance parameters (procedural / grunge in one channel) | Properties > Material > channel row | per resource | partial: built-in procedural patterns with Scale, Balance, Contrast, Seed, Low, High, Invert; bitmap images with Balance, Contrast, Invert (not normal maps) | crates/ui/src/panels/properties.rs:772 | - |
| Bitmap colour space dropdown on a slot (sRGB / linear, normal DirectX / OpenGL) | Properties > Material > slot arrow | auto; normals DirectX | missing: colour space is fixed by channel type | - | - |
| Baked mesh map used in a slot (for example baked AO or Normal in its channel) | Properties > Material > channel row | n/a | done | crates/ui/src/panels/properties.rs:789 | - |
| Anchor point referenced from a slot | Properties > Material > slot | n/a | done | crates/ui/src/panels/properties.rs:240 | - |
| Text resource parameters under a slot holding a font | Properties > Material > channel row | see properties-paint.md | missing | - | - |
| Vector graphic parameters under a slot holding an .svg / .ai | Properties > Material > channel row | see properties-paint.md | missing | - | - |
| Image sequence (UDIM) in a slot | Properties > Material > channel row | n/a | missing | - | - |

## Mask fill effect (Grayscale)

Layout notes: the FILL block is identical (projection, UV transformations); the second block is GRAYSCALE with a single uniform value slider and one resource slot.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Projection settings identical to a fill layer | Properties > Fill | n/a | partial: compact editor with UV / Tri-planar / Planar, tiling, rotation, offset | crates/ui/src/panels/properties.rs:654 | - |
| Grayscale uniform value | Properties > Grayscale | 0-1; default not documented | done | crates/ui/src/panels/properties.rs:653 | - |
| Grayscale resource slot (bitmap, Substance, text, vector graphic) | Properties > Grayscale | empty | partial: bitmaps go through a separate "Image" mask effect; built-in procedurals via a checkbox | crates/ui/src/panels/properties.rs:667 | - |
| Substance parameters of the grayscale resource (including seed) | Properties > Grayscale | per resource | partial: built-in pattern controls (Pattern, Scale, Balance, Contrast, Seed, Invert) | crates/ui/src/panels/properties.rs:644 | - |
| Effect blending mode and opacity (set on the effect row in the layer stack) | Layer stack | Normal, 100 (unverified) | partial: shown inside Properties instead of on the stack row | crates/ui/src/panels/properties.rs:505 | - |
