# subcraft-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that lets an AI agent
(Claude Code, Claude Desktop, any MCP client) texture models with SubCraft's engine. There is
no window: the server runs the same `subcraft-app` session and undoable commands as the desktop
app. It saves `.subcraft` projects that the app opens, exports textures, and renders PNG
previews so the agent can see what it made.

It speaks MCP over stdio (one JSON-RPC message per line) and holds one open project at a time.

## Tools (41)

| Area | Tools |
| --- | --- |
| Project | `status`, `inspect_model`, `project_new` (FBX/OBJ/glTF/USD/PLY/STL/.spp, or `"sample"`), `project_open`, `project_save`, `project_info`, `texture_set_update`, `undo`, `redo`, `history` |
| Layers | `layers_list`, `layer_get`, `layer_add` (paint/fill/folder, with fill values and mask in one step), `layer_update` (name, visibility, blend mode, opacity per channel), `layer_delete`, `layer_duplicate`, `layer_move`, `layer_group` |
| Fills | `fill_set`: uniform values (`"#rrggbb"` sRGB or numbers), 15 procedurals with low/high, images, baked mesh maps, projection (UV/tri-planar/planar) |
| Masks and effects | `mask_add`, `mask_remove`, `mask_add_effect` (generators: edges, cavity, concave, directional, position gradient; fill, levels, 26 filters, invert, image), `content_add_effect`, `effect_update`, `effect_remove` |
| Parts | `parts_list`, `layer_set_parts` (connected pieces, UV islands, objects, materials) |
| Shelf | `shelf_list`, `apply_material`, `apply_smart_mask`, `import_textures` (a texture set from files) |
| Painting | `brush_set`, `paint_stroke` (in UV space, or in the pixels of a `render_preview` image), `paint_fill` (triangle / mesh piece / UV island), `pick` |
| Output | `bake_mesh_maps` (9 maps, optional high poly), `export_templates`, `export_textures`, `export_scene` (OBJ, glTF, GLB, USDA), `render_preview` (any angle, named views, channel views, environments), `render_texture` (a channel or baked map as a flat image) |

Layers and texture sets can be named by id or by exact name. Every edit is an undo step.

## Typical session

1. `project_new` with a model: one texture set per material.
2. `bake_mesh_maps`: smart materials and edge/cavity masks need curvature and AO.
3. `apply_material` "Worn Painted Metal", or `layer_add` with a fill such as
   `{"base_color": "#8a6f5a", "roughness": 0.6}`, then `mask_add_effect` with an edges generator.
4. `render_preview` to look at the result, and fix it with `layer_update` / `effect_update`.
5. Paint details: `render_preview` with `yaw` / `pitch`, then `paint_stroke` with `space: "view"` and
   the same camera, using pixel positions in that image.
6. `project_save`, then `export_textures` with a template (Unreal, Unity URP/HDRP, Godot, glTF, ...).

## Build and install

```sh
cargo build --release -p subcraft-mcp      # binary: target/release/subcraft-mcp
```

On **Windows with Smart App Control** (or any Application Control policy), freshly built `.exe`
files are blocked. Build and run the server inside WSL2 instead:

```sh
wsl -d Ubuntu -e bash /mnt/d/subcraft/scripts/build-mcp-wsl.sh
claude mcp add --scope user subcraft -- wsl.exe -d Ubuntu --exec env SUBCRAFT_LIBRARY=/mnt/c/Users/<you>/Documents/SubCraft/Library /usr/local/bin/subcraft-mcp
```

Ubuntu needs `build-essential pkg-config cmake libvulkan1 mesa-vulkan-drivers` and Rust from
rustup. Under WSL the server accepts and returns Windows paths (`D:\models\a.fbx` maps to
`/mnt/d/models/a.fbx`), and previews render with Mesa's software Vulkan (llvmpipe): about half a
second for a 512 px image. `SUBCRAFT_LIBRARY` points the shelf at the same user library as the
desktop app, so your saved smart materials, masks, brushes and HDRIs show up.

On Linux or macOS, build natively and register `target/release/subcraft-mcp` directly.

## Limits

- One project at a time; `project_new` / `project_open` refuse to drop unsaved changes unless
  `discard_unsaved` is set.
- Baking runs on the CPU (GPU ray baking is not used headlessly).
- Not exposed yet: pen paths, text decals, stencils, clone tool, UDIM tile picking, editing ID
  color masks by picking, path-traced renders.
