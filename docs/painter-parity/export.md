# Painter parity: Export

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-window/export-window , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-window/export-settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-window/output-templates , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-window/list-of-exports , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/creating-export-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-presets/export-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-presets/default-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-presets/predefined-presets/predefined-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/export/export-presets/predefined-presets/usd-pbr-metal-roughness-preset , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/universal-scene-description-usd , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/flatten-layers , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/file-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/dynamic-material-layering , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/configuration/command-lines , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes
Checked against: origin/main 297dcb3 (2026-11-11)

## Opening the Export window

Layout notes: Painter has one modal Export textures window with three tabs along the top (Settings, Output templates, List of exports) and a button row along the bottom. SubCraft has one single-page floating window with everything stacked vertically and no tabs.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Export textures (menu entry) | File > Export textures | - | done | crates/ui/src/app.rs:1136 | - |
| Export textures shortcut | Keyboard | Ctrl+Shift+E (Shift+Cmd+E on macOS) | done | crates/app/src/actions.rs:220 | - |
| Settings tab | Export window, tab 1 | default tab | done (default tab) | crates/ui/src/dialogs.rs settings_tab | chunk 14 |
| Output templates tab | Export window, tab 2 | - | done (preset list, New / Duplicate / Delete / Import / Export, and the template editor) | crates/ui/src/dialogs.rs export_window | #124, chunk 14 |
| List of exports tab | Export window, tab 3 | - | done | crates/ui/src/dialogs.rs export_log | chunk 14 |
| --export-path command line argument | Launch argument (default output directory) | not documented | done (the Export window's default folder; a project's saved export folder still wins) | apps/subcraft/src/main.rs, crates/ui/src/app.rs LaunchOptions::export_folder | export-path |

## Window bottom buttons

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Reset all | Bottom row, Settings tab only: resets the project export configuration | - | done (file type, size, padding, texture set overrides, normal format, UDIM folders and map checkboxes back to a new project's; folder and template kept) | crates/ui/src/dialogs.rs | chunk 14 |
| Cancel | Bottom row: close without keeping changes | - | done (restores the window's and the project's export settings from when the window opened, then closes) | crates/ui/src/dialogs.rs | chunk 14 |
| Save settings | Bottom row: store current export settings in the project | - | done (folder, file type, bit depth, size, padding, height to normal and texture set overrides stored in the project; template, normal format, UDIM folders and map checklist still stored as they change) | crates/doc/src/settings.rs ExportConfig | chunk 14 |
| Export | Bottom row: run export, then jump to List of exports | - | done (runs the export, then shows List of exports) | crates/ui/src/dialogs.rs run_export | chunk 14 |

## Settings tab: texture set list (left column)

Layout notes: a resizable left column. First entry is Global settings; below it one entry per texture set with a checkbox; UV-tile projects show the tiles under each set. Selecting an entry shows its parameters on the right; per-set values override the global ones.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Global settings entry | Top of the list: parameters shared by all texture sets | selected by default | done (top of the list, selected by default) | crates/ui/src/dialogs.rs settings_tab | chunk 14 |
| Texture set entry (per-set override of every parameter) | List row per texture set | inherits global | partial: file type, bit depth, size and padding per texture set (marked * in the list); folder and template stay global | crates/doc/src/settings.rs ExportOverrides, crates/export/src/lib.rs ExportSettings::for_set | chunk 14 |
| Texture set export checkbox | Checkbox next to each set name | checked | done (checkbox beside each set in the list, and in the map checklist) | crates/ui/src/dialogs.rs settings_tab | chunk 14 |
| Selection dropdown: Check all | Dropdown above the list | - | done (Select all button) | crates/ui/src/dialogs.rs:396 | - |
| Selection dropdown: Uncheck all | Dropdown above the list | - | done (None button) | crates/ui/src/dialogs.rs:395 | - |
| Selection dropdown: Invert all | Dropdown above the list | - | done (Invert button) | crates/ui/src/dialogs.rs:394 | - |
| UV tile checkboxes per texture set | Tiles listed under a set in UV-tile projects | checked (unverified) | missing (tiles appear as separate sets in the checklist, no tile picker) | - | - |
| UV tile resolution-differs marker | Icon on tiles whose size differs from the project default | - | missing | - | - |
| Resizable list column | Drag the divider | - | missing | - | - |

## Settings tab: general export parameters

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Output directory | Path field + browse button | last used / project folder (unverified) | done (Folder field + Browse) | crates/ui/src/dialogs.rs:177 | - |
| Output template | Dropdown: predefined templates first, then default and custom ones | PBR Metallic Roughness (unverified) | partial: built-in list only, no custom templates | crates/ui/src/dialogs.rs:193 | #124 |
| File type | Dropdown: container format | Based on output template (unverified) | partial: 12 of Painter's types (bmp, exr, hdr, jpeg, pbm, pfm, pgm, png, ppm, targa, tiff, webp), alphabetical, plus Based on output template | crates/ui/src/export_presets.rs format_combo, crates/image/src/io.rs Container | chunk 21, netpbm |
| File type: Based on output template | First entry of File type | - | done (Template default) | crates/ui/src/dialogs.rs:203 | - |
| Bit depth | Second dropdown beside File type; choices depend on format | 8 bits (unverified) | done (second dropdown; only the depths the file type stores, plus 8-bit + dithering) | crates/ui/src/export_presets.rs format_combo | chunk 21 |
| Size | Dropdown | Based on each Texture Set's size | done (Based on each texture set's size, or 128 to 8192; a preset or map size wins) | crates/export/src/lib.rs output_size | chunk 21 |
| Size: 128 | Size dropdown | - | done | crates/export/src/lib.rs EXPORT_SIZES | chunk 21 |
| Size: 256 | Size dropdown | - | done | crates/export/src/lib.rs EXPORT_SIZES | chunk 21 |
| Size: 512 | Size dropdown | - | done | crates/export/src/lib.rs EXPORT_SIZES | chunk 21 |
| Size: 1024 | Size dropdown | - | done | crates/export/src/lib.rs EXPORT_SIZES | chunk 21 |
| Size: 2048 | Size dropdown | - | done | crates/export/src/lib.rs EXPORT_SIZES | chunk 21 |
| Size: 4096 | Size dropdown | - | done | crates/export/src/lib.rs EXPORT_SIZES | chunk 21 |
| Size: 8192 | Size dropdown (needs enough VRAM) | - | done | crates/export/src/lib.rs EXPORT_SIZES | chunk 21 |
| Padding | Dropdown | Dilation infinite (unverified) | partial: 4 of Painter's 5 modes (Dilation + transparent waits for alpha export), plus SubCraft's Dilation only | crates/ui/src/export_presets.rs padding_combo | chunk 21 |
| Padding: No padding (passthrough) | Padding dropdown | - | done | crates/export/src/lib.rs pad | chunk 21 |
| Padding: Dilation infinite | Padding dropdown | - | done (default) | crates/export/src/lib.rs pad | chunk 21 |
| Padding: Dilation + transparent | Padding dropdown | - | missing | - | - |
| Padding: Dilation + default background color | Padding dropdown | - | done (fill = what an empty texture set exports for that map: channel default, flat normal...) | crates/export/src/lib.rs background_value | chunk 21 |
| Padding: Dilation + diffusion | Padding dropdown | - | done (push-pull blend of the surrounding islands) | crates/image/src/dilate.rs diffuse_fill | chunk 21 |
| Dilation distance (pixels) | Number field beside Padding, for the three Dilation + modes | 16 (unverified); range not documented | done: default 16, range 1-256, kept when switching between dilation modes | crates/ui/src/export_presets.rs padding_combo | chunk 21 |
| Export shaders parameters | Checkbox: also writes a JSON of shader parameters | off (unverified) | missing | - | - |
| Export USD asset | Checkbox at the bottom of the tab | off | partial: separate "Export model + textures" button with USDA choice | crates/ui/src/dialogs.rs:277 | - |
| Export USD asset: textures folder | Output of the checkbox | - | partial: textures written beside the scene file | crates/export/src/scene.rs:398 | - |
| Export USD asset: .usda material layer pointing to the textures | Output of the checkbox | - | partial: one USDA holding mesh and materials | crates/export/src/scene.rs:324 | - |
| Export USD asset: .usd assembling materials with the original mesh | Optional output | - | missing | - | - |
| Export USD asset: .usd mesh payload (when source was not USD or was auto-unwrapped) | Optional output | - | missing | - | - |

## Settings tab: file types and bit depths

Layout notes: Painter lists the formats alphabetically by extension in the File type dropdown; the bit depth dropdown next to it only offers the depths valid for the chosen format.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| bmp (Bitmap) | File type | 8, 8 + dithering | done (8, 8 + dithering) | crates/image/src/io.rs | chunk 21 |
| exr (OpenEXR) | File type | 16f, 32f | done (16f through the exr crate, 32f) | crates/image/src/io.rs | chunk 21 |
| gif | File type | 8, 8 + dithering | missing | - | - |
| hdr (Radiance HDR) | File type | 32f | done (RGB only: alpha dropped) | crates/image/src/io.rs | chunk 21 |
| ico | File type | 8, 8 + dithering | missing | - | - |
| j2k (JPEG 2000) | File type | 8, 8 + dithering, 16 | missing | - | - |
| jng | File type | 8, 8 + dithering, 16 | missing | - | - |
| jp2 (JPEG 2000) | File type | 8, 8 + dithering, 16 | missing | - | - |
| jpeg | File type | 8, 8 + dithering | done (8, 8 + dithering) | crates/image/src/io.rs | chunk 21 |
| jpeg-xr | File type | 8, 8 + dithering, 16, 32f | missing | - | - |
| pbm | File type | 8, 8 + dithering, 16 | done (1-bit, black below one half; a PBM file can't store Painter's 8 / 16-bit depths) | crates/image/src/netpbm.rs | netpbm |
| pfm | File type | 32f | done (gray or RGB float, alpha dropped) | crates/image/src/netpbm.rs | netpbm |
| pgm | File type | 8, 8 + dithering, 16 | done (8, 8 + dithering, 16; colour written as Rec. 709 luma) | crates/image/src/netpbm.rs | netpbm |
| png | File type | 8, 8 + dithering, 16 | done (8, 8 + dithering, 16) | crates/image/src/io.rs | chunk 21 |
| ppm | File type | 8, 8 + dithering, 16 | done (8, 8 + dithering, 16; alpha dropped) | crates/image/src/netpbm.rs | netpbm |
| psd (container: all maps of a set in one file, folders become groups) | File type | 8, 8 + dithering, 16 | missing (layered PSD exists only for the per-channel PhotoCraft exchange, crates/export/src/psd.rs:212) | - | - |
| targa | File type | 8, 8 + dithering | done (8, 8 + dithering) | crates/image/src/io.rs | chunk 21 |
| tiff | File type | 8, 8 + dithering, 16, 32f | done (8, 8 + dithering, 16, 32f) | crates/image/src/io.rs | #124, chunk 21 |
| wbmp | File type | 8, 8 + dithering | missing | - | - |
| webp | File type | 8, 8 + dithering | done (8, 8 + dithering; lossless) | crates/image/src/io.rs | chunk 21 |
| xpm | File type | 8, 8 + dithering | missing | - | - |
| sbsar (bundle the outputs into one Substance archive) | File type (added in 8.2) | - | missing | - | - |
| Bit depth: 8 bits | Bit depth dropdown | - | done | crates/image/src/io.rs:117 | - |
| Bit depth: 8 bits + dithering | Bit depth dropdown | - | done (deterministic triangular dither; 0 and 1 stay exact) | crates/image/src/io.rs quantize8 | chunk 21 |
| Bit depth: 16 bits | Bit depth dropdown | - | done (PNG, TIFF) | crates/image/src/io.rs | #124 |
| Bit depth: 16 bits floating (EXR half) | Bit depth dropdown | - | done | crates/image/src/io.rs | chunk 21 |
| Bit depth: 32 bits floating | Bit depth dropdown | - | done (EXR, TIFF, HDR) | crates/image/src/io.rs | #124, chunk 21 |

## Settings tab: output maps of a texture set

Layout notes: shown on the right only when a texture set (not Global settings) is selected. One row per file the template will write: checkbox, resolved file name, format dropdown, bit depth dropdown, and the colour space when colour management is on.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Output maps list | Right side, per texture set | - | done (all sets under Global settings, one set's maps when a set is selected) | crates/ui/src/dialogs.rs | chunk 14 |
| Output map checkbox (skip one file) | Row checkbox | checked | done | crates/ui/src/dialogs.rs:414 | - |
| Resolved file name per map | Row text | - | done (path preview) | crates/ui/src/dialogs.rs:418 | - |
| File format override per map | Row dropdown | from template | missing | - | - |
| Bit depth override per map | Row dropdown | from template | missing | - | - |
| Colour space shown per map | Row text (colour management on) | - | missing | - | - |

## Output templates tab: presets list (left column)

Layout notes: three columns. Left: list of templates with three icon buttons above it. Middle: output maps of the selected template. Right: source lists (Input maps, Converted maps, Mesh maps) to drag from. Template files are stored one per file in the library's export-presets folder.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Presets list (default + custom templates) | Left column | - | missing | - | #124 |
| Duplicate button | Above the list | - | missing | - | #124 |
| Remove button (no undo) | Above the list | - | missing | - | #124 |
| Add button (new empty template) | Above the list | - | missing | - | #124 |
| Double-click a template to rename | Presets list | - | missing | - | #124 |
| Right-click > Delete | Presets list context menu | - | missing | - | #124 |
| Right-click > Rename | Presets list context menu | - | missing | - | #124 |
| Right-click > Duplicate | Presets list context menu | - | missing | - | #124 |
| Import a preset file (.spexp) through Import resources or the export-presets folder | File > Import resources / library folder | - | missing | - | #124 |
| Template stored as one shareable file in the library | assets/export-presets folder | - | missing | - | #124 |
| Last used template embedded in the project file | Automatic on save after an export | - | partial: only the template name is stored in project settings | crates/ui/src/dialogs.rs:355 | #124 |
| "Outdated" marker when the embedded template differs from the library one | Presets list | - | missing | - | - |

## Output templates tab: output maps list (middle column)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Output maps list of the selected template | Middle column | - | missing | - | #124 |
| Create: Gray | Button row above the list: one grayscale slot | - | missing (model supports 1-component maps, crates/export/src/template.rs:44) | - | #124 |
| Create: RGB | Button row: one RGB slot | - | missing | - | #124 |
| Create: R+G+B | Button row: three grayscale slots | - | missing | - | #124 |
| Create: RGB+A | Button row: RGB slot plus alpha slot | - | missing | - | #124 |
| Create: R+G+B+A | Button row: four grayscale slots | - | missing | - | #124 |
| Merge / split slots of a map (collapse when empty or same source) | Slot header of an output map | - | missing | - | - |
| Remove an output map | Cross button on the map row | - | missing | - | #124 |
| Map name field (file name pattern) | Text field on the map row | - | missing | - | #124 |
| $ button (insert a keyword) | Beside the name field | - | missing | - | - |
| Keyword $project | Name pattern: project file name | - | missing | - | - |
| Keyword $mesh | Name pattern: mesh file name | - | missing (fixed naming mesh_set_suffix, crates/export/src/lib.rs:142) | - | #124 |
| Keyword $textureSet | Name pattern: texture set / material name | - | missing | - | #124 |
| Keyword $sceneMaterial | Name pattern: material name as in the mesh file | (unverified) | missing | - | - |
| Keyword $udim | Name pattern: UV tile number | - | missing (suffix .1001 or per-tile folder is fixed, crates/export/src/lib.rs:146) | - | #124 |
| Keyword $colorSpace | Name pattern: colour space of the map | - | missing | - | - |
| "/" in the name creates sub-folders (groups in PSD) | Name pattern | - | missing | - | - |
| Optional part in parentheses, e.g. (.$udim) | Name pattern | (unverified) | missing | - | - |
| Map file format dropdown | First dropdown on the map row | - | missing (per-map format exists in the model, crates/export/src/template.rs:43) | - | #124 |
| Map bit depth dropdown | Second dropdown on the map row | - | missing | - | #124 |
| Channel slot (drop target) | One or more coloured slots per map | empty | missing | - | #124 |
| Drag a source map from the right column onto a slot | Drag and drop | - | missing | - | #124 (picked from a dropdown, no drag) |
| Drop menu: RGB channels | Popup after dropping on a slot | - | missing | - | #124 |
| Drop menu: R channel | Popup after dropping | - | missing | - | #124 |
| Drop menu: G channel | Popup after dropping | - | missing | - | #124 |
| Drop menu: B channel | Popup after dropping | - | missing | - | #124 |
| Drop menu: A channel (painted coverage) | Popup after dropping | - | missing | - | - |
| Drop menu: Gray channel (grayscale conversion) | Popup after dropping | - | missing | - | #124 |
| Slot colour cue (random colour per source, bar showing RGB / R / G / B / gray / alpha) | Slot button | - | missing | - | - |
| Empty slot writes a default colour | Export behaviour | - | partial: Constant source, not editable in UI | crates/export/src/template.rs:17 | #124 |
| Channel missing from a texture set writes its neutral default | Export behaviour | - | done | crates/export/src/lib.rs:219 | - |

## Output templates tab: source map list (right column)

Layout notes: three collapsible groups in this order: Input maps, Converted maps, Mesh maps (the doc page lists Mesh maps before Converted maps; the window shows Converted maps second). Recent versions group input maps by category for OpenPBR.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Input maps group (every texture set channel) | Right column | - | missing as a list (built-in templates read channels, crates/export/src/template.rs:12) | - | #124 |
| Input maps: user channels listed as user_x | Right column | - | missing | - | #124 |
| Input maps grouped by category (OpenPBR naming) | Right column | - | missing | - | - |
| Converted map: Normal OpenGL | Right column | - | partial: used by built-in templates, not selectable | crates/export/src/template.rs:14 | #124 |
| Converted map: Normal DirectX | Right column | - | partial: used by built-in templates and a global Normal maps override | crates/ui/src/dialogs.rs:363 | #124 |
| Converted map: Mixed AO | Right column | - | done (AO channel x baked AO; either alone when the other is absent) | crates/export/src/lib.rs Dense::converted | chunk 47 |
| Converted map: Diffuse | Right column | - | done (base color x (1 - metallic)) | crates/export/src/template.rs Converted | chunk 47 |
| Converted map: Specular | Right column | - | done (f0 from Specular level for dielectrics, base color for metals, blended by metallic) | crates/export/src/template.rs Converted | chunk 47 |
| Converted map: Glossiness | Right column | - | done (1 - roughness) | crates/export/src/template.rs Converted | chunk 47 |
| Converted map: Unity4 Diffuse (deprecated) | Right column | - | missing | - | - |
| Converted map: Unity4 Gloss (deprecated) | Right column | - | missing | - | - |
| Converted map: Reflection | Right column | - | done (white for dielectrics, base color for metals, blended by metallic; formula is SubCraft's reading) | crates/export/src/template.rs Converted | chunk 47 |
| Converted map: 1/ior | Right column | - | done (from Specular level: level 0.5 = IOR 1.5) | crates/export/src/template.rs Converted | chunk 47 |
| Converted map: Glossiness squared | Right column | - | done | crates/export/src/template.rs Converted | chunk 47 |
| Converted map: f0 | Right column | - | done (0.08 x Specular level) | crates/export/src/template.rs Converted | chunk 47 |
| Mesh map: Normal | Right column | - | partial: raw file via the Baked Mesh Maps section, cannot be packed | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: World space normal | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: ID | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: Ambient occlusion | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: Curvature | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: Position | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: Thickness | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: Height | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |
| Mesh map: Bent normals | Right column | - | partial: raw file only | crates/ui/src/dialogs.rs:298 | #124 |

## List of exports tab

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Exported files listed per texture set | Tab body | - | done | crates/ui/src/dialogs.rs export_log | chunk 14 |
| Export console (status and error messages) | Tab body | - | done (Console section of List of exports, errors highlighted) | crates/ui/src/dialogs.rs export_log | chunk 14 |
| Automatic switch to this tab when Export is pressed | Window behaviour | - | done | crates/ui/src/dialogs.rs run_export | chunk 14 |
| Per-file progress while exporting | Tab body | (unverified) | missing | - | - |

## Predefined output templates (not editable)

Layout notes: these sit at the top of the Output template dropdown on the Settings tab and do not appear in the Output templates tab.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| 2D View | Output template dropdown | - | done | crates/export/src/template.rs:215 | - |
| Document channels + Normal + AO (No Alpha) | Output template dropdown | - | done (every channel the set has as its own file, plus normal with height and mixed AO; no Opacity file: alpha/opacity export is #145's) | crates/export/src/builtin_presets.rs | chunk 47 |
| Document channels + Normal + AO (With Alpha) | Output template dropdown | - | missing | - | - |
| Sketchfab | Output template dropdown (maps plus upload) | - | partial: map set only (upload is Adobe/Sketchfab service) | crates/export/src/template.rs:237 | - |
| Substance 3D Stager | Output template dropdown | - | n/a | - | - |
| USDz (Apple AR) | Output template dropdown: JPG maps, PNG normal, .usdc and .usdz | - | partial: USDA with PNG maps through "Export model + textures", no usdc / usdz | crates/export/src/scene.rs:324 | - |
| glTF PBR Metal Roughness | Output template dropdown: maps plus .gltf / .glb | - | partial: named "glTF 2.0"; scene file written by a separate button | crates/export/src/template.rs:132 | - |
| glTF PBR Metal Roughness + Displacement texture (experimental) | Output template dropdown | - | done (glTF maps plus a 16-bit height file) | crates/export/src/builtin_presets.rs | chunk 47 |

## Default output templates (editable copies)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Amazon Lumberyard | Output templates list | - | done (diff, spec, ddna with glossiness in alpha, emissive; unverified against Painter's file) | crates/export/src/builtin_presets.rs | chunk 47 |
| Arnold (AiStandard) | Output templates list | - | done (unverified against Painter's file) | crates/export/src/builtin_presets.rs | chunk 47 |
| Arnold UDIM Legacy (AiStandard) | Output templates list | - | done (explicit .$udim names) | crates/export/src/builtin_presets.rs | chunk 47 |
| Blender (Principled BSDF) | Output templates list | - | done (unverified against Painter's file) | crates/export/src/builtin_presets.rs | chunk 47 |
| Corona | Output templates list | - | done (Diffuse, Reflection, Glossiness converted; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| CryEngine 3 | Output templates list | - | done (diff, spec, ddna; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Dota 2 | Output templates list | - | missing | - | - |
| Keyshot | Output templates list | - | done (unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Keyshot 9+ | Output templates list | - | done (unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Lens Studio | Output templates list | - | done (Material Params: metallic, roughness, AO; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Maxwell (Metallic Roughness) | Output templates list | - | done (unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Maxwell (Specular Glossiness) | Output templates list | - | done (converted maps; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Mesh Maps | Output templates list: each baked map as its own file | - | done (template writing each baked map, where baked; the Baked Mesh Maps section stays) | crates/export/src/builtin_presets.rs | chunk 47 |
| Non-PBR Specular Glossiness | Output templates list | - | done (converted maps; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| PBR Metallic Roughness | Output templates list | - | done | crates/export/src/template.rs:72 | - |
| Alpha variants: Base Color RGB + A = Opacity (Painter's engine templates put opacity in Base Color alpha) | Output templates list | - | done: "PBR Metallic Roughness + Opacity" and "Unreal Engine (ORM packed) + Opacity" (RGBA Base Color plus a grey Opacity map), Unity HDRP / URP, Godot 4 and glTF 2.0 "+ Alpha"; alpha is Opacity x Base Color coverage, colour bled under alpha 0 | crates/export/src/template.rs (with_alpha) | #145 |
| PBR Metallic Roughness (Folder or PSD Groups) | Output templates list | - | missing | - | - |
| PBR Specular Glossiness | Output templates list | - | missing (needs specular / glossiness document channels; use the converted template) | - | - |
| PBR Specular Glossiness (converted from Metallic Roughness) | Output templates list | - | done | crates/export/src/builtin_presets.rs | chunk 47 |
| Redshift | Output templates list | - | done (unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Renderman (pxrDisney) | Output templates list | - | done (unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Renderman (pxrSurface) | Output templates list | - | done (converted diffuse / specular colours; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Roblox (MaterialVariant) | Output templates list | - | done | crates/export/src/builtin_presets.rs | chunk 47 |
| Roblox (SurfaceAppearance) | Output templates list | - | done | crates/export/src/template.rs:222 | - |
| Shade 3D | Output templates list | - | done (unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Spark AR Studio | Output templates list | - | done (ORM packed; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Unity HD Render Pipeline (Metallic Standard) | Output templates list | - | partial: named "Unity HDRP (Mask Map)" | crates/export/src/template.rs:108 | - |
| Unity HD Render Pipeline (Specular) | Output templates list | - | done (converted maps; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Unity Universal Render Pipeline (Metallic Standard) | Output templates list | - | partial: named "Unity URP (Metallic + Occlusion)" | crates/export/src/template.rs:152 | - |
| Unity Universal Render Pipeline (Specular) | Output templates list | - | done (converted maps; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Unreal Engine (Packed) | Output templates list | - | partial: named "Unreal Engine (ORM packed)" | crates/export/src/template.rs:89 | - |
| Unreal Engine SSS (Packed) | Output templates list | - | missing | - | - |
| Vray Next (Metallic Roughness) | Output templates list | - | done (unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Vray Next (Specular Glossiness) | Output templates list | - | done (Diffuse, Reflection, Glossiness converted; unverified) | crates/export/src/builtin_presets.rs | chunk 47 |
| Vray UDIM Legacy (Metallic Roughness) | Output templates list | - | done (explicit .$udim names) | crates/export/src/builtin_presets.rs | chunk 47 |

## Export mesh

Layout notes: a small separate window opened from the File menu, with a file-type choice and two radio options, each with one sub-checkbox.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Export mesh (menu entry) | File > Export mesh | - | done (File > Export mesh… opens its own window; Export model + textures stays in Export Textures) | crates/ui/src/export_mesh.rs | chunk 48 |
| Mesh file format choice | Export mesh window / save dialog | not documented (USD confirmed; FBX, OBJ, glTF unverified) | partial: OBJ, glTF, GLB, USDA; no FBX | crates/export/src/mesh_export.rs | chunk 48 |
| Without displacement/tessellation | Export mesh window option | selected (unverified) | done (default) | crates/export/src/mesh_export.rs | chunk 48 |
| Apply triangulation | Sub-option of Without displacement | off (unverified) | done (always: SubCraft keeps meshes as triangles; shown checked and locked) | crates/ui/src/export_mesh.rs | chunk 48 |
| With displacement/tessellation | Export mesh window option | - | done (0-4 subdivision levels, then Height x scale along the normals, per texture set and UDIM tile) | crates/export/src/mesh_export.rs displaced | chunk 48 |
| Recompute vertex normals | Sub-option of With displacement | off (unverified) | done (area-weighted, shared across UV seams) | crates/export/src/mesh_export.rs smooth_normals | chunk 48 |
| USD as mesh-only export format | Export mesh window | - | done (USDA mesh with untextured material slots) | crates/export/src/mesh_export.rs | chunk 48 |
| glTF / GLB written with the glTF template (mesh + textures) | Export textures, glTF template | - | done | crates/export/src/scene.rs:292 | - |

## Export from the layer stack

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Export flattened group to files | Layer stack right-click on a layer or group | - | done | crates/ui/src/panels/layers.rs:866 | - |
| Export flattened: batch over a multi-selection | Same entry with several layers selected | - | done | crates/ui/src/panels/layers.rs:866 | - |
| Export flattened: folder picker | Dialog | - | done | crates/ui/src/panels/layers.rs:878 | - |
| Export flattened: file name pattern field | Dialog | $textureSet_$layerName_$srcMap(.$udim), remembered | partial: remembered for the session | crates/app/src/flatten.rs:25 | - |
| Keyword $layerName | Flattened export pattern | - | done | crates/app/src/flatten.rs:25 | - |
| Keyword $srcMap | Flattened export pattern | - | done | crates/app/src/flatten.rs:25 | - |
| Export flattened: resolution from texture set, bit depth from channel | Fixed behaviour | - | partial: resolution from the set; 8-bit PNG or 32-bit EXR by channel | crates/app/src/flatten.rs:34 | - |
| Export flattened: padding fixed to 1 px | Fixed behaviour | 1 px | done | crates/app/src/flatten.rs:111 | - |
| Export flattened: format by channel (EXR for height / normal, PNG otherwise) | Fixed behaviour | - | done | crates/app/src/flatten.rs:34 | - |
| Export flattened: format choice when only a mask is exported | Dialog | - | missing | - | - |
| Export mask to file | Right-click on a layer mask | (unverified label; named in release notes) | missing | - | - |

## Export of the 2D view

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| 2D View predefined template (writes what the 2D view currently shows) | Export textures > Output template | - | done (also File > Export 2D view) | crates/ui/src/app.rs:1140 | - |
| 2D View export follows the chosen file type, size and padding | Export textures settings | - | partial: file format and padding only | crates/ui/src/dialogs.rs:243 | - |
