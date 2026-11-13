# Painter parity: Texture Set List and Texture Set Settings

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/texture-set/texture-set , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/texture-set/texture-set-list , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/texture-set/texture-set-settings , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/texture-set/texture-set-reassignment , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-tiles/uv-tiles , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-tiles/image-sequence , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-reprojection , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/automatic-uv-unwrapping , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/sparse-virtual-textures , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/project-configuration , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/getting-started/project-creation , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/color-management/color-management (channel list) , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/content/creating-custom-effects/mesh-map (mesh map slots) , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/general-preferences , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes
Checked against: origin/main 297dcb3 (2026-11-11)

## Texture Set List: header buttons

Layout notes: the panel has a small header with a visibility menu button, a focus (isolate) button and a settings (gear) menu; below it one row per texture set. Each row reads left to right: eye, name (description under it), resolution, shader instance button. In UV Tile projects a row expands to show its tiles. In Baking mode every row (and tile) gains a checkbox. SubCraft's row is eye, name, size dropdown, static shader label; it also has a "Resolution" context submenu Painter does not have.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Open Menu button (visibility menu) | Texture Set List header | not documented | done | crates/ui/src/panels/texture_sets.rs (list_header) | - |
| Show All | Texture Set List > visibility menu | not documented | done | crates/app/src/session.rs (Command::SetTextureSetsVisibility) | - |
| Hide All | Texture Set List > visibility menu | not documented | done | crates/app/src/session.rs (Command::SetTextureSetsVisibility) | - |
| Invert Show/Hide | Texture Set List > visibility menu | not documented | done | crates/app/src/session.rs (Command::SetTextureSetsVisibility) | - |
| Focus Mode (isolate active set, click again to leave) | Texture Set List header | off | done | crates/app/src/session.rs (Command::SetTextureSetFocus) | - |
| Settings button (gear menu) | Texture Set List header | not documented | partial: description modes only; no Import Shader Parameters or Reassign | crates/ui/src/panels/texture_sets.rs (list_header) | - |
| Hide Empty Descriptions | Texture Set List > Settings | default choice | done | crates/ui/src/panels/texture_sets.rs (DescriptionDisplay) | - |
| Hide All Descriptions | Texture Set List > Settings | off | done | crates/ui/src/panels/texture_sets.rs (DescriptionDisplay) | - |
| Show All Descriptions | Texture Set List > Settings | off | done | crates/ui/src/panels/texture_sets.rs (DescriptionDisplay) | - |
| Import Shader Parameters (json) | Texture Set List > Settings | not documented | missing | - | - |
| Reassign Texture Sets | Texture Set List > Settings | not documented | missing | - | - |

## Texture Set List: rows

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| One row per mesh material ID | Texture Set List | not documented | done | crates/ui/src/panels/texture_sets.rs:36 | - |
| Click row to select (layer stack, shader settings and settings panel follow) | Texture Set List row | one set selected at a time | done | crates/ui/src/panels/texture_sets.rs:78 | - |
| Visibility eye per row | Texture Set List row | visible | done | crates/ui/src/panels/texture_sets.rs:46 | - |
| Texture set name | Texture Set List row | mesh material name | done | crates/ui/src/panels/texture_sets.rs:61 | - |
| Double-click name to rename | Texture Set List row | not documented | done | crates/ui/src/panels/texture_sets.rs:79 | - |
| Description line under the name (editable in place) | Texture Set List row | hidden when empty | done | crates/ui/src/panels/texture_sets.rs (set_rows) | - |
| Resolution quick dropdown | Texture Set List row | 128 to 4096 (unverified) | done | crates/ui/src/panels/texture_sets.rs:64 | - |
| Shader instance button (pick an instance) | Texture Set List row, right side | all sets share one instance | partial: static "Main shader" label, no menu | crates/ui/src/panels/texture_sets.rs:76 | - |
| New shader instance | Texture Set List row > shader button menu | not documented | missing | - | - |
| Disabled state (set no longer attached to a mesh material) | Texture Set List row | not documented | partial: unmatched sets are kept after a reload but the list does not mark them | crates/ui/src/mesh_reload.rs:146 | - |
| Sub-stacks listed under the set (material layering) | Texture Set List row | not documented | missing | - | - |
| Expand / collapse UV tile list by clicking | Texture Set List row (UV Tile projects) | collapsed (unverified) | missing | - | - |
| UV tile entry (UDIM number) | Texture Set List > tile list | not documented | missing | - | - |
| UV tile custom name (used at export) | Texture Set List > tile list | empty | missing | - | - |
| UV tile description | Texture Set List > tile list | empty | missing | - | - |
| UV tile resolution quick dropdown | Texture Set List > tile list | set resolution | missing | - | - |
| Hide other sets while painting ("Only display the selected material when painting") | Settings > General | on | missing | - | - |

## Texture Set List: context menu

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Show/Hide texture set | Right-click on a set name | not documented | done | crates/ui/src/panels/texture_sets.rs:82 | - |
| Edit name | Right-click on a set name | not documented | done | crates/ui/src/panels/texture_sets.rs:81 | - |
| Reset name to original name | Right-click on a set name | not documented | done | crates/app/src/session.rs (Command::ResetTextureSetName) | - |
| Edit Description | Right-click on a set name | not documented | done | crates/ui/src/panels/texture_sets.rs (set_rows) | - |

## Texture Set List: shortcuts

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Texture Set isolate | Shortcut | Alt+Q | done: hides the other texture sets; again shows them | crates/app/src/session.rs (Command::IsolateTextureSet) | - |
| Texture set selection in the viewport | Shortcut | Ctrl+Alt+Right mouse | missing | - | - |

## Texture Set Reassignment window

Layout notes: three columns of buttons (disabled sets, project sets, mesh materials) with Undo / Redo / Apply / Cancel at the bottom. SubCraft only matches sets to materials by name inside File > Reload mesh.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Disabled Texture Sets list | Reassign Texture Sets window | not documented | missing | - | - |
| Project Texture Sets list | Reassign Texture Sets window | not documented | missing | - | - |
| Mesh Materials list | Reassign Texture Sets window | not documented | partial: read-only match report in the reload window | crates/ui/src/mesh_reload.rs:136 | - |
| Drag and drop a set onto another slot to reassign | Reassign Texture Sets window | not documented | missing | - | - |
| Swap a disabled set with a project set to restore it | Reassign Texture Sets window | not documented | missing | - | - |
| Cross button: mark a disabled set for deletion | Reassign Texture Sets window > Disabled list | not documented | missing | - | - |
| Undo | Reassign Texture Sets window | not documented | missing | - | - |
| Redo | Reassign Texture Sets window | not documented | missing | - | - |
| Apply | Reassign Texture Sets window | not documented | missing | - | - |
| Cancel | Reassign Texture Sets window | not documented | missing | - | - |

## Texture Set Settings: General properties

Layout notes: one scrolling column: General properties (Name, Description, Size with two dropdowns and a lock between them, Shader instance), then Channels (header with add button, one row per channel with name, format label, settings and remove buttons, then the mixing dropdowns), then Mesh maps (Bake Mesh Maps button and one slot per map). SubCraft follows this order and adds an "Apply size to all" button.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Name | Texture Set Settings > General properties | mesh material name | done | crates/ui/src/panels/texture_sets.rs:172 | - |
| Description | Texture Set Settings > General properties | empty | done | crates/ui/src/panels/texture_sets.rs (general) | - |
| Size (width) | Texture Set Settings > General properties | up to 4096 in app; list 128 to 4096 (unverified) | done | crates/ui/src/panels/texture_sets.rs:188 | - |
| Size lock button (unlock for non-square) | Texture Set Settings > General properties | locked | partial: shown but always locked | crates/ui/src/panels/texture_sets.rs:191 | - |
| Size (height) | Texture Set Settings > General properties | same as width | missing | - | - |
| Shader instance | Texture Set Settings > General properties | shared main instance | partial: disabled dropdown, single shader | crates/ui/src/panels/texture_sets.rs:205 | - |

## Texture Set Settings: Channels list

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Add channel button | Texture Set Settings > Channels | not documented | done | crates/ui/src/panels/texture_sets.rs:220 | - |
| Add menu category: Supported channels | Add channel popup | not documented | done | crates/ui/src/panels/texture_sets.rs (add_menu_groups) | - |
| Add menu category: Unsupported channels | Add channel popup | not documented | done | crates/ui/src/panels/texture_sets.rs (add_menu_groups) | - |
| Add menu category: User channels | Add channel popup | not documented | done | crates/ui/src/panels/texture_sets.rs (add_menu_groups) | - |
| Select several channels at once (window) | Add channel | not documented | done: Add channel > Add several channels opens a window of tick boxes; one undo step | crates/ui/src/panels/texture_sets.rs (channels); crates/app/src/command.rs (AddChannels) | - |
| Remove channel (paint data kept, can be re-added) | Texture Set Settings > channel row | not documented | done | crates/ui/src/panels/texture_sets.rs (channels) | - |
| Channel name | Texture Set Settings > channel row | not documented | done | crates/ui/src/panels/texture_sets.rs:231 | - |
| Rename a user channel by double-click | Texture Set Settings > channel row | not documented | done | crates/app/src/session.rs (Command::RenameUserChannel) | - |
| Channel format label | Texture Set Settings > channel row | per channel | done | crates/ui/src/panels/texture_sets.rs:237 | - |
| Channel Settings button | Texture Set Settings > channel row | not documented | partial: format menu only, no Color channel toggle | crates/ui/src/panels/texture_sets.rs (channels) | - |
| Format sRGB8 | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format L8 | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format RGB8 | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format L16 | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format RGB16 | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format L16F | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format RGB16F | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format L32F | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Format RGB32F | Channel settings menu | not documented | partial: menu on the channel gear; L or RGB is fixed per channel | crates/ui/src/panels/texture_sets.rs (CHANNEL_FORMATS) | - |
| Color channel toggle (colour managed; editable on user channels only) | Channel settings menu | off for user channels | missing | - | - |
| Color managed indicator icon | Texture Set Settings > channel row | fixed per channel | done: a mark beside the name of each colour channel, with a tooltip | crates/ui/src/panels/texture_sets.rs (COLOR_MANAGED_ICON) | - |

## Texture Set Settings: channel types

Layout notes: the names below are the channel types Painter offers (from the colour management channel table). SubCraft's list is `ChannelKind` in crates/core/src/channel.rs.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Ambient occlusion | Add channel | data | done | crates/core/src/channel.rs:17 | - |
| Anisotropy angle | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Anisotropy level | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Base color | Add channel | colour managed | done | crates/core/src/channel.rs:12 | - |
| Blending mask | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Coat color | Add channel | colour managed | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Coat normal | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Coat opacity | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Coat roughness | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Coat specular level | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Diffuse | Add channel | colour managed | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Displacement | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Emissive | Add channel | (unverified) | done | crates/core/src/channel.rs:18 | - |
| Glossiness | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Height | Add channel | data | done | crates/core/src/channel.rs:13 | - |
| Ior | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Metallic | Add channel | data | done | crates/core/src/channel.rs:15 | - |
| Normal | Add channel | data | done | crates/core/src/channel.rs:16 | - |
| Opacity | Add channel | data | done | crates/core/src/channel.rs:19 | - |
| Reflection | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Roughness | Add channel | data | done | crates/core/src/channel.rs:14 | - |
| Scattering | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Scattering color | Add channel | colour managed | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Sheen color | Add channel | colour managed | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Sheen opacity | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Sheen roughness | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Specular (colour) | Add channel | colour managed | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Specular edge color | Add channel | colour managed | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Specular level | Add channel | data | done | crates/core/src/channel.rs:20 | - |
| Translucency | Add channel | data | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| Transmissive | Add channel | colour managed | done: in Add channel > Unsupported channels; painted, stored and exported, not read by the viewport shader | crates/core/src/channel.rs (ChannelKind::EXTENDED) | - |
| User0 to User15 (16 user channels) | Add channel > User channels | data unless Color channel is ticked | done | crates/core/src/channel.rs (ChannelKind::USER) | - |

## Texture Set Settings: mixing settings

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Normal mixing | Texture Set Settings > Channels | Combine (default); Replace | partial: applied at export; the viewport always combines | crates/export/src/lib.rs (Dense); crates/ui/src/panels/texture_sets.rs (mixing_rows) | - |
| Height to normal method | Texture Set Settings > Channels | Smooth (Sobel) (default); Sharp | partial: applied at export; the viewport uses the sharp method | crates/export/src/lib.rs (Dense); crates/ui/src/panels/texture_sets.rs (mixing_rows) | - |
| Ambient occlusion mixing | Texture Set Settings > Channels | Multiply (default); Replace | partial: applied at export; the viewport uses the baked map only when there is no AO channel | crates/export/src/lib.rs (Dense); crates/ui/src/panels/texture_sets.rs (mixing_rows) | - |
| UV padding | Texture Set Settings > Channels | 3D Space Neighbor (default); 2D Space Neighbor | partial: setting stored; 3D Space Neighbor not implemented, export pads from 2D neighbours | crates/ui/src/panels/texture_sets.rs (mixing_rows) | - |

## Texture Set Settings: Mesh maps

Layout notes: a "Bake Mesh Maps" button, then one slot per mesh map with a thumbnail, the map name and the resource name. The public settings page stops at the section heading, so slot interactions below are marked unverified. SubCraft lists all nine slots; an empty one shows "Select ... map" and takes an image by click or drop, a filled one has an X to clear it. PR #137 adds an "Import mesh maps..." button and an ID colour list in this section.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Bake Mesh Maps button | Texture Set Settings > Mesh maps | not documented | done | crates/ui/src/panels/texture_sets.rs:246 | - |
| Slot: Normal | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: World space normal | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: ID | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: Ambient occlusion | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: Curvature | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: Position | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: Thickness | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: Height | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: Bent normals | Texture Set Settings > Mesh maps | empty | done | crates/ui/src/panels/texture_sets.rs (MESH_MAP_SLOTS, map_slot) | #137 |
| Slot: Opacity | Texture Set Settings > Mesh maps | empty | missing (the Opacity *channel* is done, see Channels) | - | - |
| Click a slot to select an image resource | Mesh map slot | (unverified) | partial: opens a file dialog, not a resource picker | crates/ui/src/panels/texture_sets.rs (set_mesh_map_from_file) | #137 |
| Drag a texture from Assets onto a slot | Mesh map slot | (unverified) | done (also a file from the system file browser) | crates/ui/src/panels/texture_sets.rs (mesh_maps) | - |
| Replace the map in a filled slot | Mesh map slot | (unverified) | done | crates/ui/src/panels/texture_sets.rs (map_slot) | #137 |
| Remove (X) the map from a slot | Mesh map slot | (unverified) | done | crates/ui/src/panels/texture_sets.rs (map_slot) | - |
| Slot context menu | Mesh map slot (right-click) | not documented | partial: Select / Replace image, Bake this map, Remove (Painter's entries are not documented) | crates/ui/src/panels/texture_sets.rs (map_slot) | - |
| Import baked maps: Add button at project creation | New project > Import baked maps | none | missing | - | #137 |
| Auto-assign by file name TextureSetName_MeshMapName | New project > Import baked maps | ambient_occlusion, curvature, normal_base, world_space_normals, id, position, thickness | missing | - | #137 |
| Baked normal follows the project Normal Map Format | Project settings | DirectX or OpenGL | partial: maps are always OpenGL; the setting is an export convention | crates/ui/src/new_project.rs:286 | - |

## UV Tiles settings

Layout notes: UV Tile mode is chosen at project creation and is read-only afterwards (shown in Project configuration). Per-tile resolution is edited in the Texture Set Settings size area (two titled groups in the dropdown) and in the Texture Set List. SubCraft's UDIM support makes one texture set per tile, which is Painter's legacy mode.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Use UV Tile workflow | New project > UV Tile Settings (UDIMs) | off | partial: "Use UDIM tiles (one texture set per tile)" only | crates/ui/src/new_project.rs:281 | - |
| Preserve UV Tile layout per materials and enable painting across tiles | New project > UV Tile Settings (UDIMs) | not documented | missing | - | - |
| Convert UV Tiles into individual Texture Sets (legacy) | New project > UV Tile Settings (UDIMs) | not documented | done | crates/ui/src/new_project.rs:281 | - |
| UV Tiles settings shown read-only after creation | Project configuration > UV Tiles settings | read-only | missing | - | - |
| Per UV tile resolution | Texture Set Settings > Size | set resolution | missing | - | - |
| Resolution dropdown section titles (set / tile) | Texture Set Settings > Size | not documented | missing | - | - |
| Image sequence import by file name pattern (name.1001.ext, name_1001.ext, name-1001.ext) | Assets import | automatic | missing | - | - |
| Fill (Match Per UV Tile) projection for sequences | Fill layer > Projection | not documented | missing | - | - |

## Project configuration: mesh, UV reprojection

Layout notes: Painter groups these in Edit > Project configuration (3D mesh, Project settings, UV Tiles settings, Import settings, Physical size, Colour management). SubCraft has no such window; the closest are File > Reload mesh and File > Auto UV unwrap.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| 3D mesh path and Select button | Project configuration > 3D mesh | project mesh | partial: File > Reload mesh window | crates/ui/src/mesh_reload.rs:122 | - |
| Reimport mesh checkbox | Project configuration > 3D mesh | off; ticked when another file is picked | partial: Reload button in the reload window | crates/ui/src/mesh_reload.rs:169 | - |
| Normal Map Format | Project configuration > Project Settings | DirectX or OpenGL | partial: set at project creation only | crates/ui/src/new_project.rs:286 | - |
| Compute Tangent Space Per Fragment | Project configuration > Project Settings | not documented | missing | - | - |
| Import Cameras | Project configuration > Import settings | not documented | missing | - | - |
| Preserve strokes positions on mesh | Project configuration > Import settings | on (recommended) | partial: Keep UVs or Reproject choice instead | crates/ui/src/mesh_reload.rs:155 | - |
| Reprojection of all layers after a mesh reimport | Automatic | always | partial: only when Reproject is chosen; undo history is cleared | crates/ui/src/mesh_reload.rs:155 | - |
| Reprojection after a resolution change | Automatic | always | partial: paint is resampled to the new size | crates/app/src/session.rs:1074 | - |

## Automatic UV unwrapping options

Layout notes: Painter exposes Auto-unwrap as a checkbox with an Options button in the New project and Project configuration windows. SubCraft has a separate File > Auto UV unwrap window with its own settings (Seam angle, Island margin in pixels, Texture size, Pack quality, Balance texel density).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Auto-unwrap checkbox | New project / Project configuration > Import settings | not documented | partial: separate File > Auto UV unwrap window, not part of import | crates/ui/src/app.rs:1118 | - |
| Options button | Next to Auto-unwrap | not documented | partial: settings live in the Auto UV window | crates/ui/src/auto_uv.rs:68 | - |
| Seams | Auto-unwrap options > Unwrap sequence | Generate missing data (default); Recompute all | missing | - | - |
| UV islands | Auto-unwrap options > Unwrap sequence | Generate missing data (default); Recompute all | missing | - | - |
| Packing | Auto-unwrap options > Unwrap sequence | Generate missing data (default); Recompute all | missing | - | - |
| Margin size | Auto-unwrap options > Layout customization | Small 0.2% (default); No margin 0%; Medium 0.5%; Large 1% | partial: Island margin slider 0 to 32 px | crates/ui/src/auto_uv.rs (window, "Island margin") | - |
| UV island orientation | Auto-unwrap options > Layout customization | Unconstrained (default); Align with 3D mesh | missing | - | - |
| Maximum number of UV Tiles | Auto-unwrap options > UV Tiles | not documented | missing | - | - |
| Avoid elongated UV islands | Auto-unwrap options > Optimization | not documented | missing | - | - |
| Hard surface / organic segmentation option | Auto-unwrap options | (unverified) | missing | - | - |
| Lock orientation for packing | Auto-unwrap options | (unverified) | missing | - | - |
| Texel density control | Auto-unwrap options | (unverified) | partial: "Balance across islands" checkbox | crates/ui/src/auto_uv.rs (window, "Texel density") | - |

## Sparse virtual textures

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Hardware support acceleration | Settings > General > Sparse virtual textures | on where the GPU supports it | missing | - | - |
| Viewport texture streaming with software fallback | Automatic | always | missing | - | - |
| SVT cache in the cache directory | Settings > General > Cache directory | OS temporary folder | missing | - | - |
