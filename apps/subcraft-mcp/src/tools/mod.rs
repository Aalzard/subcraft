//! The tool catalog (names, descriptions, input schemas) and dispatch.

use serde_json::{Value, json};

use crate::params::{Args, Res};
use crate::server::{Reply, Server};

mod layers;
mod output;
mod paint;
mod project;

pub fn dispatch(server: &mut Server, name: &str, args: &Args) -> Res<Reply> {
    match name {
        "status" => project::status(server),
        "inspect_model" => project::inspect_model(args),
        "project_new" => project::new(server, args),
        "project_open" => project::open(server, args),
        "project_save" => project::save(server, args),
        "project_info" => project::info(server),
        "texture_set_update" => project::texture_set_update(server, args),
        "undo" => project::undo_redo(server, args, true),
        "redo" => project::undo_redo(server, args, false),
        "history" => project::history(server),

        "layers_list" => layers::list(server, args),
        "layer_get" => layers::get(server, args),
        "layer_add" => layers::add(server, args),
        "layer_update" => layers::update(server, args),
        "layer_delete" => layers::delete(server, args),
        "layer_duplicate" => layers::duplicate(server, args),
        "layer_move" => layers::move_layer(server, args),
        "layer_group" => layers::group(server, args),
        "fill_set" => layers::fill_set(server, args),
        "parts_list" => layers::parts_list(server, args),
        "layer_set_parts" => layers::set_parts(server, args),
        "mask_add" => layers::mask_add(server, args),
        "mask_remove" => layers::mask_remove(server, args),
        "mask_add_effect" => layers::mask_add_effect(server, args),
        "content_add_effect" => layers::content_add_effect(server, args),
        "effect_update" => layers::effect_update(server, args),
        "effect_remove" => layers::effect_remove(server, args),
        "shelf_list" => layers::shelf_list(server, args),
        "apply_material" => layers::apply_material(server, args),
        "apply_smart_mask" => layers::apply_smart_mask(server, args),
        "import_textures" => layers::import_textures(server, args),

        "brush_set" => paint::brush_set(server, args),
        "paint_stroke" => paint::stroke(server, args),
        "paint_fill" => paint::fill(server, args),
        "pick" => paint::pick(server, args),

        "bake_mesh_maps" => output::bake(server, args),
        "export_templates" => output::templates(),
        "export_textures" => output::export_textures(server, args),
        "export_scene" => output::export_scene(server, args),
        "render_preview" => output::render_preview(server, args),
        "render_texture" => output::render_texture(server, args),
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn tool(name: &str, title: &str, description: &str, properties: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
        "annotations": { "title": title, "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false },
    })
}

const LAYER: &str = "Layer id, or its exact name when that is unique.";
const TS: &str = "Texture set id or name (default: the selected one).";
const CAMERA: &str = "Orbit camera, as in render_preview: yaw/pitch in degrees around the model (yaw 0 looks from +Z, 90 from +X; pitch 90 from above), zoom > 1 moves closer, offset moves the target by fractions of the model radius.";

fn camera_props() -> Value {
    json!({
        "yaw": { "type": "number", "description": "Degrees around the vertical axis (0 = front/+Z, 90 = right/+X, 180 = back). Default 35." },
        "pitch": { "type": "number", "description": "Degrees up (positive) or down. Default 22." },
        "zoom": { "type": "number", "description": "1 frames the whole model; 2 is twice as close. Default 1." },
        "offset": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] target shift in model radii." },
        "width": { "type": "integer", "description": "Image width in pixels (default 768)." },
        "height": { "type": "integer", "description": "Image height in pixels (default 768)." },
    })
}

fn with(mut base: Value, extra: Value) -> Value {
    if let (Value::Object(b), Value::Object(e)) = (&mut base, extra) {
        b.extend(e);
    }
    base
}

pub fn definitions() -> Vec<Value> {
    let fill_spec = "Per channel (base_color, roughness, metallic, height, normal, emissive, ambient_occlusion, opacity, specular): a value \
(number, '#rrggbb', or [r,g,b] in sRGB for color channels), false to switch the channel off, or an object \
{value, enabled, strength, linear, procedural: {kind: clouds|cells|grunge|scratches|wood_rings|brushed|checker|gradient_u|gradient_v|dirt|cracks|streaks|speckles|pores|scratch, scale, seed, balance, contrast, invert, ...}, low, high, image: '<file path>', mesh_map: curvature|ambient_occlusion|...}. A procedural/mesh map blends from `low` (0) to `high` (1).";
    vec![
        // ----- project
        tool("status", "Server status", "Server version, whether a project is open, the rendering adapter and the shelf library location. Call first if unsure.", json!({}), &[], true),
        tool("inspect_model", "Inspect a 3D model", "Read a model file (FBX, OBJ, glTF/GLB, USD, PLY, STL) without opening it: triangles, objects, materials, UV range, UDIM tiles, warnings.", json!({ "path": { "type": "string" } }), &["path"], true),
        tool("project_new", "New project from a model", "Create a new texturing project from a model file (or a Substance Painter .spp). Replaces the open project; refuses when it has unsaved changes unless discard_unsaved is true.",
            json!({
                "model_path": { "type": "string", "description": "FBX, OBJ, glTF/GLB, USD/USDA/USDC/USDZ, PLY, STL or .spp; or \"sample\" for the built-in sample crate." },
                "template": { "type": "string", "enum": ["pbr_metal_rough", "unreal_orm", "unity", "godot", "stylized"], "description": "Channel set and export preset (default pbr_metal_rough)." },
                "resolution": { "type": "integer", "enum": [256, 512, 1024, 2048, 4096], "description": "Texture resolution (default 2048)." },
                "split": { "type": "string", "enum": ["by_material", "by_object", "single"], "description": "One texture set per material (default), per object, or one for everything." },
                "udim": { "type": "boolean", "description": "UDIM tiles become separate texture sets (default true)." },
                "channels": { "type": "array", "items": { "type": "string" }, "description": "Override the template's channels." },
                "normal_format": { "type": "string", "enum": ["opengl", "directx"] },
                "discard_unsaved": { "type": "boolean" },
            }), &["model_path"], false),
        tool("project_open", "Open project", "Open a .subcraft project (or import a Substance Painter .spp). Refuses to replace unsaved changes unless discard_unsaved is true.",
            json!({ "path": { "type": "string" }, "discard_unsaved": { "type": "boolean" } }), &["path"], false),
        tool("project_save", "Save project", "Save the project as .subcraft (the SubCraft desktop app opens it). Without `path`, saves over the file it was opened from.",
            json!({ "path": { "type": "string", "description": "Target .subcraft file." } }), &[], false),
        tool("project_info", "Project info", "The model, texture sets (channels, resolution, baked maps), selection, undo state and export template.", json!({}), &[], true),
        tool("texture_set_update", "Edit a texture set", "Rename a texture set, change its resolution (resamples paint), add channels or hide it from previews.",
            json!({
                "texture_set": { "type": "string", "description": TS },
                "name": { "type": "string" },
                "resolution": { "type": "integer", "enum": [256, 512, 1024, 2048, 4096] },
                "all_texture_sets": { "type": "boolean", "description": "Apply `resolution` to every texture set." },
                "add_channels": { "type": "array", "items": { "type": "string" }, "description": "e.g. [\"emissive\", \"opacity\", \"ambient_occlusion\"]" },
                "hidden": { "type": "boolean" },
            }), &[], false),
        tool("undo", "Undo", "Undo the last edit(s).", json!({ "steps": { "type": "integer", "description": "Default 1." } }), &[], false),
        tool("redo", "Redo", "Redo undone edit(s).", json!({ "steps": { "type": "integer" } }), &[], false),
        tool("history", "History", "The undo history (labels, current position).", json!({}), &[], true),

        // ----- layers
        tool("layers_list", "List layers", "The layer stack, top first: ids, names, types (paint, fill, folder), visibility, blend, mask effects, fill channel sources, painted channels. Without texture_set lists every set.",
            json!({ "texture_set": { "type": "string", "description": TS } }), &[], true),
        tool("layer_get", "Layer details", "Every parameter of one layer: fill sources and projection, blend per channel, mask and content effects with their settings.",
            json!({ "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" } }), &["layer"], true),
        tool("layer_add", "Add layer", "Add a paint, fill or folder layer. Goes on top of the stack unless `above` (a layer) or `parent` (a folder) is given. A fill layer can be set up in the same call with `fill` (see fill_set).",
            json!({
                "type": { "type": "string", "enum": ["paint", "fill", "folder"] },
                "name": { "type": "string" },
                "texture_set": { "type": "string", "description": TS },
                "above": { "type": "string", "description": "Insert directly above this layer." },
                "parent": { "type": "string", "description": "Put it inside this folder (on top)." },
                "fill": { "type": "object", "description": fill_spec },
                "exclusive": { "type": "boolean", "description": "With `fill`: switch off every channel not listed (so the layer only changes what you set). Default true for fill layers created with `fill`." },
                "projection": { "type": "object", "description": "Fill projection {mode: uv|triplanar|planar, scale: [u,v], rotation_deg, offset: [u,v], triplanar_sharpness, planar_axis}." },
                "mask": { "type": "string", "enum": ["white", "black"], "description": "Also add a mask." },
            }), &["type"], false),
        tool("layer_update", "Edit layer", "Rename, show/hide, change blend mode and opacity (all channels or per channel), enable/disable the mask, expand/collapse a folder.",
            json!({
                "layer": { "type": "string", "description": LAYER },
                "texture_set": { "type": "string" },
                "name": { "type": "string" },
                "visible": { "type": "boolean" },
                "blend_mode": { "type": "string", "description": "normal, multiply, screen, overlay, soft_light, linear_dodge, subtract, darken, lighten, replace, color, value, passthrough (folders), normal_map_combine, normal_map_detail, disable, ... (applies to every channel but Normal)" },
                "opacity": { "type": "number", "description": "0..1, every channel." },
                "channels": { "type": "object", "description": "Per channel {mode, opacity}, e.g. {\"roughness\": {\"opacity\": 0.5}, \"height\": {\"mode\": \"disable\"}}." },
                "mask_enabled": { "type": "boolean" },
                "expanded": { "type": "boolean" },
            }), &["layer"], false),
        tool("layer_delete", "Delete layers", "Delete one or more layers (one undo step).",
            json!({ "layers": { "type": "array", "items": { "type": "string" }, "description": "Layer ids or names (same texture set)." }, "texture_set": { "type": "string" } }), &["layers"], false),
        tool("layer_duplicate", "Duplicate layer", "Duplicate a layer (with its mask and effects); returns the copy's id.",
            json!({ "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" }, "name": { "type": "string" } }), &["layer"], false),
        tool("layer_move", "Move layer", "Reorder: direction up/down/top/bottom among siblings, or into a folder (`parent`, or \"root\") at `index` (bottom-to-top; default top).",
            json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "direction": { "type": "string", "enum": ["up", "down", "top", "bottom"] },
                "parent": { "type": "string", "description": "Folder layer, or \"root\"." },
                "index": { "type": "integer" },
            }), &["layer"], false),
        tool("layer_group", "Group into folder", "Wrap a layer in a new folder; returns the folder id.",
            json!({ "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" }, "name": { "type": "string", "description": "Folder name." } }), &["layer"], false),
        tool("fill_set", "Set fill channels", &format!("Set a fill layer's channels and projection. {fill_spec}"),
            json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "channels": { "type": "object", "description": fill_spec },
                "exclusive": { "type": "boolean", "description": "Switch off every channel not listed in `channels`." },
                "projection": { "type": "object", "description": "{mode: uv|triplanar|planar, scale: [u,v], rotation_deg, offset: [u,v], triplanar_sharpness, planar_axis (0 X, 1 Y, 2 Z)}" },
            }), &["layer"], false),
        tool("parts_list", "List mesh parts", "Parts of the model: connected pieces, UV islands, imported objects or material slots, with ids for layer_set_parts.",
            json!({ "kind": { "type": "string", "enum": ["connected", "uv_island", "object", "material"], "description": "Default object when the model has objects, else connected." } }), &[], true),
        tool("layer_set_parts", "Restrict layer to parts", "Show a layer only on some mesh parts (like a geometry mask), or everywhere again with parts: null.",
            json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "parts": { "type": ["array", "null"], "items": { "type": "integer" }, "description": "Part ids from parts_list, or null for everywhere." },
                "kind": { "type": "string", "enum": ["connected", "uv_island", "object", "material"] },
            }), &["layer", "parts"], false),

        // ----- masks and effects
        tool("mask_add", "Add mask", "Give a layer a white (shows everything) or black (hides everything) mask, optionally with a shelf smart mask on it.",
            json!({ "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" }, "color": { "type": "string", "enum": ["white", "black"] }, "smart_mask": { "type": "string", "description": "Smart mask name from shelf_list." } }), &["layer"], false),
        tool("mask_remove", "Remove mask", "Delete a layer's mask.", json!({ "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" } }), &["layer"], false),
        tool("mask_add_effect", "Add mask effect", "Add an effect on top of a layer's mask (a black mask is created when there is none). effect.type: \
generator {kind: edges|cavity|concave|directional|position_gradient|{mesh_map: curvature}, level, softness, invert, noise_amount, direction:[x,y,z], axis: x|y|z, organic_breakup, edge_spread:{distance,variation}} (edges/cavity need bake_mesh_maps); \
fill {value 0..1, procedural:{kind,...}, projection}; levels {in_low,in_high,gamma,out_low,out_high}; \
filter {kind: blur|sharpen|contrast|threshold|clamp|levels-like histogram_scan|histogram_range|median|warp|bevel|mask_outline|invert|posterize|..., params}; invert; image {image: path, invert, projection}.",
            json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "effect": { "type": "object", "description": "{type, ...parameters}" },
                "name": { "type": "string" }, "opacity": { "type": "number" }, "blend": { "type": "string", "description": "normal, multiply, screen, subtract, ..." },
            }), &["layer", "effect"], false),
        tool("content_add_effect", "Add content effect", "Add an effect to a layer's content stack: filter {channel, kind, ...} (blur, sharpen, hsl {hue, saturation, lightness}, contrast, gradient_map, warp, ...), levels {channel, ...}, fill {channels: like fill_set, projection} or paint (a raster to paint into, also on fill layers).",
            json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "effect": { "type": "object", "description": "{type: filter|levels|fill|paint, channel, ...}" },
                "name": { "type": "string" },
            }), &["layer", "effect"], false),
        tool("effect_update", "Edit effect", "Change a mask or content effect: `params` merge into its settings (generator level, filter radius, levels, fill value...), plus visible, opacity, blend, name.",
            json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "effect": { "type": "string", "description": "Effect id or name (layer_get lists them)." },
                "params": { "type": "object" }, "visible": { "type": "boolean" }, "opacity": { "type": "number" }, "blend": { "type": "string" }, "name": { "type": "string" },
            }), &["layer", "effect"], false),
        tool("effect_remove", "Remove effect", "Remove a mask or content effect.",
            json!({ "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" }, "effect": { "type": "string" } }), &["layer", "effect"], false),

        // ----- shelf
        tool("shelf_list", "Browse the shelf", "Built-in and user assets: materials (fill presets), smart_materials (layer stacks with wear/dirt), smart_masks, brushes (tips), environments (lighting for previews).",
            json!({ "category": { "type": "string", "enum": ["materials", "smart_materials", "smart_masks", "brushes", "environments"] }, "search": { "type": "string" } }), &[], true),
        tool("apply_material", "Apply material", "Add a shelf material or smart material as a new layer (top of the stack, or above `above`). Smart materials with edge/cavity wear need bake_mesh_maps first.",
            json!({ "name": { "type": "string" }, "texture_set": { "type": "string", "description": TS }, "above": { "type": "string" }, "layer_name": { "type": "string" } }), &["name"], false),
        tool("import_textures", "Import a texture set", "Add a fill layer from image files (e.g. Wood_BaseColor.png, Wood_Roughness.png, Wood_Normal.png): each file goes to the channel its name names; gloss maps are inverted and DirectX normals flipped. Packed ORM/ARM maps are skipped.",
            json!({ "paths": { "type": "array", "items": { "type": "string" } }, "texture_set": { "type": "string", "description": TS }, "above": { "type": "string" }, "layer_name": { "type": "string" } }), &["paths"], false),
        tool("apply_smart_mask", "Apply smart mask", "Put a shelf smart mask on a layer (adds a black mask if it has none).",
            json!({ "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" }, "name": { "type": "string" } }), &["layer", "name"], false),

        // ----- painting
        tool("brush_set", "Brush settings", "Set the brush used by paint_stroke. Without arguments returns the current brush. `channels` sets what the brush paints, like fill values ({\"base_color\": \"#c0392b\", \"roughness\": 0.4, \"metallic\": false}); exclusive switches the rest off.",
            json!({
                "size": { "type": "number", "description": "Diameter in % of the model's bounding-box diagonal (default 5)." },
                "flow": { "type": "number", "description": "0..1 per stamp." }, "opacity": { "type": "number", "description": "0..1 stroke maximum." },
                "hardness": { "type": "number" }, "spacing": { "type": "number" }, "angle": { "type": "number" },
                "mode": { "type": "string", "enum": ["paint", "erase", "smudge"] },
                "tip": { "type": "string", "description": "Brush tip name from shelf_list brushes (\"Round\" default)." },
                "channels": { "type": "object" }, "exclusive": { "type": "boolean" },
                "mask_value": { "type": "number", "description": "Gray painted into masks (0..1, default 1)." },
                "symmetry": { "type": "string", "description": "Mirror axes, e.g. \"x\", \"xz\", or \"\" for off." },
                "alignment": { "type": "string", "enum": ["tangent_wrap", "tangent_planar", "camera", "uv"] },
                "settings": { "type": "object", "description": "Any other BrushSettings field (size_jitter, scatter_count, follow_path, backface_culling, ...)." },
                "options": { "type": "object", "description": "StrokeOptions fields (lazy_distance_px, smoothing, radial {enabled, axis, count})." },
            }), &[], false),
        tool("paint_stroke", "Paint strokes", "Paint (or erase) brush strokes on a layer's content or mask with the current brush (brush_set). \
space \"uv\" (default): points are [u, v] texture coordinates, 0..1 with the origin top-left (render_texture images use the same orientation). \
space \"view\": points are [x, y] pixels in the render_preview image taken with the same camera arguments; the stroke lands where you see the model. Each stroke is one undo step.",
            with(camera_props(), json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "target": { "type": "string", "enum": ["content", "mask"], "description": "Default content." },
                "space": { "type": "string", "enum": ["uv", "view"] },
                "points": { "type": "array", "items": { "type": "array", "items": { "type": "number" } }, "description": "One stroke: [[u,v], ...] or [[x,y], ...]; a third number is pen pressure 0..1." },
                "strokes": { "type": "array", "items": { "type": "array" }, "description": "Several strokes, each a list of points." },
                "brush": { "type": "object", "description": "brush_set arguments applied first." },
            })), &["layer"], false),
        tool("paint_fill", "Polygon fill", "Fill the triangle, connected mesh piece or UV island at a point on a paint layer (or its mask) with the brush channels.",
            with(camera_props(), json!({
                "layer": { "type": "string", "description": LAYER }, "texture_set": { "type": "string" },
                "target": { "type": "string", "enum": ["content", "mask"] },
                "mode": { "type": "string", "enum": ["triangle", "mesh_element", "uv_island"], "description": "Default mesh_element." },
                "space": { "type": "string", "enum": ["uv", "view"] },
                "point": { "type": "array", "items": { "type": "number" } },
            })), &["layer", "point"], false),
        tool("pick", "What is at a pixel", &format!("For pixel positions in a render_preview image: the surface UV, 3D position, normal, texture set and part under each. {CAMERA}"),
            with(camera_props(), json!({ "points": { "type": "array", "items": { "type": "array", "items": { "type": "number" } }, "description": "[[x, y], ...] pixels." } })), &["points"], true),

        // ----- bake, export, render
        tool("bake_mesh_maps", "Bake mesh maps", "Bake mesh maps (normal, world_space_normal, id, ambient_occlusion, curvature, position, thickness, height, bent_normal) for smart materials, generators and export. Optional high-poly model. Runs on the CPU and can take minutes at high resolution.",
            json!({
                "maps": { "type": "array", "items": { "type": "string" }, "description": "Default all." },
                "size": { "type": "integer", "description": "Default: each texture set's resolution (max 4096)." },
                "texture_sets": { "type": "array", "items": { "type": "string" } },
                "high_poly": { "type": "string", "description": "High-poly model file for normal/height/AO detail." },
                "ao_samples": { "type": "integer" }, "ao_distance": { "type": "number" },
                "curvature_radius": { "type": "number" }, "cage_front": { "type": "number" }, "cage_rear": { "type": "number" },
                "match_by_name": { "type": "boolean", "description": "High-poly rays only hit parts with matching names (_low/_high)." },
            }), &[], false),
        tool("export_templates", "Export presets", "Export templates and the maps each writes.", json!({}), &[], true),
        tool("export_textures", "Export textures", "Write the texture maps for every texture set into a folder with an export template (default: the project's).",
            json!({
                "dir": { "type": "string" },
                "template": { "type": "string", "description": "Template name from export_templates (e.g. \"Unreal\", \"Unity URP\", \"Godot\", \"glTF\")." },
                "format": { "type": "string", "enum": ["png8", "png16", "tga8", "jpeg", "exr32"], "description": "Force every map to this format." },
                "padding": { "description": "\"infinite\" (default), \"none\", or a texel count." },
                "normal_format": { "type": "string", "enum": ["opengl", "directx"] },
                "height_to_normal": { "type": "number", "description": "Bake height detail into the normal map (default 0.02, 0 = off)." },
                "udim_folders": { "type": "boolean" },
            }), &["dir"], false),
        tool("export_scene", "Export model + textures", "Write the model with its textures as OBJ+MTL, glTF, GLB or USDA.",
            json!({
                "dir": { "type": "string" },
                "format": { "type": "string", "enum": ["obj", "gltf", "glb", "usda"] },
                "template": { "type": "string" },
                "texture_format": { "type": "string", "enum": ["png8", "png16", "tga8", "jpeg", "exr32"] },
                "normal_format": { "type": "string", "enum": ["opengl", "directx"] },
            }), &["dir", "format"], false),
        tool("render_preview", "Render preview", &format!("Render the textured model as the SubCraft viewport shows it and return the image. {CAMERA} \
`views` renders several named angles at once (front, back, left, right, top, bottom, three_quarter). view shows one channel unlit. Use it after edits to check the result."),
            with(camera_props(), json!({
                "views": { "type": "array", "items": { "type": "string", "enum": ["front", "back", "left", "right", "top", "bottom", "three_quarter"] } },
                "view": { "type": "string", "enum": ["material", "base_color", "normal", "roughness", "metallic", "height", "ambient_occlusion", "baked_normal"], "description": "Default material (full shading)." },
                "environment": { "type": "string", "description": "Environment name from shelf_list environments (default the first, Studio)." },
                "environment_rotation": { "type": "number" }, "environment_intensity": { "type": "number" }, "exposure": { "type": "number" },
                "background": { "type": "string", "description": "#rrggbb" },
                "wireframe": { "type": "boolean" },
                "save_path": { "type": "string", "description": "Also save the PNG here (with several views, _front, _back... are appended)." },
            })), &[], true),
        tool("render_texture", "Show a texture", "Return a channel of a texture set's final composite (or a baked mesh map) as a flat UV-space image, origin top-left, as exported.",
            json!({
                "texture_set": { "type": "string", "description": TS },
                "channel": { "type": "string", "description": "base_color, roughness, metallic, height, normal, emissive, ... (default base_color)" },
                "mesh_map": { "type": "string", "description": "Show a baked map instead (curvature, ambient_occlusion, normal, id, ...)." },
                "max_size": { "type": "integer", "description": "Longest side in pixels (default 1024)." },
                "save_path": { "type": "string" },
            }), &[], true),
    ]
}
