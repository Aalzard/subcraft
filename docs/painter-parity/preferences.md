# Painter parity: Settings, project settings, preference files and command line

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/general-preferences , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/libraries-configuration , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management-with-opencolorio , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management-with-adobe-ace-icc , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/auto-update , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/plugins/autosave , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/spacemouse-by-3dconnexion , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/technical-support/configuring-pens-and-tablets , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/configuration/command-lines , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/configuration/environment-variables , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/configuration/firewall-configuration , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/configuration/querying-current-software-version , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/configuration/remote-desktop , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/installation-and-pre/automated-installation , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/installation-and-pre/preferences-and-application-data-location , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/installation-and-pre/retrieving-the-installation-path , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-resource-paths-edi/adding-resource-paths-by-editing-preferences-manually , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-resource-paths-edi/editing-resource-paths-manually , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-resource-paths-edi/editing-the-shelf-preferences-with-python , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/adding-saved-searches-manually , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/excluding-resources-in-a-resource-path , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/preferences-and-content-migration , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/resource-management/shelf-and-assets-location
Checked against: origin/main 297dcb3 (2026-11-11)

## Settings window

Layout notes: Edit > Settings opens one window with three sections chosen on the left: General, Shortcuts, Libraries. SubCraft has one scrolling Preferences window (File or Edit > Preferences) with Interface, Graphics, Rendering mode and Autosave blocks.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Settings window | Edit > Settings | - | partial: Edit > Settings with General and Shortcuts pages; no Libraries page | crates/ui/src/recovery.rs (preferences_window) | #209 |
| General section | Settings | first section | partial: Settings > General page; Painter options added here, other SubCraft options share the page | crates/ui/src/recovery.rs (general_view_settings) | #210 |
| Shortcuts section | Settings | - | done: key set, editable keys, camera mouse gestures, the fixed list | crates/ui/src/shortcut_editor.rs | #209 |
| Libraries section | Settings | - | done (Settings > Libraries page) | crates/ui/src/libraries.rs (settings) | - |

## General: Interface options

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Language | Settings > General > Interface | Default (system language); needs restart | partial: different language list, applies without restart | crates/ui/src/recovery.rs:266 | - |
| Language value: Default (system language) | Settings > General > Interface | default | missing | - | - |
| Language value: English | Settings > General > Interface | - | done | crates/ui/src/i18n.rs:38 | - |
| Language value: German | Settings > General > Interface | - | missing | - | - |
| Language value: French | Settings > General > Interface | - | missing | - | - |
| Language value: Japanese | Settings > General > Interface | - | missing | - | - |
| Language value: Chinese (simplified) | Settings > General > Interface | - | partial: Traditional Chinese only | crates/ui/src/i18n.rs:38 | - |
| Show keyboard helper | Settings > General > Interface | not documented | partial: toggle is View > Viewport hints, not in Preferences | crates/ui/src/app.rs:1263 | - |
| Show world axes | Settings > General > Interface | not documented | done: the bottom-right X / Y / Z axis indicator (the same setting as Display Settings > Axis indicator, on by default) | crates/ui/src/recovery.rs (general_view_settings), viewport.rs (draw_axis_indicator) | #210 |
| Background color (first gradient colour) | Settings > General > Interface | not documented | done: top colour of the 3D view background | crates/render/src/viewport.wgsl (fs_background) | #210 |
| Background color (second gradient colour) | Settings > General > Interface | not documented | done: bottom colour; Rendering mode keeps its own background | crates/render/src/viewport.wgsl (fs_background) | #210 |
| Only display the selected material when painting | Settings > General > Interface | off recommended | missing | - | - |
| Viewport Scaling | Settings > General > Interface | None or Auto | partial: render resolution 100 / 75 / 50 / 25 % in the 3D view toolbar | crates/ui/src/viewport_bar.rs:347 | - |
| Viewport Scaling value: None | Settings > General > Interface | native resolution | partial: 100 % | crates/ui/src/viewport_bar.rs:12 | - |
| Viewport Scaling value: Auto | Settings > General > Interface | half resolution on HiDPI screens | partial: manual 50 % | crates/ui/src/viewport_bar.rs:12 | - |

## General: Layer stack options

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Default UV scale for materials | Settings > General > Layer stack | not documented | partial: UV tiling of new fill layers; smart materials and shelf drops keep their own scale | crates/app/src/session.rs (default_uv_scale) | #210 |
| Use simplified thumbnails | Settings > General > Layer stack | not documented | missing | - | - |

## General: Camera options

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Rotation speed | Settings > General > Camera | multiplier; not documented | done: 0.1-5x (default 1), every orbit mode | crates/ui/src/nav.rs (CameraSpeed) | 306 |
| Zoom speed | Settings > General > Camera | multiplier; not documented | done: 0.1-5x (default 1) for the zoom drag | crates/ui/src/nav.rs (CameraSpeed) | 306 |
| Zoom speed: Inverse direction | Settings > General > Camera | not documented | done | crates/ui/src/nav.rs (CameraSpeed) | 306 |
| Wheel speed | Settings > General > Camera | multiplier; not documented | done: 0.1-5x (default 1) for the mouse wheel in the 3D view | crates/ui/src/nav.rs (CameraSpeed) | 306 |
| Wheel speed: Inverse direction | Settings > General > Camera | not documented | done: "Invert zoom", 3D and 2D views; off = wheel forward zooms in; the wheel speed also applies to the 2D view | crates/ui/src/nav.rs (CameraSpeed::wheel_zoom, wheel_magnify_2d) | 306, #223 |

## General: Baking options

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Save preprocessed scene files | Settings > General > Baking | not documented | missing | - | - |
| Enable live preview baking process | Settings > General > Baking | not documented | partial: always shows finished maps while baking; switch lives in the Bake window state, not Preferences | crates/ui/src/dialogs.rs:541 | - |
| Enable GPU Raytracing | Settings > General > Baking | needs compatible hardware | partial: "Use GPU for baking" covers ambient occlusion and thickness only | crates/ui/src/recovery.rs:295 | - |

## General: Preview options

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Local cache directory | Settings > General > Preview | not documented | missing | - | - |
| Local cache budget (in MB) | Settings > General > Preview | not documented | missing | - | - |
| Material preview shader | Settings > General > Preview | needs restart | missing | - | - |

## General: Temporary files, Sparse virtual textures, Iray hardware, Privacy

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Cache directory | Settings > General > Temporary Files | overridable by SUBSTANCE_PAINTER_TEMP_LOCATION | partial: Cache folder for the Painter import cache and update downloads; no environment override | crates/app/src/recovery.rs (cache_dir) | #210 |
| Hardware support acceleration | Settings > General > Sparse virtual textures | overridable by SUBSTANCE_PAINTER_SVT_HARDWARE_ACCELERATION | missing | - | - |
| Iray hardware: CPU tick box | Settings > General > Iray hardware | listed on every computer | n/a | - | - |
| Iray hardware: one tick box per CUDA GPU | Settings > General > Iray hardware | listed when present | n/a | - | - |
| Automatically send usages statistics | Settings > General > Privacy | not documented | n/a | - | - |

## Libraries configuration

Layout notes: a list of resource paths, each with a name, a folder, a "default" radio button and a minus button; Name and Path fields with a plus button add a row. Paths can be added or changed only while no project is open.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| your_assets path (user Documents folder) | Settings > Libraries | present by default | done (listed with its path) | crates/ui/src/libraries.rs (settings) | - |
| starter_assets path (install folder) | Settings > Libraries | present by default | partial: listed as built in; compiled in, not a folder | crates/ui/src/libraries.rs (settings) | - |
| default radio button (where new presets are saved) | Settings > Libraries, per path | your_assets | done | crates/ui/src/libraries.rs (settings, UiState::set_libraries) | - |
| Name field | Settings > Libraries > add path | empty | done | crates/ui/src/libraries.rs (settings) | - |
| Path field | Settings > Libraries > add path | empty | done (with a folder picker) | crates/ui/src/libraries.rs (settings) | - |
| Plus button (adds the path, creates sub-folders) | Settings > Libraries > add path | - | done (creates the folder; kind sub-folders appear on first import) | crates/ui/src/libraries.rs (settings) | - |
| Minus button (removes a path; default paths are only disabled) | Settings > Libraries, per path | - | partial: removes an added library; your_assets cannot be removed or disabled | crates/ui/src/libraries.rs (settings) | - |
| Paths editable only with no project open | Settings > Libraries | rule | partial: editable any time; the panel reloads at once | crates/ui/src/libraries.rs (settings) | - |

## Automatic resource update (Assets window, double-arrow button)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Automatic updates: Assets panel | Assets > auto-update menu | off (unverified) | missing | - | - |
| Automatic updates: Resources used in project | Assets > auto-update menu | off (unverified) | missing | - | - |
| Update every x minutes | Assets > auto-update menu | not documented; 0 = every few seconds | missing | - | - |
| Check for changes when the application regains focus | Automatic behaviour | on while automatic updates are enabled | missing | - | - |
| Manual: Update assets panel | Assets > auto-update menu | - | missing | - | - |
| Manual: Update resources used in project | Assets > auto-update menu | - | missing | - | - |
| Skip assets when their parameters mismatch | Assets > auto-update menu > Advanced | not documented | missing | - | - |
| Log button (opens Log > Assets tab, warning badge on issues) | Assets > auto-update menu, top right | - | missing | - | - |
| Disable auto-update option in the old-project warning | Dialog when opening an old project | - | missing | - | - |

## Autosave plugin configuration (JavaScript > autosave > Configure)

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Autosave interval in minutes | Autosave configure window | not documented | partial: Preferences > Autosave > Every, 0 (off) to 30 min | crates/ui/src/recovery.rs:335 | - |
| Number of autosave files | Autosave configure window | not documented | partial: Keep, 1 to 10 copies | crates/ui/src/recovery.rs:338 | - |
| Snooze interval in minutes | Autosave configure window | not documented | missing | - | - |
| Warning time before save in seconds | Autosave configure window | not documented | missing | - | - |
| Always save in the following directory (override path) | Autosave configure window | off; user Documents folder | missing | - | - |
| Backups beside the saved project file | Autosave behaviour | default location | partial: copies always go to the app recovery folder | crates/ui/src/recovery.rs:343 | - |
| Backups of untitled projects in Documents/.../autosave | Autosave behaviour | default location | partial: app recovery folder | crates/ui/src/recovery.rs:343 | - |
| Timer pauses during computation, export, saving, or while configuring | Autosave behaviour | always | missing | - | - |

## Project settings: colour management

Layout notes: this block sits at the bottom of both the New project and Project configuration dialogs. The first dropdown picks the engine (Legacy, OpenColorIO, Adobe ACE) and the rest of the block changes with it. SubCraft has no colour-management settings; it always works in linear with sRGB display (the Legacy behaviour).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Color management (engine) | Project > Color management > Configuration | Legacy; also OpenColorIO, Adobe ACE | missing | - | - |
| OpenColorIO configuration | OCIO > Configuration | Substance; also ACES 1.0.3, ACES 1.2, Custom | missing | - | - |
| Configuration file (Custom only) | OCIO > Configuration | empty | missing | - | - |
| Working color space | OCIO > Color settings | from the configuration | missing | - | - |
| Standard sRGB color space | OCIO > Color settings | from the configuration | missing | - | - |
| Import default: 8 bit images | OCIO > Bitmap import color space defaults | from the configuration | missing | - | - |
| Import default: 16 bit images | OCIO > Bitmap import color space defaults | from the configuration | missing | - | - |
| Import default: Floating point images | OCIO > Bitmap import color space defaults | from the configuration | missing | - | - |
| Auto detect color spaces | OCIO > Bitmap import color space defaults | Parse file name; also Disabled, Use config files rules | missing | - | - |
| Material color space default | OCIO > Substance material | from the configuration | missing | - | - |
| Export: 8 bit images | OCIO > Export color spaces | from the configuration | missing | - | - |
| Export: 16 bit images | OCIO > Export color spaces | from the configuration | missing | - | - |
| Export: Floating point images | OCIO > Export color spaces | from the configuration | missing | - | - |
| OCIO role substance_3d_painter_standard_srgb | OCIO config file | - | missing | - | - |
| OCIO role substance_3d_painter_bitmap_import_8bit | OCIO config file | - | missing | - | - |
| OCIO role substance_3d_painter_bitmap_import_16bit | OCIO config file | - | missing | - | - |
| OCIO role substance_3d_painter_bitmap_import_floating | OCIO config file | - | missing | - | - |
| OCIO role substance_3d_painter_substance_material | OCIO config file | - | missing | - | - |
| OCIO role substance_3d_painter_bitmap_export_8bit | OCIO config file | - | missing | - | - |
| OCIO role substance_3d_painter_bitmap_export_16bit | OCIO config file | - | missing | - | - |
| OCIO role substance_3d_painter_bitmap_export_floating | OCIO config file | - | missing | - | - |
| Use a preset file | ACE > Configuration | off | n/a | - | - |
| Preset file (JSON path) | ACE > Configuration | empty | n/a | - | - |
| Working color space | ACE > Color settings | Linear sRGB IEC61966-2.1; also ACEScg, Linear Adobe RGB (1998) | n/a | - | - |
| Rendering intent | ACE > Color settings | Saturation; also Perceptual, Relative chromatic, Absolute chromatic | n/a | - | - |
| Import default: 8 bit images | ACE > Bitmap import color space defaults | not documented | n/a | - | - |
| Import default: 16 bit images | ACE > Bitmap import color space defaults | not documented | n/a | - | - |
| Import default: Floating point images | ACE > Bitmap import color space defaults | not documented | n/a | - | - |
| Use embedded ICC profiles when available (recommended) | ACE > Bitmap import color space defaults | not documented | n/a | - | - |
| Material color space default | ACE > Substance material | not documented | n/a | - | - |
| Export: 8 bit images | ACE > Export color space | not documented | n/a | - | - |
| Export: 16 bit images | ACE > Export color space | not documented | n/a | - | - |
| Export: Floating point images | ACE > Export color space | not documented | n/a | - | - |
| Settings greyed out when an environment variable or preset file drives them | Project > Color management | rule | missing | - | - |
| Display transform on/off button | Top of 2D and 3D views | on | missing | - | - |
| Display transform dropdown | Top of 2D and 3D views | from the configuration; not saved with the project | missing | - | - |
| Display transform off automatically for data channels in solo view | Viewport behaviour | always | missing | - | - |
| $colorSpace file-name keyword | Export textures window | - | missing | - | - |
| Per-resource color space override | Properties > resource > color space | project default | missing | - | - |
| Override environment map color space | Display settings | off | missing | - | - |
| Colour picker edits in the selected Display | Colour picker | when colour management is on | missing | - | #141 |
| Colour-managed channel: Base color | Fixed channel rule | managed | missing | - | - |
| Colour-managed channel: Coat color | Fixed channel rule | managed | missing | - | - |
| Colour-managed channel: Diffuse | Fixed channel rule | managed | missing | - | - |
| Colour-managed channel: Scattering color | Fixed channel rule | managed | missing | - | - |
| Colour-managed channel: Sheen color | Fixed channel rule | managed | missing | - | - |
| Colour-managed channel: Specular | Fixed channel rule | managed | missing | - | - |
| Colour-managed channel: Specular edge color | Fixed channel rule | managed | missing | - | - |
| Colour-managed channel: Transmissive | Fixed channel rule | managed | missing | - | - |
| All other built-in channels are data (never converted) | Fixed channel rule | not managed | missing | - | - |
| User channels 0-15 colour-managed per Texture Set setting | Texture Set settings | not managed | missing | - | - |

## Command line options

Layout notes: used as `"Adobe Substance 3D Painter.exe" --command [option]`. SubCraft has its own, different option set in `apps/subcraft/src/main.rs`.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| --help | Command line | - | done | apps/subcraft/src/main.rs:185 | - |
| -h | Command line | - | done | apps/subcraft/src/main.rs:185 | - |
| -? | Command line | - | missing | - | - |
| --version | Command line | - | missing | - | - |
| -v | Command line | - | missing | - | - |
| --mesh (new project from a mesh) | Command line | mesh path | partial: a bare path argument, or --new-project to open the dialog | apps/subcraft/src/main.rs:189 | - |
| --mesh with a project path (update the mesh of that project) | Command line | mesh path + .spp path | partial: --reload-mesh opens the Reload Mesh window | apps/subcraft/src/main.rs:166 | - |
| --mesh-map (repeatable baked map files) | Command line | file named TextureSetName_MapName | missing | - | #137 |
| --split-by-udim | Command line | off | missing | - | - |
| --export-path | Command line | folder | missing | - | - |
| --vram-budget | Command line | megabytes | missing | - | - |
| --disable-version-checking | Command line | off | missing | - | - |
| --enable-remote-scripting | Command line | off | missing | - | - |

## Environment variables

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| SUBSTANCE_PAINTER_LICENSE | Environment | path to a licence file | n/a | - | - |
| ALLEGO_LICENSE_IDLE_DELAY | Environment | 7200 seconds | n/a | - | - |
| ALG_PAINTER_SKIP_CHECK_FOR_UPDATES | Environment | 0; 0 or 1 | partial: Preferences tick box only | crates/ui/src/recovery.rs:293 | - |
| SUBSTANCE_PAINTER_SVT_HARDWARE_ACCELERATION | Environment | 0 or 1 | missing | - | - |
| SUBSTANCE_PAINTER_TEMP_LOCATION | Environment | folder path | missing | - | - |
| SUBSTANCE_PAINTER_PREVIEWS_MEMORY_BUDGET | Environment | 500 MB | missing | - | - |
| SUBSTANCE_PAINTER_PLUGINS_PATH | Environment | folder path | missing | - | - |
| PYTHONPATH | Environment | module paths | missing | - | - |
| OCIO | Environment | path to config.ocio; wins over PAINTER_ACE_CONFIG | missing | - | - |
| PAINTER_ACE_CONFIG | Environment | path to a JSON preset | n/a | - | - |
| SUBSTANCE_DISABLE_SPECIFIC_FEATURES | Environment | unset | missing | - | - |
| ALG_PAINTER_DEBUG_FPS | Environment | unset | missing | - | - |
| SUBSTANCE_PAINTER_VRAM_BUDGET | Environment | megabytes | missing | - | - |

## Preference and data storage

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Preferences store on Windows (registry key HKCU\Software\Adobe\Adobe Substance 3D Painter) | Operating system | - | partial: one JSON string in the app's own storage, not the registry | crates/ui/src/recovery.rs:16 | - |
| Preferences store on macOS (plist in ~/Library/Preferences) | Operating system | - | partial: same app storage file | crates/ui/src/recovery.rs:16 | - |
| Preferences store on Linux (.conf in ~/.config/Adobe) | Operating system | - | partial: same app storage file | crates/ui/src/recovery.rs:16 | - |
| Preferences hold shortcuts, asset paths and interface layout | Preferences store | - | partial: layout and key-set choice saved; no custom shortcuts or asset paths | crates/ui/src/app.rs:804 | - |
| Application data folder (thumbnails, log file) | Local / roaming app data | - | partial: log files and recovery copies in the app data folder | crates/app/src/diagnostics.rs:238 | - |
| User assets folder in Documents/Adobe/Adobe Substance 3D Painter | Documents | - | partial: one user library root | crates/ui/src/app.rs:601 | - |
| Shelf pathInfos entry: name | Preferences > Shelf | - | missing | - | - |
| Shelf pathInfos entry: path | Preferences > Shelf | - | missing | - | - |
| Shelf pathInfos entry: disabled | Preferences > Shelf | false | missing | - | - |
| Shelf pathInfos size counter | Preferences > Shelf | number of paths | missing | - | - |
| writableShelf (default save location) | Preferences > Shelf | your_assets | missing | - | - |
| Saved searches in Shelf.ini ([filters] size, Nname, Nquery) | Documents assets folder | - | missing | - | - |
| .ignore_assets_pt exclusion file (gitignore-like patterns, case sensitive) | Any resource path root | absent | missing | - | - |
| Legacy shelf content migration by copying sub-folders | Documents folders | manual | n/a | - | - |
| Silent installer flags /SILENT /NCRC /DIR | Standalone installer | - | missing | - | - |
| Version readable from the executable file properties | Windows executable | - | missing | - | - |
| Crash and bug report endpoints to allow in firewalls | Network | port 80 | n/a | - | - |

## Input devices

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Pen pressure through Windows Ink | Windows tablet driver setting | required since 2019.3 | partial: pen and touch contacts mapped through the windowing layer; no Windows Ink specific path confirmed | crates/ui/src/pen_input.rs:1 | - |
| SpaceMouse navigation of the 3D view (rotate, pan, zoom) | 3Dconnexion driver | plug and play since 7.4.2 | missing | - | - |
| SpaceMouse navigation modes (Object, Camera, Target Camera, Helicopter) | 3Dconnexion settings | Object | missing | - | - |
| SpaceMouse Lock Horizon | 3Dconnexion settings | locked | missing | - | - |
| SpaceMouse rotation-centre pivot display (Auto, Always Show, Show on Motion, Hide) | 3Dconnexion settings | Auto on; Show on Motion | missing | - | - |
| SpaceMouse button commands / radial menu | 3Dconnexion settings | driver defaults | missing | - | - |
| OpenGL 3.3 context needed over Remote Desktop | Windows RDP | - | n/a | - | - |
