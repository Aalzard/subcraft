//! JSON views of the open project for the agent: texture sets, the layer tree, one layer's
//! full parameters. Layer lists are top first (as in the Layers panel); `index` is the
//! bottom-to-top position among siblings that `layer_move` takes.

use std::sync::Arc;

use serde_json::{Value, json};
use subcraft_app::Session;
use subcraft_core::ChannelKind;
use subcraft_doc::{ChannelBlend, ContentEffectKind, FillChannel, FillSource, Layer, LayerContent, LayerKind, MaskEffect, MaskEffectKind, ResourceStore, TextureSet};

use crate::params::{name_of, round, show_value};
use crate::paths;

pub fn kind_name(l: &Layer) -> &'static str {
    match l.kind() {
        LayerKind::Paint => "paint",
        LayerKind::Path => "pen_path",
        LayerKind::Text => "text",
        LayerKind::Fill => "fill",
        LayerKind::Folder => "folder",
    }
}

pub fn project(session: &Session) -> Value {
    let mesh = &session.scene().mesh;
    let sets: Vec<Value> = session.document().texture_sets.iter().map(|ts| texture_set(session, ts)).collect();
    let hist = session.history();
    json!({
        "project_file": session.project_path().map(paths::output),
        "unsaved_changes": session.is_modified(),
        "model": {
            "name": mesh.name,
            "source_file": session.mesh_source().map(paths::output),
            "triangles": mesh.triangle_count(),
            "vertices": mesh.positions.len(),
            "objects": mesh.objects.iter().map(|o| o.name.clone()).collect::<Vec<_>>(),
            "materials": mesh.materials.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
            "bounds_min": mesh.bounds.min.to_array(),
            "bounds_max": mesh.bounds.max.to_array(),
        },
        "texture_sets": sets,
        "selected": {
            "texture_set": session.selection().texture_set.to_string(),
            "layer": session.selection().layer.map(|l| l.to_string()),
        },
        "history": { "undo": hist.undo_label(), "redo": hist.redo_label(), "steps": hist.entries().len() },
        "export_template": session.settings().export_template,
        "normal_format": session.settings().normal_format.map(|f| name_of(&f)),
    })
}

pub fn texture_set(session: &Session, ts: &TextureSet) -> Value {
    let baked: Vec<String> = session.mesh_maps(ts.id).map(|m| m.iter().map(|(k, _)| name_of(&k)).collect()).unwrap_or_default();
    json!({
        "id": ts.id.to_string(),
        "name": ts.name,
        "material_slot": ts.material_slot,
        "resolution": ts.resolution,
        "udim": ts.udim_number(),
        "channels": ts.channel_kinds().map(|k| name_of(&k)).collect::<Vec<_>>(),
        "layer_count": ts.layer_count(),
        "baked_maps": baked,
        "baked_maps_outdated": session.mesh_maps_stale(ts.id),
    })
}

/// The layer tree of a texture set, top first.
pub fn tree(session: &Session, ts: &TextureSet) -> Value {
    Value::Array(nodes(session.resources(), ts, &ts.layers, false))
}

fn nodes(res: &ResourceStore, ts: &TextureSet, layers: &[Arc<Layer>], detail: bool) -> Vec<Value> {
    layers.iter().enumerate().rev().map(|(i, l)| node(res, ts, l, i, detail)).collect()
}

fn node(res: &ResourceStore, ts: &TextureSet, l: &Layer, index: usize, detail: bool) -> Value {
    let mut v = json!({
        "id": l.id.to_string(),
        "name": l.name,
        "type": kind_name(l),
        "visible": l.visible,
        "index": index,
        "blend": blend(ts, &l.blend),
    });
    let o = v.as_object_mut().expect("object");
    if let Some(m) = &l.mask {
        o.insert("mask".into(), json!({
            "enabled": m.enabled,
            "base": if m.base.fill()[0] >= 0.5 { "white" } else { "black" },
            "base_painted": m.base.allocated_tiles() > 0,
            "effects": m.effects.iter().rev().map(|e| mask_effect(e, detail)).collect::<Vec<_>>(),
        }));
    }
    if !l.content_effects.is_empty() {
        o.insert("content_effects".into(), Value::Array(l.content_effects.iter().rev().map(|e| {
            let mut ev = json!({ "id": e.id.to_string(), "name": e.name, "visible": e.visible, "type": content_effect_type(&e.kind) });
            if detail { ev["params"] = content_effect_params(res, &e.kind); }
            ev
        }).collect()));
    }
    if let Some(p) = &l.parts {
        o.insert("parts".into(), json!({ "enabled": p.enabled, "kind": name_of(&p.kind), "ids": p.parts.iter().collect::<Vec<_>>() }));
    }
    match &l.content {
        LayerContent::Fill(f) => {
            let channels: serde_json::Map<String, Value> = f.channels.iter().filter(|(_, c)| c.enabled || detail).map(|(k, c)| (name_of(k), fill_channel(res, *k, c))).collect();
            o.insert("fill".into(), json!({ "projection": f.projection, "channels": channels }));
        }
        LayerContent::Paint(p) => {
            let painted: Vec<String> = p.channels.iter().filter(|(_, img)| img.allocated_tiles() > 0).map(|(k, _)| name_of(k)).collect();
            o.insert("painted_channels".into(), json!(painted));
        }
        LayerContent::Group(g) => {
            o.insert("children".into(), Value::Array(nodes(res, ts, &g.children, detail)));
        }
    }
    v
}

/// A short fill source: `0.45`, `"#8a6f5a"`, `"procedural clouds (#5a4030..#a08060)"`, ...
fn short_source(res: &ResourceStore, kind: ChannelKind, s: &FillSource) -> Value {
    match s {
        FillSource::Uniform { value } => show_value(kind, *value),
        FillSource::Bitmap { resource } => json!(format!("image {}", res.get(*resource).map(|r| r.name.as_str()).unwrap_or("?"))),
        FillSource::Procedural { params, low, high } => json!(format!("procedural {} ({}..{})", name_of(&params.kind), show_value(kind, *low), show_value(kind, *high)).replace('"', "")),
        FillSource::MeshMap { map } => json!(format!("mesh map {}", name_of(map))),
    }
}

/// What an edit tool returns: enough to confirm the change, without every parameter
/// (`layer_get` has those). Folders list their children by name only.
pub fn brief(session: &Session, ts: &TextureSet, l: &Layer) -> Value {
    let res = session.resources();
    let index = ts.location(l.id).map(|loc| loc.index).unwrap_or(0);
    let mut v = json!({ "id": l.id.to_string(), "name": l.name, "type": kind_name(l), "visible": l.visible, "index": index });
    let o = v.as_object_mut().expect("object");
    let b = blend(ts, &l.blend);
    let default_blend = b.get("mode").and_then(Value::as_str).is_some_and(|m| m == "normal" || m == "passthrough") && b.get("opacity").and_then(Value::as_f64) == Some(1.0);
    if !default_blend {
        o.insert("blend".into(), b);
    }
    if let Some(m) = &l.mask {
        o.insert("mask".into(), json!({
            "enabled": m.enabled,
            "base": if m.base.fill()[0] >= 0.5 { "white" } else { "black" },
            "effects": m.effects.iter().rev().map(|e| format!("{} [{}]", e.name, e.id)).collect::<Vec<_>>(),
        }));
    }
    if !l.content_effects.is_empty() {
        o.insert("content_effects".into(), json!(l.content_effects.iter().rev().map(|e| format!("{} [{}]", e.name, e.id)).collect::<Vec<_>>()));
    }
    match &l.content {
        LayerContent::Fill(f) => {
            let channels: serde_json::Map<String, Value> = f.channels.iter().filter(|(_, c)| c.enabled).map(|(k, c)| {
                let mut s = short_source(res, *k, &c.source);
                if c.strength != 1.0 {
                    s = json!(format!("{} x{}", s.to_string().trim_matches('"'), round(c.strength)));
                }
                (name_of(k), s)
            }).collect();
            o.insert("fill".into(), Value::Object(channels));
            if f.projection != subcraft_doc::Projection::default() {
                o.insert("projection".into(), json!(f.projection));
            }
        }
        LayerContent::Paint(p) => {
            let painted: Vec<String> = p.channels.iter().filter(|(_, img)| img.allocated_tiles() > 0).map(|(k, _)| name_of(k)).collect();
            o.insert("painted_channels".into(), json!(painted));
            if p.path.is_some() { o.insert("pen_path".into(), json!(true)); }
            if let Some(t) = &p.text { o.insert("text".into(), json!(t.style.text)); }
        }
        LayerContent::Group(g) => {
            o.insert("children".into(), json!(g.children.iter().rev().map(|c| format!("{} ({}) [{}]", c.name, kind_name(c), c.id)).collect::<Vec<_>>()));
        }
    }
    v
}

/// Names-only tree, top first: `["Grime (fill)", "Details (folder)", ["Blood (paint)"], ...]`.
pub fn outline(ts: &TextureSet) -> Value {
    fn walk(layers: &[Arc<Layer>]) -> Vec<Value> {
        let mut out = Vec::new();
        for l in layers.iter().rev() {
            out.push(json!(format!("{} ({}){}", l.name, kind_name(l), if l.visible { "" } else { ", hidden" })));
            if l.is_folder() {
                out.push(Value::Array(walk(l.children())));
            }
        }
        out
    }
    Value::Array(walk(&ts.layers))
}

/// One layer with every parameter (mask effect settings, fill sources, content effects).
pub fn layer(session: &Session, ts: &TextureSet, l: &Layer) -> Value {
    let index = ts.location(l.id).map(|loc| loc.index).unwrap_or(0);
    let mut v = node(session.resources(), ts, l, index, true);
    v["texture_set"] = json!(ts.name);
    v
}

fn blend(ts: &TextureSet, blend: &std::collections::BTreeMap<ChannelKind, ChannelBlend>) -> Value {
    let entries: Vec<(ChannelKind, &ChannelBlend)> = ts.channel_kinds().filter_map(|k| blend.get(&k).map(|b| (k, b))).collect();
    let same = |b: &&ChannelBlend| entries.iter().filter(|(k, _)| *k != ChannelKind::Normal).all(|(_, o)| o.mode == b.mode && o.opacity == b.opacity);
    if let Some((_, first)) = entries.iter().find(|(k, _)| *k != ChannelKind::Normal)
        && same(first) {
            let mut v = json!({ "mode": name_of(&first.mode), "opacity": round(first.opacity) });
            if let Some((_, n)) = entries.iter().find(|(k, _)| *k == ChannelKind::Normal) {
                v["normal"] = json!({ "mode": name_of(&n.mode), "opacity": round(n.opacity) });
            }
            return v;
        }
    let per: serde_json::Map<String, Value> = entries.iter().map(|(k, b)| (name_of(k), json!({ "mode": name_of(&b.mode), "opacity": round(b.opacity) }))).collect();
    json!({ "per_channel": per })
}

pub fn fill_channel(res: &ResourceStore, kind: ChannelKind, c: &FillChannel) -> Value {
    json!({ "enabled": c.enabled, "strength": round(c.strength), "source": fill_source(res, kind, &c.source) })
}

pub fn fill_source(res: &ResourceStore, kind: ChannelKind, s: &FillSource) -> Value {
    match s {
        FillSource::Uniform { value } => json!({ "type": "uniform", "value": show_value(kind, *value) }),
        FillSource::Bitmap { resource } => json!({ "type": "image", "resource": resource.to_string(), "name": res.get(*resource).map(|r| r.name.clone()) }),
        FillSource::Procedural { params, low, high } => json!({ "type": "procedural", "params": params, "low": show_value(kind, *low), "high": show_value(kind, *high) }),
        FillSource::MeshMap { map } => json!({ "type": "mesh_map", "map": name_of(map) }),
    }
}

pub fn mask_effect_type(k: &MaskEffectKind) -> &'static str {
    match k {
        MaskEffectKind::Paint { .. } => "paint",
        MaskEffectKind::Fill { .. } => "fill",
        MaskEffectKind::Generator(_) => "generator",
        MaskEffectKind::Levels(_) => "levels",
        MaskEffectKind::Filter(_) => "filter",
        MaskEffectKind::Invert => "invert",
        MaskEffectKind::Image { .. } => "image",
        MaskEffectKind::IdColor(_) => "id_color",
    }
}

pub fn mask_effect(e: &MaskEffect, detail: bool) -> Value {
    let mut v = json!({ "id": e.id.to_string(), "name": e.name, "type": mask_effect_type(&e.kind), "visible": e.visible, "blend": name_of(&e.blend), "opacity": round(e.opacity) });
    if detail {
        v["params"] = match &e.kind {
            MaskEffectKind::Paint { image } => json!({ "painted": image.allocated_tiles() > 0 }),
            MaskEffectKind::Fill { value, procedural, projection } => json!({ "value": round(*value), "procedural": procedural, "projection": projection }),
            MaskEffectKind::Generator(g) => serde_json::to_value(g).unwrap_or(Value::Null),
            MaskEffectKind::Levels(l) => serde_json::to_value(l).unwrap_or(Value::Null),
            MaskEffectKind::Filter(f) => serde_json::to_value(f).unwrap_or(Value::Null),
            MaskEffectKind::Invert => json!({}),
            MaskEffectKind::Image { resource, projection, invert } => json!({ "resource": resource.to_string(), "projection": projection, "invert": invert }),
            MaskEffectKind::IdColor(c) => serde_json::to_value(c).unwrap_or(Value::Null),
        };
    }
    v
}

pub fn content_effect_type(k: &ContentEffectKind) -> &'static str {
    match k {
        ContentEffectKind::Paint { .. } => "paint",
        ContentEffectKind::Fill(_) => "fill",
        ContentEffectKind::Levels { .. } => "levels",
        ContentEffectKind::Filter { .. } => "filter",
    }
}

fn content_effect_params(res: &ResourceStore, k: &ContentEffectKind) -> Value {
    match k {
        ContentEffectKind::Paint { channels } => json!({ "painted_channels": channels.iter().filter(|(_, i)| i.allocated_tiles() > 0).map(|(k, _)| name_of(k)).collect::<Vec<_>>() }),
        ContentEffectKind::Fill(f) => {
            let channels: serde_json::Map<String, Value> = f.channels.iter().map(|(k, c)| (name_of(k), fill_channel(res, *k, c))).collect();
            json!({ "projection": f.projection, "channels": channels })
        }
        ContentEffectKind::Levels { channel, levels } => json!({ "channel": name_of(channel), "levels": levels }),
        ContentEffectKind::Filter { channel, filter } => json!({ "channel": name_of(channel), "filter": filter }),
    }
}
