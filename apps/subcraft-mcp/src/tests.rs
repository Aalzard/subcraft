//! End-to-end tests: tool calls through `Server::call`, the same entry point the stdio loop uses,
//! on SubCraft's built-in sample crate. Rendering is optional (CI machines may have no adapter);
//! everything else runs on the CPU.

use serde_json::{Value, json};

use crate::server::Server;

/// A fresh folder under the system temp dir, removed when dropped.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("subcraft-mcp-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn join(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Call a tool and return (is_error, the first text item parsed as JSON, or as a string).
fn call(server: &mut Server, name: &str, args: Value) -> (bool, Value) {
    let reply = server.call(name, args);
    let is_error = reply["isError"].as_bool().expect("isError is always set");
    let text = reply["content"]
        .as_array()
        .and_then(|c| c.iter().find(|i| i["type"] == "text"))
        .and_then(|i| i["text"].as_str())
        .unwrap_or_default()
        .to_string();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (is_error, body)
}

#[track_caller]
fn ok(server: &mut Server, name: &str, args: Value) -> Value {
    let (err, body) = call(server, name, args);
    assert!(!err, "{name} failed: {body}");
    body
}

#[track_caller]
fn fails(server: &mut Server, name: &str, args: Value) -> String {
    let (err, body) = call(server, name, args);
    assert!(err, "{name} should have failed, got {body}");
    body.as_str().map(str::to_string).unwrap_or_else(|| body.to_string())
}

fn sample(server: &mut Server) {
    ok(server, "project_new", json!({ "model_path": "sample", "resolution": 256 }));
}

fn layer_names(server: &mut Server, texture_set: &str) -> Vec<String> {
    fn walk(nodes: &Value, out: &mut Vec<String>) {
        for l in nodes.as_array().into_iter().flatten() {
            out.extend(l["name"].as_str().map(str::to_string));
            walk(&l["children"], out);
        }
    }
    let list = ok(server, "layers_list", json!({ "texture_set": texture_set }));
    assert_eq!(list["texture_set"], json!(texture_set), "{list}");
    let mut names = Vec::new();
    walk(&list["layers"], &mut names);
    names
}

#[test]
fn every_defined_tool_is_dispatched_and_closed_to_unknown_arguments() {
    let defs = crate::tools::definitions();
    assert!(defs.len() >= 49);
    let mut server = Server::new();
    for d in &defs {
        let name = d["name"].as_str().unwrap();
        assert_eq!(d["inputSchema"]["additionalProperties"], json!(false), "{name}");
        // With no project open every tool either works or explains itself; none is unknown.
        let (_, body) = call(&mut server, name, json!({}));
        assert!(!body.to_string().contains("unknown tool"), "{name} is listed but not dispatched");
    }
    assert!(fails(&mut server, "no_such_tool", json!({})).contains("unknown tool"));
}

#[test]
fn unknown_arguments_are_reported_with_the_valid_names() {
    let mut server = Server::new();
    sample(&mut server);
    // `size` is not a render_preview argument (width/height are): it used to be ignored silently.
    let msg = fails(&mut server, "render_preview", json!({ "size": [384, 384] }));
    assert!(msg.contains("`size`") && msg.contains("width") && msg.contains("height"), "{msg}");
    let msg = fails(&mut server, "layer_add", json!({ "type": "fill", "colour": "#ff0000" }));
    assert!(msg.contains("`colour`") && msg.contains("fill"), "{msg}");
    // Nothing was added by the rejected call.
    assert!(!layer_names(&mut server, "Body").iter().any(|n| n.contains("Fill")));
}

#[test]
fn annotations_mark_writing_tools_destructive() {
    let defs = crate::tools::definitions();
    let ann = |n: &str| defs.iter().find(|d| d["name"] == n).unwrap()["annotations"].clone();
    // render_preview and render_texture write a PNG when given save_path (QA 2026-11: they were
    // marked read-only).
    for n in ["project_save", "export_textures", "export_scene", "layer_delete", "save_to_library", "open_in_app", "render_preview", "render_texture"] {
        assert_eq!(ann(n)["destructiveHint"], json!(true), "{n}");
        assert_eq!(ann(n)["readOnlyHint"], json!(false), "{n}");
    }
    for n in ["layers_list", "project_info", "status", "pick"] {
        assert_eq!(ann(n)["readOnlyHint"], json!(true), "{n}");
        assert_eq!(ann(n)["destructiveHint"], json!(false), "{n}");
    }
    assert_eq!(ann("open_in_app")["openWorldHint"], json!(true));
    assert_eq!(ann("layer_add")["openWorldHint"], json!(false));
}

#[test]
fn image_saves_must_be_png_files() {
    let dir = TempDir::new("png");
    let victim = dir.join("notes.txt");
    std::fs::write(&victim, "keep me").unwrap();
    let mut server = Server::new();
    sample(&mut server);
    let msg = fails(&mut server, "render_texture", json!({ "channel": "base_color", "save_path": victim }));
    assert!(msg.contains(".png"), "{msg}");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
    let msg = fails(&mut server, "render_preview", json!({ "save_path": dir.join("view.jpg") }));
    assert!(msg.contains(".png"), "{msg}");

    let png = dir.join("sub/base.png");
    let body = ok(&mut server, "render_texture", json!({ "channel": "base_color", "save_path": png }));
    assert!(body["saved"].as_str().is_some_and(|s| s.ends_with("base.png")), "{body}");
    let img = image::open(&png).unwrap();
    assert!(img.width() > 0 && img.height() > 0);
    assert!(crate::tools::png_path(std::path::Path::new("a/b.PNG")).is_ok());
    assert!(crate::tools::png_path(std::path::Path::new("a/.png")).is_err());
}

#[test]
fn fill_layer_reaches_the_exported_base_color() {
    let dir = TempDir::new("export");
    let mut server = Server::new();
    sample(&mut server);
    ok(&mut server, "layer_add", json!({ "type": "fill", "name": "Red", "texture_set": "Body", "fill": { "base_color": "#ff0000", "roughness": 0.25 } }));
    let body = ok(&mut server, "export_textures", json!({ "dir": dir.join("out") }));
    let files: Vec<String> = body["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
    let base = files
        .iter()
        .find(|f| {
            let n = std::path::Path::new(f).file_name().unwrap().to_string_lossy().to_lowercase();
            n.contains("body") && (n.contains("basecolor") || n.contains("base_color") || n.contains("albedo"))
        })
        .unwrap_or_else(|| panic!("no Body base color map in {files:?}"));
    let img = image::open(base).unwrap().to_rgba8();
    // Infinite padding fills every texel, so any pixel shows the fill.
    for (x, y) in [(img.width() / 2, img.height() / 2), (0, 0), (img.width() - 1, img.height() - 1)] {
        let p = img.get_pixel(x, y).0;
        assert!(p[0] >= 250 && p[1] <= 5 && p[2] <= 5, "pixel ({x},{y}) is {p:?}, expected sRGB red");
    }
}

#[test]
fn layers_masks_painting_undo_and_save_round_trip() {
    let dir = TempDir::new("session");
    let mut server = Server::new();
    // Errors before a project exists.
    assert!(fails(&mut server, "layer_add", json!({ "type": "fill" })).contains("no project"));
    sample(&mut server);

    ok(&mut server, "bake_mesh_maps", json!({ "maps": ["curvature", "ambient_occlusion"], "size": 128 }));
    ok(&mut server, "layer_add", json!({ "type": "fill", "name": "Wear", "texture_set": "Trim", "fill": { "base_color": "#d0d0d0" }, "mask": "black" }));
    let fx = ok(&mut server, "mask_add_effect", json!({ "layer": "Wear", "texture_set": "Trim", "effect": { "type": "generator", "kind": "edges", "level": 0.6 } }));
    assert!(fx["effect_id"].as_str().is_some(), "{fx}");
    // Unknown filter/generator fields are errors, not silent resets.
    fails(&mut server, "mask_add_effect", json!({ "layer": "Wear", "texture_set": "Trim", "effect": { "type": "generator", "kind": "edges", "levl": 0.6 } }));

    // Paint where the model really is: ask `pick` for the UV under the middle of the default view.
    let picked = ok(&mut server, "pick", json!({ "width": 512, "height": 512, "points": [[256, 256]] }));
    let hit = &picked["points"][0];
    assert_eq!(hit["hit"], json!(true), "{picked}");
    let ts = hit["texture_set"].as_str().unwrap().to_string();
    let uv = [hit["uv"][0].as_f64().unwrap(), hit["uv"][1].as_f64().unwrap()];
    ok(&mut server, "layer_add", json!({ "type": "paint", "name": "Stripe", "texture_set": ts }));
    let stroke = ok(&mut server, "paint_stroke", json!({
        "layer": "Stripe", "texture_set": ts, "points": [[uv[0] - 0.01, uv[1]], [uv[0] + 0.01, uv[1]]],
        "brush": { "size": 5, "channels": { "base_color": "#00ff00" } },
    }));
    assert_eq!(stroke["changed"], json!(true), "{stroke}");
    // A stroke outside every UV island paints nothing and says so.
    let missed = ok(&mut server, "paint_stroke", json!({ "layer": "Stripe", "texture_set": ts, "points": [[-0.5, -0.5]] }));
    assert_eq!(missed["changed"], json!(false), "{missed}");

    // Undo/redo: one step per call.
    let before = ok(&mut server, "history", json!({}))["entries"].as_array().unwrap().len();
    ok(&mut server, "layer_add", json!({ "type": "folder", "name": "Group" }));
    assert!(layer_names(&mut server, "Body").contains(&"Group".to_string()) || layer_names(&mut server, "Trim").contains(&"Group".to_string()));
    let undone = ok(&mut server, "undo", json!({}));
    assert_eq!(undone["undone"].as_array().unwrap().len(), 1, "{undone}");
    assert!(!layer_names(&mut server, "Body").contains(&"Group".to_string()) && !layer_names(&mut server, "Trim").contains(&"Group".to_string()));
    ok(&mut server, "redo", json!({}));
    assert_eq!(ok(&mut server, "history", json!({}))["entries"].as_array().unwrap().len(), before + 1);

    // Save, reopen, and find the same layers.
    let file = dir.join("crate.subcraft");
    let saved = ok(&mut server, "project_save", json!({ "path": file }));
    assert!(saved["saved"].as_str().unwrap().ends_with("crate.subcraft"));
    // A path without the extension gets it, so a save never writes some other kind of file.
    let other = ok(&mut server, "project_save", json!({ "path": dir.join("other.txt") }));
    assert!(other["saved"].as_str().unwrap().ends_with("other.subcraft"), "{other}");
    ok(&mut server, "project_open", json!({ "path": file }));
    assert!(layer_names(&mut server, "Trim").contains(&"Wear".to_string()));

    // Replacing a modified project needs discard_unsaved.
    ok(&mut server, "layer_add", json!({ "type": "paint", "name": "Unsaved" }));
    assert!(fails(&mut server, "project_new", json!({ "model_path": "sample" })).contains("unsaved"));
    ok(&mut server, "project_new", json!({ "model_path": "sample", "discard_unsaved": true }));

    // Readable errors for bad names.
    assert!(fails(&mut server, "fill_set", json!({ "layer": "nope", "channels": {} })).contains("nope"));
    assert!(fails(&mut server, "layer_add", json!({ "type": "bogus" })).contains("bogus"));
}

#[test]
fn render_preview_returns_an_image_when_a_gpu_is_available() {
    let mut server = Server::new();
    sample(&mut server);
    let reply = server.call("render_preview", json!({ "width": 128, "height": 96 }));
    if reply["isError"] == json!(true) {
        let msg = reply["content"][0]["text"].as_str().unwrap_or_default();
        assert!(msg.contains("adapter") || msg.contains("graphics"), "render failed for another reason: {msg}");
        eprintln!("no graphics adapter here; render_preview skipped");
        return;
    }
    let image = reply["content"].as_array().unwrap().iter().find(|c| c["type"] == "image").expect("an image item");
    // Base64 of the PNG signature.
    assert!(image["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    assert_eq!(image["mimeType"], json!("image/png"));
}

#[test]
fn flatten_instantiate_and_channel_removal() {
    let mut server = Server::new();
    sample(&mut server);
    ok(&mut server, "layer_add", json!({ "type": "fill", "name": "Shared", "texture_set": "Body", "fill": { "base_color": "#3366cc" } }));

    let inst = ok(&mut server, "layer_instantiate", json!({ "layers": ["Shared"], "texture_set": "Body" }));
    let sets = inst["instantiated"].as_array().unwrap();
    assert_eq!(sets.len(), 1, "{inst}");
    assert_eq!(sets[0]["texture_set"], json!("Trim"));
    assert_eq!(sets[0]["instances"].as_array().unwrap().len(), 1, "{inst}");
    assert!(fails(&mut server, "layer_instantiate", json!({ "layers": ["Shared"], "texture_set": "Body", "to_texture_sets": ["Body"] })).contains("no other"));

    let before = layer_names(&mut server, "Body").len();
    let flat = ok(&mut server, "layer_flatten", json!({ "layers": ["Shared"], "texture_set": "Body" }));
    assert_eq!(flat["flattened"].as_array().unwrap().len(), 1, "{flat}");
    assert_eq!(layer_names(&mut server, "Body").len(), before + 1);
    ok(&mut server, "undo", json!({}));
    assert_eq!(layer_names(&mut server, "Body").len(), before);

    let has = |server: &mut Server, kind: &str| {
        let info = ok(server, "project_info", json!({}));
        info.to_string().contains(kind)
    };
    ok(&mut server, "texture_set_update", json!({ "texture_set": "Body", "add_channels": ["emissive"] }));
    assert!(has(&mut server, "emissive"));
    ok(&mut server, "texture_set_update", json!({ "texture_set": "Body", "remove_channels": ["emissive"] }));
    let msg = fails(&mut server, "texture_set_update", json!({ "texture_set": "Body", "remove_channels": ["emissive"] }));
    assert!(msg.to_lowercase().contains("emissive"), "{msg}");
}
