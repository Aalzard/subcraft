# Painter parity: Properties panel, painting tools

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/properties , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/color-picker , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/miscellaneous/sliders , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/tool-list , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/paint-brush , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/eraser , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/polygon-fill , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/smudge-tool , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/clone-tool , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/path , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/filled-path , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/ribbon-tool , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/quick-mask , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/creating-and-saving-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/creating-particles-pre/creating-particles-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/creating-particles-pre/installing-the-particle-editor , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/photoshop-brush-presets/photoshop-brush-presets-abr , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/photoshop-brush-presets/importing-photoshop-brush-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/photoshop-brush-presets/exporting-brush-presets-from-photoshop , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/photoshop-brush-presets/photoshop-brush-parameters-compatibility , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/dynamic-strokes/dynamic-strokes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/dynamic-strokes/enabling-dynamic-stroke-feature , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/dynamic-strokes/dynamic-stroke-performances , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/dynamic-strokes/creating-custom-dynamic-strokes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/advanced-channel-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/ambient-occlusion-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/flow-map-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/height-map-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/normal-map-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/text-resource , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/vector-graphic-svg , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/lazy-mouse , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/straight-line , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/paint
Checked against: origin/main 297dcb3 (2026-11-11)

Scope note: this file covers what sits inside the Properties panel when a painting tool is active, plus the Color Picker window and the generic slider behaviour. Tool selection, the contextual toolbar (Size / Flow / Stroke opacity / Spacing shortcuts, Lazy mouse, Symmetry, path editing buttons, Paths panel) and viewport gestures (stencil S-drags, straight line, clone source key) belong to the tools/viewport checklist and are not repeated here.

## Panel behaviour

Layout notes: Painter shows one Properties panel whose content follows the selection: tool properties for a paint layer or paint effect, fill properties for a fill layer/effect. For a paint tool the order top to bottom is: previews, (Presets, path tools only), Brush, Alpha, (Physics, physical tools only), Stencil, Material (or Grayscale on a mask). Each block is a collapsible group with an upper-case title. SubCraft's order today is: preview, Paint target, Presets, Text, Stencil, Material, Brush, Jitter, Smudge, Scatter (crates/ui/src/panels/brush.rs:18-163).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Properties panel shows tool parameters when a paint layer / paint effect is selected | Properties | n/a | done | crates/ui/src/panels/properties.rs:22 (shows_brush) | - |
| Open Properties from the dock toolbar | Dock toolbar | n/a | partial: dock tab, no dock-toolbar icon strip | crates/ui/src/app.rs:872 | - |
| Right-click in the viewport opens a floating Properties at the cursor | Viewport | n/a | missing | - | #131 |
| Right-click inside Properties opens the preset context menu | Properties (paint tool) | n/a | done | crates/ui/src/panels/brush.rs (properties_menu_id) | - |
| Section order: Brush, Alpha, Stencil, Material | Properties (paint tool) | n/a | done | crates/ui/src/panels/brush.rs (show) | - |
| Collapsible parameter groups | Properties | open | done | crates/ui/src/widgets.rs:103 (section) | - |

## Sliders (all panels)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Drag a slider to change its value | Any slider | n/a | done | crates/ui/src/widgets.rs:192 (slider_row_boxed) | - |
| Precision drag: hold Shift while dragging | Any slider | n/a | partial: Shift precision only on the toolbar drag-numbers, not on panel sliders | crates/ui/src/brush_bar.rs:162 | - |
| Jump: click anywhere on the slider | Any slider | n/a | done | crates/ui/src/widgets.rs:192 | - |
| Up / Down arrow keys step a focused slider | Any slider | n/a | partial: egui default arrow stepping, step size not tuned per parameter | crates/ui/src/widgets.rs:192 | - |
| Value edition: click the number field to type | Any slider | n/a | partial: value box sits beside the slider, not above it | crates/ui/src/widgets.rs:35 (quiet_value_field) | - |

## Preview

Layout notes: two thumbnails side by side at the very top of the tool properties: a brush stroke preview on the left, a material sphere preview on the right.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Brush Preview (stroke rendered from the brush parameters) | Properties > top | n/a | partial: one stamp plus a fixed example stroke; ignores jitter, follow path, material | crates/ui/src/panels/brush_header.rs:56 | - |
| Click-drag in the Brush Preview to draw a custom test stroke | Properties > top | n/a | missing | - | - |
| Material Preview (lit sphere of the current paint material) | Properties > top | n/a | missing | - | - |
| Click-drag in the Material Preview to rotate its lighting | Properties > top | n/a | missing | - | - |

## Brush

Layout notes: one slider per row, label above. Size, Flow and the jitters carry a small pen-pressure toggle button at the right of the row. Alignment and Size Space are dropdowns; Follow Path and Backface Culling are toggles, the culling angle slider sits under its toggle.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Size | Properties > Brush | not documented | done | crates/ui/src/panels/brush_rows.rs (size_flow_opacity) | - |
| Size: pen pressure toggle | Properties > Brush > Size | not documented | done | crates/ui/src/panels/brush_rows.rs (pressure_toggle) | - |
| Minimum Size (lower bound when pressure drives size) | Properties > Brush > Size | not documented | done | crates/paint/src/settings.rs (BrushSettings::min_size) | - |
| Flow | Properties > Brush | not documented | done | crates/ui/src/panels/brush_rows.rs (size_flow_opacity) | - |
| Flow: pen pressure toggle | Properties > Brush > Flow | not documented | done | crates/ui/src/panels/brush_rows.rs (pressure_toggle) | - |
| Minimum Flow | Properties > Brush > Flow | not documented | done | crates/paint/src/settings.rs (BrushSettings::min_flow) | - |
| Stroke Opacity (no pressure control) | Properties > Brush | not documented | done | crates/ui/src/panels/brush_rows.rs (size_flow_opacity) | - |
| Spacing | Properties > Brush | not documented | partial: 0.02-2.0 brush diameters (log), default 0.1; Painter's unit/range unconfirmed | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Angle | Properties > Brush | not documented | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Follow Path | Properties > Brush | off (unverified) | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Size Jitter | Properties > Brush | 0; 0-1 | done | crates/ui/src/panels/brush_rows.rs (jitters) | - |
| Size Jitter: pen pressure toggle | Properties > Brush > Size Jitter | (unverified) | done | crates/ui/src/panels/brush_rows.rs (jitters); pressure scales the jitter | - |
| Flow Jitter | Properties > Brush | 0; 0-1 | done | crates/ui/src/panels/brush_rows.rs (jitters) | - |
| Flow Jitter: pen pressure toggle | Properties > Brush > Flow Jitter | (unverified) | done | crates/ui/src/panels/brush_rows.rs (jitters); pressure scales the jitter | - |
| Angle Jitter | Properties > Brush | 0; 0-1 | done | crates/ui/src/panels/brush_rows.rs (jitters) | - |
| Angle Jitter: pen pressure toggle | Properties > Brush > Angle Jitter | (unverified) | done | crates/ui/src/panels/brush_rows.rs (jitters); pressure scales the jitter | - |
| Position Jitter | Properties > Brush | 0; 0-1 | done | crates/ui/src/panels/brush_rows.rs (jitters) | - |
| Position Jitter: pen pressure toggle | Properties > Brush > Position Jitter | (unverified) | done | crates/ui/src/panels/brush_rows.rs (jitters); pressure scales the jitter | - |
| Position Jitter Axis (one axis / both axes) | Properties > Brush | not documented | done: Both axes (default) or One axis (only across the stroke); which axis Painter uses is not documented | crates/paint/src/settings.rs (JitterAxis); crates/ui/src/panels/brush_rows.rs (jitters) | - |
| Stamps Blending (how overlapping stamps of one stroke combine, e.g. Lighten) | Properties > Brush | not documented | done: Accumulate (default, the old behaviour) or Lighten (a texel keeps its strongest stamp); Painter may list more modes | crates/paint/src/settings.rs (StampBlend); crates/paint/src/kernel.rs (accumulate) | - |
| Alignment (dropdown) | Properties > Brush | Tangent &#124; Wrap | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Alignment: Camera | Properties > Brush > Alignment | option | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Alignment: Tangent &#124; Wrap | Properties > Brush > Alignment | default | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Alignment: Tangent &#124; Planar | Properties > Brush > Alignment | option | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Alignment: UV | Properties > Brush > Alignment | option | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Backface Culling (toggle) | Properties > Brush | on (unverified) | done | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Backface Culling angle | Properties > Brush | 90 (unverified); range not documented | partial: range 10-90 degrees | crates/ui/src/panels/brush.rs (show, Brush block) | - |
| Size Space (dropdown) | Properties > Brush | Object | done | crates/ui/src/panels/brush_rows.rs (size_space) | - |
| Size Space: Object | Properties > Brush > Size Space | default | done | crates/paint/src/settings.rs (BrushSettings::radius_in) | - |
| Size Space: Viewport | Properties > Brush > Size Space | option | partial: 3D view only; the 2D view uses the object size | crates/paint/src/stroke.rs (Stroke::base_radius) | - |
| Size Space: Texture | Properties > Brush > Size Space | option | done | crates/paint/src/stroke.rs (Stroke::base_radius) | - |
| Continue the previous stroke (A) so stroke opacity keeps accumulating | Viewport, described with Stroke Opacity | n/a | missing | - | - |

## Alpha

Layout notes: a single resource slot (thumbnail button, name, remove cross) followed by the exposed parameters of the loaded Substance, if any. When the resource supports Dynamic strokes, that block is inserted before the Substance parameters.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Alpha resource slot (bitmap or Substance) | Properties > Alpha | preset dependent | partial: bitmap tips only, no Substance alphas | crates/ui/src/panels/brush_rows.rs (alpha_slot) | - |
| Click the slot to open a mini asset browser | Properties > Alpha | n/a | done | crates/ui/src/panels/brush_rows.rs (mini_browser) | - |
| Drag an alpha from Assets onto the slot | Properties > Alpha | n/a | done | crates/ui/src/panels/brush_rows.rs (alpha_slot) | - |
| Remove the alpha (cross on the slot) | Properties > Alpha | n/a | done | crates/ui/src/panels/brush_rows.rs (alpha_slot) | - |
| Substance alpha: exposed graph parameters listed under the slot | Properties > Alpha | per resource | missing | - | - |
| Hardness (when the alpha Substance exposes a "hardness" input; driven by the hardness shortcuts) | Properties > Alpha | per resource | partial: built-in Hardness for every tip, shown in the Alpha block | crates/ui/src/panels/brush.rs (show, Alpha block) | - |
| Text resource as alpha (font dropped in the slot) | Properties > Alpha | n/a | partial: "Use as brush tip" button in the Text section | crates/ui/src/text_panel.rs:277 | - |
| Vector graphic (.svg / .ai) as alpha | Properties > Alpha | n/a | missing | - | - |

## Dynamic strokes

Layout notes: appears inside the Alpha (or Material) block, just above the Substance parameter group, only when the loaded Substance exposes stamp index, time or random seed. Compatible slots: Paint and Eraser (Alpha, Material); Projection, Smudge, Clone (Alpha only). Disabled for tools in Physics mode.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Dynamic Controls (read-only list of the dynamic inputs of the resource) | Properties > Alpha / Material > Dynamic stroke | per resource | missing | - | - |
| Stamp Start (dropdown) | Properties > Dynamic stroke | From beginning (0) | missing | - | - |
| Stamp Start: From beginning (0) | Properties > Dynamic stroke > Stamp Start | default | missing | - | - |
| Stamp Start: From Random Index | Properties > Dynamic stroke > Stamp Start | option | missing | - | - |
| Stamp Cycle Count | Properties > Dynamic stroke | default/min/max read from the Substance | missing | - | - |
| Random Seed Type (dropdown) | Properties > Dynamic stroke | Single | missing | - | - |
| Random Seed Type: Single | Properties > Dynamic stroke > Random Seed Type | default | missing | - | - |
| Random Seed Type: Random Per Stroke | Properties > Dynamic stroke > Random Seed Type | option | missing | - | - |
| Random Seed Type: Random Per Stamp | Properties > Dynamic stroke > Random Seed Type | option | missing | - | - |
| Time dynamic control (no parameter; driven by stroke duration) | Properties > Dynamic stroke | n/a | missing | - | - |
| Graph inputs fed per stamp: stampIndex, stampCycleCount, $time, strokeSpacing, strokeSize, stampStrokePosition | Substance resource | n/a | missing | - | - |
| Graph inputs fed on paths: distanceAlongCurve, distanceMaxCurve, pathCorner, pathCornerAngle, patchLengthOnCurve | Substance resource | n/a | missing | - | - |
| Graph user tags that gate regeneration: IsStampIndexActive, IsRandomSeedActive, IsTimeActive | Substance resource | n/a | missing | - | - |
| Assets badge and search keywords for compatible resources (dynamicstroke, randomseed) | Assets | n/a | missing | - | - |

## Physics (particles)

Layout notes: this block only exists when the tool is in its Physical variant (toolbar toggle or a particle brush preset). It holds the particle (PopcornFX emitter/receiver) preset slot and its exposed parameters. The help pages do not list the individual parameters.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Physics block shown for Physical paint / eraser / projection | Properties > Physics | hidden | missing | - | - |
| Particle preset slot (emitter + receiver) | Properties > Physics | preset dependent | partial: no particle engine; a "Scatter" block (count, radius, size/opacity random, rotation, fall) approximates sprays and drips | crates/ui/src/panels/brush.rs:150 | - |
| Selecting a particle brush preset in Assets switches the tool to Physical | Assets | n/a | missing | - | - |
| Exposed emitter / receiver parameters of the particle preset | Properties > Physics | not documented | missing | - | - |
| Custom particle scripts (.pkfx made in the PopcornFX editor) | Preset authoring | n/a | n/a: third-party editor pipeline | - | - |

## Stencil

Layout notes: a resource slot, then the tiling mode dropdown. Stencil transform is done in the viewport (hold S), with a Reset button at the top right of the viewport while S is held.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Stencil resource slot | Properties > Stencil | empty (no effect) | partial: "Choose image" file dialog only; no asset slot, no drop from the shelf | crates/ui/src/panels/brush.rs:67 | - |
| Remove the stencil | Properties > Stencil | n/a | done | crates/ui/src/panels/brush.rs:70 | - |
| Substance stencil: exposed graph parameters | Properties > Stencil | per resource | missing | - | - |
| Tiling mode (dropdown) | Properties > Stencil | No Tiling | done | crates/ui/src/panels/brush.rs (show, Stencil block) | - |
| Tiling mode: No Tiling | Properties > Stencil > Tiling mode | default | done | crates/paint/src/stencil.rs (StencilTiling) | - |
| Tiling mode: Horizontal Tiling | Properties > Stencil > Tiling mode | option | done | crates/paint/src/stencil.rs (StencilTiling) | - |
| Tiling mode: Vertical Tiling | Properties > Stencil > Tiling mode | option | done | crates/paint/src/stencil.rs (StencilTiling) | - |
| Tiling mode: H and V Tiling | Properties > Stencil > Tiling mode | option | done | crates/paint/src/stencil.rs (StencilTiling) | - |
| Reset stencil transform (button at the viewport's top right while S is held) | Viewport | n/a | done; also a Reset button in the Stencil block | crates/ui/src/viewport.rs (reset_stencil) | - |
| Text resource / vector graphic as stencil | Properties > Stencil | n/a | missing | - | - |

## Material

Layout notes: a row of channel toggle buttons (one per channel of the Texture Set, short names), then a wide "Material mode" button that takes one Substance / material preset for all channels at once, then one row per enabled channel: label, uniform colour or value slider, a resource slot, and an eyedropper for colour channels. With a material loaded the per-channel rows are replaced by the material's parameters. On a mask the block is named Grayscale and holds a single value/slot.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Channel toggle buttons (one per Texture Set channel) | Properties > Material | all on (unverified) | done | crates/ui/src/panels/brush_material.rs:42 | - |
| Deselected channel is neither editable nor painted | Properties > Material | n/a | done | crates/ui/src/panels/brush_material.rs:54 | - |
| Material mode button (load one Substance / preset for all channels) | Properties > Material | "No Resource Selected" | partial: "Paint whole material" picks a set of named map files; no material resource slot | crates/ui/src/panels/brush_material.rs:24 | - |
| Drop a material from Assets on Material mode | Properties > Material | n/a | missing | - | - |
| Material parameters of the loaded Substance | Properties > Material | per resource | missing | - | - |
| Random seed / Random button of the loaded Substance | Properties > Material | per resource (unverified) | missing | - | - |
| Per-channel uniform colour (Base color, Emissive ...) opens the Color Picker | Properties > Material > channel row | channel dependent | partial: egui inline picker, no Painter picker window | crates/ui/src/widgets.rs:51 | #141 |
| Eyedropper button next to each colour field | Properties > Material > channel row | n/a | missing | - | #141 |
| Per-channel uniform value slider (Roughness, Metallic, AO ...) | Properties > Material > channel row | 0-1 | done | crates/ui/src/widgets.rs:60 | - |
| Height uniform value | Properties > Material > Height | -1 to 1 | done | crates/ui/src/widgets.rs:56 | - |
| Per-channel resource slot (bitmap or Substance) | Properties > Material > channel row | empty | partial: bitmaps only (file dialog or Shelf texture drop) | crates/ui/src/panels/brush_material.rs:72 | - |
| Remove the resource from a channel slot | Properties > Material > channel row | n/a | done | crates/ui/src/panels/brush_material.rs:84 | - |
| Bitmap / Substance painted in Height is remapped from 0..1 to -1..1 (mid grey = 0) | Properties > Material > Height | n/a | partial: dedicated Height stamp brush with a strength slider; plain channel images are not remapped | crates/ui/src/panels/stamp_brush.rs:77 | - |
| Normal slot: colour space / format dropdown (DirectX default, OpenGL) | Properties > Material > Normal slot arrow | DirectX | partial: "Flip green" checkbox on the normal stamp brush only | crates/ui/src/panels/stamp_brush.rs:86 | - |
| Bitmap slot colour space dropdown (other channels) | Properties > Material > slot arrow | auto | missing | - | - |
| Flow-map style painting: Follow Path + flat normal bitmap in the Normal slot | Properties > Brush + Material | n/a | partial: needs a normal image in the brush Normal channel plus Follow path; not verified end to end | crates/ui/src/panels/brush.rs:126 | - |
| Grayscale block replaces Material when painting a mask | Properties > Grayscale | 1 (white) (unverified) | partial: "Mask value" slider only, no resource slot | crates/ui/src/panels/brush.rs:94 | - |
| Text resource in a channel slot | Properties > Material > channel row | n/a | missing | - | - |
| Vector graphic in a channel slot | Properties > Material > channel row | n/a | missing | - | - |

## Paint tool

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Paint: Preview, Brush, Alpha, Stencil, Material blocks | Properties | n/a | done | crates/ui/src/panels/brush.rs (show) | - |
| Physical paint: adds the Physics block | Properties | n/a | missing | - | - |
| Dynamic stroke on Alpha and Material slots | Properties | n/a | missing | - | - |

## Eraser

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Eraser shares Brush, Alpha and Stencil with Paint | Properties | n/a | done | crates/ui/src/panels/brush.rs:101 | - |
| Material block reduced to channel toggles (no colours, no slots) | Properties > Material | all channels on | done | crates/ui/src/panels/brush.rs (eraser_channels) | - |
| Erase only the selected channels | Properties > Material | n/a | done | crates/ui/src/panels/brush.rs (eraser_channels); crates/app/tests/tool_gaps.rs | - |
| Physical eraser (particles) | Properties > Physics | n/a | missing | - | - |

## Projection tool

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Projection tool (paint a material tiled in screen space) | Tools + Properties | n/a | partial: no tool; "Project from the view" checkbox on brush images | crates/ui/src/panels/brush_material.rs:126 | - |
| Projection: Brush, Alpha, Stencil, Material blocks | Properties | n/a | missing | - | - |
| Projection pattern transformed like a stencil (S + drag in the viewport) | Viewport | n/a | missing | - | - |
| Physical projection (particles) | Properties > Physics | n/a | missing | - | - |

## Polygon Fill

Layout notes: Selection mode icons first, then Fill mode icons (only meaningful with the rectangular marquee), then Hit-test, then Blending (mode + opacity), then the Material / Grayscale block.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Selection mode: Rectangular marquee fill | Properties > Polygon Fill > Selection mode | default (unverified) | missing: click fills one element, no box drag | - | #131 |
| Selection mode: Polygon loop fill | Properties > Polygon Fill > Selection mode | option | missing | - | - |
| Selection mode: Partial polygon loop fill | Properties > Polygon Fill > Selection mode | option | missing | - | - |
| Fill mode: Triangle fill | Properties > Polygon Fill > Fill mode | option | done | crates/ui/src/panels/brush.rs:26 | - |
| Fill mode: Polygon fill (source quads / n-gons) | Properties > Polygon Fill > Fill mode | option | missing | - | #131 |
| Fill mode: Mesh fill (connected polygons) | Properties > Polygon Fill > Fill mode | option | done: "Mesh" | crates/ui/src/brush_bar.rs | #131 |
| Fill mode: UV chunk fill | Properties > Polygon Fill > Fill mode | option | done: "UV island" | crates/ui/src/brush_bar.rs | - |
| Hit-test: Intersection | Properties > Polygon Fill > Hit-test | default | missing | - | #131 |
| Hit-test: Contained center | Properties > Polygon Fill > Hit-test | option | missing | - | - |
| Visible polygons only | Properties > Polygon Fill > Hit-test | not documented | missing | - | #131 |
| Blending Mode (same list as layer blending modes) | Properties > Polygon Fill > Blending | Normal (unverified) | missing | - | - |
| Opacity | Properties > Polygon Fill > Blending | not documented | missing | - | - |
| Material block: channel toggles and values used by the fill | Properties > Material | n/a | done | crates/ui/src/panels/brush_material.rs:16 | - |
| Grayscale value when filling a mask | Properties > Grayscale | not documented | partial: "Mask value" slider | crates/ui/src/panels/brush.rs:94 | - |
| Alt: extend a loop on both sides of the first click | Viewport (loop modes) | n/a | missing | - | - |
| Shift: partial loop fills only the primary loop | Viewport (partial loop) | n/a | missing | - | - |
| Shift+A: temporary Polygon loop fill | Viewport | n/a | missing | - | - |
| A while dragging: combine with the previous result | Viewport | n/a | missing | - | - |
| Wireframe of the active Texture Set shown while the tool is active | Viewport | n/a | missing | - | - |

## Smudge tool

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Smudge shares Brush, Alpha and Stencil parameters with Paint | Properties | n/a | done | crates/ui/src/panels/brush.rs:101 | - |
| Smudge through layers below via Pass through blending on the smudge layer | Layer stack + tool | n/a | missing | - | - |
| Dynamic stroke on the Alpha slot | Properties > Alpha | n/a | missing | - | - |

## Clone tool

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Clone shares Brush, Alpha and Stencil parameters with Paint | Properties | n/a | done | crates/ui/src/panels/brush.rs:101 | - |
| Clone source behaviour: relative (source follows after each stroke) | Tool variant / button | default | partial: "Source" dropdown in Properties, not a toolbar variant | crates/app/src/session.rs (CloneSourceMode) | - |
| Clone source behaviour: absolute (source returns to where V set it) | Tool variant / button | option | partial: "Source" dropdown in Properties, not a toolbar variant | crates/app/src/session.rs (CloneSourceMode) | - |
| Clone from all layers below via Pass through blending | Layer stack + tool | n/a | missing | - | - |
| Dynamic stroke on the Alpha slot | Properties > Alpha | n/a | missing | - | - |

## Path tools: common properties

Layout notes: with a path tool active the Properties panel starts with a Presets strip (thumbnails, a Favorites entry, a link to show compatible presets in Assets), then Brush / Alpha / Material as for Paint, with two extra Brush rows. Per-point values (pressure, tangents) are edited from the contextual toolbar, not from Properties. SubCraft's Pen path is a UV-space vector stroke with its own short property list (crates/ui/src/pen.rs:88).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Path type: Paint along path | Properties / toolbar menu | default | partial: a continuous stroke, or the brush tip stamped along the path with spacing and Follow path (UV space); no jitter or per-stamp pressure | crates/paint/src/pen.rs (run_stamped), crates/ui/src/pen.rs | - |
| Path type: Ribbon path | Properties / toolbar menu | option | done: an image laid along the path over its width, square ends | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Path type: Filled path | Properties / toolbar menu | option | done: Path type dropdown in the pen Properties; the interior is filled (even-odd), anti-aliased, closing on the first point; saved with the project | crates/paint/src/pen.rs (run_filled), crates/ui/src/pen.rs | - |
| Path type: Erase along path | Properties / toolbar menu | option | missing | - | - |
| Path type: Smudge along path | Properties / toolbar menu | option | missing | - | - |
| Brush block applies to the whole path | Properties > Brush | n/a | partial: Width, Hardness, Opacity only | crates/ui/src/pen.rs:100 | - |
| Alpha block applies to the whole path | Properties > Alpha | n/a | partial: "Stamp the brush tip" stores the current tip with the project and stamps it along the path; no separate Alpha block | crates/app/src/session.rs (brush_tip_resource) | - |
| Material block applies to the whole path | Properties > Material | n/a | partial: channel checkboxes with a uniform value each | crates/ui/src/pen.rs:107 | - |
| Projection depth | Properties > Brush (path only) | not documented | missing | - | - |
| Up axis (stamp orientation when Follow path is off) | Properties > Brush (path only) | not documented | missing | - | - |
| Presets section at the top of Properties | Properties > Presets | n/a | missing | - | - |
| Presets: Favorites entry | Properties > Presets | empty | missing | - | - |
| Presets: Show compatible presets in assets | Properties > Presets | n/a | missing | - | - |
| Preset right-click: Add to favorites | Properties > Presets / Assets | n/a | partial: from the Assets preset menu; no Presets strip in Properties | crates/ui/src/panels/shelf.rs (preset_menu) | - |
| Preset right-click: Remove from favorites | Properties > Presets / Assets | n/a | partial: from the Assets preset menu; no Presets strip in Properties | crates/ui/src/panels/shelf.rs (preset_menu) | - |
| Create tool preset from a path tool (selecting it re-equips the path tool) | Properties right-click | n/a | missing | - | - |
| Open / close path | Contextual toolbar | open | partial: "Closed path" checkbox in Properties | crates/ui/src/pen.rs:98 | - |
| Toggle corner / smooth on the selected point | Contextual toolbar / double-click | smooth | partial: Corner / Smooth buttons in Properties | crates/ui/src/pen.rs:92 | - |
| Delete vertex | Contextual toolbar | n/a | partial: "Delete point" button in Properties | crates/ui/src/pen.rs:95 | - |

## Ribbon path

Layout notes: groups in order: Size, Opacity, Stroke, Stretching & Tiling (a mode switch that swaps the rows below it), Channel blending, then Alpha / Material.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Stroke width | Properties > Size | not documented | done: Width (% UV) | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Stroke opacity | Properties > Opacity | not documented | done: Opacity | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Image orientation | Properties > Stroke | not documented | missing | - | - |
| Flip image | Properties > Stroke | off (unverified) | done | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Corner (dropdown) | Properties > Stroke | not documented | missing | - | - |
| Corner: Miter join | Properties > Stroke > Corner | option | missing | - | - |
| Corner: Round join | Properties > Stroke > Corner | option | missing | - | - |
| Corner: Bevel join | Properties > Stroke > Corner | option | missing | - | - |
| Corner: Cut join | Properties > Stroke > Corner | option | missing | - | - |
| Omit ends when closed | Properties > Stroke | not documented | missing | - | - |
| Mode: Stretch along path | Properties > Stretching & Tiling | default | done: Copies over the length | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Mode: Keep aspect ratio | Properties > Stretching & Tiling | option | done: copies keep the image's aspect at the path width, scaled by Aspect factor | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Stretch between offsets only | Stretching & Tiling (Stretch along path) | not documented | missing | - | - |
| Start offset | Stretching & Tiling (Stretch along path) | not documented | missing | - | - |
| End offset | Stretching & Tiling (Stretch along path) | not documented | missing | - | - |
| Tiling mode (dropdown) | Stretching & Tiling (both modes) | Auto | missing | - | - |
| Tiling mode: None | Stretching & Tiling > Tiling mode | option | missing | - | - |
| Tiling mode: Auto | Stretching & Tiling > Tiling mode | default | missing | - | - |
| Tiling mode: Custom | Stretching & Tiling > Tiling mode | option | missing | - | - |
| Tiling amount (Custom tiling) | Stretching & Tiling | not documented | partial: the Copies slider (no Auto / None modes) | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Mirror every 2nd tile | Stretching & Tiling (both modes) | not documented | done | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Aspect ratio factor | Stretching & Tiling (both modes) | not documented | done | crates/paint/src/pen.rs (run_ribbon), crates/ui/src/pen.rs (ribbon_rows) | - |
| Ratio (dropdown) | Stretching & Tiling (Keep aspect ratio) | Fit to path width | missing | - | - |
| Ratio: Fit to path width | Stretching & Tiling > Ratio | default | missing | - | - |
| Ratio: Fit to path length | Stretching & Tiling > Ratio | option | missing | - | - |
| Remove clipped tiles (disabled with Fit to path length) | Stretching & Tiling (Keep aspect ratio) | not documented | missing | - | - |
| Alignment (dropdown) | Stretching & Tiling (Keep aspect ratio) | not documented | missing | - | - |
| Alignment: Align at start | Stretching & Tiling > Alignment | option | missing | - | - |
| Alignment: Align at center | Stretching & Tiling > Alignment | option | missing | - | - |
| Alignment: Align at end | Stretching & Tiling > Alignment | option | missing | - | - |
| Channel blending: Alpha (dropdown) | Properties > Channel blending | Lighten (Max) | missing | - | - |
| Alpha blending: Normal | Channel blending > Alpha | option | missing | - | - |
| Alpha blending: Lighten (Max) | Channel blending > Alpha | default | missing | - | - |
| Alpha blending: Linear dodge (Add) | Channel blending > Alpha | option | missing | - | - |
| Channel blending: Normal (dropdown; disabled for a uniform colour) | Properties > Channel blending | Normal map combine | missing | - | - |
| Normal blending: Normal | Channel blending > Normal | option | missing | - | - |
| Normal blending: Normal map combine | Channel blending > Normal | default | missing | - | - |
| Normal blending: Normal map details | Channel blending > Normal | option | missing | - | - |
| Channel blending: Height (dropdown; disabled for a uniform colour) | Properties > Channel blending | Lighten (Max) | missing | - | - |
| Height blending: Normal | Channel blending > Height | option | missing | - | - |
| Height blending: Linear dodge (Add) | Channel blending > Height | option | missing | - | - |
| Height blending: Darken (Min) | Channel blending > Height | option | missing | - | - |
| Height blending: Lighten (Max) | Channel blending > Height | default | missing | - | - |
| Height blending: Screen | Channel blending > Height | option | missing | - | - |
| Text resource / non-square image auto-scaled to the ribbon | Properties > Alpha / Material | n/a | missing | - | - |

## Filled path

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Filled path: interior filled with the uniform Material values | Properties > Material | n/a | done: the path's per-channel values | crates/paint/src/pen.rs (run_filled) | - |
| Opacity | Properties > Filled path settings | not documented | done: the path Opacity slider | crates/ui/src/pen.rs | - |

## Quick mask

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Quick mask edit mode (paint a temporary mask with the current brush) | Tools / U (docs also cite Y) | off | done | crates/app/src/session.rs | - |
| Leave quick mask edit and paint through the mask (U) | Shortcut | n/a | done | crates/app/src/session.rs | - |
| Reset / disable the quick mask (Y) | Shortcut | n/a | done | crates/app/src/session.rs | - |
| Invert the quick mask (I) | Shortcut | n/a | done | crates/app/src/session.rs | - |

## Text resource parameters

Layout notes: shown under any slot that holds a font (brush alpha, stencil, material channel, fill channel, Substance image input). Basic rows first (Text, Font size, Alignment, Color), then an advanced group. SubCraft's text is a layer type ("text decal") edited from a Text section, not a resource (crates/ui/src/text_panel.rs:262).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Create a text resource by putting a font in a resource slot | Any resource slot | n/a | partial: Font row in the Text section; text is a decal layer or a brush tip, not a slot resource | crates/ui/src/text_panel.rs:90 | - |
| Drag a font from Assets onto the viewport | Viewport | n/a | missing | - | - |
| .ttf and .otf fonts (embeddable fonts only) | Assets import | n/a | partial: "Choose font file" embeds the file in the project; no licence-flag check | crates/ui/src/text_panel.rs:121 | - |
| Text | Slot > Text resource | not documented | done | crates/ui/src/text_panel.rs:84 | - |
| Font size: Auto | Slot > Text resource > Font size | default (unverified) | missing | - | - |
| Font size: Custom + size value | Slot > Text resource > Font size | not documented | partial: Size slider as % of texture width | crates/ui/src/text_panel.rs:127 | - |
| Alignment: horizontal buttons | Slot > Text resource > Alignment | not documented | done | crates/ui/src/text_panel.rs:134 | - |
| Alignment: vertical buttons | Slot > Text resource > Alignment | not documented | missing | - | - |
| Color (grayscale in a mask / grayscale channel) | Slot > Text resource | not documented | partial: one value per material channel | crates/ui/src/text_panel.rs:154 | - |
| Line spacing | Slot > Text resource > advanced | not documented | partial: range 0.5-3.0 | crates/ui/src/text_panel.rs:132 | - |
| Character spacing (may be negative) | Slot > Text resource > advanced | not documented | partial: "Letter spacing" -0.2 to 1.0 em | crates/ui/src/text_panel.rs:130 | - |
| Offset (horizontal, vertical; relative to font size) | Slot > Text resource > advanced | 0, 0 (unverified) | partial: Position X / Y in texture space | crates/ui/src/text_panel.rs:146 | - |
| Background fill (colour) | Slot > Text resource > advanced | not documented | missing | - | - |
| Background opacity | Slot > Text resource > advanced | not documented | missing | - | - |
| Resolution: Auto | Slot > Text resource > advanced | default (unverified) | missing | - | - |
| Resolution: Custom + value | Slot > Text resource > advanced | not documented | missing | - | - |

## Vector graphic (.svg / .ai) parameters

Layout notes: shown under any slot that holds a vector file; three groups: Artboard, Resolution, Crop area, plus Scope with a Change button that opens a separate window.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Import .svg as an image resource | Assets import | n/a | missing | - | - |
| Import Illustrator .ai as an image resource | Assets import | n/a | missing | - | - |
| Artboard (Illustrator files only) | Slot > Artboard | first artboard (unverified) | missing | - | - |
| Resolution: Auto (Texture Set resolution in fills; 512 px in a brush) | Slot > Resolution | default (unverified) | missing | - | - |
| Resolution: Asset (pixel size stored in the file) | Slot > Resolution | option | missing | - | - |
| Resolution: Custom | Slot > Resolution | option | missing | - | - |
| Custom resolution value | Slot > Resolution | not documented | missing | - | - |
| Crop to: Asset bounds | Slot > Crop area | default (unverified) | missing | - | - |
| Crop to: Custom | Slot > Crop area | option | missing | - | - |
| Square aspect ratio (with Asset bounds) | Slot > Crop area | not documented | missing | - | - |
| Top left (custom crop corner) | Slot > Crop area | not documented | missing | - | - |
| Bottom right (custom crop corner) | Slot > Crop area | not documented | missing | - | - |
| Scope | Slot > Scope | Document | missing | - | - |
| Scope: Change button | Slot > Scope | n/a | missing | - | - |
| Scope window: element list with include / exclude | Scope window | all included | missing | - | - |
| Scope window: Show thumbnails | Scope window | not documented | missing | - | - |
| Non-destructive re-rasterisation when resolution or the source file changes | Layer stack | n/a | missing | - | - |

## Tool presets

Layout notes: presets are created from a context menu opened by right-clicking empty space in the tool Properties; they then live in Assets (Brushes / Tools / Materials). Updating is done from the asset's own right-click menu.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Create tool preset (brush + material + tool type, resources embedded) | Properties right-click | n/a | done | crates/app/src/brush_preset.rs (Session::create_preset) | - |
| Create material preset (material values and resources only) | Properties right-click | n/a | done | crates/app/src/brush_preset.rs (Session::create_preset) | - |
| Create brush preset (brush parameters + alpha + stencil only) | Properties right-click | n/a | done | crates/app/src/brush_preset.rs (Session::create_preset) | - |
| Preset stores the pen-pressure toggles | Preset content | n/a | done | crates/app/src/brush_preset.rs (BrushPreset.settings) | - |
| Preset stores the stencil | Preset content | n/a | done | crates/app/src/brush_preset.rs (PresetStencil) | - |
| Preset embeds every resource it uses (portable file) | Preset content | n/a | partial: tip and channel images embedded | crates/app/src/brush_preset.rs:31 | - |
| Selecting a tool preset in Assets switches to its tool | Assets | n/a | done | crates/ui/src/state.rs (select_shelf_brush) | - |
| Selecting a brush preset applies it to the current tool | Assets | n/a | done | crates/app/src/brush_preset.rs:79 | - |
| Update from current tool (right-click a preset in Assets) | Assets right-click | n/a | done | crates/ui/src/panels/shelf.rs (apply_preset_action) | - |
| Particle brush presets | Assets | n/a | missing | - | - |
| Import Photoshop .abr brush presets (bitmap-tip brushes only) | Assets import | n/a | missing | - | - |
| ABR import scope: Project / Session / Library | Import resources window | n/a | missing | - | - |
| ABR: tip bitmap also added to Alphas under the preset name | Assets | n/a | missing | - | - |
| ABR mapping: Size, Angle, Spacing, Size Jitter, Angle Jitter, Scatter to Position Jitter, Flow Jitter | Import | n/a | missing | - | - |
| ABR mapping: pressure control for Size and Flow with minimum values | Import | n/a | missing | - | - |
| ABR mapping: Flip X/Y, Roundness, Hardness and their jitters through a dedicated brush-maker Substance | Import | n/a | missing | - | - |

## Color Picker window

Layout notes: a floating, movable, non-dockable window opened by clicking any colour field. It stays open until the context changes (for example going from a paint layer to a fill layer). Vertical layout in three parts: gradient picker (display selector, spectrum square with a vertical hue bar, current/previous colour swatches, hex field, eyedropper), sliders (mode dropdown HSV or RGB, three sliders, a settings menu), swatches (add button, grid, settings menu). With colour management on, a Working color space read-out sits between sliders and swatches. SubCraft currently uses egui's stock popup picker on each colour field.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Clicking any colour field opens the Color Picker | Any colour field | n/a | partial: egui popup picker, closes on click away | crates/ui/src/widgets.rs:51 | #141 |
| Semi-persistent: stays open until the context changes | Color Picker | n/a | missing | - | #141 |
| Window can be moved to any screen | Color Picker | n/a | missing | - | #141 |
| Window cannot be docked | Color Picker | n/a | missing | - | #141 |
| Display selector (colour management only) | Color Picker > Gradient picker | viewport Display | missing | - | #141 |
| Spectrum: saturation/value field with crosshair | Color Picker > Gradient picker | n/a | partial: egui 2D field | crates/ui/src/widgets.rs:51 | #141 |
| Spectrum: vertical hue bar | Color Picker > Gradient picker | n/a | partial: egui hue bar, horizontal | crates/ui/src/widgets.rs:51 | #141 |
| HDR colours clamped to the working space when colour managed | Color Picker > Gradient picker | n/a | missing | - | #141 |
| Current colour rectangle | Color Picker > Gradient picker | n/a | partial: egui preview strip | crates/ui/src/widgets.rs:51 | #141 |
| Previous colour rectangle (colour when the picker opened) | Color Picker > Gradient picker | n/a | missing | - | #141 |
| Click the previous colour to restore it | Color Picker > Gradient picker | n/a | missing | - | #141 |
| Hexadecimal field (#RRGGBB, always sRGB) | Color Picker > Gradient picker | n/a | missing | - | #141 |
| Eyedropper button (pick from anywhere on screen, inside or outside the app) | Color Picker > Gradient picker | n/a | missing | - | #141 |
| Eyedropper + Shift in the viewport: pick the edited channel's raw value | Color Picker > Eyedropper | n/a | missing | - | #141 |
| Eyedropper next to colour fields (pick without opening the picker) | Any colour field | n/a | missing | - | #141 |
| Eyedropper color space setting | Color Picker > Color settings | auto (project standard sRGB) | missing | - | #141 |
| Slider mode dropdown | Color Picker > Sliders | not documented | missing | - | #141 |
| Slider mode: HSV | Color Picker > Sliders | option | missing | - | #141 |
| Slider mode: RGB | Color Picker > Sliders | option | partial: egui RGB number fields, no sliders | crates/ui/src/widgets.rs:51 | #141 |
| Hue slider | Color Picker > Sliders (HSV) | 0-1, or 0-360 degrees | missing | - | #141 |
| Saturation slider | Color Picker > Sliders (HSV) | 0-1, or 0-100 % | missing | - | #141 |
| Value slider | Color Picker > Sliders (HSV) | 0-1, or 0-100 % | missing | - | #141 |
| Red slider | Color Picker > Sliders (RGB) | 0-1, or 0-255 | missing | - | #141 |
| Green slider | Color Picker > Sliders (RGB) | 0-1, or 0-255 | missing | - | #141 |
| Blue slider | Color Picker > Sliders (RGB) | 0-1, or 0-255 | missing | - | #141 |
| Sliders settings: Dynamic sliders (slider backgrounds follow the colour) | Color Picker > Sliders settings menu | not documented | missing | - | #141 |
| Sliders settings: Floating point values (0.0-1.0 vs degrees / % / 0-255) | Color Picker > Sliders settings menu | not documented | missing | - | #141 |
| Working color space read-out (title hover shows the space name; colour management only) | Color Picker > Working color space | n/a | missing | - | #141 |
| Add swatch button (adds only if different from the last swatch) | Color Picker > Swatches | n/a | missing | - | #141 |
| Click a swatch to load its colour | Color Picker > Swatches | n/a | missing | - | #141 |
| Hover a swatch to see its hex value | Color Picker > Swatches | n/a | missing | - | #141 |
| Right-click a swatch: delete | Color Picker > Swatches | n/a | missing | - | #141 |
| Swatches settings menu: delete all swatches | Color Picker > Swatches | n/a | missing | - | #141 |
| Swatches persist across projects and sessions (user config file, stored as sRGB) | Color Picker > Swatches | n/a | missing | - | #141 |
