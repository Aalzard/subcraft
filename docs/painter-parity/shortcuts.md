# Painter parity: Keyboard and mouse shortcuts

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/miscellaneous/sliders , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/viewport/viewport , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/straight-line , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/paint-brush , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/polygon-fill , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/tool-list/path , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/uv-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/planar-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/fill-projections/warp-projection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/blending-modes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/creating-layers , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/flatten-layers , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/layer-instancing , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/masking-and-effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/smart-materials-and-masks , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/color-picker , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/display-settings/camera-settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/customizing-the-layout , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/navigation , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/texture-set/texture-set-list , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes
Checked against: origin/main 297dcb3 (2026-11-11)

In this file the "Default / range" column holds the default binding (Windows; macOS in brackets where it is not just Ctrl to Cmd). SubCraft bindings were read from `crates/app/src/actions.rs` (key table) and `crates/ui/src/viewport.rs` / `nav.rs` (mouse gestures). SubCraft has two fixed key sets (SubCraft default and Substance Painter); status is for the default set unless noted.

## Editable shortcuts: application and project

Layout notes: Painter lists editable shortcuts in Settings > Shortcuts as one long list in the order below (blank lines in the doc table separate the groups used as sub-sections here).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Hide UI | Settings > Shortcuts | Tab | done | crates/app/src/actions.rs (Action::HideUi) | #164 |
| Undo | Settings > Shortcuts | Ctrl+Z | done | crates/app/src/actions.rs:190 | - |
| Redo | Settings > Shortcuts | Ctrl+Y | done | crates/app/src/actions.rs:191 | - |
| Baking | Settings > Shortcuts | F8 | partial: opens the Bake window, not a mode | crates/app/src/actions.rs:221 | - |
| Painting | Settings > Shortcuts | F9 | partial: F9 closes the Bake window and leaves Rendering mode; modes are not separate interfaces | crates/app/src/actions.rs (Action::PaintingMode) | #164 |
| Rendering (Iray) | Settings > Shortcuts | F10 | done | crates/app/src/actions.rs:222 | - |
| Open project | Settings > Shortcuts | Ctrl+O | done | crates/app/src/actions.rs:219 | - |
| Close project | Settings > Shortcuts | Ctrl+F4 (Cmd+W) | partial: Ctrl+F4 returns to the sample model (there is no empty state) | crates/app/src/actions.rs (Action::CloseProject) | #164 |
| Save project | Settings > Shortcuts | Ctrl+S | done | crates/app/src/actions.rs:217 | - |
| Save as project | Settings > Shortcuts | unassigned | partial: bound to Ctrl+Shift+S by default | crates/app/src/actions.rs:218 | - |
| New project | Settings > Shortcuts | Ctrl+N | done | crates/app/src/actions.rs:215 | - |
| Quit application | Settings > Shortcuts | Alt+F4 (Cmd+Q) | done | crates/app/src/actions.rs:216 | - |

## Editable shortcuts: manipulator, symmetry, engine

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide manipulator | Settings > Shortcuts | Q | missing | - | - |
| Increase the manipulator size | Settings > Shortcuts | + | missing | - | - |
| Decrease the manipulator size | Settings > Shortcuts | - | missing | - | - |
| Cycle through manipulation spaces | Settings > Shortcuts | T | missing | - | - |
| Toggle warp edition mode | Settings > Shortcuts | Shift+V | done: Shift+V over the 3D view with a Warp fill layer selected | crates/ui/src/warp_view.rs | - |
| Translate tool | Settings > Shortcuts | W | missing | - | - |
| Rotate tool | Settings > Shortcuts | E | missing | - | - |
| Scale tool | Settings > Shortcuts | R | missing | - | - |
| Surface tool | Settings > Shortcuts | Shift+W | missing | - | - |
| Symmetry | Settings > Shortcuts | L | done: Symmetry on / off in both key sets, the last axes come back (as the toolbar button); X mirror alone stays on X (Shift+X in the Painter key set) | crates/app/src/actions.rs (ToggleSymmetry), paint/src/settings.rs (toggle_symmetry) | - |
| Pause engine computation | Settings > Shortcuts | Shift+Escape | missing | - | - |

## Editable shortcuts: tools

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Select Paint tool | Settings > Shortcuts | 1 | done | crates/app/src/actions.rs:198 | - |
| Select Paint tool + Particles | Settings > Shortcuts | Ctrl+1 | missing | - | - |
| Select Eraser tool | Settings > Shortcuts | 2 | done | crates/app/src/actions.rs:199 | - |
| Select Eraser tool + Particles | Settings > Shortcuts | Ctrl+2 | missing | - | - |
| Select Projection tool | Settings > Shortcuts | 3 | missing | - | - |
| Select Projection tool + Particles | Settings > Shortcuts | Ctrl+3 | missing | - | - |
| Select Polygon Fill | Settings > Shortcuts | 4 | done | crates/app/src/actions.rs:201 | #131 |
| Select Smudge tool | Settings > Shortcuts | 5 | done | crates/app/src/actions.rs:202 | - |
| Select Clone tool (relative source) | Settings > Shortcuts | 6 | partial: one clone mode only | crates/app/src/actions.rs:203 | - |
| Select Clone tool (absolute source) | Settings > Shortcuts | Ctrl+6 | missing | - | - |
| Select paint along path tool | Settings > Shortcuts | unassigned | partial: Pen paths tool is bound to P (Shift+P in the Painter key set) | crates/app/src/actions.rs:200 | - |
| Bake Mesh Maps | Settings > Shortcuts | Ctrl+Shift+B | done | crates/app/src/actions.rs:221 | - |
| Increase tool size | Settings > Shortcuts | ] | done | crates/app/src/actions.rs:206 | - |
| Decrease tool size | Settings > Shortcuts | [ | done | crates/app/src/actions.rs:205 | - |
| Invert grayscale tool | Settings > Shortcuts | X | partial: Painter key set only (X is symmetry in the SubCraft set) | crates/app/src/actions.rs (Action::InvertMaskGray) | #164 |
| Pick stroke material | Settings > Shortcuts | P | partial: only in the Substance Painter key set; default set has no key (P is Pen paths) | crates/app/src/actions.rs:204 | - |
| Lazy Mouse | Settings > Shortcuts | D | done | crates/app/src/actions.rs:208 | - |
| Hide/ignore excluded geometry | Settings > Shortcuts | Alt+H | done | crates/app/src/actions.rs | - |
| Reload mesh | Settings > Shortcuts | Ctrl+Shift+R (release notes) | missing | - | - |
| Break tangent (path) | Settings > Shortcuts | unassigned (release notes) | missing | - | - |
| Show/hide viewport visualization (baking mode) | Settings > Shortcuts | unassigned (release notes) | missing | - | - |

## Editable shortcuts: viewport and display

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Camera Perspective | Settings > Shortcuts | F5 | done | crates/app/src/actions.rs:210 | - |
| Camera Orthographic | Settings > Shortcuts | F6 | done | crates/app/src/actions.rs:211 | - |
| Display next channel | Settings > Shortcuts | C | done | crates/app/src/actions.rs:212 | - |
| Display previous channel | Settings > Shortcuts | Shift+C | done | crates/app/src/actions.rs:213 | - |
| Display material | Settings > Shortcuts | M | done | crates/app/src/actions.rs:214 | - |
| Display next mesh map | Settings > Shortcuts | B | partial: Painter key set only (B is Brush in the SubCraft set); cycles every map kind, baked or not | crates/app/src/actions.rs (Action::NextMeshMap) | #164 |
| Display previous mesh map | Settings > Shortcuts | Shift+B | done | crates/app/src/actions.rs (Action::PrevMeshMap) | #164 |
| Toggle animation | Settings > Shortcuts | Space | missing | - | - |
| Export textures | Settings > Shortcuts | Ctrl+Shift+E | done | crates/app/src/actions.rs:220 | - |
| Center the whole mesh | Settings > Shortcuts | F | done | crates/app/src/actions.rs:209 | - |
| Toggle quick mask edition | Settings > Shortcuts | U | missing | - | - |
| Clear quick mask | Settings > Shortcuts | Y | missing | - | - |
| Invert quick mask | Settings > Shortcuts | I | missing | - | - |
| Viewport layout 3D/2D | Settings > Shortcuts | F1 | done | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Viewport layout 3D only | Settings > Shortcuts | F2 | partial: Painter key set only (F2 renames a layer in the SubCraft set); the 2D view stays a tab behind | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Viewport layout 2D only | Settings > Shortcuts | F3 | partial: the 3D view stays a tab behind | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Swap 2D / 3D view | Settings > Shortcuts | F4 | done | crates/ui/src/app.rs (apply_view_layout) | #164 |
| Texture Set isolate | Settings > Shortcuts | Alt+Q | done | crates/app/src/session.rs (Command::IsolateTextureSet) | #164 |
| Next camera | Settings > Shortcuts | > (release notes) | missing | - | - |
| Previous camera | Settings > Shortcuts | < (release notes) | missing | - | - |

## Editable shortcuts: mouse

Layout notes: these are listed in the same editable list; mouse combinations can be rebound like keys.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Use tool / paint | Viewport | Mouse left | done | crates/ui/src/viewport.rs:318 | - |
| Draw straight lines | Viewport | Shift+Mouse left | done | crates/ui/src/viewport.rs:326 | - |
| Straight line preview (dotted line while Shift is held) | Viewport | shown while Shift held | missing | - | - |
| Draw straight lines with snapping | Viewport | Ctrl+Shift+Mouse left; 5 degree steps | done | crates/ui/src/nav.rs (snap_line_end) | #163 |
| Camera rotate | Viewport 3D | Alt+Mouse left | done | crates/ui/src/nav.rs:84 | - |
| Camera snap rotate | Viewport | Alt+Shift+Mouse left; 90 degree steps | done | crates/ui/src/nav.rs (NavMode::OrbitSnap) | #163 |
| Camera translate | Viewport | Alt+Middle mouse | done | crates/ui/src/nav.rs:82 | - |
| Camera translate (alternative) | Viewport | Ctrl+Alt+Mouse left | done | crates/ui/src/nav.rs:82 | - |
| Camera zoom | Viewport | Alt+Mouse right | done | crates/ui/src/nav.rs:80 | - |
| Camera zoom with the wheel | Viewport | Mouse wheel | done | crates/ui/src/viewport.rs:104 | - |
| Stencil rotate | Viewport | S+Mouse left | done | crates/ui/src/viewport.rs:85 | - |
| Stencil snap rotate | Viewport | Shift+S+Mouse left; 90 degree steps | done | crates/ui/src/viewport.rs (stencil_drag_rotation) | #158 |
| Stencil translation | Viewport | S+Middle mouse | done | crates/ui/src/viewport.rs:89 | - |
| Stencil zoom | Viewport | S+Right mouse | done | crates/ui/src/viewport.rs:93 | - |
| Change tool Size / Hardness | Viewport | Ctrl+Mouse right; horizontal = size, vertical = hardness | done | crates/ui/src/viewport.rs:146 | - |
| Change tool Flow / Rotation | Viewport | Ctrl+Mouse left; horizontal = flow, vertical = rotation | done | crates/ui/src/viewport.rs:150 | - |
| Rotate environment | Viewport | Shift+Mouse right | done | crates/ui/src/nav.rs:76 | - |
| Auto-Focus (Depth of Field) | Viewport | Ctrl+Middle mouse | partial: a pick-focus button then a click, no direct gesture | crates/ui/src/render_export.rs:104 | - |
| Texture set selection | Viewport | Ctrl+Alt+Mouse right | done | crates/ui/src/viewport.rs (pick_set) | #163 |
| Context menu (properties at the cursor) | Viewport | Mouse right | partial: opens a part-actions menu, not the tool properties | crates/ui/src/viewport.rs:162 | #131 |
| Set Clone tool source location | Viewport | V+Mouse left | done | crates/ui/src/viewport.rs:154 | - |
| Ignore stencil mask | Viewport | N (hold) | done | crates/ui/src/viewport.rs:79 | - |
| Resume previous stroke | Viewport | A | missing | - | - |

## 2D view navigation

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Rotate the UV space around the cursor | 2D view | Alt+Left click | missing | - | - |
| Snap UV rotation to orthogonal angles | 2D view | Alt+Shift+Left click | missing | - | - |
| Pan | 2D view | Alt+Middle click | partial: plain middle drag or Alt+left drag | crates/ui/src/panels/uv_view.rs:83 | - |
| Zoom / dolly | 2D view | Alt+Right click | partial: wheel only | crates/ui/src/panels/uv_view.rs:77 | - |
| Orbit around the cursor position (3D) | 3D view | Alt+Left click | done | crates/ui/src/orbit.rs (orbit_around) | #163 |

## Non-editable shortcuts: layer stack

Layout notes: these act only while the pointer is over the layer stack.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Copy layer | Layer stack | Ctrl+C | done | crates/ui/src/clipboard.rs:45 | - |
| Cut layer | Layer stack | Ctrl+X | done | crates/ui/src/clipboard.rs:46 | - |
| Paste layer | Layer stack | Ctrl+V | done | crates/ui/src/clipboard.rs:47 | - |
| Delete layer | Layer stack | Delete | done | crates/app/src/actions.rs:196 | - |
| Duplicate layer | Layer stack | Ctrl+D | done | crates/app/src/actions.rs:197 | - |
| Group layer | Layer stack | Ctrl+G | done (no selection: adds an empty folder) | crates/app/src/session.rs:1908 | - |
| Copy layer content | Layer stack | Ctrl+Shift+C (Option+Cmd+C) | done | crates/ui/src/clipboard.rs:66 | - |
| Paste layer content | Layer stack | Ctrl+Shift+V (Option+Cmd+V) | done | crates/ui/src/clipboard.rs:66 | - |
| Display mask in viewport | Layer stack, mask thumbnail | Alt+Mouse left | done | crates/ui/src/panels/layers.rs:481 | - |
| Disable/Enable mask | Layer stack, mask thumbnail | Shift+Mouse left | done | crates/ui/src/panels/layers.rs:308 | - |
| Drag and drop material onto one part of the model | Assets to viewport | Ctrl+Drag and drop | missing | - | - |
| Manipulator constraint/snapping | Viewport manipulator | Shift | missing | - | - |
| Manipulator mirror transformation | 2D view manipulator | Ctrl | missing | - | - |

## Shortcuts documented on feature pages: layers, effects, assets

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Merge selected layers | Layer stack | Ctrl+M | done | crates/ui/src/panels/layers.rs:797 | - |
| Paste as instance | Layer stack | Ctrl+Shift+V after Ctrl+C | missing | - | - |
| Instantiate across texture sets | Layer stack | Ctrl+Shift+D (release notes) | missing | - | - |
| Duplicate layers by dragging | Layer stack | Ctrl + drag and drop | done | crates/ui/src/panels/layers.rs:136 | - |
| Copy / cut / paste effects | Layer stack, effect selected | Ctrl+C / Ctrl+X / Ctrl+V | done | crates/ui/src/clipboard.rs:61 | - |
| Delete selected effect | Layer stack, effect selected | Delete | done | crates/app/src/session.rs:1914 | - |
| Reorder effect up | Layer stack, effect selected | Alt+Up arrow | done | crates/ui/src/panels/effects.rs:287 | - |
| Reorder effect down | Layer stack, effect selected | Alt+Down arrow | done | crates/ui/src/panels/effects.rs:287 | - |
| Change blending mode with keys | Blending mode dropdown focused | Up / Down arrows | missing | - | - |
| Change blending mode with the wheel | Blending mode dropdown focused | Mouse wheel up / down | missing | - | - |
| Create a mask when dropping a fill | Assets to layer stack | Ctrl while dropping | missing | - | - |
| Replace the effect stack when dropping a smart material | Assets to layer | Ctrl while dropping | missing | - | - |
| Replace the effect list when dropping a smart mask | Assets to mask | Ctrl while dropping | missing | - | - |
| Drop a material as a positioned decal (warp projection preview) | Assets to viewport | Alt while dragging | missing | - | - |
| Rename layer | Layer stack | Double-click name (unverified) | done | crates/ui/src/panels/layers.rs:403 | - |
| Rename Texture Set | Texture Set List | Double-click name | done | crates/ui/src/panels/texture_sets.rs:79 | - |
| Multi-select asset type filters | Assets | Ctrl+click | missing | - | - |
| Scroll any panel by dragging | Any window | Ctrl+Alt+drag | missing | - | - |
| Scroll assets | Assets | Mouse wheel | done | crates/ui/src/panels/shelf.rs | - |
| Pick the edited channel's raw value with the eyedropper | Colour picker eyedropper over the viewport | Shift | partial: Shift+click with the Eyedropper picks the whole material instead | crates/ui/src/viewport.rs:295 | #141 |
| Isolate one mesh map in the bake list | Baking window | Alt+click | missing | - | - |

## Shortcuts documented on feature pages: tools

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Projection tool: rotate projection | Viewport, Projection tool | S+Mouse left | missing | - | - |
| Projection tool: snap rotation | Viewport, Projection tool | S+Shift+Mouse left | missing | - | - |
| Projection tool: zoom projection | Viewport, Projection tool | S+Mouse right | missing | - | - |
| Projection tool: translate projection | Viewport, Projection tool | S+Middle mouse | missing | - | - |
| Polygon fill: extend loop to the other side | Viewport, Polygon loop modes | Alt while dragging | missing | - | - |
| Polygon fill: primary loop only | Viewport, Partial polygon loop | Shift | missing | - | - |
| Polygon fill: temporary Polygon loop mode | Viewport, Polygon fill | Shift+A | missing | - | - |
| Polygon fill: combine with the previous result | Viewport, Polygon fill | A while dragging | missing | - | - |
| Path: leave path edition | Viewport, Path tool | Escape or Enter | done | crates/ui/src/pen.rs:52 | - |
| Path: toggle corner / smooth on a point | Viewport, Path tool | Double-click vertex | missing | - | - |
| Path: break tangents while moving | Viewport, Path tool | Alt | missing | - | - |
| Path: scale both tangents together | Viewport, Path tool | Ctrl | missing | - | - |
| Path: select all points | Viewport, Path tool | Ctrl+A (release notes) | missing | - | - |
| Path: delete selected path or point | Paths panel / viewport | Delete | partial: Delete removes the selected anchor only | crates/ui/src/pen.rs:60 | - |
| Path: rename | Paths panel | Double-click | done: double-click in the Paths panel | crates/ui/src/paths_panel.rs | - |
| Path: copy / paste / duplicate | Paths panel | Ctrl+C / Ctrl+V / Ctrl+D | missing | - | - |
| Hardness shortcut drives a Substance brush "hardness" parameter | Paint tool | Ctrl+Mouse right vertical | missing | - | - |

## Fill projection manipulators

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV transform: translate | 2D view manipulator | Drag inside | missing | - | - |
| UV transform: translate constrained to one axis | 2D view manipulator | Shift+drag inside | missing | - | - |
| UV transform: rotate | 2D view manipulator | Drag outside | missing | - | - |
| UV transform: rotate in 45 degree steps | 2D view manipulator | Shift+drag outside | missing | - | - |
| UV transform: scale | 2D view manipulator | Drag a handle | missing | - | - |
| UV transform: scale keeping ratio | 2D view manipulator | Shift+drag a handle | missing | - | - |
| UV transform: scale mirrored around the pivot | 2D view manipulator | Ctrl+drag a handle | missing | - | - |
| UV transform: scale mirrored and keeping ratio | 2D view manipulator | Shift+Ctrl+drag a handle | missing | - | - |
| 3D manipulator: translate along one, two or three axes | 3D view manipulator | Drag axis / plane / centre | missing | - | - |
| 3D manipulator: translate in steps | 3D view manipulator | Shift+drag | missing | - | - |
| 3D manipulator: rotate | 3D view manipulator | Drag ring | missing | - | - |
| 3D manipulator: rotate in angle steps | 3D view manipulator | Shift+drag | missing | - | - |
| 3D manipulator: scale along an axis | 3D view manipulator | Drag handle | missing | - | - |
| 3D manipulator: scale in steps | 3D view manipulator | Shift+drag | missing | - | - |
| 3D manipulator: precise mode | 3D view manipulator | Ctrl (release notes) | missing | - | - |
| 3D manipulator: surface snap drag (Planar and Warp) | 3D view manipulator | Drag with Surface tool | missing | - | - |

## Slider widget interactions

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Drag | Any slider | Click and drag | done | crates/ui/src/widgets.rs:180 | - |
| Precision drag | Any slider | Shift while dragging | partial: only on number-box drags (brush bar), not on slider bars | crates/ui/src/brush_bar.rs:186 | - |
| Jump | Any slider | Click on the bar | done | crates/ui/src/widgets.rs:180 | - |
| Up / Down | Focused slider | Up and Down arrow keys | done | crates/ui/src/widgets.rs:180 | - |
| Value edition | Text field above the slider | Click and type | partial: value box is beside the slider, not above it | crates/ui/src/widgets.rs:192 | - |

## Shortcut editor (Settings > Shortcuts)

Layout notes: a scrolling list of action name and current binding with a pen icon per row; editable and non-editable shortcuts are separate lists.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Shortcuts tab in Settings | Edit > Settings > Shortcuts | - | done | crates/ui/src/recovery.rs (SettingsTab::Shortcuts) | #209 |
| Pen icon starts editing a binding | Settings > Shortcuts row | - | done | crates/ui/src/shortcut_editor.rs (Capture) | #209 |
| Pressing the last key ends editing and stores the combination | Settings > Shortcuts row | - | done | crates/ui/src/shortcut_editor.rs (read_key) | #209 |
| Right-click a shortcut to reset it to default | Settings > Shortcuts row | - | done | crates/ui/src/shortcut_editor.rs (row context menu) | #209 |
| Mouse buttons with modifiers can be bound | Settings > Shortcuts row | - | partial: the six camera gestures (rotate, snap rotate, translate, zoom, environment, light) in Painter navigation; brush size / flow drags stay fixed | crates/ui/src/nav.rs (MouseBindings) | #209 |
| Actions with no default key can be given one | Settings > Shortcuts row | - | done | crates/app/src/keymap.rs (Keymap::bind) | #209 |
| Custom bindings saved with the application preferences | Preferences store | - | done | crates/ui/src/recovery.rs (custom_keys, custom_mouse) | #209 |
| Non-editable shortcuts listed separately | Settings > Shortcuts | - | done | crates/ui/src/shortcut_editor.rs (Not editable list) | #209 |
| Search field in the list | Settings > Shortcuts | (unverified) | done | crates/ui/src/shortcuts_window.rs:80 | - |
| Keyboard helper in the viewport corner while a modifier is held | Viewport, bottom left | on (unverified) | done | crates/ui/src/viewport_hints.rs:72 | - |
