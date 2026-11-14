//! Brush settings and painting: strokes and polygon fills in UV space or in a rendered view.

use glam::Vec2;
use serde_json::{Value, json};
use subcraft_app::{Command, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;
use subcraft_paint::{Alignment, PolygonFillMode, StrokeMode};
use subcraft_ui::orbit::CameraView;

use crate::params::{Args, Res, channel, channel_value, enum_from, name_of, names, show_value, with_defaults};
use crate::render;
use crate::server::{self, Reply, Server, exec};
use crate::tools::layers::{find_named, select_target};

pub fn camera_view(args: &Args) -> Res<CameraView> {
    let d = render::default_view();
    let offset = match args.array("offset")? {
        Some(a) if a.len() == 3 => glam::Vec3::new(
            a[0].as_f64().unwrap_or(0.0) as f32,
            a[1].as_f64().unwrap_or(0.0) as f32,
            a[2].as_f64().unwrap_or(0.0) as f32,
        ),
        Some(_) => return Err("`offset` must be [x, y, z]".into()),
        None => d.offset,
    };
    Ok(CameraView {
        yaw_deg: args.f32("yaw")?.unwrap_or(d.yaw_deg),
        pitch_deg: args.f32("pitch")?.unwrap_or(d.pitch_deg),
        zoom: args.f32("zoom")?.unwrap_or(d.zoom).max(1e-3),
        offset,
    })
}

pub fn view_size(args: &Args) -> Res<[u32; 2]> {
    let w = args.u32("width")?.unwrap_or(768).clamp(16, 4096);
    let h = args.u32("height")?.unwrap_or(w).clamp(16, 4096);
    Ok([w, h])
}

fn brush_json(session: &Session) -> Value {
    let b = &session.brush;
    let channels: serde_json::Map<String, Value> = b.material.channels.iter()
        .map(|(k, c)| (name_of(k), if c.enabled { show_value(*k, c.value) } else { Value::Bool(false) }))
        .collect();
    let mode = match b.mode { StrokeMode::Paint => "paint", StrokeMode::Erase => "erase", StrokeMode::Smudge => "smudge", StrokeMode::Clone => "clone" };
    let symmetry: String = ["x", "y", "z"].iter().zip(b.options.symmetry).filter(|(_, on)| *on).map(|(a, _)| *a).collect();
    json!({
        "mode": mode,
        "tip": b.alpha_name,
        "size": b.settings.size, "flow": b.settings.flow, "opacity": b.settings.stroke_opacity,
        "hardness": b.settings.hardness, "spacing": b.settings.spacing, "angle": b.settings.angle_deg,
        "channels": channels,
        "mask_value": b.material.mask_value,
        "symmetry": symmetry,
        "settings": b.settings,
        "options": b.options,
    })
}

/// Apply brush_set-style arguments.
pub fn apply_brush(server: &mut Server, a: &Args) -> Res<()> {
    let (library, session) = server.library_and_session()?;
    let mut s = session.brush.settings;
    if let Some(patch) = a.obj("settings")? {
        s = with_defaults("brush settings", &s, &Value::Object(patch.clone()))?;
    }
    if let Some(v) = a.f32("size")? { s.size = v.max(0.01); }
    if let Some(v) = a.f32("flow")? { s.flow = v.clamp(0.0, 1.0); }
    if let Some(v) = a.f32("opacity")? { s.stroke_opacity = v.clamp(0.0, 1.0); }
    if let Some(v) = a.f32("hardness")? { s.hardness = v.clamp(0.0, 1.0); }
    if let Some(v) = a.f32("spacing")? { s.spacing = v.max(0.01); }
    if let Some(v) = a.f32("angle")? { s.angle_deg = v; }
    if let Some(al) = a.str("alignment")? {
        s.alignment = enum_from::<Alignment>("alignment", al, &names(&[Alignment::TangentWrap, Alignment::TangentPlanar, Alignment::Camera, Alignment::Uv]))?;
    }
    if s != session.brush.settings {
        exec(session, Command::SetBrushSettings(s))?;
    }
    let mut o = session.brush.options;
    if let Some(patch) = a.obj("options")? {
        o = with_defaults("stroke options", &o, &Value::Object(patch.clone()))?;
    }
    if let Some(sym) = a.str("symmetry")? {
        let sym = sym.to_ascii_lowercase();
        o.symmetry = [sym.contains('x'), sym.contains('y'), sym.contains('z')];
    }
    if o != session.brush.options {
        exec(session, Command::SetStrokeOptions(o))?;
    }
    let mut m = session.brush.material.clone();
    let mut changed = false;
    if let Some(chs) = a.obj("channels")? {
        let listed: Vec<ChannelKind> = chs.keys().map(|k| channel(k)).collect::<Res<_>>()?;
        for (name, v) in chs {
            let k = channel(name)?;
            match v {
                Value::Bool(false) => m.disable(k),
                Value::Bool(true) => {
                    let value = m.channels.get(&k).map(|c| c.value).unwrap_or_else(|| k.default_paint_value());
                    m.set(k, value);
                }
                v => m.set(k, channel_value(k, v, false)?),
            }
        }
        if a.bool("exclusive")?.unwrap_or(false) {
            let others: Vec<ChannelKind> = m.channels.keys().copied().filter(|k| !listed.contains(k)).collect();
            for k in others {
                m.disable(k);
            }
        }
        changed = true;
    }
    if let Some(v) = a.f32("mask_value")? {
        m.mask_value = v.clamp(0.0, 1.0);
        changed = true;
    }
    if changed {
        exec(session, Command::SetBrushMaterial(m))?;
    }
    if let Some(mode) = a.str("mode")? {
        session.brush.mode = match mode.to_ascii_lowercase().as_str() {
            "paint" | "brush" => StrokeMode::Paint,
            "erase" | "eraser" => StrokeMode::Erase,
            "smudge" => StrokeMode::Smudge,
            "clone" => {
                if session.clone_source().is_none() {
                    return Err("set a clone source first with clone_source".into());
                }
                StrokeMode::Clone
            }
            other => return Err(format!("mode must be paint, erase, smudge or clone, got `{other}`")),
        };
    }
    if let Some(tip) = a.str("tip")? {
        let alpha = find_named(&library.alphas, |t| &t.name, tip, "brush tip")?.clone();
        session.set_brush_alpha(&alpha);
    }
    Ok(())
}

pub fn brush_set(server: &mut Server, args: &Args) -> Res<Reply> {
    apply_brush(server, args)?;
    Ok(Reply::json(json!({ "brush": brush_json(server.session()?) })))
}

fn point(v: &Value) -> Res<(Vec2, Option<f32>)> {
    let a = v.as_array().filter(|a| a.len() == 2 || a.len() == 3).ok_or_else(|| format!("a point is [x, y] or [x, y, pressure], got {v}"))?;
    let n = |i: usize| a[i].as_f64().map(|f| f as f32).ok_or_else(|| format!("point coordinates must be numbers, got {v}"));
    Ok((Vec2::new(n(0)?, n(1)?), if a.len() == 3 { Some(n(2)?.clamp(0.0, 1.0)) } else { None }))
}

enum Space {
    Uv,
    View(subcraft_paint::Camera),
}

/// Points must lie near what they paint: UV from -1 to 2, view pixels within ±65536. A far point
/// used to make the stroke run (and allocate) until the server died.
fn in_range(space: &Space, p: Vec2) -> Res<()> {
    let (lo, hi, what) = match space {
        Space::Uv => (-1.0, 2.0, "UV coordinates must be between -1 and 2"),
        Space::View(_) => (-65536.0, 65536.0, "view coordinates must be within ±65536 pixels"),
    };
    if p.is_finite() && p.min_element() >= lo && p.max_element() <= hi { Ok(()) } else { Err(format!("point [{}, {}] is out of range: {what}", p.x, p.y)) }
}

fn space(session: &Session, args: &Args) -> Res<Space> {
    Ok(match args.str("space")?.unwrap_or("uv").to_ascii_lowercase().as_str() {
        "uv" => Space::Uv,
        "view" | "screen" | "pixels" => {
            let cam = render::orbit(session, &camera_view(args)?);
            Space::View(cam.paint_camera(view_size(args)?))
        }
        other => return Err(format!("space must be uv or view, got `{other}`")),
    })
}

fn prepare(server: &mut Server, args: &Args) -> Res<(subcraft_core::TextureSetId, subcraft_core::LayerId, bool)> {
    if let Some(b) = args.obj("brush")? {
        apply_brush(server, &Args(b))?;
    }
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let mask = match args.str("target")?.unwrap_or("content").to_ascii_lowercase().as_str() {
        "content" | "layer" => false,
        "mask" => true,
        other => return Err(format!("target must be content or mask, got `{other}`")),
    };
    let l = server::layer_ref(session, ts, id)?;
    if mask && l.mask.is_none() {
        return Err("the layer has no mask: mask_add first".into());
    }
    if !mask && !matches!(l.content, LayerContent::Paint(_)) {
        return Err("paint on a paint layer (layer_add type paint), on a mask (target mask), or add a paint content effect".into());
    }
    exec(session, Command::SelectTextureSet(ts))?;
    select_target(session, id, mask)?;
    Ok((ts, id, mask))
}

/// Run `f` with the call's `brush` arguments applied, then restore the brush: a per-call brush
/// does not leak into later calls (brush_set is the persistent way).
fn with_call_brush(server: &mut Server, args: &Args, f: impl FnOnce(&mut Server, &Args) -> Res<Reply>) -> Res<Reply> {
    let saved = server.session.as_ref().map(|s| s.brush.clone());
    let out = f(server, args);
    if let (Some(b), Some(s)) = (saved, server.session.as_mut())
        && args.get("brush").is_some() {
            s.brush = b;
        }
    out
}

pub fn stroke(server: &mut Server, args: &Args) -> Res<Reply> {
    with_call_brush(server, args, stroke_inner)
}

pub fn fill(server: &mut Server, args: &Args) -> Res<Reply> {
    with_call_brush(server, args, fill_inner)
}

fn stroke_inner(server: &mut Server, args: &Args) -> Res<Reply> {
    let (ts, id, mask) = prepare(server, args)?;
    let session = server.session()?;
    let space = space(session, args)?;
    if matches!(space, Space::Uv) && session.brush.mode == StrokeMode::Clone {
        return Err("the clone tool works in space \"view\" (the source offset is on screen)".into());
    }
    if matches!(space, Space::Uv) && session.brush.stencil.is_some() {
        eprintln!("[subcraft-mcp] note: the stencil only applies to view-space strokes");
    }
    let mut strokes: Vec<&Vec<Value>> = Vec::new();
    if let Some(p) = args.array("points")? {
        strokes.push(p);
    }
    if let Some(s) = args.array("strokes")? {
        for st in s {
            strokes.push(st.as_array().ok_or("each stroke is a list of points")?);
        }
    }
    if strokes.is_empty() || strokes.iter().all(|s| s.is_empty()) {
        return Err("give `points` (one stroke) or `strokes` (several)".into());
    }
    let mut painted = 0usize;
    let mut tiles = 0usize;
    let head = |s: &Session| s.history().entries().get(s.history().cursor()).map(|e| e.time);
    let head_before = head(session);
    let resolution = server::ts_ref(session, ts)?.resolution as f32;
    let mut entries = 0usize;
    // Every point is read and checked before anything is painted, so a bad point in a later
    // stroke does not leave the earlier ones painted behind an error.
    let all = strokes
        .iter()
        .filter(|s| !s.is_empty())
        .map(|pts| pts.iter().map(|v| point(v).and_then(|(p, pressure)| in_range(&space, p).map(|()| (p, pressure)))).collect::<Res<Vec<_>>>())
        .collect::<Res<Vec<_>>>()?;
    for mut parsed in all {
        if parsed.len() == 1 {
            // A dab: the stroke path only stamps once the brush moves, so nudge it by half a texel/pixel.
            let nudge = match space { Space::Uv => 0.5 / resolution, Space::View(_) => 0.5 };
            parsed.push((parsed[0].0 + Vec2::new(nudge, 0.0), parsed[0].1));
        }
        let before = head(session);
        session.begin_stroke().map_err(|e| e.to_string())?;
        let mut result = Ok(());
        for (p, pressure) in parsed {
            session.set_stroke_pressure(pressure.unwrap_or(1.0));
            let r = match &space {
                Space::Uv => session.stroke_to_uv(p, false, 1.0),
                Space::View(cam) => session.stroke_to(cam, p, false),
            };
            match r {
                Ok(t) => tiles += t.len(),
                Err(e) => { result = Err(e.to_string()); break; }
            }
        }
        if let Err(e) = result {
            session.cancel_stroke();
            return Err(e);
        }
        session.end_stroke();
        if head(session) != before {
            entries += 1;
        }
        painted += 1;
    }
    // Several strokes in one call become one undo step.
    if entries > 1 {
        let layers = server::ts_ref(session, ts)?.layers.clone();
        for _ in 0..entries {
            exec(session, Command::Undo)?;
        }
        exec(session, Command::ReplaceTextureSetLayers { texture_set: ts, layers, label: format!("Paint {entries} strokes") })?;
    }
    let layer = server::layer_ref(session, ts, id)?.name.clone();
    let changed = head(session) != head_before;
    let note = if tiles == 0 {
        Some("nothing was painted: the points missed the model. In uv space, put them inside UV islands (render_texture shows them; gaps between islands are empty); in view space, on pixels that show the model (pick tells)")
    } else if !changed {
        Some("the brush covered texels but nothing changed: the layer already had exactly this paint there, or the brush has no enabled channel the layer uses")
    } else {
        None
    };
    Ok(Reply::json(json!({ "strokes": painted, "layer": layer, "target": if mask { "mask" } else { "content" }, "tiles_touched": tiles, "changed": changed, "note": note })))
}

fn fill_inner(server: &mut Server, args: &Args) -> Res<Reply> {
    let (ts, id, _) = prepare(server, args)?;
    let session = server.session()?;
    let mode: PolygonFillMode = enum_from("fill mode", args.str("mode")?.unwrap_or("mesh_element"), &names(&[PolygonFillMode::Triangle, PolygonFillMode::MeshElement, PolygonFillMode::UvIsland]))?;
    let (p, _) = point(args.get("point").ok_or("missing required argument `point`")?)?;
    let tiles = match space(session, args)? {
        Space::Uv => session.polygon_fill_uv(p, mode),
        Space::View(cam) => session.polygon_fill(&cam, p, mode),
    }
    .map_err(|e| e.to_string())?;
    let layer = server::layer_ref(session, ts, id)?.name.clone();
    Ok(Reply::json(json!({ "filled": !tiles.is_empty(), "layer": layer, "mode": name_of(&mode) })))
}

pub fn pick(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let cam = render::orbit(session, &camera_view(args)?).paint_camera(view_size(args)?);
    let pts = args.array("points")?.ok_or("missing required argument `points`")?;
    let sets = &session.document().texture_sets;
    let out: Vec<Value> = pts.iter().map(|v| -> Res<Value> {
        let (p, _) = point(v)?;
        Ok(match session.pick(&cam, p) {
            None => json!({ "pixel": [p.x, p.y], "hit": false }),
            Some(h) => json!({
                "pixel": [p.x, p.y], "hit": true,
                "uv": [h.uv.x, h.uv.y],
                "position": h.position.to_array(), "normal": h.normal.to_array(),
                "texture_set": sets.iter().find(|t| t.material_slot == h.material_slot).map(|t| t.name.clone()),
                "part": session.part_at(&cam, p),
            }),
        })
    }).collect::<Res<_>>()?;
    Ok(Reply::json(json!({ "points": out })))
}
