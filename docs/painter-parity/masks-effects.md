# Painter parity: Masks and the effect stack

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/masking-and-effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/layer-stack , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/blending-modes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/paint , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/fill , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generator , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/filter , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/levels , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/compare-mask , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/anchor-point , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/smart-materials-and-masks , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-9-1 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2021-1-7-1-0 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2019-1 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2017-4 , https://experienceleague.adobe.com/en/docs/substance-3d-dev/painter-python/api/substancepainter-package/layerstack-module/layers-and-effects/levels , https://experienceleague.adobe.com/en/docs/substance-3d-dev/painter-python/api/substancepainter-package/layerstack-module/layers-and-effects/comparemask , https://experienceleague.adobe.com/en/docs/substance-3d-dev/painter-python/api/substancepainter-package/layerstack-module/layers-and-effects/colorselection , https://experienceleague.adobe.com/en/docs/substance-3d-dev/painter-python/api/substancepainter-package/layerstack-module/layers-and-effects/anchorpoint , https://experienceleague.adobe.com/en/docs/substance-3d-dev/painter-python/api/substancepainter-package/layerstack-module/navigation
Checked against: origin/main 297dcb3 (2026-11-11)

## Adding a mask

Layout notes: a mask is added from the Create Mask button in the Layers header or from the layer's right-click menu; both list the same five entries in this order. A mask is always greyscale and multiplies the layer after its blending. Adding a mask again, or removing it, discards the old mask and its effects.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add white mask | Create Mask menu / layer context menu | background White | done | crates/app/src/command.rs:114 | - |
| Add black mask | Create Mask menu / layer context menu | background Black | done | crates/app/src/command.rs:114 | - |
| Add bitmap mask (pick an image; becomes a mask with a fill effect holding the bitmap) | Create Mask menu / layer context menu | n/a | done (image effect on a black mask) | crates/ui/src/panels/layers.rs:57 | - |
| Add mask with color selection (black mask plus a Color Selection effect) | Create Mask menu / layer context menu | n/a | done | crates/ui/src/panels/layers.rs:61 | - |
| Add mask with height combination (mask plus Compare Mask on Height; Height blend switched to Normal) | Create Mask menu / layer context menu | n/a | done | crates/ui/src/panels/layers.rs:70 | chunk 43 |
| Mask on a folder (applies to every layer inside) | Folder row | n/a | done | crates/ui/src/panels/layers.rs:457 | - |
| Mask created automatically when a material is dropped with Ctrl held | Assets > Layer stack | n/a | missing | - | - |
| Mask created automatically when a resource is dropped on the ghost mask slot | Layer stack (while dragging) | n/a | missing | - | - |
| Mask created automatically when mask effects are pasted on a mask-less layer | Layer stack | n/a | done | crates/app/src/session.rs:1238 | - |
| Re-adding a mask replaces the existing mask and its effects | Create Mask menu | n/a | done | crates/app/src/session.rs:1399 | - |
| Colours painted or filled into a mask are converted to greyscale | Mask | n/a | done | crates/doc/src/mask.rs:451 | - |

## Mask thumbnail interactions

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Click the mask thumbnail to edit the mask and show its effect stack | Layer row | n/a | done | crates/ui/src/panels/layers.rs:310 | - |
| Click the content thumbnail to return to the layer content | Layer row | n/a | done | crates/ui/src/panels/layers.rs:296 | - |
| Alt + left click: show the mask alone in the viewport | Mask thumbnail | off | partial: opens a floating UV preview window; the 3D viewport is not switched | crates/ui/src/panels/layers.rs:312 | - |
| Mask display also reachable from the viewport display dropdown | Viewport display mode | n/a | missing | - | - |
| Alt + click again deselects effects / leaves mask view | Mask thumbnail | n/a | partial: the preview window is closed by its own button | crates/ui/src/layer_previews.rs:138 | - |
| Shift + left click: disable or re-enable the mask | Mask thumbnail | enabled | done | crates/ui/src/panels/layers.rs:308 | - |
| Disabled mask is marked on the thumbnail | Mask thumbnail | n/a | done | crates/ui/src/panels/layers.rs:306 | - |
| Red line under the mask thumbnail when the mask has effects | Mask thumbnail | grey | partial: "N fx" text under the layer name | crates/ui/src/panels/layers.rs:339 | - |
| Paint tools switch to greyscale when the mask is selected | Tool properties | n/a | done | crates/ui/src/panels/mask.rs:69 | - |
| Invert grayscale tool value while painting a mask | Shortcut | X | missing (X is symmetry; unbound in the Painter preset) | - | - |
| Tool blending mode greyed out when painting in a mask | Tool properties | n/a | missing | - | - |

## Mask context menu (right-click on the mask thumbnail)

Layout notes: Adobe documents the entries Toggle mask, Copy mask content, Paste into mask, Invert mask background and Create smart mask; the remaining entries and the exact order are from memory of the application and marked (unverified). The Add-effect entries appear above these, as on a layer.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add generator | Mask context menu | n/a | partial: effect-stack "+ Add" menu and row menu submenu, built-in kinds only | crates/ui/src/panels/effects.rs:127 | - |
| Add paint | Mask context menu | n/a | partial: "+ Add > Paint" and row menu "Add paint mask effect" | crates/ui/src/panels/effects.rs:125 | - |
| Add fill | Mask context menu | n/a | partial: "+ Add > Fill" and row menu "Add fill mask effect" | crates/ui/src/panels/effects.rs:126 | - |
| Add levels | Mask context menu | n/a | partial: "+ Add > Levels" only | crates/ui/src/panels/effects.rs:128 | - |
| Add compare mask | Mask context menu | n/a | partial: in the mask stack's Add effect menu | crates/ui/src/panels/effects.rs:146 | chunk 43 |
| Add filter | Mask context menu | n/a | partial: "+ Add > Filter" submenu only | crates/ui/src/panels/effects.rs:94 | - |
| Add color selection | Mask context menu | n/a | partial: "+ Add > ID Color Mask" | crates/ui/src/panels/effects.rs:130 | - |
| Add anchor point | Mask context menu | n/a | partial: "+ Add > Anchor Point" only | crates/ui/src/panels/effects.rs:106 | - |
| Toggle mask | Mask context menu | Shift + click | done ("Disable mask" / "Enable mask") | crates/ui/src/panels/mask.rs:19 | - |
| Remove mask | Mask context menu | (unverified) | done | crates/ui/src/panels/mask.rs:49 | - |
| Invert mask (inverts the painted content) | Mask context menu | (unverified) | partial: appends an Invert effect instead of inverting the pixels | crates/ui/src/panels/mask.rs:33 | - |
| Invert mask background (flips white/black base, keeps effects) | Mask context menu | n/a | missing | - | - |
| Clear mask | Mask context menu | (unverified) | missing | - | - |
| Copy mask content | Mask context menu | n/a | done ("Copy mask") | crates/ui/src/panels/mask.rs:37 | - |
| Paste into mask | Mask context menu | n/a | done ("Paste mask"; also offered on mask-less layers) | crates/ui/src/panels/mask.rs:41 | - |
| Export mask to file | Mask context menu | (unverified) | missing | - | - |
| Create smart mask | Mask context menu | n/a | partial: on the layer row menu, only when the mask has effects | crates/ui/src/panels/layers.rs:480 | - |
| Preview mask (SubCraft extra, same as Alt + click) | n/a | n/a | done | crates/ui/src/panels/mask.rs:14 | - |

## Effect stack: general mechanics

Layout notes: each layer has two independent stacks, one for its content and one for its mask. Selecting a thumbnail shows that stack as indented rows under the layer, top effect first. An effect row has an eye, an icon for its type, a name, and (for most types) its own blending mode and opacity on the right, like a layer row. Properties shows the selected effect's parameters. SubCraft lists the rows the same way, with blend and opacity on the row and in Properties.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Separate effect stacks for content and for mask | Layer row | n/a | done | crates/ui/src/panels/effects.rs:156 | - |
| Effects listed under the layer when its thumbnail is selected | Layer stack | n/a | done | crates/ui/src/panels/layers.rs:753 | - |
| Add effect from the header button | Layers header | n/a | done | crates/ui/src/panels/layers.rs:301 | - |
| Add effect from the right-click menu of a layer or mask | Context menu | n/a | done | crates/ui/src/panels/layers.rs:617 | - |
| New effect is inserted above the selected effect | Effect stack | n/a | done | crates/app/src/session.rs:670 | - |
| Effect visibility toggle (eye) | Effect row | visible | done | crates/ui/src/panels/effects.rs:195 | - |
| Click and slide over eyes to toggle several effects | Effect rows | n/a | done | crates/ui/src/panels/effects.rs:327 | - |
| Effect blending mode (same list as layers; per channel on content, single on mask) | Effect row, right | Normal (unverified) | partial: on the row (content: the view-mode channel) and in Properties; Passthrough not offered | crates/ui/src/panels/effects.rs:192 | - |
| Effect opacity | Effect row, right | 100; 0-100 | done (0-100 on the row) | crates/ui/src/panels/effects.rs:192 | - |
| Blending mode and opacity on filters | Effect row | n/a | done | crates/ui/src/panels/properties.rs:505 | - |
| Blending mode and opacity on Color Selection | Effect row | n/a | done | crates/ui/src/panels/properties.rs:505 | - |
| Reorder by drag and drop inside the same stack | Effect rows | n/a | done (several selected effects move together) | crates/ui/src/panels/effects.rs:439 | - |
| Reorder with Alt + Arrow Up / Arrow Down | Effect rows | Alt+Up / Alt+Down | done | crates/ui/src/panels/effects.rs:287 | - |
| Move an effect to another layer by drag and drop | Effect rows | n/a | done (drop on the layer's row: lands on top of its stack) | crates/ui/src/panels/effects.rs:460 | - |
| Dropping on the other kind of stack switches content/mask automatically (creates the mask if needed) | Effect rows | n/a | partial: mask effects dropped on a layer go to its mask and create it when missing; effects are not converted between content and mask | crates/app/src/session.rs:1182 | - |
| Duplicate with Alt + drag | Effect rows | n/a | done | crates/ui/src/panels/effects.rs:439 | - |
| Duplicate with Ctrl+D | Effect rows | Ctrl+D | done | crates/ui/src/panels/effects.rs:233 | - |
| Multi-selection of effects | Effect rows | Ctrl / Shift click | done | crates/app/src/effect_stack.rs:94 | - |
| Copy effects | Effect rows | Ctrl+C | done | crates/ui/src/clipboard.rs:181 | - |
| Cut effects | Effect rows | Ctrl+X | done | crates/ui/src/clipboard.rs:181 | - |
| Paste effects (inserted above the selection; works across texture sets) | Effect rows / layer | Ctrl+V | done | crates/app/src/session.rs:1369 | - |
| Delete the selected effect with the trash icon | Layers header | n/a | done | crates/ui/src/panels/layers.rs:343 | - |
| Delete the selected effect with the Delete key | Effect rows | Delete | done | crates/ui/src/panels/effects.rs:233 | - |
| Rename Fill and Paint effects | Effect row (double-click) | type name; Fill takes the substance name | done (any effect: double-click or right-click > Rename; a Fill does not take its substance name) | crates/ui/src/panels/effects.rs:221 | - |
| Clicking an effect does not open or close its folder | Effect rows | n/a | done | crates/ui/src/panels/effects.rs:219 | - |
| Drag a resource from Assets onto a stack position to create an effect there | Assets > effect stack | n/a | missing | - | - |
| Effect thumbnails (SubCraft extra: result through that effect) | n/a | n/a | done | crates/ui/src/panels/effects.rs:199 | - |

## Effect context menu (right-click on an effect)

Layout notes: reworked in 7.1 to hold clipboard and move actions; Adobe shows the menu only as an image, so labels and order are (unverified) apart from the documented capabilities (cut, copy, paste, duplicate, move, remove).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Cut effect(s) | Effect context menu | Ctrl+X (unverified label) | done | crates/ui/src/panels/effects.rs:509 | - |
| Copy effect(s) | Effect context menu | Ctrl+C (unverified label) | done | crates/ui/src/panels/effects.rs:513 | - |
| Paste effect(s) | Effect context menu | Ctrl+V (unverified label) | done | crates/ui/src/panels/effects.rs:518 | - |
| Duplicate effect(s) | Effect context menu | Ctrl+D (unverified label) | done | crates/ui/src/panels/effects.rs:522 | - |
| Remove effect(s) | Effect context menu | Delete (unverified label) | done | crates/ui/src/panels/effects.rs:526 | - |
| Move effect up | Effect context menu | Alt+Up (unverified label) | done | crates/ui/src/panels/effects.rs:531 | - |
| Move effect down | Effect context menu | Alt+Down (unverified label) | done | crates/ui/src/panels/effects.rs:531 | - |
| Add-effect entries (insert above this effect) | Effect context menu | (unverified) | done | crates/ui/src/panels/effects.rs:539 | - |

## Add effect menu

Layout notes: entries carry an icon each. Compare Mask and Color Selection are offered only on a mask. Order here follows Adobe's effects page with Color Selection placed before Anchor Point (unverified position).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add generator (content or mask) | Add effect menu | empty slot until a generator is picked | partial: mask only; adds an Edges generator, kind changed in Properties | crates/ui/src/panels/effects.rs:127 | - |
| Add paint (content or mask) | Add effect menu | n/a | done | crates/ui/src/panels/effects.rs:67 | - |
| Add fill (content or mask) | Add effect menu | n/a | done | crates/ui/src/panels/effects.rs:68 | - |
| Add levels (content or mask) | Add effect menu | n/a | done | crates/ui/src/panels/effects.rs:69 | - |
| Add compare mask (mask only) | Add effect menu | n/a | done | crates/ui/src/panels/effects.rs:146 | chunk 43 |
| Add filter (content or mask) | Add effect menu | empty slot until a filter is picked | partial: a submenu of built-in filters instead of an empty slot with a picker | crates/ui/src/panels/effects.rs:82 | - |
| Add color selection (mask only) | Add effect menu | n/a | done | crates/ui/src/panels/effects.rs:142 | - |
| Add anchor point (content or mask) | Add effect menu | n/a | done | crates/ui/src/panels/effects.rs:78 | - |
| Invert (SubCraft extra mask effect; Painter uses the Invert filter) | n/a | n/a | done | crates/ui/src/panels/effects.rs:129 | - |
| Anchor Reference (SubCraft extra mask effect; Painter uses a fill effect that references the anchor) | n/a | n/a | done | crates/ui/src/panels/effects.rs:113 | - |

## Paint effect

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Paint over the effects below it with any paint tool | Effect stack | n/a | done | crates/app/src/command.rs:36 | - |
| Per-channel blending mode and opacity (content) | Effect row | Normal; 100 | done (in Properties) | crates/ui/src/panels/effects.rs:441 | - |
| Blending mode and opacity (mask) | Effect row | Normal; 100 | done (in Properties) | crates/ui/src/panels/properties.rs:505 | - |
| Tool properties shown in Properties while the paint effect is selected | Properties | n/a | partial: effect info is shown; brush settings stay in the Brush panel | crates/ui/src/panels/effects.rs:353 | - |
| Eraser removes only this effect's strokes | Eraser tool | n/a | done | crates/ui/src/panels/effects.rs:354 | - |
| Paint effect usable on a fill layer | Effect stack | n/a | done | crates/ui/src/panels/effects.rs:354 | - |
| Paint effect usable on a folder | Effect stack | not documented | missing (folders accept adjustment effects only) | - | - |

## Fill effect

Layout notes: same Properties as a fill layer (projection, UV transform, material slot, per-channel sources); see the fill-layer checklist for the individual parameters.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Fill effect on content with the full fill-layer parameters | Properties > Fill | n/a | partial: Uniform and Procedural sources only; no image, mesh-map or anchor source | crates/ui/src/panels/effects.rs:380 | - |
| Per-channel enable buttons | Properties > Fill | all channels on (unverified) | partial: only Base Color on by default | crates/ui/src/panels/effects.rs:57 | - |
| Projection and UV transform | Properties > Fill | UV projection | done | crates/ui/src/panels/effects.rs:366 | - |
| Fill effect on a mask (single greyscale source) | Properties > Fill | white (1.0) since 11.x | done | crates/ui/src/panels/effects.rs:126 | - |
| Greyscale source can be a uniform value | Properties > grayscale | 1.0; 0-1 | done | crates/ui/src/panels/properties.rs:653 | - |
| Greyscale source can be any resource (texture, procedural, alpha) via the mini-shelf | Properties > grayscale slot | n/a | partial: built-in procedural patterns; images through the separate Image effect | crates/ui/src/panels/properties.rs:643 | - |
| Greyscale source can be an anchor point | Properties > grayscale slot > Anchor points tab | n/a | partial: separate "Anchor Reference" effect | crates/ui/src/panels/properties.rs:526 | - |
| Drop a single-channel image on a fill effect to set its source | Assets > effect | n/a | missing | - | - |
| Blending mode and opacity | Effect row | Normal; 100 | done (in Properties) | crates/ui/src/panels/properties.rs:505 | - |
| Rename the fill effect | Effect row | substance name | partial: content effects only | crates/ui/src/panels/effects.rs:344 | - |

## Generator effect (mechanics only)

Layout notes: parameter lists of individual generators are in the generators checklist.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add a generator on a mask | Mask context menu / Add effect | empty slot | done | crates/ui/src/panels/effects.rs:127 | - |
| Add a generator on layer content | Layer context menu / Add effect | empty slot | missing | - | - |
| "Generator" button in Properties opens the mini-shelf to pick one | Properties > Generator | no generator selected | partial: kind chosen from fixed menus, no shelf picker | crates/ui/src/panels/layers.rs:36 | - |
| Generator rows visible under the layer even when the mask is not selected (SubCraft extra) | n/a | n/a | done | crates/ui/src/panels/layers.rs:562 | - |
| Output selector for generators with several outputs when used in a mask | Properties > Generator | n/a | missing | - | - |
| Image inputs of a generator can reference an anchor point | Properties > Generator > image inputs | n/a | done | crates/ui/src/panels/properties.rs:555 | - |
| Blending mode and opacity | Effect row | Normal; 100 | done (in Properties) | crates/ui/src/panels/properties.rs:505 | - |
| Drag a generator from Assets onto a layer or mask | Assets > Layer stack | n/a | missing | - | - |
| Custom generators (sbsar) from the shelf | Assets | n/a | missing | - | - |

## Filter effect (mechanics only)

Layout notes: parameter lists of individual filters are in the filters checklist.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add a filter on layer content | Add effect > Add filter | empty slot | done | crates/ui/src/panels/effects.rs:82 | - |
| Add a filter on a mask | Add effect > Add filter | empty slot | done | crates/ui/src/panels/effects.rs:94 | - |
| "Filter" button in Properties opens the mini-shelf to pick one | Properties > Filter | no filter selected | partial: filter chosen in the Add submenu; cannot be swapped afterwards | crates/ui/src/panels/effects.rs:83 | - |
| Filter applies to all channels it supports | Properties > Filter | n/a | partial: one channel per filter effect, picked in Properties | crates/ui/src/panels/effects.rs:430 | - |
| Blending mode and opacity on filters | Effect row | Normal; 100 | done (in Properties) | crates/ui/src/panels/effects.rs:441 | - |
| Filter on a Passthrough layer affects everything below | Layer blending | n/a | missing (Passthrough is folder-only) | - | - |
| Drag a filter from Assets into the stack: new layer with the filter, all channels Passthrough | Assets > Layer stack | n/a | missing | - | - |
| Drag a filter onto the mesh in the viewport | Assets > Viewport | n/a | missing | - | - |
| Error when an incompatible filter is used in a mask | Layer stack | n/a | done (mask menu lists mask-capable filters only) | crates/ui/src/panels/effects.rs:96 | - |
| Custom filters (sbsar) from the shelf | Assets | n/a | missing | - | - |

## Levels effect

Layout notes: Properties shows, top to bottom: the Affected channel dropdown (content only) and the colour-component dropdown at the top right; the histogram; three handles above the histogram (shadows, midtones, highlights = input min, gamma, input max); two handles below (output black point and white point); numeric fields; Auto and Invert buttons (unverified); Clamp checkbox at the bottom right.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Affected channel (one channel per Levels effect) | Properties > Levels | first compatible channel | done (Normal excluded) | crates/ui/src/panels/effects.rs:411 | - |
| Colour component dropdown (full RGB or R, G, B separately; Luminance for some channels) | Properties > Levels, top right | RGB | done: RGB / R / G / B on colour channels (no Luminance entry) | crates/ui/src/panels/properties.rs:878 | chunk 43 |
| Histogram display of the input | Properties > Levels | n/a | done: input histogram (128² UV sample) with the handles marked | crates/ui/src/panels/properties.rs:878 | chunk 43 |
| Input low handle (shadows) | Properties > Levels, above histogram | 0; 0-1 | done (slider "In low") | crates/ui/src/panels/effects.rs:418 | - |
| Midtone handle (gamma) | Properties > Levels, above histogram | 1.0 (0.5 handle position); range not documented | partial: "Gamma" slider 0.1-4 (no handle on the histogram) | crates/ui/src/panels/properties.rs:878 | chunk 43 |
| Input high handle (highlights) | Properties > Levels, above histogram | 1; 0-1 | done (slider "In high") | crates/ui/src/panels/effects.rs:419 | - |
| Output low handle (black point) | Properties > Levels, below histogram | 0; 0-1 | done (slider "Out low") | crates/ui/src/panels/effects.rs:421 | - |
| Output high handle (white point) | Properties > Levels, below histogram | 1; 0-1 | done (slider "Out high") | crates/ui/src/panels/effects.rs:422 | - |
| Swapping the output handles inverts the image | Properties > Levels | n/a | done (out low above out high) | crates/doc/src/mask.rs:29 | - |
| Per-component values (separate R, G, B levels) | Properties > Levels | (0,0,0) / (1,1,1) / gamma (1,1,1) | done | crates/ui/src/panels/properties.rs:878 | chunk 43 |
| Auto button | Properties > Levels | (unverified) | done: input handles at the input's darkest and brightest values | crates/ui/src/panels/properties.rs:878 | chunk 43 |
| Invert button | Properties > Levels | (unverified) | done: swaps the output handles | crates/ui/src/panels/properties.rs:878 | chunk 43 |
| Clamp checkbox | Properties > Levels, bottom right | off (API default); should be on for non-HDR channels | done: on by default (older projects unchanged); off extends past the input handles | crates/ui/src/panels/properties.rs:878 | chunk 43 |
| Levels on a mask (single greyscale channel, no Affected channel) | Properties > Levels | n/a | done | crates/ui/src/panels/properties.rs:656 | - |
| Blending mode and opacity | Effect row | Normal; 100 | done (in Properties) | crates/ui/src/panels/properties.rs:505 | - |
| Levels on a folder | Effect stack | n/a | done | crates/ui/src/panels/effects.rs:72 | - |

## Compare Mask effect

Layout notes: mask-only effect. Properties shows the Channel dropdown, then one row with three controls (left operand, operation, right operand), then Constant, Tolerance, Hardness and the histogram of both operands.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Compare Mask effect (mask only) | Add effect > Add compare mask | n/a | done | crates/doc/src/mask.rs:198 | chunk 43 |
| Channel (which channel is compared; from Texture Set Settings) | Properties > Compare Mask | Height when created by "height combination"; otherwise not documented | done: Height when created by height combination | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Left operand: Layers Below | Properties > Compare, left dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Left operand: This Layer | Properties > Compare, left dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Left operand: This Mask | Properties > Compare, left dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Left operand: Constant | Properties > Compare, left dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Operation: Lesser than | Properties > Compare, middle dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Operation: Within tolerance | Properties > Compare, middle dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Operation: Greater than | Properties > Compare, middle dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Right operand: Layers Below | Properties > Compare, right dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Right operand: This Layer | Properties > Compare, right dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Right operand: This Mask | Properties > Compare, right dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Right operand: Constant | Properties > Compare, right dropdown | not documented | done | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Constant (value used by a Constant operand) | Properties > Compare Mask | default not documented; 0-1 | done: default 0.5 | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Tolerance (used by Within tolerance) | Properties > Compare Mask | default not documented; 0-1 | done: default 0.1 | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Hardness (edge softness of the result) | Properties > Compare Mask | default not documented; 0-1 | done: default 0.9; linear edge (1 - hardness) x 0.25 wide (SubCraft choice, not documented) | crates/ui/src/panels/properties.rs:957 | chunk 43 |
| Source channels histogram (both operands overlaid) | Properties > Compare Mask | n/a | missing | - | - |
| "Add mask with height combination" shortcut | Layer context menu / Create Mask menu | sets Height blend to Normal | done: Height blending set to Normal | crates/ui/src/panels/layers.rs:70 | chunk 43 |

## Color Selection effect

Layout notes: mask-only effect that selects areas of an ID map by colour. Properties shows the ID mask slot, a "Pick color" button with the list of picked colours, then Hardness, Tolerance, Output value and Background colour. Adobe's user guide has no parameter table; names and ranges come from the public Python API reference, layout from memory (unverified).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Color Selection effect (mask only) | Add effect > Add color selection | n/a | partial: "ID Color Mask" | crates/doc/src/mask.rs:320 | - |
| ID mask slot (which image to select from) | Properties > Color selection | the baked ID mesh map | partial: always the baked ID map, no slot | crates/ui/src/panels/properties.rs:692 | - |
| Pick color button (eyedropper in 3D or 2D view) | Properties > Color selection | n/a | partial: 3D view only | crates/ui/src/panels/properties.rs:711 | - |
| Several picked colours in one effect | Properties > Color selection | empty | done (up to 64) | crates/ui/src/panels/properties.rs:698 | - |
| Remove a picked colour | Properties > Color selection | n/a | done | crates/ui/src/panels/properties.rs:703 | - |
| Hardness | Properties > Color selection | default not documented; 0-1 | done: stored as softness = 1 - hardness, default 1 | crates/ui/src/panels/properties.rs:824 | chunk 43 |
| Tolerance | Properties > Color selection | default not documented; 0-1 | done (default 0.05) | crates/ui/src/panels/properties.rs:719 | - |
| Output value (grey level written where colours match) | Properties > Color selection | default not documented; 0-1 | done: default 1 | crates/ui/src/panels/properties.rs:824 | chunk 43 |
| Background color: Transparent | Properties > Color selection | not documented | done | crates/ui/src/panels/properties.rs:824 | chunk 43 |
| Background color: Black | Properties > Color selection | not documented | done (default) | crates/ui/src/panels/properties.rs:824 | chunk 43 |
| Background color: White | Properties > Color selection | not documented | done (the older Invert switch still shows while on) | crates/ui/src/panels/properties.rs:824 | chunk 43 |
| Background color: Gray | Properties > Color selection | not documented | done: 0.5 | crates/ui/src/panels/properties.rs:824 | chunk 43 |
| Blending mode and opacity | Effect row | Normal; 100 | done (in Properties) | crates/ui/src/panels/properties.rs:505 | - |
| Warning when no ID map is baked | Properties > Color selection | n/a | done | crates/ui/src/panels/properties.rs:692 | - |
| Created automatically by Ctrl-dropping a resource on the mesh (selection from the clicked ID colour) | Assets > Viewport | n/a | partial: viewport part menu "Mask by ID color" on the selected layer | crates/ui/src/panels/parts.rs:19 | - |

## Anchor Point effect

Layout notes: an anchor point is a named marker in a content or mask stack. Its own Properties show the name and the list of places that reference it. A reference is made in a fill layer, a fill effect, or an image input of a filter/generator by opening the resource mini-shelf and choosing the "Anchor points" tab; the slot then offers the referenced channel.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add anchor point on layer content | Add effect > Add anchor point | name "Anchor point" plus number (unverified) | done | crates/ui/src/panels/effects.rs:78 | - |
| Add anchor point on a mask | Add effect > Add anchor point | n/a | done | crates/ui/src/panels/effects.rs:106 | - |
| Anchor name (rename on the row) | Effect row | n/a | partial: Name field in Properties | crates/ui/src/panels/properties.rs:516 | - |
| Anchor has no blending mode or opacity | Effect row | n/a | done | crates/ui/src/panels/properties.rs:504 | - |
| Reference an anchor from a Fill layer channel | Fill layer slot > mini-shelf > Anchor points tab | n/a | partial: Source dropdown "Anchor" plus anchor picker, no mini-shelf tab | crates/ui/src/panels/properties.rs:240 | - |
| Reference an anchor from a Fill effect | Fill effect slot > Anchor points tab | n/a | partial: on masks via the "Anchor Reference" effect; content fill effects cannot | crates/ui/src/panels/effects.rs:113 | - |
| Reference an anchor from an image input of a filter or generator | Properties > image inputs > Anchor points tab | n/a | partial: generator input only (not Directional / Position gradient); no filter inputs | crates/ui/src/panels/properties.rs:555 | - |
| Referenced channel dropdown (which channel of the anchored content to read) | Slot using the anchor | not documented | done ("Channel", content anchors) | crates/ui/src/panels/anchor.rs:67 | - |
| Reading a mask anchor (greyscale) | Slot using the anchor | n/a | done | crates/ui/src/panels/anchor.rs:14 | - |
| Extract alpha option on a referenced anchor | Slot using the anchor | (unverified) | partial: coverage is always multiplied in, no toggle | crates/ui/src/panels/properties.rs:544 | - |
| Only anchors below the referencing layer are listed | Anchor picker | n/a | done | crates/ui/src/panels/anchor.rs:36 | - |
| Moving an anchor above its reference breaks the link (undo restores it) | Layer stack | n/a | done (reference flagged, input ignored) | crates/ui/src/panels/layers.rs:342 | - |
| No references across texture sets | Layer stack | n/a | done | crates/ui/src/panels/anchor.rs:36 | - |
| Anchor Properties list every layer that references it | Properties > Anchor point | n/a | done | crates/ui/src/panels/anchor.rs:102 | - |
| Click a reference in that list to jump to the referencing layer | Properties > Anchor point | n/a | missing | - | - |
| Jump from a fill layer/effect to the anchor it uses | Properties > slot | n/a | missing | - | - |
| References kept when layers are duplicated or copy/pasted together | Layer stack | n/a | done | crates/app/src/clipboard.rs:209 | - |
| Levels on the referenced value (SubCraft extra on Anchor Reference) | n/a | n/a | done | crates/ui/src/panels/properties.rs:533 | - |
| Micro details / micro height options in generators driven by an anchor | Generator parameters | off | partial: "Micro effect" on generators | crates/ui/src/panels/properties.rs:625 | - |

## Smart masks

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Create smart mask (right-click a mask or layer; saved to the shelf) | Mask / layer context menu | named after the layer (unverified) | partial: "Save mask as smart mask" on the layer row, needs at least one effect | crates/app/src/library.rs:35 | - |
| Apply by dragging a smart mask onto a layer | Assets > Layer stack | n/a | done | crates/ui/src/panels/layers.rs:411 | - |
| A black mask is created when the layer has none | Layer stack | black | done | crates/app/src/session.rs:1138 | - |
| Effects are merged with an existing mask's effects | Layer stack | n/a | done | crates/app/src/session.rs:1139 | - |
| Dropping several smart masks accumulates them | Layer stack | n/a | done | crates/app/src/session.rs:1139 | - |
| Ctrl while dropping replaces the whole effect stack | Layer stack | n/a | missing | - | - |
| Alt while dropping on a thumbnail removes the previous effects first (9.1) | Layer stack | n/a | missing | - | - |
| Drop a smart mask in the viewport or empty stack area: new fill layer with that mask (9.1) | Assets > Viewport / Layer stack | n/a | missing | - | - |
| Smart masks only apply to mask stacks | Layer stack | n/a | done | crates/app/src/command.rs:71 | - |
| Smart mask files stored in the shelf folder and shareable | Assets | n/a | done | crates/app/src/library.rs:35 | - |
