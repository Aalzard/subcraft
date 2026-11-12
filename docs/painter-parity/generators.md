# Painter parity: Generators

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generator , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/anchor-point , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/3d-distance , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/3d-linear-gradient , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/ambient-occlusion , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/auto-stitcher , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/curvature , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/dirt , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/dripping-rust , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/fiber-glass-edge-wear , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/grayscale-conversion , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/inflate-shrinkwrap , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/light , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/mask-builder , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/mask-editor , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/metal-edge-wear , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/position , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/tri-planar-advanced , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/uv-border-distance , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/uv-checker , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/uv-random-color , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/uv-texel-density , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/generators/world-space-normals , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/creating-custom-effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/generators , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/mesh-map , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/mesh-based-input , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/user-data , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-9-1 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-11-0 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-12-2 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2019-1 , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2018-3
Checked against: origin/main 297dcb3 (2026-11-11)

## Generic generator UI

Layout notes: A generator is an effect row under a layer (content stack) or under its mask (mask stack). Selecting the row shows, top to bottom in Properties: the effect's blending mode and opacity, a Generator button that opens the mini shelf to pick the resource, the generator's Parameters (Seed with Random first, then its sliders and collapsible sub-groups), then the Image inputs slots (mesh maps are filled in automatically, the others take a resource or an anchor point). The generator pages give no numeric defaults or ranges, so those cells read "not documented". SubCraft generators exist only as mask effects with seven built-in kinds (Edges, Dirt, Ambient Occlusion, Concave Curvature, Directional, Position Gradient, Mesh Map), picked from a combo box.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add generator (on the mask) | Layers panel > right-click a mask | - | done | crates/ui/src/panels/layers.rs:467 | - |
| Add generator (on the layer content) | Layers panel > right-click a layer | - | done: "Add content generator" (the layer menu's "Add generator" lists mask generators); gray result into every channel but Normal, per-channel blending | crates/ui/src/panels/effects.rs:74 | chunk 35 |
| Add generator | Layers panel > effects button above the stack | - | done | crates/ui/src/panels/layers.rs:176 | - |
| Generator row under its layer (mask effects show when the mask is selected) | Layers panel > effect rows | - | done | crates/ui/src/panels/layers.rs:562 | - |
| Right-click an effect to reorder it | Layers panel > effect rows | - | done | crates/ui/src/panels/effects.rs:274 | - |
| ALT+Up arrow / ALT+Down arrow reorders the effect | Layers panel > effect rows | - | done | crates/ui/src/panels/effects.rs:290 | parity chunk 19 |
| Generator button (opens the mini shelf to pick the generator resource) | Properties > Generator | - | partial: combo box of 7 built-in kinds plus mesh maps, no mini shelf | crates/ui/src/panels/properties.rs:547 | - |
| Mini shelf: thumbnails of generator resources | Properties > Generator | not documented | missing | - | - |
| Mini shelf: search field | Properties > Generator | not documented | missing | - | - |
| Empty generator (added before a resource is picked) | Properties > Generator | - | partial: a new generator always starts as Edges | crates/ui/src/panels/effects.rs:127 | - |
| Parameters section (per-generator sliders, toggles, dropdowns) | Properties > Generator | - | partial: only for the built-in kinds | crates/ui/src/panels/properties.rs:411 | - |
| Seed value field (pencil shows / edits the number) | Properties > Generator | not documented | partial: plain number field, always visible | crates/ui/src/panels/properties.rs:601 | - |
| Seed Random button (new random seed) | Properties > Generator | - | done: Random next to Next | crates/ui/src/panels/properties.rs:762 | chunk 35 |
| Image inputs section (one slot per input) | Properties > Generator | - | missing: no resource slots | - | - |
| Image input slot: pick a texture / procedural from the mini shelf | Properties > Generator | - | missing | - | - |
| Image input slot: use an anchor point (anchor must be below the referencing layer, same texture set) | Properties > Generator | - | partial: an anchor can replace the baked map the generator reads; no extra inputs | crates/ui/src/panels/properties.rs:553 | - |
| Image input slot: clear / remove the resource | Properties > Generator | not documented | partial: anchor picker has a "Baked mesh map" entry to go back | crates/ui/src/panels/properties.rs:555 | - |
| Mesh map inputs connected automatically from the baked maps | Properties > Generator | - | done | crates/materials/src/sample/mesh_maps.rs:12 | - |
| Selector for generators with several outputs when used in a mask (9.1) | Properties > Generator | (unverified) placement and options | missing | - | - |
| Monochrome generator used on a fill layer (as content) | Properties > Generator | - | missing | - | - |
| Full-colour generator used as a mask | Properties > Generator | - | missing | - | - |
| Effect blending mode | Properties > Generator | Normal (unverified) | done | crates/ui/src/panels/properties.rs:506 | - |
| Effect opacity | Properties > Generator | 100; 0-100 (unverified) | done | crates/ui/src/panels/properties.rs:512 | - |
| Generators category in the Assets panel | Assets panel | - | missing | - | - |
| Drag a custom generator (.sbsar) onto the Assets panel to import it | Assets panel | - | missing | - | - |
| Generator preview sphere thumbnails | Assets panel | - | missing | - | - |
| Painter Generator graph template | Substance 3D Designer | - | n/a: Designer feature | - | - |

## 3D Distance

Layout notes: Output: monochrome gradient from a 3D source point. Needs the baked Position map.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| 3D Distance (generator) | Properties > Generator | used on: mask or fill | done: 1 within Offset, linear to 0 over Radius (units of the longest side); positions in the bounding box 0..1 | crates/doc/src/mask.rs:91 | chunk 49 |
| Position | Properties > Generator > Image inputs | baked Position map | partial: positions come from the mesh (no baked Position map needed or read) | crates/compositor/src/mask.rs:182 | chunk 49 |
| Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Position X | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Position Y | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Position Z | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Radius | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Offset | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Contrast | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |

## 3D Linear Gradient

Layout notes: Gradient between two points picked as colours of the Position map (show the Position map in the viewport and use the colour picker).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| 3D Linear Gradient (generator) | Properties > Generator | used on: mask or fill | done | crates/doc/src/mask.rs:91 | chunk 49 |
| Position | Properties > Generator > Image inputs | baked Position map | partial: positions come from the mesh (no baked Position map needed or read) | crates/compositor/src/mask.rs:182 | chunk 49 |
| Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Balance | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Contrast | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| 3D Position Start | Properties > Generator > Parameters | colour picked from the Position map | partial: typed as X / Y / Z in the bounding box (0..1), no colour picker on the Position map | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| 3D Position End | Properties > Generator > Parameters | colour picked from the Position map | partial: typed as X / Y / Z in the bounding box (0..1), no colour picker on the Position map | crates/ui/src/panels/properties.rs:427 | chunk 49 |

## Ambient Occlusion

Layout notes: Global controls first, then the Use Texture / Use Micro Details toggles, then the Ambient Occlusion, Texture and Micro Details groups. Occluded areas are dark by default.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Ambient Occlusion (generator) | Properties > Generator | used on: mask or fill | partial: Ambient Occlusion kind with Balance, Contrast, Invert only (occluded = white) | crates/doc/src/mask.rs:222 | - |
| Texture | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Normal | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Height | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Ambient Occlusion | Properties > Generator > Image inputs | grayscale; baked AO map | partial: baked map automatic, anchor can replace it, no resource slot | crates/ui/src/panels/properties.rs:553 | - |
| World Space Normals | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Position Gradient | Properties > Generator > Image inputs | colour; baked Position map | missing | - | - |
| Global Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:616 | - |
| Global Blur | Properties > Generator > Parameters | not documented | missing | - | - |
| Global Balance | Properties > Generator > Parameters | not documented | partial: one Balance stage only, it doubles as the per-map balance | crates/ui/src/panels/properties.rs:423 | - |
| Global Contrast | Properties > Generator > Parameters | not documented | partial: one Contrast stage only, it doubles as the per-map contrast | crates/ui/src/panels/properties.rs:428 | - |
| Use Texture | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Use Micro Details | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Invert | Properties > Generator > Ambient Occlusion | not documented (toggle) | missing | - | - |
| Blur | Properties > Generator > Ambient Occlusion | not documented | missing | - | - |
| Balance | Properties > Generator > Ambient Occlusion | not documented | missing | - | - |
| Contrast | Properties > Generator > Ambient Occlusion | not documented | missing | - | - |
| Texture Opacity | Properties > Generator > Texture | not documented | missing | - | - |
| Invert | Properties > Generator > Texture | not documented | missing | - | - |
| Grayscale Conversion | Properties > Generator > Texture | not documented; options as in the Grayscale Conversion generator | missing | - | - |
| Blending Mode | Properties > Generator > Texture | not documented; layer blending-mode list | missing | - | - |
| Scale | Properties > Generator > Texture | not documented | missing | - | - |
| Contrast | Properties > Generator > Texture | not documented | missing | - | - |
| Brightness | Properties > Generator > Texture | not documented | missing | - | - |
| Triplanar | Properties > Generator > Texture | not documented (toggle) | missing | - | - |
| Triplanar Contrast | Properties > Generator > Texture | not documented | missing | - | - |
| Micro Height (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Micro Normal (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Radius | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Depth | Properties > Generator > Micro Details | not documented | missing | - | - |

## Auto Stitcher

Layout notes: Stitches along paths found from UV seams, curvature or a custom map. Monochrome output.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Auto Stitcher (generator) | Properties > Generator | used on: mask or fill | missing | - | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked Curvature map | missing | - | - |
| Custom Input | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Mask Mode | Properties > Generator > Parameters | not documented; options UV Mask / Curvature / Custom Input | missing | - | - |
| Path Smoothness | Properties > Generator > Parameters | not documented | missing | - | - |
| Path Position | Properties > Generator > Parameters | not documented | missing | - | - |
| Stitch Size | Properties > Generator > Parameters | not documented | missing | - | - |
| Stitch Width | Properties > Generator > Parameters | not documented | missing | - | - |
| Stitch Length | Properties > Generator > Parameters | not documented | missing | - | - |
| Stitch Roundness | Properties > Generator > Parameters | not documented | missing | - | - |
| Jitter | Properties > Generator > Parameters | not documented | missing | - | - |

## Curvature

Layout notes: Same frame as Ambient Occlusion: global controls, two Use toggles, then Curvature, Texture and Micro Details groups. The seven band sliders run from the finest (Sharp) to the widest (Huge) detail.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Curvature (generator) | Properties > Generator | used on: mask or fill | partial: split over the Edges, Concave Curvature and Mesh Map: Curvature kinds; no bands, texture or micro details | crates/doc/src/mask.rs:209 | #131 |
| Texture | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Normal | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Height | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked Curvature map | partial: baked map automatic, anchor can replace it, no resource slot | crates/ui/src/panels/properties.rs:553 | - |
| World Space Normals | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Position Gradient | Properties > Generator > Image inputs | colour; baked Position map | missing | - | - |
| Global Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:616 | - |
| Global Blur | Properties > Generator > Parameters | not documented | missing | - | - |
| Global Balance | Properties > Generator > Parameters | not documented | partial: one Balance stage only, it doubles as the per-map balance | crates/ui/src/panels/properties.rs:423 | - |
| Global Contrast | Properties > Generator > Parameters | not documented | partial: one Contrast stage only, it doubles as the per-map contrast | crates/ui/src/panels/properties.rs:428 | - |
| Use Texture | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Use Micro Details | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Invert | Properties > Generator > Curvature | not documented (toggle) | missing | - | - |
| Mode | Properties > Generator > Curvature | not documented; options Edges / Cavities / Dual / Unprocessed | partial: chosen as the generator kind (Edges, Concave Curvature, Mesh Map: Curvature); no Dual | crates/ui/src/panels/properties.rs:548 | #131 |
| Sharp | Properties > Generator > Curvature | not documented | missing | - | - |
| Fine | Properties > Generator > Curvature | not documented | missing | - | #131 |
| Soft | Properties > Generator > Curvature | not documented | missing | - | - |
| Medium | Properties > Generator > Curvature | not documented | missing | - | #131 |
| Large | Properties > Generator > Curvature | not documented | missing | - | #131 |
| Big | Properties > Generator > Curvature | not documented | missing | - | - |
| Huge | Properties > Generator > Curvature | not documented | missing | - | - |
| Contrast | Properties > Generator > Curvature | not documented | missing | - | - |
| Brightness | Properties > Generator > Curvature | not documented | missing | - | - |
| Texture Opacity | Properties > Generator > Texture | not documented | missing | - | - |
| Invert | Properties > Generator > Texture | not documented | missing | - | - |
| Grayscale Conversion | Properties > Generator > Texture | not documented; options as in the Grayscale Conversion generator | missing | - | - |
| Blending Mode | Properties > Generator > Texture | not documented; layer blending-mode list | missing | - | - |
| Scale | Properties > Generator > Texture | not documented | missing | - | - |
| Contrast | Properties > Generator > Texture | not documented | missing | - | - |
| Brightness | Properties > Generator > Texture | not documented | missing | - | - |
| Triplanar | Properties > Generator > Texture | not documented (toggle) | missing | - | - |
| Triplanar Contrast | Properties > Generator > Texture | not documented | missing | - | - |
| Micro Height (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Micro Normal (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Curvature Type | Properties > Generator > Micro Details | not documented; options Standard / Sobel / Smooth | missing | - | - |
| Curvature Intensity | Properties > Generator > Micro Details | not documented (applies to Standard and Sobel) | missing | - | - |
| Height Details Intensity | Properties > Generator > Micro Details | not documented | missing | - | - |

## Dirt

Layout notes: Seed and Invert first, then level / contrast, triplanar, grunge, Edges Masking, then the Micro Details group.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Dirt (generator) | Properties > Generator | used on: mask or fill | partial: Dirt kind (occlusion based); no custom grunge, triplanar contrast or micro-detail inputs | crates/doc/src/mask.rs:211 | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked map | partial: read automatically for Edges Masking, no slot | crates/ui/src/panels/properties.rs:450 | - |
| Ambient Occlusion | Properties > Generator > Image inputs | grayscale; baked map | partial: baked map automatic, anchor can replace it, no resource slot | crates/ui/src/panels/properties.rs:553 | - |
| World Space Normal | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Position | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Custom grunge | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Micro Normal | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Height | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | partial: number field and "Next" (seed + 1), no random | crates/ui/src/panels/properties.rs:600 | - |
| Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:616 | - |
| Dirt Level | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:418 | - |
| Dirt Contrast | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:428 | - |
| Use Triplanar | Properties > Generator > Parameters | not documented (toggle) | partial: no toggle; "Reduce repetition" switches to object-space noise | crates/ui/src/panels/properties.rs:614 | - |
| Triplanar Blending Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Grunge Amount | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:437 | - |
| Grunge Scale | Properties > Generator > Parameters | not documented | partial: "Pattern size" in percent of the model, 1-100 | crates/ui/src/panels/properties.rs:596 | - |
| Use Custom Grunge | Properties > Generator > Parameters | not documented (toggle) | partial: "Grunge map" switches to one bundled map, no custom image | crates/ui/src/panels/properties.rs:466 | - |
| Edges Masking | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:450 | - |
| Micro Height (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Micro Normal (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Curvature Type | Properties > Generator > Micro Details | not documented; options Standard / Sobel / Smooth | missing | - | - |
| Curvature Intensity | Properties > Generator > Micro Details | not documented (applies to Standard and Sobel) | missing | - | - |
| Height Details Intensity | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Radius | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Depth | Properties > Generator > Micro Details | not documented | missing | - | - |

## Dripping Rust

Layout notes: Downward streaks from curvature and occlusion along one Position axis.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Dripping Rust (generator) | Properties > Generator | used on: mask or fill | missing | - | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked map | missing | - | - |
| Ambient occlusion | Properties > Generator > Image inputs | grayscale; baked map | missing | - | - |
| Position | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | missing | - | - |
| Invert | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Rust Spreading | Properties > Generator > Parameters | not documented | missing | - | - |
| Rust Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Spreading Smoothness | Properties > Generator > Parameters | not documented | missing | - | - |
| Drips Intensity | Properties > Generator > Parameters | not documented | missing | - | - |
| Drips Smoothness | Properties > Generator > Parameters | not documented | missing | - | - |
| Drips Samples Amount | Properties > Generator > Parameters | not documented | missing | - | - |
| Position axis | Properties > Generator > Parameters | not documented; options Y (green) / X (red) / Z (blue) | missing | - | - |

## Fiber Glass Edge Wear

Layout notes: Same layout as Metal Edge Wear, without Grunge Scale.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Fiber Glass Edge Wear (generator) | Properties > Generator | used on: mask or fill | missing | - | - |
| Custom grunge | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked map | missing | - | - |
| Ambient Occlusion | Properties > Generator > Image inputs | grayscale; baked map | missing | - | - |
| World Space Normal | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Position | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Micro Normal | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Height | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | missing | - | - |
| Invert | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Wear Level | Properties > Generator > Parameters | not documented | missing | - | - |
| Wear Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Use Triplanar | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Triplanar Blending Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Grunge Amount | Properties > Generator > Parameters | not documented | missing | - | - |
| Use Custom Grunge | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Edges Smoothness | Properties > Generator > Parameters | not documented | missing | - | - |
| Ambient Occlusion Masking | Properties > Generator > Parameters | not documented | missing | - | - |
| Curvature Weight | Properties > Generator > Parameters | not documented | missing | - | - |
| Micro Height (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Micro Normal (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Curvature Type | Properties > Generator > Micro Details | not documented; options Standard / Sobel / Smooth | missing | - | - |
| Curvature Intensity | Properties > Generator > Micro Details | not documented (applies to Standard and Sobel) | missing | - | - |
| Height Details Intensity | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Radius | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Depth | Properties > Generator > Micro Details | not documented | missing | - | - |

## Grayscale Conversion

Layout notes: Turns a colour source (texture or anchor) into a grayscale mask.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Grayscale Conversion (generator) | Properties > Generator | used on: mask or fill | done | crates/doc/src/mask.rs:91 | chunk 49 |
| Source | Properties > Generator > Image inputs | colour; texture or anchor | partial: an image resource (anchors not offered) | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Grayscale type | Properties > Generator > Parameters | not documented; options Desaturation / Luma / Average / Max / Min / Red channel / Green channel / Blue channel | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Balance | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Contrast | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Tile | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Rotation | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Safe Rotation | Properties > Generator > Parameters | not documented (toggle; locks rotation to 45 degree steps) | done | crates/ui/src/panels/properties.rs:427 | chunk 49 |

## Inflate Shrinkwrap

Layout notes: Preset and Seed first, then the mode switch, edge and wrinkle sliders, then a Technical Parameters group.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Inflate Shrinkwrap (generator) | Properties > Generator | used on: mask, or fill for height / normal | missing | - | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked map | missing | - | - |
| Preset | Properties > Generator > Parameters | not documented; options Inflated / Vacuum Pulled / Tight | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | missing | - | - |
| Inflate or Shrinkwrap | Properties > Generator > Parameters | not documented; two modes | missing | - | - |
| Seam Intensity | Properties > Generator > Parameters | not documented | missing | - | - |
| Raised Edge Width | Properties > Generator > Parameters | not documented | missing | - | - |
| Raised Edge Intensity | Properties > Generator > Parameters | not documented | missing | - | - |
| Wrinkle Density | Properties > Generator > Parameters | not documented | missing | - | - |
| Wrinkle Tightness | Properties > Generator > Parameters | not documented | missing | - | - |
| Wrinkle Range | Properties > Generator > Parameters | not documented | missing | - | - |
| Wrinkle Scale | Properties > Generator > Parameters | not documented | missing | - | - |
| Height Range | Properties > Generator > Technical Parameters | not documented | missing | - | - |
| Height Position | Properties > Generator > Technical Parameters | not documented; 0 (black) to 1 (white) | missing | - | - |
| Surface Size (cm) | Properties > Generator > Technical Parameters | not documented | missing | - | - |
| Surface Depth (cm) | Properties > Generator > Technical Parameters | not documented | missing | - | - |

## Light

Layout notes: Fake directional light from world-space normals and position. On a fill layer it writes colour, metalness, roughness, normal and height.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Light (generator) | Properties > Generator | used on: mask or fill | partial: Directional kind with Painter's angles and attenuation; mask only | crates/ui/src/panels/properties.rs:834 | chunk 49 |
| World Space Normal | Properties > Generator > Image inputs | colour; baked map | partial: read automatically, no slot | crates/doc/src/mask.rs:213 | - |
| Position | Properties > Generator > Image inputs | colour; baked map | partial: positions come from the mesh (no baked Position map needed or read) | crates/compositor/src/mask.rs:182 | chunk 49 |
| Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:616 | - |
| Horizontal Angle | Properties > Generator > Parameters | not documented | done (the X / Y / Z vector stays editable) | crates/ui/src/panels/properties.rs:834 | chunk 49 |
| Vertical Angle | Properties > Generator > Parameters | not documented | done (the X / Y / Z vector stays editable) | crates/ui/src/panels/properties.rs:834 | chunk 49 |
| Highlight Glossiness | Properties > Generator > Parameters | not documented | partial: "Softness" of the threshold | crates/ui/src/panels/properties.rs:434 | - |
| Highlight Level | Properties > Generator > Parameters | not documented | partial: "Level" threshold | crates/ui/src/panels/properties.rs:433 | - |
| Light Attenuation | Properties > Generator > Parameters | not documented | done: fades toward the far side of the bounding box | crates/ui/src/panels/properties.rs:834 | chunk 49 |

## Mask Builder

Layout notes: Legacy version of Mask Editor. Global Seed / Level / Contrast / Invert, triplanar, eight contribution sliders, then one group per contribution and Micro Details.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Mask Builder (generator, legacy) | Properties > Generator | used on: mask or fill | missing | - | - |
| World space normal | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Custom grunge 1 | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Custom grunge 2 | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Scatter input | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Position | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked map | missing | - | - |
| Ambient Occlusion | Properties > Generator > Image inputs | grayscale; baked map | missing | - | - |
| Micro Normal | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Height | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | missing | - | - |
| Level | Properties > Generator > Parameters | not documented | missing | - | - |
| Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Invert | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Use Triplanar | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Triplanar Blending Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Grunge | Properties > Generator > Parameters | not documented | missing | - | - |
| AO | Properties > Generator > Parameters | not documented | missing | - | - |
| Curvature | Properties > Generator > Parameters | not documented | missing | - | - |
| Top/Down Gradient | Properties > Generator > Parameters | not documented | missing | - | - |
| World Space Normal | Properties > Generator > Parameters | not documented | missing | - | - |
| Scratches | Properties > Generator > Parameters | not documented; needs Grunge, AO or Curvature above 0 | missing | - | - |
| Scatter | Properties > Generator > Parameters | not documented | missing | - | - |
| Scale | Properties > Generator > Grunge | not documented | missing | - | - |
| Use Custom Grunge | Properties > Generator > Grunge | not documented (toggle; shows Custom grunge 1) | missing | - | - |
| Secondary Custom Grunge | Properties > Generator > Grunge | not documented (visibility of Custom grunge 2) | missing | - | - |
| Invert | Properties > Generator > Grunge | not documented (toggle) | missing | - | - |
| Range | Properties > Generator > Ambient Occlusion | not documented | missing | - | - |
| Contrast | Properties > Generator > Ambient Occlusion | not documented | missing | - | - |
| Noisiness | Properties > Generator > Ambient Occlusion | not documented | missing | - | - |
| Invert | Properties > Generator > Ambient Occlusion | not documented (toggle) | missing | - | - |
| Convex Range | Properties > Generator > Curvature | not documented | missing | - | - |
| Convex Contrast | Properties > Generator > Curvature | not documented | missing | - | - |
| Convex Invert | Properties > Generator > Curvature | not documented (toggle) | missing | - | - |
| Concave Range | Properties > Generator > Curvature | not documented | missing | - | - |
| Concave Contrast | Properties > Generator > Curvature | not documented | missing | - | - |
| Concave Invert | Properties > Generator > Curvature | not documented (toggle) | missing | - | - |
| Smoothness | Properties > Generator > Curvature | not documented | missing | - | - |
| Level Boost | Properties > Generator > Curvature | not documented | missing | - | - |
| Noisiness | Properties > Generator > Curvature | not documented | missing | - | - |
| Range | Properties > Generator > Gradient | not documented | missing | - | - |
| Contrast | Properties > Generator > Gradient | not documented | missing | - | - |
| Invert | Properties > Generator > Gradient | not documented (toggle) | missing | - | - |
| Top Intensity | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Bottom Intensity | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Front Intensity | Properties > Generator > World Space Normal | not documented; Front = +X by default | missing | - | - |
| Back Intensity | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Right Intensity | Properties > Generator > World Space Normal | not documented; Right = +Z by default | missing | - | - |
| Left Intensity | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Amount | Properties > Generator > Scratches | not documented | missing | - | - |
| Scale | Properties > Generator > Scratches | not documented | missing | - | - |
| Scale | Properties > Generator > Scatter | not documented | missing | - | - |
| Density | Properties > Generator > Scatter | not documented | missing | - | - |
| Size | Properties > Generator > Scatter | not documented | missing | - | - |
| Size Variation | Properties > Generator > Scatter | not documented | missing | - | - |
| Opacity Variation | Properties > Generator > Scatter | not documented | missing | - | - |
| Micro Height (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Micro Normal (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Curvature Type | Properties > Generator > Micro Details | not documented; options Standard / Sobel / Smooth | missing | - | - |
| Curvature Intensity | Properties > Generator > Micro Details | not documented (applies to Standard and Sobel) | missing | - | - |
| Height Details Intensity | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Radius | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Depth | Properties > Generator > Micro Details | not documented | missing | - | - |

## Mask Editor

Layout notes: Four global controls, seven opacity sliders (one per contribution), then one collapsible group per contribution in the same order, each with its own Invert and Blending Mode, and Micro Details last. The World Space Normal and Position Gradient groups have three nested per-direction sub-groups.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Mask Editor (generator) | Properties > Generator | used on: mask or fill | partial: the global controls, opacities and every group except per-map Blur, the curvature scale sliders and Micro Details; picked from the Generator dropdown | crates/doc/src/mask.rs:341 | chunk 16 |
| Texture | Properties > Generator > Image inputs | colour; texture or anchor | partial: an image resource; anchors not offered | crates/compositor/src/mask.rs:191 | chunk 16 |
| Texture (Secondary) | Properties > Generator > Image inputs | colour; texture or anchor | partial: an image resource; anchors not offered | crates/compositor/src/mask.rs:191 | chunk 16 |
| World Space Normals | Properties > Generator > Image inputs | colour; baked map | done: reads the baked map, neutral when not baked | crates/compositor/src/mask.rs:191 | chunk 16 |
| Position Gradient | Properties > Generator > Image inputs | colour; baked Position map | done: reads the baked map, neutral when not baked | crates/compositor/src/mask.rs:191 | chunk 16 |
| Thickness | Properties > Generator > Image inputs | grayscale; baked map | done: reads the baked map, neutral when not baked | crates/compositor/src/mask.rs:191 | chunk 16 |
| Curvature | Properties > Generator > Image inputs | grayscale; baked map | done: reads the baked map, neutral when not baked | crates/compositor/src/mask.rs:191 | chunk 16 |
| Ambient Occlusion | Properties > Generator > Image inputs | grayscale; baked map | done: reads the baked map, neutral when not baked | crates/compositor/src/mask.rs:191 | chunk 16 |
| Micro Normal | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Height | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Global Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Global Blur | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Global Balance | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Global Contrast | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Texture Opacity | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Texture 2 Opacity | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Ambient Occlusion Opacity | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Curvature Opacity | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| World Space Normal Opacity | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Position Gradient Opacity | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Thickness Opacity | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Texture | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Grayscale Conversion | Properties > Generator > Texture | not documented; options as in the Grayscale Conversion generator | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > Texture | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Scale | Properties > Generator > Texture | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Contrast | Properties > Generator > Texture | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Brightness | Properties > Generator > Texture | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Triplanar | Properties > Generator > Texture | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Triplanar Contrast | Properties > Generator > Texture | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Non-Square Tiling | Properties > Generator > Texture | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Texture 2 | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Grayscale Conversion | Properties > Generator > Texture 2 | not documented; options as in the Grayscale Conversion generator | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > Texture 2 | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Scale | Properties > Generator > Texture 2 | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Contrast | Properties > Generator > Texture 2 | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Brightness | Properties > Generator > Texture 2 | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Triplanar | Properties > Generator > Texture 2 | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Triplanar Contrast | Properties > Generator > Texture 2 | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Non-Square Tiling | Properties > Generator > Texture 2 | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Ambient Occlusion | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > Ambient Occlusion | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blur | Properties > Generator > Ambient Occlusion | not documented | missing | - | - |
| Balance | Properties > Generator > Ambient Occlusion | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Contrast | Properties > Generator > Ambient Occlusion | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Curvature | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > Curvature | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Mode | Properties > Generator > Curvature | not documented; options Edges / Cavities / Dual / Unprocessed | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Sharp | Properties > Generator > Curvature | not documented | missing | - | - |
| Fine | Properties > Generator > Curvature | not documented | missing | - | - |
| Soft | Properties > Generator > Curvature | not documented | missing | - | - |
| Medium | Properties > Generator > Curvature | not documented | missing | - | - |
| Large | Properties > Generator > Curvature | not documented | missing | - | - |
| Big | Properties > Generator > Curvature | not documented | missing | - | - |
| Huge | Properties > Generator > Curvature | not documented | missing | - | - |
| Contrast | Properties > Generator > Curvature | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Brightness | Properties > Generator > Curvature | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > World Space Normal | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > World Space Normal | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blur | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Balance | Properties > Generator > World Space Normal | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Contrast | Properties > Generator > World Space Normal | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Brightness | Properties > Generator > World Space Normal | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Right to Left | Properties > Generator > World Space Normal | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Top To Bottom | Properties > Generator > World Space Normal | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Front to Back | Properties > Generator > World Space Normal | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > World Space Normal > Right to Left | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > World Space Normal > Right to Left | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > World Space Normal > Top To Bottom | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > World Space Normal > Top To Bottom | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > World Space Normal > Front to Back | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > World Space Normal > Front to Back | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Position Gradient | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Balance | Properties > Generator > Position Gradient | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Contrast | Properties > Generator > Position Gradient | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Brightness | Properties > Generator > Position Gradient | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending mode | Properties > Generator > Position Gradient | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Right to Left | Properties > Generator > Position Gradient | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Top To Bottom | Properties > Generator > Position Gradient | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Front to Back | Properties > Generator > Position Gradient | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Position Gradient > Right to Left | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > Position Gradient > Right to Left | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Position Gradient > Top To Bottom | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > Position Gradient > Top To Bottom | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Position Gradient > Front to Back | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blending Mode | Properties > Generator > Position Gradient > Front to Back | not documented; layer blending-mode list | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Invert | Properties > Generator > Thickness | not documented (toggle) | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Blur | Properties > Generator > Thickness | not documented | missing | - | - |
| Contrast | Properties > Generator > Thickness | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Brightness | Properties > Generator > Thickness | not documented | done | crates/ui/src/panels/properties.rs:440 | chunk 16 |
| Micro Height (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Micro Normal (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Curvature Type | Properties > Generator > Micro Details | not documented; options Standard / Sobel / Smooth | missing | - | - |
| Curvature Intensity | Properties > Generator > Micro Details | not documented (applies to Standard and Sobel) | missing | - | - |
| Height Details Intensity | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Radius | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Depth | Properties > Generator > Micro Details | not documented | missing | - | - |

## Metal Edge Wear

Layout notes: Seed and Invert, wear level / contrast, triplanar, grunge, then Edges Smoothness, Ambient Occlusion Masking, Curvature Weight, then Micro Details.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Metal Edge Wear (generator) | Properties > Generator | used on: mask or fill | partial: Edges kind ("Edge wear"); no custom grunge, triplanar contrast or micro-detail inputs; adds Spread / Variation / Grain | crates/doc/src/mask.rs:209 | - |
| World space normal | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Position | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Custom grunge | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Curvature | Properties > Generator > Image inputs | grayscale; baked map | partial: baked map automatic, anchor can replace it, no resource slot | crates/ui/src/panels/properties.rs:553 | - |
| Ambient Occlusion | Properties > Generator > Image inputs | grayscale; baked map | partial: read automatically for AO masking, no slot | crates/ui/src/panels/properties.rs:456 | - |
| Micro Normal | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Micro Height | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | partial: number field and "Next" (seed + 1), no random | crates/ui/src/panels/properties.rs:600 | - |
| Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:616 | - |
| Wear Level | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:419 | - |
| Wear Contrast | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:428 | - |
| Use Triplanar | Properties > Generator > Parameters | not documented (toggle) | partial: no toggle; natural edge wear is always object-space | crates/ui/src/panels/properties.rs:570 | - |
| Triplanar Blending Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Grunge Amount | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:437 | - |
| Grunge Scale | Properties > Generator > Parameters | not documented | partial: "Pattern size" in percent of the model, 1-100 | crates/ui/src/panels/properties.rs:596 | - |
| Use Custom Grunge | Properties > Generator > Parameters | not documented (toggle) | partial: "Grunge map" switches to one bundled map, no custom image | crates/ui/src/panels/properties.rs:466 | - |
| Edges Smoothness | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:460 | - |
| Ambient Occlusion Masking | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:456 | - |
| Curvature Weight | Properties > Generator > Parameters | not documented | done | crates/ui/src/panels/properties.rs:458 | - |
| Micro Height (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Micro Normal (toggle) | Properties > Generator > Micro Details | not documented | missing | - | - |
| Curvature Type | Properties > Generator > Micro Details | not documented; options Standard / Sobel / Smooth | missing | - | - |
| Curvature Intensity | Properties > Generator > Micro Details | not documented (applies to Standard and Sobel) | missing | - | - |
| Height Details Intensity | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Radius | Properties > Generator > Micro Details | not documented | missing | - | - |
| AO Depth | Properties > Generator > Micro Details | not documented | missing | - | - |

## Position

Layout notes: Global controls and Use Texture, then the Position Gradient group with three direction sliders and a nested Invert / Blending Mode pair per direction, then Texture.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Position (generator) | Properties > Generator | used on: mask or fill | partial: Position Gradient kind, one world axis, Level / Softness / Breakup | crates/doc/src/mask.rs:215 | - |
| Texture | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| Position Gradient | Properties > Generator > Image inputs | colour; baked Position map | partial: mesh bounds used directly, no slot | crates/doc/src/mask.rs:215 | - |
| World Space Normals | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Global Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:616 | - |
| Global Blur | Properties > Generator > Parameters | not documented | missing | - | - |
| Global Balance | Properties > Generator > Parameters | not documented | partial: "Level" threshold | crates/ui/src/panels/properties.rs:433 | - |
| Global Contrast | Properties > Generator > Parameters | not documented | partial: "Softness" (inverse of contrast) | crates/ui/src/panels/properties.rs:434 | - |
| Use Texture | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Invert | Properties > Generator > Position Gradient | not documented (toggle) | missing | - | - |
| Balance | Properties > Generator > Position Gradient | not documented | missing | - | - |
| Contrast | Properties > Generator > Position Gradient | not documented | missing | - | - |
| Brightness | Properties > Generator > Position Gradient | not documented | missing | - | - |
| Right to Left | Properties > Generator > Position Gradient | not documented | partial: one world axis at a time (Axis X / Y / Z selector), no per-axis amount | crates/ui/src/panels/properties.rs:563 | - |
| Top To Bottom | Properties > Generator > Position Gradient | not documented | partial: one world axis at a time (Axis X / Y / Z selector), no per-axis amount | crates/ui/src/panels/properties.rs:563 | - |
| Front to Back | Properties > Generator > Position Gradient | not documented | partial: one world axis at a time (Axis X / Y / Z selector), no per-axis amount | crates/ui/src/panels/properties.rs:563 | - |
| Invert | Properties > Generator > Position Gradient > Right to Left | not documented (toggle) | missing | - | - |
| Blending Mode | Properties > Generator > Position Gradient > Right to Left | not documented; layer blending-mode list | missing | - | - |
| Invert | Properties > Generator > Position Gradient > Top To Bottom | not documented (toggle) | missing | - | - |
| Blending Mode | Properties > Generator > Position Gradient > Top To Bottom | not documented; layer blending-mode list | missing | - | - |
| Invert | Properties > Generator > Position Gradient > Front to Back | not documented (toggle) | missing | - | - |
| Blending Mode | Properties > Generator > Position Gradient > Front to Back | not documented; layer blending-mode list | missing | - | - |
| Texture Opacity | Properties > Generator > Texture | not documented | missing | - | - |
| Invert | Properties > Generator > Texture | not documented | missing | - | - |
| Grayscale Conversion | Properties > Generator > Texture | not documented; options as in the Grayscale Conversion generator | missing | - | - |
| Blending Mode | Properties > Generator > Texture | not documented; layer blending-mode list | missing | - | - |
| Scale | Properties > Generator > Texture | not documented | missing | - | - |
| Contrast | Properties > Generator > Texture | not documented | missing | - | - |
| Brightness | Properties > Generator > Texture | not documented | missing | - | - |
| Triplanar | Properties > Generator > Texture | not documented (toggle) | missing | - | - |
| Triplanar Contrast | Properties > Generator > Texture | not documented | missing | - | - |

## Tri-Planar Advanced

Layout notes: Projects a custom mask (texture or anchor) on three axes with per-axis rotation and offsets. Main controls, then Axis X, Axis Y, Axis Z groups.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Tri-Planar Advanced (generator) | Properties > Generator | used on: mask or fill | missing | - | - |
| World Space Normal | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| Position | Properties > Generator > Image inputs | colour; baked map | missing | - | - |
| mask | Properties > Generator > Image inputs | grayscale; texture or anchor | missing | - | - |
| Projection | Properties > Generator > Parameters | not documented; all axes or a single axis | missing | - | - |
| Blending Mode | Properties > Generator > Parameters | not documented; options Linear / Advanced | missing | - | - |
| Blending Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Texture Tiling | Properties > Generator > Parameters | not documented | missing | - | - |
| Rotation X | Properties > Generator > Axis X | not documented | missing | - | - |
| Offset X X | Properties > Generator > Axis X | not documented | missing | - | - |
| Offset X Y | Properties > Generator > Axis X | not documented | missing | - | - |
| Rotation X (Y-axis projection, label as documented) | Properties > Generator > Axis Y | not documented | missing | - | - |
| Offset Y X | Properties > Generator > Axis Y | not documented | missing | - | - |
| Offset Y Y | Properties > Generator > Axis Y | not documented | missing | - | - |
| Rotation X (Z-axis projection, label as documented) | Properties > Generator > Axis Z | not documented | missing | - | - |
| Offset Z X | Properties > Generator > Axis Z | not documented | missing | - | - |
| Offset Z Y | Properties > Generator > Axis Z | not documented | missing | - | - |

## UV Border Distance

Layout notes: Grayscale falloff from UV island borders (uses the engine's mesh-based inputs, no baked map).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV Border Distance (generator) | Properties > Generator | used on: mask or fill | missing | - | - |
| Invert | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Balance | Properties > Generator > Parameters | not documented | missing | - | - |
| Contrast | Properties > Generator > Parameters | not documented | missing | - | - |
| Smoothness | Properties > Generator > Parameters | not documented | missing | - | - |
| Distance | Properties > Generator > Parameters | not documented | missing | - | - |

## UV Checker

Layout notes: Colour checker for inspecting UVs, used on a fill layer.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV Checker (generator) | Properties > Generator | used on: fill layer | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | missing | - | - |
| Checker Color Mode | Properties > Generator > Parameters | not documented; options Uniform / Random (per UV island) | missing | - | - |
| Checker Color | Properties > Generator > Parameters | not documented | missing | - | - |
| Checker Grid Tile | Properties > Generator > Parameters | not documented | missing | - | - |
| Checker Grid Opacity | Properties > Generator > Parameters | not documented; 0 hides the grid | missing | - | - |
| UV Borders Color | Properties > Generator > Parameters | not documented | missing | - | - |
| UV Borders Width | Properties > Generator > Parameters | not documented | missing | - | - |

## UV Random Color

Layout notes: One flat colour per UV island; usable as a mask or as fill colour.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV Random Color (generator) | Properties > Generator | used on: mask or fill | partial: as a mask, one random gray per UV island | crates/doc/src/mask.rs:91 | chunk 49 |
| Custom Gradient | Properties > Generator > Image inputs | gradient map for the colour range | missing | - | - |
| Seed | Properties > Generator > Parameters | not documented; Random button and pencil field | done: Random button and number field | crates/ui/src/panels/properties.rs:427 | chunk 49 |
| Color Source Mode | Properties > Generator > Parameters | not documented; options Random / Custom Gradient | partial: Random only (Custom Gradient needs image inputs) | crates/ui/src/panels/properties.rs:427 | chunk 49 |

## UV Texel Density

Layout notes: Colour gradient from low to high texel density; used on a fill layer.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| UV Texel Density (generator) | Properties > Generator | used on: fill layer | missing | - | - |
| Color Low | Properties > Generator > Parameters | not documented | missing | - | - |
| Color Medium | Properties > Generator > Parameters | not documented | missing | - | - |
| Color High | Properties > Generator > Parameters | not documented | missing | - | - |

## World Space Normals

Layout notes: Same frame as Position: global controls and Use Texture, a World Space Normal group with three direction sliders and nested per-direction Invert / Blending Mode, then Texture.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| World Space Normals (generator) | Properties > Generator | used on: mask or fill | partial: Directional kind (one direction vector, Level / Softness / Breakup) | crates/doc/src/mask.rs:213 | - |
| Texture | Properties > Generator > Image inputs | colour; texture or anchor | missing | - | - |
| World Space Normals | Properties > Generator > Image inputs | colour; baked map | partial: read automatically, no slot | crates/doc/src/mask.rs:213 | - |
| Position Gradient | Properties > Generator > Image inputs | colour; baked Position map | missing | - | - |
| Global Invert | Properties > Generator > Parameters | not documented (toggle) | done | crates/ui/src/panels/properties.rs:616 | - |
| Global Blur | Properties > Generator > Parameters | not documented | missing | - | - |
| Global Balance | Properties > Generator > Parameters | not documented | partial: "Level" threshold | crates/ui/src/panels/properties.rs:433 | - |
| Global Contrast | Properties > Generator > Parameters | not documented | partial: "Softness" (inverse of contrast) | crates/ui/src/panels/properties.rs:434 | - |
| Use Texture | Properties > Generator > Parameters | not documented (toggle) | missing | - | - |
| Invert | Properties > Generator > World Space Normal | not documented (toggle) | missing | - | - |
| Blur | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Balance | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Contrast | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Brightness | Properties > Generator > World Space Normal | not documented | missing | - | - |
| Right to Left | Properties > Generator > World Space Normal | not documented | partial: Direction X of one vector; no per-axis blend | crates/ui/src/panels/properties.rs:617 | - |
| Top To Bottom | Properties > Generator > World Space Normal | not documented | partial: Direction Y of one vector; no per-axis blend | crates/ui/src/panels/properties.rs:617 | - |
| Front to Back | Properties > Generator > World Space Normal | not documented | partial: Direction Z of one vector; no per-axis blend | crates/ui/src/panels/properties.rs:617 | - |
| Invert | Properties > Generator > World Space Normal > Right to Left | not documented (toggle) | missing | - | - |
| Blending Mode | Properties > Generator > World Space Normal > Right to Left | not documented; layer blending-mode list | missing | - | - |
| Invert | Properties > Generator > World Space Normal > Top To Bottom | not documented (toggle) | missing | - | - |
| Blending Mode | Properties > Generator > World Space Normal > Top To Bottom | not documented; layer blending-mode list | missing | - | - |
| Invert | Properties > Generator > World Space Normal > Front to Back | not documented (toggle) | missing | - | - |
| Blending Mode | Properties > Generator > World Space Normal > Front to Back | not documented; layer blending-mode list | missing | - | - |
| Texture Opacity | Properties > Generator > Texture | not documented | missing | - | - |
| Invert | Properties > Generator > Texture | not documented | missing | - | - |
| Grayscale Conversion | Properties > Generator > Texture | not documented; options as in the Grayscale Conversion generator | missing | - | - |
| Blending Mode | Properties > Generator > Texture | not documented; layer blending-mode list | missing | - | - |
| Scale | Properties > Generator > Texture | not documented | missing | - | - |
| Contrast | Properties > Generator > Texture | not documented | missing | - | - |
| Brightness | Properties > Generator > Texture | not documented | missing | - | - |
| Triplanar | Properties > Generator > Texture | not documented (toggle) | missing | - | - |
| Triplanar Contrast | Properties > Generator > Texture | not documented | missing | - | - |

## Generators named in release notes without a doc page

Layout notes: These are "texture generators" (procedural resources used in fill slots), listed here so they are not lost. Parameters are not documented.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Scratches Generator (11.0) | Assets > Procedurals | (unverified) parameters | partial: Scratch procedural pattern with direction, length, width, density, curvature, clustering | crates/doc/src/fill.rs:112 | - |
| Tile Random (11.0) | Assets > Procedurals | (unverified) parameters | missing | - | - |
| Triangle Grid (11.0) | Assets > Procedurals | (unverified) parameters | missing | - | - |
| Tile Generator (updated in 10.0) | Assets > Procedurals | (unverified) parameters | missing | - | - |

## Custom generators: what a Substance graph can expose

Layout notes: A custom generator is a filter graph with no channel input: only mesh-map inputs, engine mesh inputs and free image inputs, plus exposed parameters. SubCraft cannot load Substance graphs, so every row here is missing; they define what a future custom-generator format has to cover.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Generator graph: no channel input, only additional maps | Substance graph (sbsar) | - | missing | - | - |
| Exposed graph parameters shown as Properties controls | Substance graph (sbsar) | - | missing | - | - |
| Free image input (texture, procedural or anchor) | Substance graph (sbsar) | - | missing | - | - |
| Ambient occlusion (usage ambientOcclusionBase, id ambient_occlusion) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| ID (usage id) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Curvature (usage curvature) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Normal (usage normalBase, id normal_base) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| World space normals (usage normalWS, id world_space_normals) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Position (usage position) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Thickness (usage thickness) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Height (usage heightBase, id height_base) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Bent Normals (usage bentNormalsBase, id bent_normals_base) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Opacity (usage opacityBase, id opacity_base) | Substance graph > mesh map input (auto-connected) | - | missing | - | - |
| Position RGB (mesh_position / meshPosition) | Substance graph > mesh based input (from topology, 32-bit float) | - | missing | - | - |
| World Space Normal RGB (mesh_world_space_normal / meshNormalWS) | Substance graph > mesh based input (from topology, 32-bit float) | - | missing | - | - |
| World Space Tangent RGB (mesh_world_space_tangent / meshTangentWS) | Substance graph > mesh based input (from topology, 32-bit float) | - | missing | - | - |
| World Space Bitangent RGB (mesh_world_space_bitangent / meshBitangentWS) | Substance graph > mesh based input (from topology, 32-bit float) | - | missing | - | - |
| Texel Size grayscale (mesh_texel_size / meshTexelSize) | Substance graph > mesh based input (from topology, 32-bit float) | - | missing | - | - |
| UV Mask grayscale (mesh_uv_mask / meshUVMask) | Substance graph > mesh based input (from topology, 32-bit float) | - | missing | - | - |
| UV neighbor (mesh_uv_neighbor, 12.2) | Substance graph > mesh based input (from topology, 32-bit float) | - | missing | - | - |
| colorspace: auto | Substance graph > user data (key = value; separated by semicolons) | default | missing | - | - |
| colorspace: linear | Substance graph > user data (key = value; separated by semicolons) | Legacy colour management only | missing | - | - |
| colorspace: srgb | Substance graph > user data (key = value; separated by semicolons) | Legacy colour management only | missing | - | - |
| colorspace: passthru | Substance graph > user data (key = value; separated by semicolons) | deprecated | missing | - | - |
| colorspace: snorm | Substance graph > user data (key = value; separated by semicolons) | inputs / outputs only | missing | - | - |
| colorspace: normalxyzright | Substance graph > user data (key = value; separated by semicolons) | OpenGL normal | missing | - | - |
| colorspace: normalxyzleft | Substance graph > user data (key = value; separated by semicolons) | DirectX normal | missing | - | - |
| colorspace: unorm | Substance graph > user data (key = value; separated by semicolons) | 8.1 and newer | missing | - | - |
| colorspace: data | Substance graph > user data (key = value; separated by semicolons) | 8.1 and newer | missing | - | - |
| colorspace: raw | Substance graph > user data (key = value; separated by semicolons) | 8.1 and newer | missing | - | - |
| colorspace: $standardsrgb | Substance graph > user data (key = value; separated by semicolons) | 8.1 and newer | missing | - | - |
| colorspace: $working | Substance graph > user data (key = value; separated by semicolons) | 8.1 and newer | missing | - | - |
| colorspace: $raw | Substance graph > user data (key = value; separated by semicolons) | alias of raw | missing | - | - |
| colorspace: $auto | Substance graph > user data (key = value; separated by semicolons) | alias of auto | missing | - | - |
| alpha: straight | Substance graph > user data (key = value; separated by semicolons) | - | missing | - | - |
| alpha: premultiplied | Substance graph > user data (key = value; separated by semicolons) | - | missing | - | - |
| alpha: none | Substance graph > user data (key = value; separated by semicolons) | - | missing | - | - |
| defaultcolor=(r,g,b) for an empty image input | Substance graph > user data (key = value; separated by semicolons) | default black, alpha 0; floats 0-1 or integers 0-255 | missing | - | - |
| padding=extend (infinite dilation of an image input) | Substance graph > user data (key = value; separated by semicolons) | default no padding | missing | - | - |
| disable=(true) / disable=(channel,...) to start with outputs off | Substance graph > user data (key = value; separated by semicolons) | - | missing | - | - |
| channels_Alpha output / IsChannelsAlpha=true (material mode only) | Substance graph > user data (key = value; separated by semicolons) | - | missing | - | - |
| blendingmode=<mode> default blend per material output | Substance graph > user data (key = value; separated by semicolons) | 31 mode names (normal ... normalinversedetail) | missing | - | - |
