# Painter parity: Layers panel (layer stack)

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/layer-stack , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/creating-layers , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/managing-layers , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/masking-and-effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/blending-modes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/flatten-layers , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/geometry-mask , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/layer-instancing , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/smart-materials-and-masks , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/dynamic-material-layering , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-tiles/uv-tiles , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-tiles/image-sequence , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/advanced-channel-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/ambient-occlusion-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/height-map-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/normal-map-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/advanced-channel-painting/flow-map-painting , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-9-1 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2021-1-7-1-0 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2019-3 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2019-1 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2018-3 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2017-4 , https://experienceleague.adobe.com/en/docs/substance-3d-dev/painter-python/api/substancepainter-package/layerstack-module/navigation (enum names only)
Checked against: origin/main 297dcb3 (2026-11-11)

## Panel header: view mode and actions

Layout notes: one header row. Far left is the view-mode (channel) dropdown; the action icons sit on the right in the order below. The stack lists the top layer first; a folder's content is evaluated before its siblings. SubCraft puts create buttons on the left, arrange buttons (move up/down, duplicate, delete) on the right and the channel dropdown on a second row.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| View mode dropdown (channel shown in thumbnails; channel whose blend mode and opacity each row edits) | Layers header, top left | first channel of the texture set; options = channels in Texture Set Settings | partial: on its own row below the toolbar, labelled "Channel" | crates/ui/src/panels/layers.rs:207 | - |
| Add Effect button (opens the effect menu for the selected layer or mask) | Layers header, icon 1 | not documented | done | crates/ui/src/panels/layers.rs:301 | - |
| Create Mask button (opens the mask menu) | Layers header, icon 2 | not documented | done | crates/ui/src/panels/layers.rs:309 | - |
| Create Mask > Add white mask | Layers header, mask menu | not documented | done | crates/ui/src/panels/layers.rs:51 | - |
| Create Mask > Add black mask | Layers header, mask menu | not documented | done | crates/ui/src/panels/layers.rs:51 | - |
| Create Mask > Add bitmap mask | Layers header, mask menu | not documented | done (file picker; a black mask is created first) | crates/ui/src/panels/layers.rs:62 | - |
| Create Mask > Add mask with color selection | Layers header, mask menu | not documented | done | crates/ui/src/panels/layers.rs:66 | - |
| Create Mask > Add mask with height combination | Layers header, mask menu | not documented | missing (no Compare Mask effect) | - | - |
| Create New Paint Layer (above the selection) | Layers header, icon 3 | not documented | done | crates/ui/src/panels/layers.rs:162 | - |
| Create New Fill Layer (above the selection) | Layers header, icon 4 | not documented | done | crates/ui/src/panels/layers.rs:165 | - |
| Add New Smart Materials (mini-shelf of smart materials, inserts above the selection) | Layers header, icon 5 | not documented | missing (smart materials are added from the shelf only) | - | - |
| Add New Folder (empty, above the selection) | Layers header, icon 6 | not documented | done | crates/ui/src/panels/layers.rs:168 | - |
| Delete Layer (selected layer, folder or effect) | Layers header, icon 7 | not documented | done | crates/ui/src/panels/layers.rs:343 | - |
| New layer goes inside a selected empty folder | Layers header buttons | not documented | done | crates/app/src/session.rs:1207 | - |

## Layer row

Layout notes: left to right a Painter row has the visibility eye, the content thumbnail, the mask thumbnail (when a mask exists), the name, then at the far right the blending-mode dropdown with the opacity value under or beside it, the instance icon (when instanced) and the geometry-mask icon. A thin line under each thumbnail turns red when that stack has effects. Folders have a disclosure arrow and indent their children.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Visibility toggle (eye) | Layer row, left | visible | done | crates/ui/src/panels/layers.rs:290 | - |
| Click and slide over several eyes to hide/unhide many rows | Layer row, eye column | not documented | done | crates/ui/src/panels/layers.rs:173 | - |
| Un-hiding a layer inside a hidden folder also un-hides the folder | Layer row, eye | not documented | done | crates/app/src/session.rs:1845 | - |
| Content thumbnail (click selects the layer content and shows the content effect stack) | Layer row | shows the view-mode channel | done | crates/ui/src/panels/layers.rs:294 | - |
| Fill-layer thumbnail drawn as a material sphere (optimised thumbnails) | Layer row | optimised thumbnails on by default | partial: flat channel preview, no sphere | crates/ui/src/panels/layers.rs:294 | - |
| Thumbnail mode setting (legacy vs optimised thumbnails) | Settings (referenced from layer stack) | optimised | missing | - | - |
| Thumbnail loading indicator | Layer row thumbnail | not documented | partial: previews fill in asynchronously, no explicit indicator | crates/ui/src/layer_previews.rs:33 | - |
| Mask thumbnail (click selects the mask and shows the mask effect stack) | Layer row, right of content thumbnail | absent until a mask is added | done | crates/ui/src/panels/layers.rs:298 | - |
| Effect indicator line under each thumbnail (grey none, red at least one effect) | Layer row, under thumbnails | grey | done (a "N fx" text under the name as well) | crates/ui/src/panels/layers.rs:453 | - |
| Layer name | Layer row | "Layer N" / "Fill layer N" / "Folder N" (unverified) | done | crates/ui/src/panels/layers.rs:324 | - |
| Rename by double-click on the name | Layer row | not documented | done (also F2 and context menu) | crates/ui/src/panels/layers.rs:403 | - |
| Blending mode dropdown for the view-mode channel | Layer row, right | per channel, see Blending modes | done | crates/ui/src/panels/layers.rs:360 | - |
| Opacity for the view-mode channel | Layer row, right | 100; 0-100 | done | crates/ui/src/panels/layers.rs:373 | - |
| Blending dropdown: Arrow Up / Arrow Down step through modes when focused | Layer row, blend dropdown | n/a | done | crates/ui/src/panels/layers.rs:206 | - |
| Blending dropdown: mouse wheel steps through modes when focused | Layer row, blend dropdown | n/a | done | crates/ui/src/panels/layers.rs:206 | - |
| Right-click blending mode > Apply to all channels | Layer row, blend dropdown | n/a | done | crates/ui/src/panels/layers.rs:534 | - |
| Right-click opacity > Apply to all channels | Layer row, opacity | n/a | done | crates/ui/src/panels/layers.rs:547 | - |
| Folder disclosure arrow (open/close group) | Folder row | open | done | crates/ui/src/panels/layers.rs:286 | - |
| Folder closes when moved in or out of another folder (fixed in recent versions: state kept) | Folder row | state kept | done | crates/app/src/layer_tree.rs:17 | - |
| Instance icon (none / source / target; click opens the instance list) | Layer row, right | hidden when not instanced | missing | - | - |
| Geometry mask icon (click to edit, shows count of included items) | Layer row, far right | all geometry included | partial: no row icon; the "Parts" section in Properties plays this role | crates/ui/src/panels/properties.rs:88 | - |
| Gamma-corrected alpha "i" icon | Layer row | off | missing | - | - |
| Layer colour stripe (from Set color) | Layer row | none | done (at the right edge of the row; dimmed inside a tagged folder) | crates/ui/src/panels/layers.rs:559 | - |
| Broken instance / cycle indicator | Layer row | n/a | missing | - | - |
| Anchor reference broken warning on the row | Layer row | n/a | done | crates/ui/src/panels/layers.rs:342 | - |

## Layer types

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Paint layer (brush, particles, polygon fill, smudge, clone) | Layer stack | n/a | done | crates/doc/src/layer.rs:157 | - |
| Fill layer (material / per-channel sources with projection) | Layer stack | n/a | done | crates/ui/src/panels/properties.rs:193 | - |
| Folder / group (holds layers, can carry a mask and geometry mask) | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:284 | - |
| Instance layer (read-only copy driven by a source layer) | Layer stack | n/a | missing | - | - |
| Every layer is multi-channel (one blend mode and opacity per channel) | Layer stack | n/a | done | crates/app/src/command.rs:102 | - |
| Painting writes to all enabled tool channels whatever the view-mode channel is | Layer stack | n/a | done | crates/app/src/command.rs:109 | - |

## Blending modes

Layout notes: one flat dropdown, grouped by separators in this order: Normal / Passthrough / Disable / Replace; Multiply group; Add-Subtract group; Overlay group; Light group; HSV group; Normal-map group. All modes work in linear space. The same list is used by layers and by effects.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Normal | Layer row > blending dropdown | default for most channels | done | crates/doc/src/blend.rs:8 | - |
| Passthrough | Layer row > blending dropdown | default for instanced layers and for layers made by dropping a filter | partial: offered on every layer; on effects it is not offered; instanced layers do not exist | crates/doc/src/blend.rs:189 | - |
| Disable | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:12 | - |
| Replace | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:14 | - |
| Multiply | Layer row > blending dropdown | default for Ambient Occlusion channel | done | crates/doc/src/blend.rs:158 | - |
| Divide | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:16 | - |
| Inverse Divide | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:17 | - |
| Darken (Min) | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:18 | - |
| Lighten (Max) | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:19 | - |
| Linear Dodge (Add) | Layer row > blending dropdown | default for Height channel | done | crates/doc/src/blend.rs:158 | - |
| Subtract | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:21 | - |
| Inverse Subtract | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:22 | - |
| Difference | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:23 | - |
| Exclusion | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:24 | - |
| Signed Addition (AddSub) | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:25 | - |
| Overlay | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:26 | - |
| Screen | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:27 | - |
| Linear Burn | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:28 | - |
| Color Burn | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:29 | - |
| Color Dodge | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:30 | - |
| Soft Light | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:31 | - |
| Hard Light | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:32 | - |
| Vivid Light | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:33 | - |
| Linear Light | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:34 | - |
| Pin Light | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:35 | - |
| Tint | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:36 | - |
| Saturation | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:37 | - |
| Color | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:38 | - |
| Value | Layer row > blending dropdown | n/a | done | crates/doc/src/blend.rs:39 | - |
| Normal Map Combine | Layer row > blending dropdown | Normal channel only | done | crates/doc/src/blend.rs:40 | - |
| Normal Map Detail | Layer row > blending dropdown | default for Normal channel | done | crates/doc/src/blend.rs:158 | - |
| Normal Map Inverse Detail | Layer row > blending dropdown | Normal channel only | done | crates/doc/src/blend.rs:42 | - |
| Separators between the seven mode groups | Layer row > blending dropdown | n/a | done | crates/ui/src/panels/layers.rs:192 | - |
| Height channel is HDR: Add / Subtract are not clamped | Texture Set channel format | Height is HDR | done | crates/compositor/src/blend.rs:157 | - |

## Creating layers

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Duplicate via Right-click > Duplicate | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:499 | - |
| Duplicate via Ctrl+D | Layer stack shortcut | Ctrl+D | done | crates/app/src/actions.rs:197 | - |
| Duplicate via Ctrl + drag and drop of the layer(s) | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:136 | - |
| Drag a Material from Assets into the stack (new fill layer at the drop position) | Assets > Layer stack | n/a | partial: lands above the row it is dropped on, no between-rows insert bar | crates/ui/src/panels/layers.rs:411 | - |
| Drag a Smart Material from Assets into the stack (new folder) | Assets > Layer stack | n/a | partial: lands above the row it is dropped on | crates/ui/src/panels/shelf.rs:95 | - |
| Drag an effect (filter / generator) from Assets into the stack (new layer set to Passthrough on all channels) | Assets > Layer stack | n/a | partial: filters only (generators are not on the shelf) | crates/ui/src/panels/shelf.rs:133 | - |
| Drag a texture from Assets into the stack (new fill layer) | Assets > Layer stack | n/a | done | crates/ui/src/panels/shelf.rs:111 | - |
| Drag an alpha / procedural / environment map into the stack | Assets > Layer stack | n/a | partial: alphas go to the row's mask as an image effect; no procedural or environment drop | crates/ui/src/panels/shelf.rs:115 | - |
| Drag a Smart Mask onto the stack as a new layer (fill layer with that mask) | Assets > Layer stack | n/a | partial: applies to the row it is dropped on, never creates a layer | crates/ui/src/panels/shelf.rs:99 | - |
| Drag an external file from the OS into the stack (auto-import and create layer/effect) | OS > Layer stack | n/a | missing | - | - |
| Ctrl while dropping a resource in the stack: new layer with the resource inside a black mask | Assets > Layer stack | n/a | missing | - | - |
| Alt while dropping on a layer thumbnail: remove the previous effects first | Assets > Layer stack | n/a | partial: Alt means "apply in place" (replace the fill material / set the active channel), a different rule | crates/ui/src/panels/shelf.rs:80 | - |
| Hovering a thumbnail while dragging switches to that stack (content or mask) | Layer stack | n/a | partial: mask thumbnail is a separate drop target, no stack switch | crates/ui/src/panels/layers.rs:300 | - |
| Ghost mask icon on mask-less layers while dragging (drop creates a black mask plus the resource) | Layer stack | n/a | missing | - | - |
| Ctrl + drag a material creates the fill layer with a mask immediately | Assets > Layer stack | n/a | missing | - | - |
| Automatic ID mask when dropping a material on a mesh with an ID map (Ctrl in viewport) | Assets > Viewport | n/a | missing | - | - |

## Managing layers

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Drag a layer to move it; insertion bar above a layer | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:82 | - |
| Insertion bar between two layers | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:75 | - |
| Insertion bar below a layer | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:86 | - |
| Drop on a folder to move the layer inside (lands at the top of the folder) | Layer stack | n/a | done | crates/app/src/layer_tree.rs:20 | - |
| Drag several selected layers at once | Layer stack | n/a | missing (drag carries one layer) | - | - |
| Auto-scroll the stack while dragging near its edges | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:247 | - |
| Single click selects a layer | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:398 | - |
| Ctrl / Cmd + click adds or removes a layer from the selection | Layer stack | n/a | done | crates/app/src/multi_select.rs:37 | - |
| Shift + click selects the range between first and last | Layer stack | n/a | done | crates/app/src/multi_select.rs:24 | - |
| Group layers: Right-click > Group Layers | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:659 | - |
| Group layers: Ctrl+G | Layer stack shortcut | Ctrl+G | done (Ctrl+G with no selection adds an empty folder) | crates/app/src/session.rs:1422 | - |
| Selection stays near the deleted layer after a delete | Layer stack | n/a | done | crates/app/src/session.rs:1150 | - |
| Layer and effect cannot both be selected | Layer stack | n/a | done | crates/app/src/command.rs:37 | - |

## Keyboard and mouse shortcuts (layer stack)

Layout notes: these are Painter's non-editable shortcuts; copy/paste only fire while the pointer is over the layer stack.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Copy layer | Layer stack | Ctrl+C | done | crates/ui/src/clipboard.rs:46 | - |
| Cut layer | Layer stack | Ctrl+X | done | crates/ui/src/clipboard.rs:47 | - |
| Paste layer | Layer stack | Ctrl+V | done | crates/ui/src/clipboard.rs:48 | - |
| Delete layer | Layer stack | Delete | done | crates/app/src/actions.rs:196 | - |
| Duplicate layer | Layer stack | Ctrl+D | done | crates/app/src/actions.rs:197 | - |
| Group layer | Layer stack | Ctrl+G | done | crates/app/src/session.rs:1908 | - |
| Copy layer content (painting and effects) | Layer stack | Ctrl+Shift+C | done | crates/ui/src/clipboard.rs:140 | - |
| Paste layer content | Layer stack | Ctrl+Shift+V | done | crates/app/src/session.rs:1561 | - |
| Paste as instance | Layer stack | Ctrl+Shift+V (per instancing page) | missing | - | - |
| Instantiate across texture sets | Layer stack | Ctrl+Shift+D (2017.4 release notes) | missing | - | - |
| Merge / flatten the selected group | Layer stack | Ctrl+M | done | crates/ui/src/panels/layers.rs:797 | - |
| Display mask in viewport | Mask thumbnail | Alt + left click | partial: opens a floating UV preview window of the mask, viewport unchanged | crates/ui/src/panels/layers.rs:312 | - |
| Disable / enable mask | Mask thumbnail | Shift + left click | done | crates/ui/src/panels/layers.rs:308 | - |
| Move effect up / down | Effect row | Alt + Arrow Up / Arrow Down | missing | - | - |
| Hide / ignore excluded geometry | Viewport contextual toolbar | Alt+H | missing | - | - |
| Copy/paste limited to pointer over the layer stack | Layer stack | n/a | partial: fires whenever no text field has focus | crates/ui/src/clipboard.rs:40 | - |

## Layer context menu (right-click on a layer)

Layout notes: Painter's menu is, top to bottom: effect entries (with icons), mask entries, clipboard entries, instancing, grouping and flatten, blending options, layer options, smart presets, colour. Adobe's docs name the entries but do not print the full ordered menu; entries marked (unverified) are placed from memory of the application. SubCraft's menu follows that order: Add paint / fill / levels / filter / anchor point (or the mask effects when the mask is the shown stack), Add generator, the four mask entries, Mask actions, Cut, Copy, Paste, Duplicate, Remove, Group layers, Create smart material, Create smart mask; its own entries come last (Rename, Move into folder, Move to top level, Add paint / fill / text layer, Hide / Show for a multi-selection).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add paint (effect) | Layer context menu | n/a | done | crates/ui/src/panels/effects.rs:68 | - |
| Add fill (effect) | Layer context menu | n/a | done | crates/ui/src/panels/effects.rs:69 | - |
| Add generator | Layer context menu | n/a | partial: adds a built-in generator to the mask from a submenu of kinds; no generator on content | crates/ui/src/panels/layers.rs:619 | - |
| Add levels | Layer context menu | n/a | done | crates/ui/src/panels/effects.rs:70 | - |
| Add compare mask (mask only) | Layer context menu | n/a | missing | - | - |
| Add filter | Layer context menu | n/a | partial: a submenu of built-in filters instead of an empty slot with a picker | crates/ui/src/panels/effects.rs:79 | - |
| Add color selection (mask only) | Layer context menu | n/a | done | crates/ui/src/panels/effects.rs:142 | - |
| Add anchor point | Layer context menu | n/a | done | crates/ui/src/panels/effects.rs:89 | - |
| Effect icons shown next to the Add entries | Layer context menu | n/a | done | crates/ui/src/panels/effects.rs:65 | - |
| Add white mask | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:51 | - |
| Add black mask | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:51 | - |
| Add bitmap mask | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:62 | - |
| Add mask with color selection | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:66 | - |
| Add mask with height combination (Compare Mask on Height, sets Height blend to Normal) | Layer context menu | n/a | missing | - | - |
| Cut layer(s) | Layer context menu | Ctrl+X | done | crates/ui/src/panels/layers.rs:493 | - |
| Copy layer(s) | Layer context menu | Ctrl+C | done | crates/ui/src/panels/layers.rs:490 | - |
| Paste layer(s) | Layer context menu | Ctrl+V | done | crates/ui/src/panels/layers.rs:496 | - |
| Paste as instance | Layer context menu | Ctrl+Shift+V | missing | - | - |
| Duplicate layer(s) | Layer context menu | Ctrl+D | done | crates/ui/src/panels/layers.rs:499 | - |
| Remove layer(s) | Layer context menu | Delete (unverified label) | done | crates/ui/src/panels/layers.rs:507 | - |
| Instantiate across texture sets | Layer context menu | Ctrl+Shift+D | missing | - | - |
| Group layer(s) | Layer context menu | Ctrl+G | done | crates/ui/src/panels/layers.rs:659 | - |
| Merge / flatten group | Layer context menu | Ctrl+M | done | crates/ui/src/panels/layers.rs:677 | - |
| Export flattened group to files | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:680 | - |
| Blending options > Copy (blend mode and opacity of all channels) | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:665 | - |
| Blending options > Paste | Layer context menu | n/a | done | crates/app/src/session.rs:1535 | - |
| Copy layer content (unverified entry; shortcut is documented) | Layer context menu | Ctrl+Shift+C | done | crates/ui/src/panels/layers.rs:678 | - |
| Paste layer content (unverified entry; shortcut is documented) | Layer context menu | Ctrl+Shift+V | done | crates/ui/src/panels/layers.rs:682 | - |
| Gamma corrected alpha/mask (per-layer toggle) | Layer context menu | off | missing | - | - |
| Create smart material (folders) | Layer context menu | n/a | partial: offered on any layer, not folders only | crates/ui/src/panels/layers.rs:688 | - |
| Create smart mask | Layer context menu | n/a | done (greyed until the mask has an effect) | crates/ui/src/panels/layers.rs:692 | - |
| Set color (colour tag submenu: none plus a fixed palette) | Layer context menu | none (palette unverified) | done (None plus nine colours) | crates/ui/src/panels/layers.rs:696 | - |
| Rename (SubCraft extra; Painter renames by double-click) | Layer context menu | F2 in SubCraft | done | crates/ui/src/panels/layers.rs:427 | - |

## Copy and paste

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Copy / cut / paste one or several layers; pasted above the selection | Layer stack | Ctrl+C / X / V | done | crates/app/src/command.rs:81 | - |
| Paste layers into another texture set | Layer stack | n/a | done | crates/app/src/session.rs:694 | - |
| Copy/paste layers across projects (not supported in Painter) | Layer stack | not supported | done (SubCraft allows it) | crates/app/src/command.rs:80 | - |
| Pasted duplicates are renamed ("copy N") | Layer stack | not documented | partial: names kept, ids regenerated | crates/app/src/session.rs:1061 | - |
| Copy layer content / paste layer content (painting plus effects into an existing layer) | Layer stack | Ctrl+Shift+C / V | done (the content replaces the target's; the mask is not copied) | crates/app/src/session.rs:1561 | - |
| Copy / paste blending options between layers | Layer context menu > Blending options | n/a | done | crates/app/src/session.rs:1535 | - |
| Copy / cut / paste effects (see masks-effects.md) | Effect rows | Ctrl+C / X / V | partial: copy and paste from the effect row menu only | crates/ui/src/panels/effects.rs:286 | - |
| Anchor references survive duplicate and copy/paste | Layer stack | n/a | done | crates/app/src/clipboard.rs:209 | - |

## Layer colours

Layout notes: colours are set from the context menu. A layer without its own colour inherits its folder's colour (drawn dimmed); a layer with its own colour keeps it inside a coloured folder.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Assign a colour to a layer or folder | Layer context menu > Set color | none | done | crates/app/src/session.rs:1550 | - |
| Children inherit the folder colour (dimmed) | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:559 | - |
| A layer's own colour is not overridden by its folder | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:559 | - |
| Search / filter box in the layer stack (Painter has none) | Layer stack | not present | n/a | - | - |

## Layer instancing

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Paste as instance (copy a layer, paste a linked instance) | Layer context menu / Ctrl+Shift+V | n/a | missing | - | - |
| Instantiate across texture sets dialog (checkbox per texture set, OK) | Layer context menu | all unchecked (unverified) | missing | - | - |
| Channel mismatch warning (exclamation mark next to a texture set in the dialog) | Instantiate dialog | n/a | missing | - | - |
| Instance of a folder (layers added to the source appear in every instance) | Layer stack | n/a | missing | - | - |
| Only the source layer is editable; instances are read-only | Layer stack | n/a | missing | - | - |
| Paint strokes exist only in the source's texture set | Layer stack | n/a | missing | - | - |
| Instance icon states: non-instanced, source, target | Layer row | n/a | missing | - | - |
| Instance properties button: Properties shows the source and all instances as a list | Layer row > Properties | n/a | missing | - | - |
| Click a list entry to jump to that layer (switches texture set) | Properties > instance list | n/a | missing | - | - |
| Instanced layers default to Passthrough blending | Layer stack | Passthrough | missing | - | - |
| Deleting the source deletes its instances | Layer stack | n/a | missing | - | - |
| Cycle detection: an instance moved inside its own source is disabled and flagged | Layer stack | n/a | missing | - | - |
| Anchor references may not cross an instanced folder boundary | Layer stack | n/a | missing | - | - |
| Flatten all instanced layers across texture sets | Layer stack (11.x) | n/a | missing | - | - |

## Flatten layers

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Flatten / merge the selected group into a new fill layer of bitmaps | Layer context menu / Ctrl+M | n/a | done (a paint layer of rasters: the same result without new resources) | crates/app/src/flatten.rs:153 | - |
| Source group is kept but disabled after flattening | Layer stack | n/a | done (hidden) | crates/app/src/flatten.rs:168 | - |
| One bitmap per enabled channel at the texture set resolution and channel bit depth | Flatten result | n/a | partial: the set's resolution; rasters are stored like painted ones (f16), not per channel bit depth | crates/app/src/flatten.rs:44 | - |
| Mask is flattened separately (flattened fill plus flattened mask) | Flatten result | n/a | done | crates/app/src/flatten.rs:136 | - |
| Hidden layers and effects are left out | Flatten | n/a | done | crates/app/src/flatten.rs:58 | - |
| Channels set to Disable (or disabled on a fill layer) are skipped | Flatten | n/a | done | crates/app/src/flatten.rs:59 | - |
| Empty selection logs an error instead of creating a layer | Flatten | n/a | done (status line) | crates/ui/src/panels/layers.rs:800 | - |
| Geometry-mask UV tile selection is kept in the flattened layer | Flatten | n/a | done (the part mask is copied) | crates/app/src/flatten.rs:88 | - |
| Export flattened group to files (folder picker) | Layer context menu | n/a | done | crates/ui/src/panels/layers.rs:866 | - |
| Export filename pattern field | Export flattened dialog | $textureSet_$layerName_$srcMap(.$udim) | done | crates/app/src/flatten.rs:25 | - |
| Pattern is remembered for the next export | Export flattened dialog | n/a | partial: for the session, not saved in preferences | crates/ui/src/state.rs:98 | - |
| Batch export when several layers or groups are selected | Export flattened dialog | n/a | done | crates/ui/src/panels/layers.rs:866 | - |
| Fixed padding of 1 px | Export flattened | 1 px | done | crates/app/src/flatten.rs:111 | - |
| Format by channel (EXR for height/normal, PNG otherwise) | Export flattened | n/a | done | crates/app/src/flatten.rs:34 | - |
| Format choice when only a mask is exported | Export flattened dialog | not documented | missing | - | - |
| Flattened images tagged "flattened" and listed in the "Flattened layers" saved search | Assets panel | n/a | missing | - | - |
| File > Remove unused resources cleans flattened images | File menu | n/a | missing | - | - |

## Geometry mask (includes UV tile mask)

Layout notes: the geometry mask is a second, list-based mask on every layer and folder. Clicking its icon on the row switches Properties to a list of mesh names or UV tiles with checkboxes; clicking the content or mask thumbnail leaves the mode. SubCraft's nearest feature is the per-layer "Parts" section in Properties (connected pieces, named objects or UV islands), without a row icon or UV tile mode.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Geometry mask on every layer and folder | Layer row icon | everything included | partial: "Parts" on a layer, edited in Properties | crates/app/src/command.rs:133 | - |
| Icon state: nothing excluded | Layer row icon | default | missing | - | - |
| Icon state: mesh names excluded (shows number still included) | Layer row icon | n/a | partial: text "Visible on N of M parts" in Properties | crates/ui/src/panels/properties.rs:113 | - |
| Icon state: UV tiles excluded (shows number still included) | Layer row icon | n/a | missing | - | - |
| Icon state: nothing included (layer has no effect) | Layer row icon | n/a | missing | - | - |
| Click the icon to enter geometry-mask editing | Layer row icon | n/a | partial: "Pick parts in the 3D view" button | crates/ui/src/panels/properties.rs:105 | - |
| Leave editing by clicking the content or mask thumbnail (folder thumbnail for folders) | Layer row | n/a | partial: "Done picking" button | crates/ui/src/panels/properties.rs:105 | - |
| Masking type dropdown: UV Tiles | Properties > Geometry mask | disabled when the project has no UV tiles | missing | - | - |
| Masking type dropdown: Mesh names | Properties > Geometry mask | Mesh names | partial: part kind comes from the Parts panel (objects, connected pieces, UV islands) | crates/ui/src/panels/parts.rs:51 | - |
| Count of included items over total | Properties > Geometry mask | n/a | done | crates/ui/src/panels/properties.rs:113 | - |
| Selection menu: all | Properties > Geometry mask | n/a | done | crates/ui/src/panels/properties.rs:118 | - |
| Selection menu: none | Properties > Geometry mask | n/a | done | crates/ui/src/panels/properties.rs:121 | - |
| Selection menu: invert | Properties > Geometry mask | n/a | done | crates/ui/src/panels/properties.rs:124 | - |
| Checkbox list of mesh names / UV tiles | Properties > Geometry mask | all checked | done | crates/ui/src/panels/properties.rs:129 | - |
| Click and drag over checkboxes to toggle several | Properties > Geometry mask | n/a | missing | - | - |
| Alt + click a row to isolate it | Properties > Geometry mask | n/a | missing | - | - |
| Hovering a list row highlights that geometry in the viewports | Properties > Geometry mask | n/a | missing | - | - |
| Click geometry in the 3D or 2D view to toggle it | Viewport (edit mode) | n/a | partial: 3D view picking only | crates/ui/src/panels/properties.rs:105 | - |
| Rectangle selection in both viewports | Viewport (edit mode) | n/a | missing | - | - |
| Excluded geometry drawn grey with diagonal lines while editing | Viewport (edit mode) | n/a | missing | - | - |
| Right-click icon > Copy geometry mask | Layer row icon menu | n/a | missing | - | - |
| Right-click icon > Paste into geometry mask | Layer row icon menu | n/a | missing | - | - |
| Right-click icon > Include all | Layer row icon menu | n/a | partial: "All" button in Properties | crates/ui/src/panels/properties.rs:118 | - |
| Right-click icon > Exclude all | Layer row icon menu | n/a | partial: "None" button in Properties | crates/ui/src/panels/properties.rs:121 | - |
| Hide/ignore excluded geometry button (paint through hidden parts) | Viewport contextual toolbar | off; Alt+H | partial: parts can be hidden or isolated per part (H / I under the cursor), not tied to the layer's mask | crates/ui/src/viewport.rs:183 | - |
| Mask is dynamic: strokes are recomputed when the include list changes | Engine | n/a | partial: the part mask multiplies the layer result; strokes are not reprojected | crates/doc/src/layer.rs:134 | - |
| Survives mesh re-import by name / tile number | Engine | n/a | partial: part ids are stable for a stored mesh only | crates/doc/src/layer.rs:137 | - |
| Excluded UV tiles are skipped by the engine (performance) | Engine | n/a | missing | - | - |
| Toggle to ignore the geometry mask temporarily (SubCraft extra: eye in Parts header) | n/a | n/a | done | crates/ui/src/panels/properties.rs:99 | - |

## Smart materials from the layer stack

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Create smart material from a folder (named after the folder, saved to the current shelf) | Folder context menu | n/a | partial: any layer can be saved; label differs | crates/app/src/library.rs:17 | - |
| Add smart material by drag and drop into the stack | Assets > Layer stack | n/a | done | crates/ui/src/panels/shelf.rs:95 | - |
| Add smart material from the header mini-shelf button | Layers header, icon 5 | n/a | missing | - | - |
| Smart material arrives as a folder of layers | Layer stack | n/a | done | crates/ui/src/panels/shelf.rs:62 | - |
| No link back to the source preset once added | Layer stack | n/a | done | crates/app/src/command.rs:51 | - |
| Instanced layers inside a smart material need their source in the folder | Smart material save | n/a | missing (no instancing) | - | - |

## Dynamic material layering and UV tiles (layer-stack side)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Shader-defined sub-layer stacks (extra stacks per texture set, e.g. Mask stacks with an opacity channel) | Texture Set list > sub-stacks | pbr-material-layering: 4 materials, 3 masks | missing | - | - |
| Channels of a sub-stack are fixed by the shader (cannot be added in Texture Set Settings) | Texture Set Settings | n/a | missing | - | - |
| Shader material inputs filled from the shelf | Shader settings | n/a | missing | - | - |
| Export shader parameters JSON for the layered setup | Export window | off | missing | - | - |
| UV tile (UDIM) aware layer stack: each tile its own texture | Project | per project | partial: each UDIM tile becomes its own texture set with its own stack | crates/app/src/new_project.rs:122 | - |
| Fill projection "Fill (match per UV Tile)" for image sequences | Fill layer / fill effect | n/a | missing | - | - |
| Image sequence resource (files grouped by UDIM suffix) usable in any slot | Assets | n/a | missing | - | - |

## Channel-specific layer behaviour (advanced channel painting)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Height: bitmap/substance sources are remapped from 0..1 to -1..1 | Fill and paint sources | n/a | missing (no remap) | - | - |
| Height: uniform colour picks -1..1 directly | Fill / tool material | -1..1 | done | crates/ui/src/widgets.rs:54 | - |
| Height default layer blend: Linear Dodge (Add) | New layer | Linear Dodge (Add) | done | crates/doc/src/blend.rs:158 | - |
| Normal default layer blend: Normal Map Detail | New layer | Normal Map Detail | done | crates/doc/src/blend.rs:158 | - |
| Ambient Occlusion default layer blend: Multiply | New layer | Multiply | done | crates/doc/src/blend.rs:158 | - |
| Normal resource colour space choice (DirectX / OpenGL) on the slot | Fill layer / tool normal slot | DirectX | partial: one project-wide normal format for export, no per-slot choice | crates/doc/src/settings.rs:9 | - |
| Texture set normal mixing (Combine / Replace) for painting over baked normals | Texture Set Settings | Combine | missing | - | - |
| Texture set AO mixing (Multiply / Replace) for painting over baked AO | Texture Set Settings | Multiply | missing | - | - |
| Flow maps painted through the Normal channel with a flat-normal brush and Follow Path | Tool settings | n/a | missing | - | - |
