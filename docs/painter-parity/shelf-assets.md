# Painter parity: Assets panel (Shelf)

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/assets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/navigation , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/advanced-search-queries , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/filter-by-path , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/saved-searches , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/sub-library-tab , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/customizing-the-layout , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/assets/adding-a-new-library , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/substance-3d-assets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/importing-assets/adding-content-on-the-hard-drive , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/importing-assets/adding-resources-via-the-import-window , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/importing-assets/import-drag-and-drop , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/importing-assets/receiving-assets-from-other-substance-3d-applications , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/creating-custom-effects , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/shelf-and-assets-location , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-saved-searches-manually , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/excluding-resources-in-a-resource-path , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/preferences-and-content-migration , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-resource-paths-edi/adding-resource-paths-by-editing-preferences-manually , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-resource-paths-edi/editing-resource-paths-manually , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-resource-paths-edi/editing-the-shelf-preferences-with-python , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/libraries-configuration , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/smart-materials-and-masks , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/auto-update , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/plugins/resources-updater , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/effects/filter , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/creating-and-saving-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/painting/presets/photoshop-brush-presets/importing-photoshop-brush-presets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/technical-support/project-resources , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/file-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes
Checked against: origin/main 297dcb3 (2026-11-11)

## Panel placement and layout

Layout notes: the Assets panel is docked vertically on the left edge by default. From top to bottom: filter area (breadcrumbs, search field, asset-type icon row; optional Filter by path and Saved searches sections), the assets list (thumbnail grid or list), and a bottom bar of icon buttons. The Substance 3D Assets tab sits next to it in the same dock.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Assets panel | Dock, left side, vertical | open | done (Shelf panel) | crates/ui/src/panels/shelf.rs:515 | - |
| Undock / dock horizontally or on another screen | Panel title bar drag | - | missing (unverified in code: no floating shelf) | - | - |
| Reduced width: asset-type icons collapse into one multi-select menu | Narrow panel | automatic | partial: icons wrap onto more rows, never a menu | crates/ui/src/panels/shelf.rs:820 | - |
| Mouse wheel scroll in the list | Assets list | - | done | crates/ui/src/panels/shelf.rs:534 | - |
| Ctrl+Alt+drag to scroll a panel | Any panel | - | missing | - | - |
| Filter area | Top of panel | - | done | crates/ui/src/panels/shelf.rs:797 | - |
| Assets list | Middle of panel | - | done | crates/ui/src/panels/shelf.rs:534 | - |
| Bottom bar | Bottom of panel | - | done | crates/ui/src/panels/shelf.rs:856 | - |

## Breadcrumbs

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Breadcrumb path (All libraries > library > folder ...) | Top line of the filter area | All libraries | done | crates/ui/src/panels/shelf.rs (breadcrumbs) | - |
| Arrow between crumbs opens a list of sub-locations | Breadcrumbs | - | done | crates/ui/src/panels/shelf.rs (crumb_menu) | - |
| Greyed location when it holds no asset of the selected type | Breadcrumb dropdown | - | done | crates/ui/src/panels/shelf.rs (locations) | - |
| "All libraries" selected when every folder is deselected or a saved search is picked | Breadcrumbs | - | partial: All libraries when no folder is selected; a saved search shows its own folder | crates/ui/src/panels/shelf.rs (breadcrumbs) | - |

## Search field

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Search field (matches name, location and tags) | Filter area | empty | done | crates/ui/src/panels/shelf_filter.rs (Field::Any) | - |
| Clear (cross) button | Right end of the search field | - | done | crates/ui/src/panels/shelf.rs:815 | - |
| Search tag hover shows what the saved search contains | Tag inside the search field | - | done | crates/ui/src/panels/shelf.rs (header) | - |
| Fuzzy (approximate) matching | Default behaviour | on | done (one typo forgiven, two in long words) | crates/ui/src/panels/shelf_filter.rs (approximately_in) | - |

## Advanced search queries

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| n: (name) | Search field prefix | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| s: (library; includes session and project) | Search field prefix | - | partial: starter_assets, your_assets, extra libraries and session; no project library | crates/ui/src/panels/shelf_filter.rs | - |
| p: (path) | Search field prefix | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| u: (usage) | Search field prefix | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Usage keyword typed as a plain word (e.g. images) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Backslash escaping of spaces | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Quoted phrase | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Or-group after a prefix (n:a,b,c) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Comma between terms | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Trailing ! (exact / no fuzzy; "contains" suffix) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Leading - (exclude) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Suffix # (regex) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Suffix = (equal) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Suffix ^ (starts with) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |
| Suffix & (ends with) | Search field | - | done | crates/ui/src/panels/shelf_filter.rs | - |

## Asset type icons

Layout notes: one row of eight icons under the search field, in the order below. Click selects one type; Ctrl+click adds or removes a type. Materials is selected on first start.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Materials (base materials, material presets) | Icon 1 | selected | done | crates/ui/src/panels/shelf.rs:766 | - |
| Smart materials | Icon 2 | - | done | crates/ui/src/panels/shelf.rs:767 | - |
| Smart masks | Icon 3 | - | done | crates/ui/src/panels/shelf.rs:768 | - |
| Filters | Icon 4 | - | done | crates/ui/src/panels/shelf.rs:769 | - |
| Brushes (brush, particle and tool presets) | Icon 5 | - | partial: brush, tool and material presets (tips are under Alphas); no particle presets | crates/ui/src/panels/shelf.rs:770 | - |
| Alphas (alphas and brush makers) | Icon 6 | - | partial: own icon with the user's, built-in and starter tips; no brush makers | crates/ui/src/panels/shelf.rs (ShelfTab::Alphas) | - |
| Textures (grunges, procedurals, baked maps, hard-surface normals, LUTs) | Icon 7 | - | partial: split into Textures and an extra Normal Maps icon; no procedurals or LUTs | crates/ui/src/panels/shelf.rs:771 | - |
| Environment maps | Icon 8 | - | done | crates/ui/src/panels/shelf.rs:773 | - |
| Ctrl+click to multi-select asset types | Icon row | - | done | crates/ui/src/panels/shelf.rs (ShelfNav::click_type) | - |
| No type selected shows every type | Icon row | (unverified) | partial: one type always stays selected (Ctrl+click can add every type) | crates/ui/src/panels/shelf.rs (ShelfNav::click_type) | - |

## Filter by path

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Filter by path section (tree of libraries and folders) | Optional section of the filter area | closed (unverified) | done | crates/ui/src/panels/shelf.rs (filter_by_path) | - |
| Hide non-applicable folders | Toggle in the section | on | done | crates/ui/src/panels/shelf.rs (filter_by_path) | - |
| Multi-select folders | Tree | (unverified) | done (Ctrl+click) | crates/ui/src/panels/shelf.rs (ShelfNav::click_path) | - |
| Shift+click to collapse / expand the whole tree | Tree | - | done | crates/ui/src/panels/shelf.rs (filter_by_path) | - |

## Saved searches

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Saved searches section | Optional section of the filter area | closed (unverified) | done | crates/ui/src/panels/shelf.rs (saved_searches) | - |
| Save current search (text + types + path) | Button in the section | (unverified label) | done | crates/ui/src/panels/shelf.rs (saved_searches) | - |
| Click a saved search to apply it | Section list | - | done | crates/ui/src/panels/shelf.rs (saved_searches) | - |
| Right-click > Create new tab | Saved search context menu | - | done | crates/ui/src/panels/shelf.rs (saved_searches) | - |
| Right-click > Rename | Saved search context menu | - | done | crates/ui/src/panels/shelf.rs (saved_searches) | - |
| Right-click > Delete | Saved search context menu | - | done | crates/ui/src/panels/shelf.rs (saved_searches) | - |
| Saved searches stored in Shelf.ini ([filters] size / Nname / Nquery) | Documents folder config file | - | partial: kept in SubCraft's preferences as name + query in Painter's query form; no Shelf.ini | crates/ui/src/recovery.rs (shelf_saved_searches) | - |

## Sub-library tabs

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Open current search as a sub-library tab | Bottom bar button | - | done | crates/ui/src/panels/shelf.rs (bottom_toolbar) | - |
| Sub-library tab docks beside the Assets panel, can float or move | Dock system | - | done (dock tab beside Assets; can be dragged elsewhere) | crates/ui/src/app.rs (sync_sub_libraries) | - |
| Reset of a sub-library returns to its original query | Sub-library tab | - | done | crates/ui/src/panels/shelf.rs (header) | - |
| Closed sub-library must be recreated; survives deletion of its saved search | Behaviour | - | done | crates/ui/src/panels/shelf.rs (close_sub_library) | - |

## Assets list and view modes

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Thumbnail grid | Assets list | - | done | crates/ui/src/panels/shelf.rs:311 | - |
| Thumbnail size control | Left-most icon of the bottom bar | Medium | done (menu) | crates/ui/src/panels/shelf.rs (size_menu) | - |
| Thumbnail size: Small | Size menu | - | done | crates/ui/src/panels/shelf.rs (PREVIEW_SIZES) | - |
| Thumbnail size: Medium | Size menu | default | done | crates/ui/src/panels/shelf.rs (PREVIEW_SIZES) | - |
| Thumbnail size: Large | Size menu | - | done | crates/ui/src/panels/shelf.rs (PREVIEW_SIZES) | - |
| List view | Size menu | - | done | crates/ui/src/panels/shelf.rs (list_row) | - |
| Hover tooltip with asset name and info | Assets list | - | done | crates/ui/src/panels/shelf.rs:311 | - |
| Multi-selection of assets (Ctrl / Shift click) | Assets list | (unverified) | missing | - | - |
| Material thumbnails rendered on a sphere with displacement | Assets list | - | partial: sphere preview, no displacement | crates/app/src/thumbnails.rs:76 | - |
| Thumbnail read from the resource when it carries one | Assets list | - | missing | - | - |
| Automatic thumbnail for new smart materials | Assets list | - | done | crates/ui/src/panels/shelf.rs:921 | - |
| Photoshop badge on ABR brush thumbnails | Assets list | - | missing | - | - |
| Particle tag on particle brushes | Assets list | - | missing | - | - |

## Bottom bar

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Thumbnail size icon | Bottom bar, left | Medium | done | crates/ui/src/panels/shelf.rs (size_menu) | - |
| Import resources (+) button | Bottom bar | - | partial: per-category import button opening a file picker, no import window | crates/ui/src/panels/shelf.rs:867 | - |
| New sub-library tab button | Bottom bar | - | done | crates/ui/src/panels/shelf.rs (bottom_toolbar) | - |
| Auto-update (double arrows) menu button | Bottom bar | - | missing | - | - |
| Auto-update: Assets panel | Auto-update menu, Automatic updates | off (unverified) | missing | - | - |
| Auto-update: Resources used in project | Auto-update menu, Automatic updates | off (unverified) | missing | - | - |
| Auto-update: Update every x minutes | Auto-update menu | not documented; 0 = every few seconds | missing | - | - |
| Auto-update: Update assets panel (manual) | Auto-update menu, Manual updates | - | partial: Refresh libraries in Settings > Libraries; no auto-update menu | crates/ui/src/libraries.rs (refresh_libraries) | - |
| Auto-update: Update resources used in project (manual) | Auto-update menu, Manual updates | - | missing | - | - |
| Auto-update: Skip assets when their parameters mismatch | Auto-update menu, Advanced | on (unverified) | missing | - | - |
| Auto-update: open log icon (warning badge on issues) | Top right of the auto-update menu | - | missing | - | - |
| Auto-update results in Log window, Assets tab | Log window | - | missing | - | - |
| Update check when the application regains focus | Behaviour with automatic updates on | - | missing | - | - |

## Libraries

Layout notes: libraries are the top level of breadcrumbs and of Filter by path. Presets created in the app go to the library marked Default.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| All libraries (root) | Breadcrumbs / Filter by path | selected | done | crates/ui/src/panels/shelf.rs (breadcrumbs, filter_by_path) | - |
| Current session (temporary, lost on quit) | Library list / s:session | - | done (imports read in place, not copied) | crates/ui/src/libraries.rs (SESSION) | - |
| Project (resources embedded in the project file) | Library list / s:project | - | missing (the import window lists it, disabled) | - | - |
| Your assets (your_assets; Documents folder) | Library list | default write target | done (user library, Documents/SubCraft/Library; "Mine" chip) | crates/assets/src/user_library.rs:60 | - |
| Starter assets (starter_assets; install folder, read only) | Library list | - | done (built-in content compiled in) | crates/assets/src/lib.rs:36 | - |
| Custom libraries added in settings | Library list | - | done | crates/ui/src/libraries.rs (settings) | - |

## Using assets: click, double-click and drag and drop

Layout notes: rows are grouped by asset type, then by target (viewport, layer stack, mask, Properties slot). Painter highlights the mesh part under the cursor while a material is dragged over the viewport.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Material: click with a paint tool loads it as the tool material | Assets list | (unverified) | partial: click swaps the selected fill layer's material or adds a fill layer | crates/ui/src/panels/shelf.rs:34 | - |
| Material: drag onto the viewport (applies to the texture set under the cursor) | Viewport | - | done | crates/ui/src/viewport.rs:190 | - |
| Material: drag into the layer stack (new fill layer at the drop position) | Layer stack | - | done | crates/ui/src/panels/layers.rs:411 | - |
| Material: drag onto a material slot in Properties | Properties | (unverified) | partial: Alt+drop or Alt+click replaces the selected fill layer's material | crates/app/src/shelf_apply.rs:29 | - |
| Smart material: drag into the layer stack | Layer stack | - | done | crates/ui/src/panels/layers.rs:411 | - |
| Smart material: drag onto the viewport | Viewport | (unverified) | done | crates/ui/src/viewport.rs:190 | - |
| Smart material: add from the layer stack Smart Material button (mini-shelf) | Layer stack toolbar | - | missing | - | - |
| Smart mask: drag onto a layer (adds to its mask effect stack) | Layer stack | - | done | crates/ui/src/panels/layers.rs:411 | - |
| Smart mask: several drops accumulate | Layer stack | - | missing (unverified in code: a drop replaces the mask) | - | - |
| Smart mask: Ctrl+drag replaces the whole effect stack | Layer stack | - | missing | - | - |
| Filter: drag into the layer stack (adds a filter effect) | Layer stack | - | partial: click on the tile adds or swaps the filter, no drag | crates/ui/src/panels/shelf.rs:709 | - |
| Brush preset: click to load it in the current tool | Assets list | (unverified) | done | crates/ui/src/panels/shelf.rs:663 | - |
| Particle preset: click to load it | Assets list | (unverified) | missing | - | - |
| Tool preset: click to load tool + brush + material | Assets list | (unverified) | done | crates/ui/src/state.rs (select_shelf_brush) | - |
| Alpha: click sets the brush alpha | Assets list | (unverified) | done | crates/ui/src/panels/shelf.rs:690 | - |
| Alpha: drag onto the brush Alpha slot in Properties | Properties > Brush | - | missing | - | - |
| Alpha / texture: drag into the layer stack opens a menu asking the target channel | Layer stack | - | partial: texture drop adds a fill layer without a channel menu; alpha drop goes to the layer mask | crates/ui/src/panels/layers.rs:411 | - |
| Texture: drag onto a channel slot of a fill layer | Properties | - | done | crates/ui/src/panels/properties.rs:299 | - |
| Texture: drag onto a channel slot of the paint tool material | Properties > Material | - | done | crates/ui/src/panels/brush_material.rs:112 | - |
| Texture (grunge / procedural): drag onto a mask to add a fill effect | Layer stack mask | (unverified) | partial: alphas only | crates/ui/src/panels/layers.rs:305 | - |
| Hard-surface normal: click to stamp with the normal channel | Assets list | (unverified) | done (normal / height stamp brush) | crates/ui/src/panels/shelf.rs:566 | - |
| Environment map: drag onto the viewport | Viewport | - | partial: click on the tile selects it, no drag | crates/ui/src/panels/shelf.rs:737 | - |
| Environment map: pick in Display settings | Display settings | - | partial: rotation and intensity sliders inside the shelf | crates/ui/src/panels/shelf.rs:838 | - |
| LUT: pick as colour profile in Display settings | Display settings | - | missing | - | - |
| Shader: drag to Shader settings | Shader settings | (unverified) | missing | - | - |
| OS file: drag onto the Assets panel opens Import resources | Assets panel | - | partial: .scsmart, folders and textures are imported directly, .spsm applied to the set | crates/ui/src/panels/shelf.rs:413 | - |
| OS file: drag a material onto the viewport (import and apply) | Viewport | - | partial: .spsm, models and projects only | crates/ui/src/files.rs:165 | - |
| OS file: drag an SVG onto the viewport (new layer, warp projection, Graphic to Material) | Viewport | - | missing | - | - |
| OS file: drag into the layer stack (creates layers or effects, channel menu) | Layer stack | - | missing | - | - |
| OS file: drag onto a resource slot (auto import and apply) | Properties | - | done (image onto a fill channel row) | crates/ui/src/panels/properties.rs:295 | - |
| Double-click a texture to preview and adjust it | Assets list | (unverified; Painter has no such preview) | done (SubCraft extra) | crates/ui/src/panels/shelf.rs:566 | - |

## Asset right-click context menu

Layout notes: the public help pages do not list this menu; entries below marked unverified come from memory and release notes.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Update from current tool (overwrite a preset with the current Properties values) | Right-click a brush / tool / material preset | - | done | crates/ui/src/panels/shelf.rs (preset_menu) | - |
| Open in explorer (show file on disk; local resources) | Right-click an asset | named in release notes | done (Show in folder; user library items only) | crates/ui/src/panels/shelf.rs:373 | - |
| Resource info (library name, usage, location) | Top of the context menu | (unverified) | missing | - | - |
| Add to / remove from favorites | Right-click an asset | (unverified) | partial: brush, tool and material presets only; favorites are listed first | crates/ui/src/panels/shelf.rs (preset_menu) | - |
| Rename | Right-click an asset | (unverified) | partial: user brush, tool and material presets only | crates/ui/src/panels/shelf.rs (preset_menu) | - |
| Duplicate | Right-click an asset | (unverified) | partial: brush, tool and material presets only | crates/ui/src/panels/shelf.rs (preset_menu) | - |
| Delete | Right-click an asset | (unverified) | partial: "Remove from library" for user library items | crates/ui/src/panels/shelf.rs:389 | - |
| Export resource(s) | Right-click an asset | (unverified) | partial: "Export smart material" for .scsmart only | crates/ui/src/panels/shelf.rs:378 | - |
| Import to project / library | Right-click an asset | (unverified) | missing | - | - |
| Reload / relocate resource | Right-click an asset (named in the auto-update page) | - | missing | - | - |
| Set custom preview / regenerate preview | Right-click an asset | (unverified) | missing | - | - |
| Open in Resource Updater | Right-click an asset | (unverified) | missing | - | - |
| Export to Substance Share | Right-click an asset (removed by Adobe) | - | n/a | - | - |

## Import resources window

Layout notes: toolbar on top (Add resources, Remove selected resources, usage filter dropdown), a table with one row per file (usage dropdown, prefix, file name, warning icon), and at the bottom the import-location dropdown with the Import button.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Import resources (menu entry) | File > Import resources | - | done (File > Import resources > Import resources…; the older direct entries stay) | crates/ui/src/app.rs (File menu) | - |
| Open from the Assets panel + button | Assets bottom bar | - | done | crates/ui/src/panels/shelf.rs (bottom_toolbar) | - |
| Open by dropping files or folders on the Assets panel | Assets panel | - | done (the Smart Materials type keeps building smart materials from PBR sets) | crates/ui/src/panels/shelf.rs (handle_drops) | - |
| Add resources | Window toolbar | - | done | crates/ui/src/import_resources.rs | - |
| Remove selected resources | Window toolbar | - | done | crates/ui/src/import_resources.rs | - |
| Usage filter dropdown (e.g. show undefined only) | Window toolbar | all | done | crates/ui/src/import_resources.rs | - |
| Usage per file | Table column dropdown | preset when the format has one usage or the graph defines it; otherwise undefined | done (format with one use preset; images undefined until chosen) | crates/ui/src/import_resources.rs (Usage::guess) | - |
| Prefix (sub-folder path) | Table column | filled from the substance category when present | done (sub-folder inside the library, becomes the category) | crates/ui/src/import_resources.rs, crates/assets/src/user_library.rs (add_file_to) | - |
| Filename | Table column | - | done | crates/ui/src/import_resources.rs | - |
| Warning icon on unreadable or incompatible files | Table row | - | done | crates/ui/src/import_resources.rs (Row::warning) | - |
| Multi-select rows to set usage on several files | Table | - | done (row checkboxes) | crates/ui/src/import_resources.rs (ImportWindow::set_usage) | - |
| Drop more files onto the window | Window | - | done | crates/ui/src/import_resources.rs (window) | - |
| Import location: Current session | Bottom dropdown | - | done | crates/ui/src/import_resources.rs | - |
| Import location: Project | Bottom dropdown | - | missing (listed, disabled) | - | - |
| Import location: Library (the writable library) | Bottom dropdown | remembered default import location | done (any library; the default one preselected) | crates/ui/src/import_resources.rs | - |
| Import button | Bottom right | - | done | crates/ui/src/import_resources.rs | - |
| Cancel button | Bottom right | (unverified) | done | crates/ui/src/import_resources.rs | - |

## Importable file formats and usages

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| SBSAR as base material | Usage basematerial; folder materials | - | missing | - | - |
| SBSAR as filter | Usage filter; folder effects | - | missing | - | - |
| SBSAR as generator | Usage generator; folder generators | - | missing | - | - |
| Image (PNG, TGA, JPEG...) as texture | Usage texture; folder textures | - | done | crates/assets/src/user_library.rs:44 | - |
| Image as alpha | Usage alpha; folder alphas | - | done | crates/assets/src/user_library.rs:40 | - |
| HDR / EXR as environment | Usage environment; folder environments | - | done | crates/assets/src/user_library.rs:41 | - |
| HDR / EXR / image as colour LUT | Usage colorlut; folder colorluts | - | missing | - | - |
| GLSL as shader | Usage shader; folder shaders | - | missing | - | - |
| SPPR as brush preset | Folder presets/brush | - | missing (own .scbrush format) | - | - |
| SPPR as particle preset | Folder presets/particles | - | missing | - | - |
| SPPR as material preset | Folder presets/materials or materials | - | missing | - | - |
| SPPR as tool preset | Folder presets/tools | - | missing | - | - |
| SPSM smart material | Folder smart-materials | - | partial: applied to the selected texture set on drop, not added to the library | crates/ui/src/files.rs:148 | - |
| SPMSK smart mask | Folder smart-masks | - | missing (own .scmask format) | - | - |
| SPEXP export preset | Folder export-presets | - | missing | - | #124 |
| ABR Photoshop brushes (creates brush presets plus their alpha images) | Import resources | - | missing | - | - |
| SBS files are rejected (must be published as SBSAR) | Import resources | - | n/a | - | - |
| Custom sub-folders become categories (Filter by path / breadcrumbs) | Library folders | - | done | crates/assets/src/user_library.rs (list, subfolder) | - |

## Library folders on disk

Layout notes: every library root holds the same fixed set of sub-folders; the folder a file sits in defines its usage. Painter creates them when a library is added.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| alphas | Library sub-folder | - | done | crates/assets/src/user_library.rs:30 | - |
| colorluts | Library sub-folder | - | missing | - | - |
| effects | Library sub-folder (filters) | - | missing | - | - |
| emitters | Library sub-folder (particles) | - | missing | - | - |
| environments | Library sub-folder | - | done | crates/assets/src/user_library.rs:31 | - |
| export-presets | Library sub-folder | - | missing | - | #124 |
| generators | Library sub-folder | - | missing | - | - |
| materials | Library sub-folder | - | missing | - | - |
| presets | Library sub-folder (brush, particles, materials, tools) | - | partial: brush-presets folder only | crates/assets/src/user_library.rs:34 | - |
| procedurals | Library sub-folder | - | missing | - | - |
| receivers | Library sub-folder (particles) | - | missing | - | - |
| shaders | Library sub-folder | - | missing | - | - |
| smart-masks | Library sub-folder | - | done | crates/assets/src/user_library.rs:33 | - |
| smart-materials | Library sub-folder | - | done | crates/assets/src/user_library.rs:32 | - |
| templates | Library sub-folder (project templates) | - | missing | - | - |
| textures | Library sub-folder | - | done | crates/assets/src/user_library.rs:35 | - |
| .ignore_assets_pt exclude file (gitignore-like patterns, case sensitive) | Any folder of a library | - | missing | - | - |

## Libraries settings

Layout notes: Edit > Settings > Libraries. A Name field, a Path field with a browse (...) button and a + button on one line; below, the list of libraries, each with a Default radio button and a - button. Editable only while no project is open.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| your_assets default library | Libraries list | Documents/Adobe/Adobe Substance 3D Painter | partial: fixed user library, no settings page | crates/assets/src/user_library.rs:60 | - |
| starter_assets default library | Libraries list | install folder | partial: built in, not listed | crates/assets/src/lib.rs:36 | - |
| Name field (spaces and special characters become underscores) | Libraries settings | empty | missing | - | - |
| Path field | Libraries settings | empty | missing (SUBCRAFT_LIBRARY environment variable only) | - | - |
| Browse (...) button | Beside Path | - | missing | - | - |
| + button (add library, creates the sub-folders) | Libraries settings | - | missing | - | - |
| - button (remove library; default ones are only disabled) | Per library row | - | missing | - | - |
| Default radio button (where new presets and imports are written) | Per library row | your_assets | missing | - | - |
| Settings locked while a project is open | Libraries settings | - | missing | - | - |
| Manual edit: pathInfos entries (disabled / name / path, size counter) | Registry / plist / config file | - | missing | - | - |
| Manual edit: writableShelf (default library name) | Registry / plist / config file | - | missing | - | - |
| Open library folder on disk | Assets bottom bar in SubCraft; file explorer in Painter | (unverified in Painter UI) | done | crates/ui/src/panels/shelf.rs:889 | - |

## Creating presets from the application

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Create tool preset | Right-click in Properties (paint tool) | - | done | crates/ui/src/panels/brush.rs (create_preset) | - |
| Create material preset | Right-click in Properties (paint tool) | - | done | crates/ui/src/panels/brush.rs (create_preset) | - |
| Create brush preset | Right-click in Properties (paint tool) | - | done | crates/ui/src/panels/brush.rs (create_preset) | - |
| Update from current tool | Right-click the preset in Assets | - | done | crates/ui/src/panels/shelf.rs (apply_preset_action) | - |
| Create smart material | Right-click a folder in the layer stack | named after the folder | partial: "Save as smart material" on any layer | crates/ui/src/panels/layers.rs:476 | - |
| Create smart mask | Right-click a layer in the layer stack | - | done ("Save mask as smart mask") | crates/ui/src/panels/layers.rs:480 | - |
| Presets embed the resources they use | Preset files | - | partial: brush preset saves its tip image beside it | crates/app/src/brush_preset.rs:56 | - |
| New presets are written to the Default library | Libraries | your_assets | done (user library) | crates/app/src/library.rs:17 | - |
| Path tool Presets section with favorites | Properties with the Path tool | - | missing | - | - |
| Particle presets (particle editor) | Separate editor | - | missing | - | - |
| Smart material source cannot be traced after it is added | Layer stack behaviour | - | done (same behaviour) | crates/app/src/library.rs:17 | - |

## Resource updater and project resources

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Resources Updater window (lists resources used by the project) | Plugin window | - | partial: text summary of embedded images only | crates/app/src/images.rs:74 | - |
| Outdated resources shown in red | Resources Updater | - | missing | - | - |
| Replace a project resource with a library one | Resources Updater, per resource | - | missing | - | - |
| Update / Update all buttons | Resources Updater | (unverified) | missing | - | - |
| Clean (remove unused resources from the project) | File > Clean | - | missing | - | - |
| Save and compact | File > Save and compact | - | missing | - | - |

## Substance 3D Assets and other Adobe services

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Substance 3D Assets panel (online library browser) | Tab next to Assets | - | n/a | - | - |
| Substance 3D Assets: search field | Panel top | - | n/a | - | - |
| Substance 3D Assets: filter | Panel top | - | n/a | - | - |
| Substance 3D Assets: download button per asset | Asset card | - | n/a | - | - |
| Substance 3D Assets: download manager | Bottom left button | - | n/a | - | - |
| Substance 3D Assets control menu: Open folder location | Bottom right button | - | n/a | - | - |
| Substance 3D Assets control menu: Reload page | Bottom right button | - | n/a | - | - |
| Substance 3D Assets control menu: Previous page | Bottom right button | - | n/a | - | - |
| Substance 3D Assets control menu: Next page | Bottom right button | - | n/a | - | - |
| Substance 3D Community Assets | Online library | (unverified) | n/a | - | - |
| Receive assets sent from Designer (Send to Substance 3D Painter) | Assets panel, default library | - | n/a | - | - |
| Receive assets sent from Sampler | Assets panel, default library | - | n/a | - | - |

## Mini-shelf popups

Layout notes: a mini-shelf is a small floating asset browser, pre-filtered to one usage, with its own search field. It opens when a resource slot or picker button is clicked and closes after a pick.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Mini-shelf search field | Top of every mini-shelf | usage query followed by a space | partial: on the brush alpha slot only; plain name search, no usage query | crates/ui/src/panels/brush_rows.rs (mini_browser) | - |
| Smart Material button mini-shelf | Layer stack toolbar | - | missing | - | - |
| Filter selection mini-shelf | Properties of a filter effect | - | partial: searchable list without thumbnails | crates/ui/src/panels/filter.rs:23 | - |
| Generator selection mini-shelf | Properties of a generator effect | (unverified) | partial: plain menu in the layer context menu | crates/ui/src/panels/layers.rs:470 | - |
| Fill channel resource slot mini-shelf | Properties of a fill layer / fill effect | - | partial: file picker and anchor picker, no library browser | crates/ui/src/panels/properties.rs:760 | - |
| Material mode picker mini-shelf | Properties > Material | (unverified) | missing | - | - |
| Brush alpha slot mini-shelf | Properties > Brush > Alpha | (unverified) | done | crates/ui/src/panels/brush_rows.rs (mini_browser) | - |
| Stencil slot mini-shelf | Properties > Stencil | (unverified) | partial: file picker | crates/ui/src/files.rs:25 | - |
| Environment map slot mini-shelf | Display settings | (unverified) | missing | - | - |
| Colour profile (LUT) slot mini-shelf | Display settings | (unverified) | missing | - | - |
| Shader picker mini-shelf | Shader settings | (unverified) | missing | - | - |

## Default content categories

Layout notes: counts of Adobe's starter assets are not given in the public help pages; none of Adobe's content is copied. The Status column says whether SubCraft ships its own content for the category.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Materials | Starter assets | count not documented | done (own content) | crates/assets/src/presets.rs:131 | - |
| Smart materials | Starter assets | count not documented | done (own content) | crates/assets/src/smart.rs:249 | - |
| Smart masks | Starter assets | count not documented | done (own content) | crates/assets/src/presets.rs:250 | - |
| Filters | Starter assets | count not documented | partial: built-in filter effects, not assets | crates/ui/src/panels/shelf.rs:698 | - |
| Generators | Starter assets | count not documented | partial: built in, reached from the layer menu, not listed in the shelf | crates/ui/src/panels/layers.rs:470 | - |
| Brushes | Starter assets | count not documented | done (own content) | crates/ui/src/panels/shelf.rs:647 | - |
| Particles | Starter assets | count not documented | missing | - | - |
| Tools | Starter assets | count not documented | missing | - | - |
| Alphas | Starter assets | count not documented | done (own content) | crates/assets/src/alphas.rs:39 | - |
| Brush makers | Starter assets, Alphas | count not documented | missing | - | - |
| Grunges | Starter assets, Textures | count not documented | partial: starter texture pack | crates/assets/src/starter.rs:1 | - |
| Procedurals | Starter assets, Textures | count not documented | missing | - | - |
| Hard-surface normals | Starter assets, Textures | count not documented | partial: starter normal maps | crates/assets/src/starter.rs:1 | - |
| Colour LUTs | Starter assets, Textures | count not documented | missing | - | - |
| Environment maps | Starter assets | count not documented | done (own content) | crates/assets/src/environment.rs:152 | - |
| Fonts | Starter assets | count not documented | partial: user picks a .ttf / .otf file | crates/ui/src/files.rs:27 | - |
| Shaders | Starter assets | count not documented | missing | - | - |
| Export presets | Starter assets | 34 default + 8 predefined | partial: 9 built-in templates | crates/export/src/template.rs:252 | #124 |
| Project templates | Starter assets | count not documented | partial: built-in new-project templates | crates/app/src/new_project.rs:72 | - |
