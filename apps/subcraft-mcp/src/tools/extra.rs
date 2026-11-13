//! Pen paths, text decals, stencils, the clone source, fonts and the hand-off to the desktop app.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::Vec2;
use serde_json::{Value, json};
use subcraft_app::{Command, Session};
use subcraft_core::{AnchorId, ChannelKind, ChannelValue, TextureSetId};
use subcraft_doc::{LayerContent, PenAnchor, PenPath, TextDecal};
use subcraft_text::{Align, FontRef};

use crate::params::{Args, Res, channel, channel_value, enum_from, name_of};
use crate::paths;
use crate::render;
use crate::server::{self, Reply, Server, exec};
use crate::tools::layers::{added_layer, select_insertion, summary};
use crate::tools::paint::{camera_view, view_size};

// ----- shared helpers ---------------------------------------------------------------------------

fn vec2(v: &Value, what: &str) -> Res<Vec2> {
    let a = v.as_array().filter(|a| a.len() >= 2).ok_or_else(|| format!("{what} must be [x, y], got {v}"))?;
    let n = |i: usize| a[i].as_f64().map(|f| f as f32).ok_or_else(|| format!("{what} must hold numbers, got {v}"));
    Ok(Vec2::new(n(0)?, n(1)?))
}

/// The texture set and UV (wrapped into its 0..1 tile) under a pixel of a rendered view.
fn view_uv(session: &Session, args: &Args, px: Vec2) -> Res<(TextureSetId, Vec2)> {
    let cam = render::orbit(session, &camera_view(args)?).paint_camera(view_size(args)?);
    let pick = session.pick(&cam, px).ok_or_else(|| format!("pixel [{}, {}] does not show the model (pick tells)", px.x, px.y))?;
    let mesh = &session.scene().mesh;
    let i = &mesh.indices[pick.triangle as usize * 3..][..3];
    let (_, offset) = subcraft_mesh::uv_raster::wrap_to_tile([mesh.uvs[i[0] as usize], mesh.uvs[i[1] as usize], mesh.uvs[i[2] as usize]]);
    let ts = session.document().texture_set_for_slot(pick.material_slot).ok_or("that point has no texture set")?.id;
    Ok((ts, pick.uv - Vec2::new(offset[0] as f32, offset[1] as f32)))
}

fn in_view(args: &Args) -> Res<bool> {
    Ok(match args.str("space")?.unwrap_or("uv").to_ascii_lowercase().as_str() {
        "uv" => false,
        "view" | "screen" | "pixels" => true,
        other => return Err(format!("space must be uv or view, got `{other}`")),
    })
}

/// Channel values from `channels` (like brush_set), else the brush's enabled channels.
fn channel_values(session: &Session, ts: TextureSetId, args: &Args) -> Res<BTreeMap<ChannelKind, ChannelValue>> {
    let set = server::ts_ref(session, ts)?;
    let values: BTreeMap<ChannelKind, ChannelValue> = match args.obj("channels")? {
        Some(chs) => chs.iter().filter(|(_, v)| !matches!(v, Value::Bool(false))).map(|(k, v)| {
            let k = channel(k)?;
            Ok((k, channel_value(k, v, false)?))
        }).collect::<Res<_>>()?,
        None => session.brush.material.channels.iter().filter(|(_, c)| c.enabled).map(|(k, c)| (*k, c.value)).collect(),
    };
    let values: BTreeMap<_, _> = values.into_iter().filter(|(k, _)| set.has_channel(*k)).collect();
    if values.is_empty() {
        return Err("no channel to paint: pass `channels` (e.g. {\"base_color\": \"#ffffff\"}) for channels this texture set has".into());
    }
    Ok(values)
}

fn show_values(v: &BTreeMap<ChannelKind, ChannelValue>) -> Value {
    Value::Object(v.iter().map(|(k, c)| (name_of(k), crate::params::show_value(*k, *c))).collect())
}

// ----- pen paths ----------------------------------------------------------------------------------

fn smooth_tangents(anchors: &mut [PenAnchor], closed: bool) {
    let n = anchors.len();
    if n < 2 {
        return;
    }
    let pos: Vec<Vec2> = anchors.iter().map(|a| a.position).collect();
    for i in 0..n {
        let prev = if i > 0 { pos[i - 1] } else if closed { pos[n - 1] } else { pos[i] };
        let next = if i + 1 < n { pos[i + 1] } else if closed { pos[0] } else { pos[i] };
        let t = (next - prev) / 6.0;
        anchors[i].incoming = -t;
        anchors[i].outgoing = t;
    }
}

pub fn pen_path(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let view = in_view(args)?;
    let existing = match args.str("layer")? {
        Some(l) => {
            let (ts, id) = server::layer_arg(session, args, "layer")?;
            match &server::layer_ref(session, ts, id)?.content {
                LayerContent::Paint(p) if p.path.is_some() => Some((ts, id, p.path.clone().expect("checked"))),
                _ => return Err(format!("`{l}` is not a pen path layer; leave `layer` out to create one")),
            }
        }
        None => None,
    };
    // Anchors: [[u,v], ...] points (auto-smoothed) or explicit {position, in, out} objects.
    let mut ts_from_points = None;
    let mut anchors: Option<Vec<PenAnchor>> = None;
    if let Some(pts) = args.array("points")? {
        let mut out = Vec::new();
        for p in pts {
            let p = vec2(p, "a point")?;
            let uv = if view {
                let (ts, uv) = view_uv(session, args, p)?;
                if ts_from_points.is_some_and(|t| t != ts) {
                    return Err("the points land on different texture sets; a path stays on one".into());
                }
                ts_from_points = Some(ts);
                uv
            } else {
                p
            };
            out.push(PenAnchor::new(uv));
        }
        anchors = Some(out);
    } else if let Some(list) = args.array("anchors")? {
        let mut out = Vec::new();
        for a in list {
            let o = Args(a.as_object().ok_or("each anchor is {position: [u,v], in: [du,dv], out: [du,dv]}")?);
            let mut anchor = PenAnchor::new(vec2(o.get("position").ok_or("anchor needs `position`")?, "position")?);
            if let Some(v) = o.get("in") { anchor.incoming = vec2(v, "in")?; }
            if let Some(v) = o.get("out") { anchor.outgoing = vec2(v, "out")?; }
            out.push(anchor);
        }
        anchors = Some(out);
    }
    let ts = match (&existing, ts_from_points, args.str("texture_set")?) {
        (Some((ts, _, _)), _, _) => *ts,
        (None, Some(ts), _) => ts,
        (None, None, t) => server::texture_set(session, t)?,
    };
    let mut path = match &existing {
        Some((_, _, p)) => p.clone(),
        None => PenPath {
            anchors: Vec::new(),
            closed: false,
            radius: (session.brush.settings.size * 0.005).clamp(0.00001, 0.5),
            hardness: session.brush.settings.hardness.clamp(0.0, 1.0),
            opacity: session.brush.settings.stroke_opacity.clamp(0.0, 1.0),
            values: BTreeMap::new(),
            kind: subcraft_doc::PathKind::Paint,
            stamp: None,
            ribbon: None,
        },
    };
    let smooth = args.bool("smooth")?.unwrap_or(true);
    if let Some(c) = args.bool("closed")? { path.closed = c; }
    if let Some(f) = args.bool("filled")? {
        path.kind = if f { subcraft_doc::PathKind::Filled } else { subcraft_doc::PathKind::Paint };
    }
    if let Some(mut a) = anchors {
        if args.get("points").is_some() && smooth {
            smooth_tangents(&mut a, path.closed);
        }
        path.anchors = a;
    }
    if let Some(w) = args.f32("width")? { path.radius = (w * 0.5).clamp(0.00001, 0.5); }
    if let Some(h) = args.f32("hardness")? { path.hardness = h.clamp(0.0, 1.0); }
    if let Some(o) = args.f32("opacity")? { path.opacity = o.clamp(0.0, 1.0); }
    if existing.is_none() || args.get("channels").is_some() {
        path.values = channel_values(session, ts, args)?;
    }
    if path.anchors.len() < 2 {
        return Err("a path needs at least 2 points".into());
    }
    if path.anchors.iter().any(|a| a.id == AnchorId::nil()) {
        return Err("invalid anchor id".into());
    }
    path.validate()?;
    let resolution = server::ts_ref(session, ts)?.resolution;
    let mut raster = subcraft_paint::pen::PenRasterizer::new(&path, resolution)?;
    while !raster.is_done() {
        raster.step(1 << 16);
    }
    let images = raster.finish().ok_or("pen rasterization did not finish")?;
    let id = match existing {
        Some((_, id, _)) => {
            let mut layer = server::layer_ref(session, ts, id)?.clone();
            if let LayerContent::Paint(p) = &mut layer.content {
                p.path = Some(path.clone());
                p.channels = images;
            }
            if let Some(n) = args.str("name")? { layer.name = n.to_string(); }
            exec(session, Command::ReplaceLayer { layer: id, replacement: Box::new(layer), label: "Edit pen path".into() })?;
            id
        }
        None => {
            let set = server::ts_ref(session, ts)?.clone();
            let mut layer = subcraft_doc::Layer::new_paint(args.str("name")?.unwrap_or("Pen path"), &set);
            if let LayerContent::Paint(p) = &mut layer.content {
                p.path = Some(path.clone());
                p.channels = images;
            }
            let id = layer.id;
            let above = args.str("above")?.map(|a| server::layer(session, a, Some(ts)).map(|r| r.1)).transpose()?;
            select_insertion(session, ts, above)?;
            exec(session, Command::InsertLayer { layer: Box::new(layer), label: "New pen path".into() })?;
            id
        }
    };
    Ok(Reply::json(json!({
        "layer": summary(session, ts, id)?,
        "path": {
            "anchors": path.anchors.iter().map(|a| json!({ "position": a.position.to_array(), "in": a.incoming.to_array(), "out": a.outgoing.to_array() })).collect::<Vec<_>>(),
            "closed": path.is_closed(), "filled": path.kind == subcraft_doc::PathKind::Filled, "width": path.radius * 2.0, "hardness": path.hardness, "opacity": path.opacity, "channels": show_values(&path.values),
        },
    })))
}

// ----- text ---------------------------------------------------------------------------------------

fn font_ref(session: &mut Session, args: &Args, current: Option<FontRef>) -> Res<FontRef> {
    if let Some(file) = args.str("font_file")? {
        let p = paths::input(file);
        if !p.exists() {
            return Err(format!("font file not found: {}", paths::output(&p)));
        }
        return session.import_font(&p);
    }
    let bold = args.bool("bold")?.unwrap_or(false);
    let italic = args.bool("italic")?.unwrap_or(false);
    match args.str("font")? {
        Some(name) => {
            let wanted = if bold && !name.to_lowercase().contains("bold") { format!("{name} Bold") } else { name.to_string() };
            if let Some((b, _)) = subcraft_text::BUNDLED_FONTS.iter().find(|(b, _)| b.eq_ignore_ascii_case(&wanted) || b.eq_ignore_ascii_case(name)) {
                return Ok(FontRef::Bundled { name: b.to_string() });
            }
            Ok(FontRef::System { family: name.to_string(), bold, italic })
        }
        None if bold => Ok(FontRef::Bundled { name: "DejaVu Sans Bold".into() }),
        None => Ok(current.unwrap_or_default()),
    }
}

fn apply_text_args(session: &mut Session, ts: TextureSetId, args: &Args, decal: &mut TextDecal) -> Res<()> {
    if let Some(t) = args.str("text")? { decal.style.text = t.to_string(); }
    if let Some(s) = args.f32("size")? { decal.size = s.clamp(TextDecal::MIN_SIZE, TextDecal::MAX_SIZE); }
    if let Some(r) = args.f32("rotation_deg")? { decal.rotation = r.to_radians(); }
    if let Some(o) = args.f32("opacity")? { decal.opacity = o.clamp(0.0, 1.0); }
    if let Some(l) = args.f32("letter_spacing")? { decal.style.letter_spacing = l; }
    if let Some(l) = args.f32("line_spacing")? { decal.style.line_spacing = l; }
    if let Some(a) = args.str("align")? {
        decal.style.align = enum_from::<Align>("align", a, &["left", "center", "right"].map(String::from))?;
    }
    if args.get("channels").is_some() {
        decal.values = channel_values(session, ts, args)?;
    }
    if args.get("font").is_some() || args.get("font_file").is_some() || args.get("bold").is_some() {
        decal.font = font_ref(session, args, Some(decal.font.clone()))?;
    }
    Ok(())
}

/// Where to put text: `position` [u,v], or `point` [x,y] pixels with space view.
fn text_position(session: &Session, args: &Args) -> Res<Option<(Option<TextureSetId>, Vec2)>> {
    if in_view(args)? {
        let p = vec2(args.get("point").ok_or("space view needs `point` [x, y] in the rendered image")?, "point")?;
        let (ts, uv) = view_uv(session, args, p)?;
        return Ok(Some((Some(ts), uv)));
    }
    Ok(args.get("position").map(|p| vec2(p, "position")).transpose()?.map(|uv| (None, uv)))
}

pub fn text_add(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let text = args.req_str("text")?;
    let placed = text_position(session, args)?;
    let ts = match placed.and_then(|p| p.0) {
        Some(t) => t,
        None => server::texture_set(session, args.str("texture_set")?)?,
    };
    let above = args.str("above")?.map(|a| server::layer(session, a, Some(ts)).map(|r| r.1)).transpose()?;
    select_insertion(session, ts, above)?;
    let mut decal = session.new_text_decal(text);
    if let Some((_, uv)) = placed {
        decal.center = uv;
    }
    if args.get("channels").is_none() && !session.brush.material.channels.values().any(|c| c.enabled) {
        decal.values = BTreeMap::from([(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 1.0, 1.0))]);
    }
    decal.font = font_ref(session, args, None)?;
    apply_text_args(session, ts, args, &mut decal)?;
    let id = added_layer(session, ts, |s| s.add_text_layer(decal).map_err(|e| e.to_string()))?;
    if let Some(n) = args.str("name")? {
        exec(session, Command::Rename { layer: id, name: n.to_string() })?;
    }
    text_reply(session, ts, id)
}

pub fn text_update(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    exec(session, Command::SelectTextureSet(ts))?;
    exec(session, Command::SelectLayer(Some(id)))?;
    let mut decal = session.selected_text().ok_or("that layer is not a text layer")?.clone();
    if let Some((placed_ts, uv)) = text_position(session, args)? {
        if placed_ts.is_some_and(|t| t != ts) {
            return Err("that point is on another texture set than the text layer".into());
        }
        decal.center = uv;
    }
    apply_text_args(session, ts, args, &mut decal)?;
    session.update_selected_text(decal).map_err(|e| e.to_string())?;
    text_reply(session, ts, id)
}

fn text_reply(session: &Session, ts: TextureSetId, id: subcraft_core::LayerId) -> Res<Reply> {
    let layer = server::layer_ref(session, ts, id)?;
    let decal = match &layer.content { LayerContent::Paint(p) => p.text.clone(), _ => None }.ok_or("not a text layer")?;
    let font = match &decal.font {
        FontRef::Bundled { name } => json!({ "bundled": name }),
        FontRef::System { family, bold, italic } => json!({ "system": family, "bold": bold, "italic": italic }),
        FontRef::Embedded { resource } => json!({ "embedded": session.resources().get(*resource).map(|r| r.name.clone()) }),
    };
    Ok(Reply::json(json!({
        "layer": layer.name, "id": id.to_string(),
        "text": decal.style.text, "position": decal.center.to_array(), "size": decal.size,
        "rotation_deg": decal.rotation.to_degrees(), "opacity": decal.opacity, "font": font,
        "channels": show_values(&decal.values), "align": name_of(&decal.style.align),
    })))
}

pub fn fonts(server: &mut Server) -> Res<Reply> {
    let session = server.session.as_ref();
    let system: std::collections::BTreeSet<String> = subcraft_app::text::font_library().system_fonts().into_iter().map(|f| f.family).collect();
    Ok(Reply::json(json!({
        "bundled": subcraft_text::BUNDLED_FONTS.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        "system": system.into_iter().take(300).collect::<Vec<_>>(),
        "embedded_in_project": session.map(|s| s.embedded_fonts().into_iter().map(|(_, n)| n).collect::<Vec<_>>()).unwrap_or_default(),
        "tip": "Any .ttf/.otf works with font_file, e.g. C:\\Windows\\Fonts\\arial.ttf; it is embedded in the project.",
    })))
}

// ----- stencil and clone --------------------------------------------------------------------------

pub fn stencil_set(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    match args.get("image") {
        Some(Value::String(s)) if s.eq_ignore_ascii_case("none") => session.brush.stencil = None,
        Some(Value::Bool(false)) => session.brush.stencil = None,
        Some(Value::String(p)) => {
            let path = paths::input(p);
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", paths::output(&path)))?;
            let name = path.file_name().and_then(|n| n.to_str()).map(str::to_string);
            let bmp = subcraft_image::io::decode_bitmap(&bytes, name.as_deref()).map_err(|e| format!("{}: {e}", paths::output(&path)))?;
            session.brush.stencil = Some(Arc::new(subcraft_paint::Stencil::new(Arc::new(bmp))));
        }
        Some(v) => return Err(format!("`image` must be a file path or \"none\", got {v}")),
        None => {}
    }
    if let Some(cur) = session.brush.stencil.clone() {
        let mut s = (*cur).clone();
        if let Some(c) = args.get("center") { s.center = vec2(c, "center")?; }
        if let Some(v) = args.f32("scale")? { s.scale = v.max(0.01); }
        if let Some(v) = args.f32("rotation_deg")? { s.rotation = v.to_radians(); }
        if let Some(v) = args.get("tiling") { s.tiling = stencil_tiling(v)?; }
        session.brush.stencil = Some(Arc::new(s));
    } else if args.get("image").is_none() {
        return Err("no stencil is set: pass `image`".into());
    }
    let s = session.brush.stencil.as_ref();
    Ok(Reply::json(json!({
        "stencil": s.map(|s| json!({ "image_size": [s.bitmap.width, s.bitmap.height], "center": s.center.to_array(), "scale": s.scale, "rotation_deg": s.rotation.to_degrees(), "tiling": name_of(&s.tiling) })),
        "note": "the stencil masks paint_stroke / paint_fill in space \"view\" (white paints, black blocks); center and scale are fractions of that view's size",
    })))
}

pub fn clone_source(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let p = vec2(args.get("point").ok_or("missing required argument `point`")?, "point")?;
    let cam = render::orbit(session, &camera_view(args)?).paint_camera(view_size(args)?);
    if !session.set_clone_source(&cam, p) {
        return Err(format!("pixel [{}, {}] does not show the model", p.x, p.y));
    }
    session.brush.mode = subcraft_paint::StrokeMode::Clone;
    Ok(Reply::json(json!({
        "source": session.clone_source().map(|v| v.to_array()),
        "brush_mode": "clone",
        "next": "paint_stroke with space \"view\" and the same camera: the first stroke point fixes the offset to the source. brush_set mode paint to stop cloning.",
    })))
}

// ----- desktop app ---------------------------------------------------------------------------------

fn app_executable() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("SUBCRAFT_APP") {
        return Some(paths::input(&p.to_string_lossy()));
    }
    let candidates: Vec<std::path::PathBuf> = if cfg!(windows) {
        let mut v = vec![std::path::PathBuf::from(r"C:\Program Files\SubCraft\SubCraft.exe")];
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(std::path::PathBuf::from(l).join(r"Programs\SubCraft\SubCraft.exe"));
        }
        v
    } else {
        let mut v = vec![paths::input(r"C:\Program Files\SubCraft\SubCraft.exe")];
        if let Ok(user) = std::env::var("WIN_USER") {
            v.push(paths::input(&format!(r"C:\Users\{user}\AppData\Local\Programs\SubCraft\SubCraft.exe")));
        }
        v.extend(std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).map(|d| d.join("subcraft")).collect::<Vec<_>>()));
        v
    };
    candidates.into_iter().find(|p| p.is_file())
}

pub fn open_in_app(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let target = args.str("path")?.map(paths::input);
    let file = match (target, session.project_path().map(|p| p.to_path_buf())) {
        (Some(p), _) => {
            let mut p = p;
            if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case(subcraft_app::PROJECT_EXTENSION)) {
                p.set_extension(subcraft_app::PROJECT_EXTENSION);
            }
            session.save(Some(&p))?
        }
        (None, Some(p)) => {
            if session.is_modified() {
                session.save(None)?;
            }
            p
        }
        (None, None) => return Err("save the project first: pass `path` (a .subcraft file)".into()),
    };
    let app = app_executable().ok_or("SubCraft app not found; install it, or set SUBCRAFT_APP to SubCraft.exe")?;
    let mut cmd = std::process::Command::new(&app);
    let arg = if app.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) { paths::output(&file) } else { file.to_string_lossy().to_string() };
    cmd.arg(&arg).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if cfg!(target_os = "linux") && std::path::Path::new("/mnt/c").is_dir() {
        cmd.current_dir("/mnt/c"); // Windows programs started from WSL want a Windows working folder.
    }
    cmd.spawn().map_err(|e| format!("could not start {}: {e}", paths::output(&app)))?;
    Ok(Reply::json(json!({
        "opened": paths::output(&file),
        "app": paths::output(&app),
        "note": "The app works on its own copy. After editing and saving there, project_open the same file here to continue (with discard_unsaved if this side changed since).",
    })))
}

/// `tiling`: true (repeat both ways), false, or none / horizontal / vertical / both.
fn stencil_tiling(v: &Value) -> Res<subcraft_paint::StencilTiling> {
    use subcraft_paint::StencilTiling as T;
    Ok(match v {
        Value::Bool(true) => T::Both,
        Value::Bool(false) => T::None,
        Value::String(s) => match crate::params::snake(s).as_str() {
            "none" | "off" => T::None,
            "horizontal" | "x" | "u" => T::Horizontal,
            "vertical" | "y" | "v" => T::Vertical,
            "both" | "on" | "xy" => T::Both,
            other => return Err(format!("tiling must be none, horizontal, vertical or both, got `{other}`")),
        },
        other => return Err(format!("tiling must be true/false or none, horizontal, vertical, both; got {other}")),
    })
}
