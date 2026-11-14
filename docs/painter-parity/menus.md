# Painter parity: Main menu bar

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/main-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/file-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/edit-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/mode-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/window-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/viewport-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/plugins-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/help-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/interface , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/project-configuration , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/getting-started/project-creation , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/send-to , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/plugins/plugins , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/plugins/autosave , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/plugins/resources-updater , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/plugins/zbrush-bridge , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/miscellaneous/update-checker , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes
Checked against: origin/main 297dcb3 (2026-11-11)

In this file the "Default / range" column holds the entry's default shortcut (Windows) or the field's default value.

## Menu bar

Layout notes: one row of text menus above the toolbars, in this order: File, Edit, Mode, Window, Viewport, JavaScript, Python, Help. The two plugin menus are what the docs call the "Plugins menu". SubCraft's bar is File, Edit, Layer, View, Help inside its own header (brand mark left, undo/redo/save/Bake/Export buttons right).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| File | Menu bar, 1st | - | done | crates/ui/src/app.rs:1104 | - |
| Edit | Menu bar, 2nd | - | partial: Undo, Redo, Bake mesh maps, Project configuration, Settings plus a Layers submenu and Auto UV unwrap | crates/ui/src/app.rs (menu) | #164 |
| Mode | Menu bar, 3rd | - | done | crates/ui/src/app.rs (menu) | #164 |
| Window | Menu bar, 4th | - | done | crates/ui/src/app.rs (menu) | #164 |
| Viewport | Menu bar, 5th | - | done | crates/ui/src/app.rs (menu) | #164 |
| JavaScript | Menu bar, 6th (plugins) | - | partial: one empty Plugins menu; no scripting API | crates/ui/src/app.rs (menu) | #164 |
| Python | Menu bar, 7th (plugins) | - | partial: one empty Plugins menu; no scripting API | crates/ui/src/app.rs (menu) | #164 |
| Help | Menu bar, last | - | done | crates/ui/src/app.rs:1276 | - |

## File menu

Layout notes: project lifetime first (New to Close), then the save family, Clean, then import/export, Send to, and Quit last. Save is incremental by default; Clean and Save and compact rewrite the whole file.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| New | File | Ctrl+N | done | crates/ui/src/app.rs:1110 | - |
| Open | File | Ctrl+O | done | crates/ui/src/app.rs:1122 | - |
| Recent files (submenu, one entry per recent project) | File | not documented | done | crates/ui/src/app.rs (menu) | #164 |
| Open sample (submenu, one entry per bundled sample project) | File | not documented | partial: one built-in sample, a single entry | crates/ui/src/app.rs (menu) | #164 |
| Close | File | Ctrl+F4 | partial: returns to the sample model (there is no empty state) | crates/ui/src/app.rs (menu) | #164 |
| Save | File | Ctrl+S | done | crates/ui/src/app.rs:1127 | - |
| Save and compact | File | not documented | missing | - | - |
| Save as | File | no default key (editable) | partial: default key differs (Ctrl+Shift+S) | crates/ui/src/app.rs:1131 | - |
| Save as copy | File | not documented | missing | - | - |
| Save as template | File | not documented | missing | - | - |
| Clean | File | not documented | missing | - | - |
| Import resources | File | not documented | partial: a submenu with texture set / smart material entries, no single import window | crates/ui/src/app.rs (menu) | #164 |
| Export mesh | File | not documented | partial: scene-file export sits inside the Export textures window, no File entry | crates/ui/src/dialogs.rs:271 | - |
| Export textures | File | Ctrl+Shift+E | done | crates/ui/src/app.rs:1136 | #124 |
| Send to (submenu) | File | not documented | partial: submenu with one entry (PhotoCraft) | crates/ui/src/app.rs (menu) | #164 |
| Send to > Photoshop | File > Send to | not documented | partial: File > Send to > Edit in PhotoCraft (different target app) | crates/ui/src/app.rs (menu) | #164 |
| Send to > Substance 3D Stager | File > Send to | not documented | n/a | - | - |
| Send to > After Effects | File > Send to | not documented | n/a | - | - |
| Quit | File | Alt+F4 | done | crates/ui/src/app.rs:1159 | - |
| Unsaved-changes prompt on Quit | File > Quit | shown when project is modified | done | crates/ui/src/dialogs.rs:125 | - |
| Incremental save (only changed data rewritten) | File > Save behaviour | default save method | missing | - | - |
| Full / compact save after Clean or Save and compact | File > Save behaviour | not documented | missing | - | - |

## New project dialog (File > New)

Layout notes: a single dialog. Top block: File, Template, Resolution. When a USD is picked, a file-type block appears (Scope and variants, Subdivision level, Frame). Then Advanced settings (normal format, tangent space), UV Tile settings, Import settings, the Import baked maps list with its Add button, Physical size, and Colour management at the bottom. UV Tile settings cannot be changed after creation.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| File (Select button) | New project > basic | none; any supported mesh format | done | crates/ui/src/new_project.rs:168 | - |
| Template | New project > basic | list of .spt templates from the Assets folder | partial: five built-in channel templates, no .spt files, no display/bake/shader/environment content | crates/ui/src/new_project.rs:253 | - |
| Resolution | New project > basic | up to 4096 in app (8192 at export) | done | crates/ui/src/new_project.rs:266 | - |
| Scope and variants (Change... button, USD only) | New project > file type-specific | Root | missing | - | - |
| Subdivision level (USD only) | New project > file type-specific | not documented | missing | - | - |
| Frame (USD with animation only) | New project > file type-specific | not documented | missing | - | - |
| Normal Map Format | New project > Advanced | DirectX or OpenGL | done | crates/ui/src/new_project.rs:286 | - |
| Compute Tangent Space per Fragment | New project > Advanced | not documented | missing | - | - |
| Use UV Tile workflow | New project > UV Tile settings | not documented | partial: one checkbox that always splits tiles into texture sets | crates/ui/src/new_project.rs:281 | - |
| Preserve UV Tile layout per materials and enable painting across tiles | New project > UV Tile settings | not documented | missing | - | - |
| Convert UV Tiles into individual Texture Sets (legacy) | New project > UV Tile settings | not documented | done | crates/ui/src/new_project.rs:281 | - |
| Import Cameras | New project > Import settings | not documented | partial: cameras are imported later from the 3D view camera menu (glTF or JSON) | crates/ui/src/view_extras.rs:186 | - |
| Auto-unwrap | New project > Import settings | not documented | partial: separate File > Auto UV unwrap after creation | crates/ui/src/app.rs:1118 | - |
| Auto-unwrap Options button | New project > Import settings | not documented | partial: options are in the separate Auto UV window | crates/ui/src/auto_uv.rs | - |
| Import baked maps: Add button | New project > Import baked maps | empty list | missing | - | #137 |
| Baked map name: TextureSetName_ambient_occlusion | New project > Import baked maps | naming rule | missing | - | #137 |
| Baked map name: TextureSetName_curvature | New project > Import baked maps | naming rule | missing | - | #137 |
| Baked map name: TextureSetName_normal_base | New project > Import baked maps | naming rule | missing | - | #137 |
| Baked map name: TextureSetName_world_space_normals | New project > Import baked maps | naming rule | missing | - | #137 |
| Baked map name: TextureSetName_id | New project > Import baked maps | naming rule | missing | - | #137 |
| Baked map name: TextureSetName_position | New project > Import baked maps | naming rule | missing | - | #137 |
| Baked map name: TextureSetName_thickness | New project > Import baked maps | naming rule | missing | - | #137 |
| Use mesh file's internal unit scale | New project > Physical size | not documented | missing | - | - |
| Custom unit scale | New project > Physical size | not documented | missing | - | - |
| Switch fill layer scaling to Physical size when assigning materials | New project > Physical size | not documented | missing | - | - |
| Color management section | New project > Color management | Legacy | missing | - | - |
| OK | New project footer | (unverified) | done | crates/ui/src/new_project.rs:127 | - |
| Cancel | New project footer | (unverified) | done | crates/ui/src/new_project.rs:130 | - |
| One Texture Set per material found on the mesh | New project behaviour | always | done | crates/ui/src/new_project.rs:273 | - |

## Edit menu

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Undo | Edit | Ctrl+Z | done | crates/ui/src/app.rs:1170 | - |
| Redo | Edit | Ctrl+Y | done | crates/ui/src/app.rs:1173 | - |
| Project configuration | Edit | not documented | done: Edit > Project configuration… opens the Project configuration window (3D mesh, UV Tiles, Import settings, OK / Cancel) | crates/ui/src/app.rs (menu), crates/ui/src/mesh_reload.rs | - |
| Reimport mesh | Edit | Ctrl+Shift+R (release notes) | done: Edit > Reimport mesh… and Ctrl+Shift+R (both key sets, rebindable) open Project configuration with Reimport mesh ticked and the model file checked | crates/app/src/actions.rs (ReimportMesh), crates/ui/src/mesh_reload.rs (open_reimport) | - |
| Settings | Edit | not documented | partial: Edit > Settings, single page | crates/ui/src/app.rs (menu) | #164 |
| Bake Mesh maps | Edit | Ctrl+Shift+B | done | crates/ui/src/app.rs (menu) | #164 |

## Project configuration dialog (Edit > Project configuration)

Layout notes: same groups as the New project dialog, top to bottom: 3D mesh, Project settings, file type-specific (USD), UV Tiles (read only), Import settings, Physical size, Colour management. Changes apply on OK.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| 3D mesh path with Select button | Project configuration > 3D mesh | current mesh file | done: path box (the file the mesh came from) and Select… | crates/ui/src/mesh_reload.rs | - |
| Reimport mesh | Project configuration > 3D mesh | off; ticks itself when another file is selected | done: off by default, ticks itself when another file is typed or selected; ticking it checks the model and lists how its materials match the texture sets | crates/ui/src/mesh_reload.rs (is_other_file) | - |
| Normal Map Format | Project configuration > Project settings | value from creation | partial: can only be overridden in the Export window, not for the viewport | crates/ui/src/dialogs.rs:362 | - |
| Compute Tangent Space Per Fragment | Project configuration > Project settings | value from creation | missing | - | - |
| Scope and variants (Change...) | Project configuration > file type-specific | Root | missing | - | - |
| Subdivision level | Project configuration > file type-specific | not documented | missing | - | - |
| Frame | Project configuration > file type-specific | not documented | missing | - | - |
| UV Tiles settings (read-only view) | Project configuration > UV Tiles | value from creation | partial: read-only line: UV tile workflow on / off and the tiles used; no per-material layout option | crates/ui/src/mesh_reload.rs (uv_tile_summary) | - |
| Import Cameras | Project configuration > Import settings | not documented | done: replaces the project's cameras with the model file's when reimporting | crates/ui/src/mesh_reload.rs | - |
| Preserve strokes positions on mesh | Project configuration > Import settings | recommended on | done: on = paint moves through the surface onto the new UVs (Reproject), off = texels stay (Keep UVs); the check suggests one | crates/ui/src/mesh_reload.rs (preserve_strokes) | - |
| Auto-Unwrap | Project configuration > Import settings | not documented | partial: separate File > Auto UV unwrap | crates/ui/src/app.rs:1118 | - |
| Auto-Unwrap Option button | Project configuration > Import settings | not documented | partial: options in the Auto UV window | crates/ui/src/auto_uv.rs | - |
| Use mesh file's internal unit scale | Project configuration > Physical size | not documented | missing | - | - |
| Custom unit scale | Project configuration > Physical size | not documented | missing | - | - |
| Switch fill layer scaling to Physical size when assigning materials | Project configuration > Physical size | not documented | missing | - | - |
| Color management settings | Project configuration > Color management | Legacy | missing | - | - |
| OK (applies, reimports if ticked) | Project configuration footer | - | done: reimports when ticked (waits for the model check), otherwise closes | crates/ui/src/mesh_reload.rs (ok_action) | - |
| Cancel | Project configuration footer | (unverified) | done: closes; during a reimport it stops it | crates/ui/src/mesh_reload.rs | - |

## Mode menu

Layout notes: three exclusive entries; each mode has its own interface. The same switches exist as buttons at the top right of the viewport.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Bake mesh maps | Mode | F8 | partial: Mode > Bake mesh maps (F8) opens the Bake window over the workspace, no separate interface | crates/ui/src/app.rs (menu) | #164 |
| Painting | Mode | F9 | partial: Mode > Painting (F9) closes the Bake window and leaves Rendering mode | crates/ui/src/app.rs (menu) | #164 |
| Rendering (Iray) | Mode | F10 | partial: Mode > Rendering (F10), own path tracer | crates/ui/src/app.rs (menu) | #164 |

## Window menu

Layout notes: two submenus with a tick per item (Views for dockable windows, Toolbars), then Hide UI and Reset UI. The Views list below is the set of windows named in the interface page; its exact order in the menu is not documented.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Views (submenu with tick boxes) | Window | - | partial: submenu brings a panel to the front; panels cannot be closed | crates/ui/src/app.rs (menu) | #164 |
| Views > Assets | Window > Views | shown (unverified) | partial: entry brings the panel to the front; it cannot be hidden | crates/ui/src/app.rs (menu) | #164 |
| Views > Display settings | Window > Views | (unverified) | partial: Render quality window from the View menu | crates/ui/src/app.rs:1254 | - |
| Views > History | Window > Views | (unverified) | partial: entry brings the panel to the front; it cannot be hidden | crates/ui/src/app.rs (menu) | #164 |
| Views > Layers | Window > Views | shown (unverified) | partial: entry brings the panel to the front; it cannot be hidden | crates/ui/src/app.rs (menu) | #164 |
| Views > Log | Window > Views | (unverified) | done: opens the Log panel (closed by default) and brings it to the front | crates/ui/src/app.rs (menu), panels/log.rs | - |
| Views > Properties | Window > Views | shown (unverified) | partial: entry brings the panel to the front; it cannot be hidden | crates/ui/src/app.rs (menu) | #164 |
| Views > Shader settings | Window > Views | (unverified) | missing | - | - |
| Views > Texture Set List | Window > Views | shown (unverified) | partial: entry brings the panel to the front; it cannot be hidden | crates/ui/src/app.rs (menu) | #164 |
| Views > Texture Set Settings | Window > Views | shown (unverified) | partial: entry brings the panel to the front; it cannot be hidden | crates/ui/src/app.rs (menu) | #164 |
| Toolbars (submenu with tick boxes) | Window | - | partial: ticks for the Tools rail and the status bar | crates/ui/src/app.rs (menu) | #164 |
| Toolbars > Docks | Window > Toolbars | shown | missing | - | - |
| Toolbars > Plugins | Window > Toolbars | shown | missing | - | - |
| Toolbars > Tools | Window > Toolbars | shown | done | crates/ui/src/app.rs (menu) | #164 |
| Hide UI | Window | Tab | done | crates/ui/src/app.rs (menu) | #164 |
| Reset UI | Window | not documented | done | crates/ui/src/app.rs (menu) | #164 |

## Viewport menu

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Display material | Viewport | M | done | crates/ui/src/app.rs (menu) | #164 |
| Display next channel | Viewport | C | done | crates/ui/src/app.rs (menu) | #164 |
| Display previous channel | Viewport | Shift+C | done | crates/ui/src/app.rs (menu) | #164 |
| Display next mesh map | Viewport | B | done | crates/ui/src/app.rs (menu) | #164 |
| Display previous mesh map | Viewport | Shift+B | done | crates/ui/src/app.rs (menu) | #164 |
| Show entire mesh | Viewport | F | done | crates/ui/src/app.rs (menu) | #164 |
| Enabled quick mask | Viewport | not documented | missing | - | - |
| Edit quick mask | Viewport | U | missing | - | - |
| Invert quick mask | Viewport | I | missing | - | - |

## Plugins menus (JavaScript, Python)

Layout notes: every plugin found at start-up gets a submenu named after it in the menu of its API. The submenu always has enable/disable and reload; configure and about appear when the plugin supports them. Autosave and Resources Updater ship by default; the ZBrush bridge is a Python plugin.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| One submenu per discovered plugin | JavaScript / Python | built at start-up | missing | - | - |
| Plugin > disable / enable | JavaScript / Python > plugin | not documented | missing | - | - |
| Plugin > reload | JavaScript / Python > plugin | - | missing | - | - |
| Plugin > configure | JavaScript / Python > plugin | only if supported | missing | - | - |
| Plugin > about | JavaScript / Python > plugin | only if supported | missing | - | - |
| Autosave plugin (backup copies beside the project) | JavaScript > autosave | enabled (unverified) | partial: built-in autosave to a recovery folder, not a plugin | crates/ui/src/recovery.rs:152 | - |
| Autosave > Disable | JavaScript > autosave | - | partial: interval slider set to off | crates/ui/src/recovery.rs:335 | - |
| Autosave > Configure | JavaScript > autosave | - | partial: Preferences > Autosave | crates/ui/src/recovery.rs:329 | - |
| Autosave snooze button | Plugins toolbar | appears near the end of the timer | missing | - | - |
| Resources Updater plugin window | JavaScript > resources-updater | - | missing | - | - |
| Resources Updater: outdated resources shown in red | Resources Updater window | - | missing | - | - |
| Resources Updater: replace a project resource with a shelf one | Resources Updater window | - | missing | - | - |
| zbrush_painter_plugin tick box | Python | off (unverified) | missing | - | - |

## Help menu

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Tutorials | Help | - | partial: in-app Quick start guide instead of a web link | crates/ui/src/app.rs:1279 | - |
| Release notes | Help | - | missing | - | - |
| Documentation | Help | - | missing | - | - |
| Shortcuts list | Help | - | done | crates/ui/src/app.rs:1285 | - |
| Scripting documentation | Help | - | missing | - | - |
| Forums | Help | - | missing | - | - |
| Report a bug | Help | - | done | crates/ui/src/app.rs:1290 | - |
| Show log | Help | - | partial: opens the log folder, not the file in a text editor | crates/ui/src/app.rs:1295 | - |
| Export log | Help | - | done: Help > Export log… saves a copy of this session's log file | crates/ui/src/panels/log.rs (export_log) | - |
| Give feedback | Help | - | missing | - | - |
| Manage my account | Help | - | n/a | - | - |
| Sign out | Help | - | n/a | - | - |
| Welcome screen | Help | - | done | crates/ui/src/app.rs:1278 | - |
| What's new | Help | - | done | crates/ui/src/app.rs:1280 | - |
| Home screen | Help | - | partial: the Welcome screen doubles as home | crates/ui/src/welcome.rs:25 | - |
| Check for updates | Help | - | done | crates/ui/src/app.rs:1284 | - |
| About Substance 3D Painter | Help | - | done | crates/ui/src/app.rs:1281 | - |
| Partners | Help | - | missing | - | - |
