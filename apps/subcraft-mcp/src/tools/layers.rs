//! Layer stack tools: layers, fills, masks, effects, mesh parts and the shelf.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId, PartKind, TextureSetId};
use subcraft_doc::{
    BlendMode, ContentEffectKind, FillChannel, FillContent, FillSource, Filter, Generator, GeneratorKind, LayerContent, Levels, MaskEffectKind,
    PartMask, ProceduralKind, ProceduralParams, Projection,
};

use crate::describe;
use crate::params::{Args, Res, channel, channel_value, enum_from, mesh_map, name_of, names, with_defaults};
use crate::paths;
use crate::server::{self, Reply, Server, Stack, exec};

// ----- listing -----------------------------------------------------------------------------------

pub fn list(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    if let Some(t) = args.str("texture_set")? {
        let id = server::texture_set(session, Some(t))?;
        let ts = server::ts_ref(session, id)?;
        return Ok(Reply::json(json!({ "texture_set": ts.name, "order": "top first", "layers": describe::tree(session, ts) })));
    }
    let sets: Vec<Value> = session.document().texture_sets.iter().map(|ts| json!({ "texture_set": ts.name, "id": ts.id.to_string(), "layers": describe::tree(session, ts) })).collect();
    Ok(Reply::json(json!({ "order": "top first", "texture_sets": sets })))
}

pub fn get(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let t = server::ts_ref(session, ts)?;
    let l = t.find(id).ok_or("layer not found")?;
    Ok(Reply::json(describe::layer(session, t, l)))
}

fn all_layer_ids(session: &Session, ts: TextureSetId) -> BTreeSet<LayerId> {
    let mut ids = BTreeSet::new();
    if let Some(t) = session.document().texture_set(ts) {
        t.walk(|l, _| {
            ids.insert(l.id);
        });
    }
    ids
}

/// Run something that adds layer(s) to `ts`, and return the new top-level-most one.
pub(crate) fn added_layer(session: &mut Session, ts: TextureSetId, f: impl FnOnce(&mut Session) -> Res<()>) -> Res<LayerId> {
    let before = all_layer_ids(session, ts);
    f(session)?;
    let after = all_layer_ids(session, ts);
    let new: Vec<LayerId> = after.difference(&before).copied().collect();
    let t = server::ts_ref(session, ts)?;
    // The new node whose parent is not itself new (a smart material adds a folder with children).
    new.iter()
        .copied()
        .find(|id| t.location(*id).is_none_or(|loc| loc.parent.is_none_or(|p| !new.contains(&p))))
        .or_else(|| session.selection().layer)
        .ok_or_else(|| "the layer was not added".to_string())
}

pub(crate) fn select_insertion(session: &mut Session, ts: TextureSetId, above: Option<LayerId>) -> Res<()> {
    exec(session, Command::SelectTextureSet(ts))?;
    exec(session, Command::SelectLayer(above))
}

fn siblings_len(session: &Session, ts: TextureSetId, parent: Option<LayerId>) -> Res<usize> {
    let t = server::ts_ref(session, ts)?;
    Ok(match parent {
        None => t.layers.len(),
        Some(p) => t.find(p).ok_or("parent not found")?.children().len(),
    })
}

pub(crate) fn summary(session: &Session, ts: TextureSetId, id: LayerId) -> Res<Value> {
    let t = server::ts_ref(session, ts)?;
    let l = t.find(id).ok_or("layer not found")?;
    Ok(describe::brief(session, t, l))
}

// ----- add / edit / delete / move ------------------------------------------------------------------

pub fn add(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let ts = server::texture_set(session, args.str("texture_set")?)?;
    let kind = match args.req_str("type")?.to_ascii_lowercase().as_str() {
        "paint" => NewLayer::Paint,
        "fill" => NewLayer::Fill,
        "folder" | "group" => NewLayer::Folder,
        other => return Err(format!("type must be paint, fill or folder, got `{other}`")),
    };
    let above = args.str("above")?.map(|a| server::layer(session, a, Some(ts)).map(|r| r.1)).transpose()?;
    let parent = args.str("parent")?.map(|p| server::layer(session, p, Some(ts)).map(|r| r.1)).transpose()?;
    if (args.obj("fill")?.is_some() || args.obj("projection")?.is_some()) && kind != NewLayer::Fill {
        return Err("`fill` and `projection` need type fill".into());
    }
    if let Some(p) = parent
        && !server::layer_ref(session, ts, p)?.is_folder() {
            return Err("`parent` must be a folder layer".into());
        }
    if let Some(chs) = args.obj("fill")? {
        ensure_channels(session, ts, chs)?;
    }
    // Build the whole layer, then insert it as one undo step.
    let set = server::ts_ref(session, ts)?.clone();
    let (word, label) = match kind {
        NewLayer::Paint => ("Paint", "New Paint Layer"),
        NewLayer::Fill => ("Fill", "New Fill Layer"),
        NewLayer::Folder => ("Folder", "New Folder"),
    };
    let name = match args.str("name")? {
        Some(n) => n.to_string(),
        None => format!("{word} {}", set.layer_count() + 1),
    };
    let mut layer = match kind {
        NewLayer::Paint => subcraft_doc::Layer::new_paint(name, &set),
        NewLayer::Fill => subcraft_doc::Layer::new_fill(name, &set),
        NewLayer::Folder => subcraft_doc::Layer::new_folder(name, &set),
    };
    if let LayerContent::Fill(fill) = &mut layer.content {
        if let Some(chs) = args.obj("fill")? {
            edit_fill(session, fill, chs, args.bool("exclusive")?.unwrap_or(true))?;
        }
        if let Some(p) = args.obj("projection")? {
            fill.projection = crate::params::projection_with_defaults(&fill.projection, &Value::Object(p.clone()))?;
        }
    }
    if let Some(m) = args.str("mask")? {
        layer.mask = Some(subcraft_doc::Mask::new(set.resolution, !m.eq_ignore_ascii_case("black")));
    }
    let id = layer.id;
    select_insertion(session, ts, above)?;
    exec(session, Command::InsertLayer { layer: Box::new(layer), label: label.to_string() })?;
    if let Some(p) = parent {
        let index = siblings_len(session, ts, Some(p))?;
        exec(session, Command::MoveLayer { layer: id, parent: Some(p), index })?;
    }
    Ok(Reply::json(json!({ "added": summary(session, ts, id)? })))
}

pub fn blend_mode(s: &str) -> Res<BlendMode> {
    let all = [
        "normal", "passthrough", "disable", "replace", "multiply", "divide", "inverse_divide", "darken", "lighten", "linear_dodge", "subtract",
        "inverse_subtract", "difference", "exclusion", "signed_addition", "overlay", "screen", "linear_burn", "color_burn", "color_dodge",
        "soft_light", "hard_light", "vivid_light", "linear_light", "pin_light", "tint", "saturation", "color", "value", "normal_map_combine",
        "normal_map_detail", "normal_map_inverse_detail",
    ];
    let s = match crate::params::snake(s).as_str() {
        "add" | "additive" => "linear_dodge".to_string(),
        "pass_through" => "passthrough".to_string(),
        "off" | "none" => "disable".to_string(),
        other => other.to_string(),
    };
    enum_from("blend mode", &s, &all.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

pub fn update(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    // Every change is applied to a copy and committed as one undo step.
    let mut layer = server::layer_ref(session, ts, id)?.clone();
    let mut what = Vec::new();
    if let Some(name) = args.str("name")? {
        layer.name = name.to_string();
        what.push("name");
    }
    if let Some(v) = args.bool("visible")? {
        layer.visible = v;
        what.push(if v { "show" } else { "hide" });
    }
    let channels: Vec<ChannelKind> = server::ts_ref(session, ts)?.channel_kinds().collect();
    let set_blend = |layer: &mut subcraft_doc::Layer, k: ChannelKind, mode: Option<BlendMode>, opacity: Option<f32>| {
        let b = layer.blend.entry(k).or_insert_with(|| subcraft_doc::ChannelBlend::new(BlendMode::Normal));
        if let Some(m) = mode { b.mode = m; }
        if let Some(o) = opacity { b.opacity = o.clamp(0.0, 1.0); }
    };
    if let Some(mode) = args.str("blend_mode")? {
        let mode = blend_mode(mode)?;
        for &k in channels.iter().filter(|k| **k != ChannelKind::Normal || mode == BlendMode::Disable || mode == BlendMode::Passthrough) {
            set_blend(&mut layer, k, Some(mode), None);
        }
        what.push("blend mode");
    }
    if let Some(o) = args.f32("opacity")? {
        for &k in &channels {
            set_blend(&mut layer, k, None, Some(o));
        }
        what.push("opacity");
    }
    if let Some(per) = args.obj("channels")? {
        for (name, spec) in per {
            let k = channel(name)?;
            if !channels.contains(&k) {
                return Err(format!("texture set has no {} channel", k.display_name()));
            }
            let spec = Args(spec.as_object().ok_or_else(|| format!("channels.{name} must be {{mode, opacity}}"))?);
            let mode = spec.str("mode")?.map(blend_mode).transpose()?;
            set_blend(&mut layer, k, mode, spec.f32("opacity")?);
        }
        what.push("channel blending");
    }
    if let Some(e) = args.bool("mask_enabled")? {
        layer.mask.as_mut().ok_or("the layer has no mask")?.enabled = e;
        what.push("mask");
    }
    if let Some(e) = args.bool("expanded")?
        && let LayerContent::Group(g) = &mut layer.content { g.expanded = e; }
    if what.is_empty() && args.get("expanded").is_none() {
        return Err("nothing to change: pass name, visible, blend_mode, opacity, channels, mask_enabled or expanded".into());
    }
    let label = format!("Layer {}: {}", what.join(", "), layer.name);
    exec(session, Command::ReplaceLayer { layer: id, replacement: Box::new(layer), label })?;
    Ok(Reply::json(json!({ "updated": summary(session, ts, id)? })))
}

pub fn delete(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let within = args.str("texture_set")?.map(|t| server::texture_set(session, Some(t))).transpose()?;
    let keys = args.strings("layers")?.ok_or("missing required argument `layers`")?;
    let found: Vec<(TextureSetId, LayerId)> = keys.iter().map(|k| server::layer(session, k, within)).collect::<Res<_>>()?;
    if found.iter().any(|(t, _)| *t != found[0].0) {
        return Err("layers to delete together must be in the same texture set".into());
    }
    let ids: Vec<LayerId> = found.iter().map(|f| f.1).collect();
    exec(session, if ids.len() == 1 { Command::DeleteLayer(ids[0]) } else { Command::DeleteLayers(ids.clone()) })?;
    Ok(Reply::json(json!({ "deleted": ids.len() })))
}

pub fn duplicate(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let copy = added_layer(session, ts, |s| exec(s, Command::DuplicateLayer(id)))?;
    if let Some(name) = args.str("name")? {
        exec(session, Command::Rename { layer: copy, name: name.to_string() })?;
    }
    Ok(Reply::json(json!({ "copy": summary(session, ts, copy)? })))
}

pub fn move_layer(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let loc = server::ts_ref(session, ts)?.location(id).ok_or("layer not found")?;
    if let Some(dir) = args.str("direction")? {
        match dir.to_ascii_lowercase().as_str() {
            "up" => exec(session, Command::MoveUp(id))?,
            "down" => exec(session, Command::MoveDown(id))?,
            "top" => {
                let n = siblings_len(session, ts, loc.parent)?;
                exec(session, Command::MoveLayer { layer: id, parent: loc.parent, index: n.saturating_sub(1) })?
            }
            "bottom" => exec(session, Command::MoveLayer { layer: id, parent: loc.parent, index: 0 })?,
            other => return Err(format!("direction must be up, down, top or bottom, got `{other}`")),
        }
    } else {
        let parent = match args.str("parent")? {
            None => loc.parent,
            Some(p) if p.eq_ignore_ascii_case("root") => None,
            Some(p) => {
                let pid = server::layer(session, p, Some(ts))?.1;
                if !server::layer_ref(session, ts, pid)?.is_folder() {
                    return Err("`parent` must be a folder layer".into());
                }
                Some(pid)
            }
        };
        let len = siblings_len(session, ts, parent)?;
        let max = if parent == loc.parent { len.saturating_sub(1) } else { len };
        let index = args.u32("index")?.map_or(max, |i| (i as usize).min(max));
        exec(session, Command::MoveLayer { layer: id, parent, index })?;
    }
    let t = server::ts_ref(session, ts)?;
    Ok(Reply::json(json!({ "moved": t.find(id).map(|l| l.name.clone()), "stack_top_first": describe::outline(t) })))
}

pub fn group(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    exec(session, Command::GroupLayer(id))?;
    let folder = server::ts_ref(session, ts)?.location(id).and_then(|l| l.parent).ok_or("grouping failed")?;
    if let Some(name) = args.str("name")? {
        exec(session, Command::Rename { layer: folder, name: name.to_string() })?;
    }
    Ok(Reply::json(json!({ "folder": summary(session, ts, folder)? })))
}

/// Layers of one texture set named by `layers` (ids or names), in the order given.
fn layers_of_one_set(session: &Session, args: &Args) -> Res<(TextureSetId, Vec<LayerId>)> {
    let within = args.str("texture_set")?.map(|t| server::texture_set(session, Some(t))).transpose()?;
    let keys = args.strings("layers")?.ok_or("missing required argument `layers`")?;
    if keys.is_empty() {
        return Err("`layers` is empty".into());
    }
    let found: Vec<(TextureSetId, LayerId)> = keys.iter().map(|k| server::layer(session, k, within)).collect::<Res<_>>()?;
    if found.iter().any(|(t, _)| *t != found[0].0) {
        return Err("the layers must all be in one texture set".into());
    }
    Ok((found[0].0, found.iter().map(|f| f.1).collect()))
}

/// Painter's flatten: each layer or folder evaluated into a new paint layer right above it; the
/// sources stay, hidden.
pub fn flatten(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, ids) = layers_of_one_set(session, args)?;
    let before = all_layer_ids(session, ts);
    exec(session, Command::FlattenLayers(ids))?;
    let after = all_layer_ids(session, ts);
    let new: Vec<Value> = after.difference(&before).map(|id| summary(session, ts, *id)).collect::<Res<_>>()?;
    if new.is_empty() {
        return Err("nothing was flattened".into());
    }
    Ok(Reply::json(json!({ "flattened": new, "note": "the source layers are kept, hidden; undo restores them as they were" })))
}

/// Painter's "Instantiate across texture sets": a linked copy of each layer on top of each other
/// texture set. Instances follow their source's content, effects and mask.
pub fn instantiate(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, ids) = layers_of_one_set(session, args)?;
    let targets: Vec<TextureSetId> = match args.strings("to_texture_sets")? {
        Some(list) => list.iter().map(|t| server::texture_set(session, Some(t))).collect::<Res<_>>()?,
        None => session.document().texture_sets.iter().map(|t| t.id).filter(|t| *t != ts).collect(),
    };
    let targets: Vec<TextureSetId> = targets.into_iter().filter(|t| *t != ts).collect();
    if targets.is_empty() {
        return Err("no other texture set to instantiate into".into());
    }
    let before: Vec<_> = targets.iter().map(|t| all_layer_ids(session, *t)).collect();
    exec(session, Command::InstantiateAcross { layers: ids, texture_sets: targets.clone() })?;
    let mut out = Vec::new();
    for (t, before) in targets.iter().zip(before) {
        let after = all_layer_ids(session, *t);
        let name = server::ts_ref(session, *t)?.name.clone();
        let added: Vec<Value> = after.difference(&before).map(|id| summary(session, *t, *id)).collect::<Res<_>>()?;
        out.push(json!({ "texture_set": name, "instances": added }));
    }
    Ok(Reply::json(json!({ "instantiated": out, "note": "instances follow their source; only name, visibility, blending and colour are their own" })))
}

// ----- fills ---------------------------------------------------------------------------------------

pub fn fill_set(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    if !matches!(server::layer_ref(session, ts, id)?.content, LayerContent::Fill(_)) {
        return Err("fill_set works on fill layers (layer_add type fill); paint layers take paint_stroke".into());
    }
    let projection = args.obj("projection")?.map(|p| Value::Object(p.clone()));
    replace_fill(session, ts, id, args.obj("channels")?, args.bool("exclusive")?.unwrap_or(false), projection.as_ref())?;
    Ok(Reply::json(json!({ "layer": summary(session, ts, id)? })))
}

/// What one channel entry of a fill spec asks for.
enum ChannelSpec {
    Off,
    Set { source: Option<FillSource>, image: Option<std::path::PathBuf>, enabled: bool, strength: Option<f32> },
}

fn procedural(kind: ChannelKind, spec: &Value, current: Option<&FillSource>, low: Option<&Value>, high: Option<&Value>, linear: bool) -> Res<FillSource> {
    let obj = spec.as_object().ok_or("`procedural` must be an object with a `kind`")?;
    let pk: ProceduralKind = match obj.get("kind").and_then(Value::as_str) {
        Some(k) => enum_from("procedural kind", k, &names(&ProceduralKind::ALL))?,
        None => match current {
            Some(FillSource::Procedural { params, .. }) => params.kind,
            _ => return Err(format!("procedural needs a `kind`: {}", names(&ProceduralKind::ALL).join(", "))),
        },
    };
    let base = match current {
        Some(FillSource::Procedural { params, .. }) if params.kind == pk => *params,
        _ => ProceduralParams::new(pk),
    };
    let params: ProceduralParams = with_defaults("procedural", &base, spec)?;
    let (cur_low, cur_high) = match current {
        Some(FillSource::Procedural { low, high, .. }) => (*low, *high),
        Some(FillSource::Uniform { value }) => (ChannelValue::scalar(0.0), *value),
        _ => (ChannelValue::scalar(0.0), if kind.components() == 3 && !kind.is_color() { kind.default_value() } else { ChannelValue::scalar(1.0) }),
    };
    let low = low.map(|v| channel_value(kind, v, linear)).transpose()?.unwrap_or(cur_low);
    let high = high.map(|v| channel_value(kind, v, linear)).transpose()?.unwrap_or(cur_high);
    Ok(FillSource::Procedural { params, low, high })
}

fn channel_spec(kind: ChannelKind, v: &Value, current: Option<&FillChannel>) -> Res<ChannelSpec> {
    match v {
        Value::Bool(false) => Ok(ChannelSpec::Off),
        Value::Bool(true) => Ok(ChannelSpec::Set { source: None, image: None, enabled: true, strength: None }),
        Value::Object(m) => {
            let a = Args(m);
            let linear = a.bool("linear")?.unwrap_or(false);
            let enabled = a.bool("enabled")?.unwrap_or(true);
            let strength = a.f32("strength")?;
            let image = a.str("image")?.map(paths::input);
            let source = if let Some(p) = a.get("procedural") {
                Some(procedural(kind, p, current.map(|c| &c.source), a.get("low"), a.get("high"), linear)?)
            } else if let Some(map) = a.str("mesh_map")? {
                Some(FillSource::MeshMap { map: mesh_map(map)? })
            } else if let Some(val) = a.get("value") {
                Some(FillSource::Uniform { value: channel_value(kind, val, linear)? })
            } else if a.get("low").is_some() || a.get("high").is_some() {
                match current.map(|c| &c.source) {
                    Some(FillSource::Procedural { params, .. }) => Some(procedural(kind, &serde_json::to_value(params).unwrap_or_default(), current.map(|c| &c.source), a.get("low"), a.get("high"), linear)?),
                    _ => return Err("`low`/`high` need a procedural source".into()),
                }
            } else {
                None
            };
            Ok(ChannelSpec::Set { source, image, enabled, strength })
        }
        other => Ok(ChannelSpec::Set { source: Some(FillSource::Uniform { value: channel_value(kind, other, false)? }), image: None, enabled: true, strength: None }),
    }
}

/// Channels a fill spec turns on that the texture set lacks are added to the set first (each
/// its own undo step, as in the app), so the edited layer can carry them.
fn ensure_channels(session: &mut Session, ts: TextureSetId, chs: &Map<String, Value>) -> Res<()> {
    for (name, v) in chs {
        let k = channel(name)?;
        if !matches!(v, Value::Bool(false)) && !server::ts_ref(session, ts)?.has_channel(k) {
            exec(session, Command::AddChannel { texture_set: ts, kind: k })?;
        }
    }
    Ok(())
}

/// Import an image for a fill channel. A file whose name says it is a gloss or DirectX normal
/// map for this same channel is converted, as when it is dropped on the channel in the app.
fn import_channel_image(session: &mut Session, k: ChannelKind, path: &std::path::Path) -> Res<subcraft_core::ResourceId> {
    use subcraft_app::assets::texture_maps::{Classified, MapConversion, classify};
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let conversion = match classify(name) {
        Classified::Map(m) if m.channel == k => m.conversion,
        _ => MapConversion::None,
    };
    session.import_converted(path, conversion)
}

/// Apply a `{channel: spec}` object to fill content. `exclusive` switches unlisted channels off.
fn edit_fill(session: &mut Session, fill: &mut FillContent, chs: &Map<String, Value>, exclusive: bool) -> Res<()> {
    let mut listed = BTreeSet::new();
    for (name, v) in chs {
        let k = channel(name)?;
        listed.insert(k);
        let current = fill.channels.get(&k).cloned();
        match channel_spec(k, v, current.as_ref())? {
            ChannelSpec::Off => {
                if let Some(c) = fill.channels.get_mut(&k) {
                    c.enabled = false;
                }
            }
            ChannelSpec::Set { source, image, enabled, strength } => {
                let mut c = current.unwrap_or_else(|| FillChannel::default_for(k));
                if let Some(path) = image {
                    if !path.exists() {
                        return Err(format!("image not found: {}", paths::output(&path)));
                    }
                    c.source = FillSource::Bitmap { resource: import_channel_image(session, k, &path)? };
                }
                if let Some(s) = source {
                    c.source = s;
                }
                c.enabled = enabled;
                if let Some(st) = strength {
                    c.strength = st;
                }
                fill.channels.insert(k, c);
            }
        }
    }
    if exclusive {
        for (k, c) in fill.channels.iter_mut() {
            if !listed.contains(k) {
                c.enabled = false;
            }
        }
    }
    Ok(())
}

/// Edit a fill layer's channels and projection as one undo step.
fn replace_fill(session: &mut Session, ts: TextureSetId, id: LayerId, chs: Option<&Map<String, Value>>, exclusive: bool, projection: Option<&Value>) -> Res<()> {
    if let Some(chs) = chs {
        ensure_channels(session, ts, chs)?;
    }
    let mut layer = server::layer_ref(session, ts, id)?.clone();
    let LayerContent::Fill(fill) = &mut layer.content else { return Err("not a fill layer".into()) };
    if let Some(chs) = chs {
        edit_fill(session, fill, chs, exclusive)?;
    }
    if let Some(p) = projection {
        fill.projection = crate::params::projection_with_defaults(&fill.projection, p)?;
    }
    let label = format!("Fill: {}", layer.name);
    exec(session, Command::ReplaceLayer { layer: id, replacement: Box::new(layer), label })
}

/// Fill channels for a content Fill effect (images are imported as plain resources).
fn fill_channels(session: &mut Session, ts: TextureSetId, chs: &Map<String, Value>) -> Res<BTreeMap<ChannelKind, FillChannel>> {
    let mut out = BTreeMap::new();
    let have: Vec<ChannelKind> = server::ts_ref(session, ts)?.channel_kinds().collect();
    for (name, v) in chs {
        let k = channel(name)?;
        if !have.contains(&k) {
            return Err(format!("texture set has no {} channel (texture_set_update add_channels)", k.display_name()));
        }
        match channel_spec(k, v, None)? {
            ChannelSpec::Off => {}
            ChannelSpec::Set { source, image, enabled, strength } => {
                let source = match image {
                    Some(p) => FillSource::Bitmap { resource: session.import_image(&p)? },
                    None => source.unwrap_or_else(|| FillSource::uniform(k.default_paint_value())),
                };
                out.insert(k, FillChannel { enabled, source, strength: strength.unwrap_or(1.0) });
            }
        }
    }
    Ok(out)
}

// ----- parts ----------------------------------------------------------------------------------------

fn part_kind(s: Option<&str>, session: &Session) -> Res<PartKind> {
    match s {
        Some(s) => enum_from("part kind", s, &["connected", "uv_island", "object", "material"].map(String::from)),
        None => Ok(if session.scene().mesh.objects.len() > 1 { PartKind::Object } else { PartKind::Connected }),
    }
}

pub fn parts_list(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let kind = part_kind(args.str("kind")?, session)?;
    let sets = &session.document().texture_sets;
    let parts: Vec<Value> = session.parts(kind).iter().take(500).map(|p| json!({
        "id": p.id, "name": p.name, "triangles": p.triangles,
        "texture_sets": p.materials.iter().filter_map(|m| sets.iter().find(|t| t.material_slot == *m).map(|t| t.name.clone())).collect::<Vec<_>>(),
    })).collect();
    let total = session.parts(kind).len();
    Ok(Reply::json(json!({ "kind": name_of(&kind), "count": total, "parts": parts, "truncated": total > 500 })))
}

pub fn set_parts(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let parts = match args.get("parts") {
        None => None,
        Some(Value::Array(a)) => {
            let kind = part_kind(args.str("kind")?, session)?;
            // An id beyond u32 used to wrap round to another part (2^32 + 1 became part 1).
            let ids: Vec<u32> = a.iter().map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("part ids are non-negative integers from parts_list, got {v}"))).collect::<Res<_>>()?;
            Some(PartMask::for_kind(kind, ids))
        }
        Some(v) => return Err(format!("`parts` must be a list of ids or null, got {v}")),
    };
    exec(session, Command::SetParts { layer: id, parts })?;
    Ok(Reply::json(json!({ "layer": summary(session, ts, id)? })))
}

// ----- masks and effects ----------------------------------------------------------------------------

pub fn mask_add(server: &mut Server, args: &Args) -> Res<Reply> {
    let (library, session) = server.library_and_session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let white = !args.str("color")?.is_some_and(|c| c.eq_ignore_ascii_case("black"));
    if server::layer_ref(session, ts, id)?.mask.is_none() {
        exec(session, Command::AddMask { layer: id, white })?;
    }
    if let Some(name) = args.str("smart_mask")? {
        let mask = find_named(&library.smart_masks, |m| &m.name, name, "smart mask")?.clone();
        exec(session, Command::SelectLayer(Some(id)))?;
        session.apply_smart_mask(&mask).map_err(|e| e.to_string())?;
    }
    Ok(Reply::json(json!({ "layer": summary(session, ts, id)? })))
}

pub fn mask_remove(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    exec(session, Command::RemoveMask(id))?;
    Ok(Reply::json(json!({ "layer": summary(session, ts, id)? })))
}

fn without_type(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("type");
        o.remove("channel");
    }
    v
}

/// Allowed values, from `Filter::valid`, for error messages and the tool description.
pub const FILTER_RANGES: &str = "contrast {gain 0..4, pivot}; hsl {hue -180..180 degrees, saturation 0..2 (multiplier, 1 = unchanged, 0 = gray), lightness -1..1 (added)}; \
blur {radius 0..32 texels}; sharpen / highpass {radius 0..32, amount 0..4}; directional_blur {radius 0..32, angle_deg -180..180}; median {radius}; \
warp {intensity 0..32, scale, seed}; bevel {radius, amount -1..1, threshold -1..1}; mask_outline {radius, threshold -1..1, position inner|outer|both}; \
histogram_scan {position -1..1, contrast 0..1, invert}; histogram_range {position -1..1, range 0..2, softness 0..1, invert}; threshold {cutoff}; clamp {low, high}; \
posterize {levels}; histogram_shift {shift}; color_balance {shadows, midtones, highlights: [r,g,b]}; replace_color {from, to, tolerance, softness}; \
normal_invert {flip_x, flip_y, flip_z}; edge_detect {radius, strength}; glow {radius, amount, threshold}; directional_warp {intensity, angle_deg}; mosaic {size}; \
gradient_map {gradient, to_gray}; grayscale; invert";

pub fn filter(spec: &Value, current: Option<&Filter>) -> Res<Filter> {
    let spec = &crate::params::unwrap_params(spec, &["params", "filter", "settings", "parameters"]);
    let kind = spec.get("kind").and_then(Value::as_str).map(crate::params::snake);
    let defaults: Vec<Value> = Filter::ALL.iter().filter_map(|f| serde_json::to_value(f).ok()).collect();
    let mut base = match (&kind, current) {
        (None, Some(c)) => serde_json::to_value(c).map_err(|e| e.to_string())?,
        (Some(k), Some(c)) if serde_json::to_value(c).ok().and_then(|v| v.get("kind").cloned()) == Some(Value::String(k.clone())) => serde_json::to_value(c).map_err(|e| e.to_string())?,
        (Some(k), _) => defaults.iter().find(|d| d.get("kind").and_then(Value::as_str) == Some(k)).cloned().unwrap_or_else(|| json!({ "kind": k })),
        (None, None) => {
            let kinds: Vec<&str> = defaults.iter().filter_map(|d| d.get("kind").and_then(Value::as_str)).collect();
            return Err(format!("filter needs a `kind`: {}", kinds.join(", ")));
        }
    };
    let mut patch = without_type(spec);
    if let Some(o) = patch.as_object_mut() {
        let snaked: serde_json::Map<String, Value> = std::mem::take(o).into_iter().map(|(k, v)| (crate::params::snake(&k), v)).collect();
        *o = snaked;
        o.remove("name");
        o.remove("opacity");
        o.remove("visible");
        o.remove("blend");
        if let Some(k) = &kind {
            o.insert("kind".into(), Value::String(k.clone()));
        }
    }
    let known: Vec<String> = base.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    if let Some(p) = patch.as_object() {
        let unknown: Vec<&String> = p.keys().filter(|k| !known.contains(k)).collect();
        if !unknown.is_empty() {
            return Err(format!("unknown field(s) {unknown:?} for filter `{}`; it takes: {}", base["kind"].as_str().unwrap_or("?"), known.join(", ")));
        }
    }
    crate::params::merge(&mut base, &patch);
    let f: Filter = serde_json::from_value(base).map_err(|e| format!("invalid filter: {e}"))?;
    if !f.valid() {
        return Err(format!("filter values out of range: {}. Allowed: {FILTER_RANGES}", serde_json::to_string(&f).unwrap_or_default()));
    }
    Ok(f)
}

fn generator(spec: &Value, current: Option<&Generator>) -> Res<Generator> {
    let mut patch = without_type(spec);
    let kind_v = patch.get("kind").cloned();
    let kind: Option<GeneratorKind> = match &kind_v {
        Some(Value::String(s)) => Some(match crate::params::snake(s).as_str() {
            "dirt" | "cavity" => GeneratorKind::Cavity,
            "curvature" => GeneratorKind::MeshMap(subcraft_core::MeshMapKind::Curvature),
            "gradient" | "position" => GeneratorKind::PositionGradient,
            "up" | "top" => GeneratorKind::Directional,
            other => serde_json::from_value(Value::String(other.to_string())).map_err(|_| format!("unknown generator kind `{s}`: edges, cavity, concave, directional, position_gradient, or {{\"mesh_map\": \"curvature\"}}"))?,
        }),
        Some(Value::Object(m)) if m.contains_key("mesh_map") => Some(GeneratorKind::MeshMap(mesh_map(m["mesh_map"].as_str().unwrap_or_default())?)),
        Some(other) => return Err(format!("invalid generator kind {other}")),
        None => None,
    };
    if let Some(o) = patch.as_object_mut() {
        o.remove("kind");
    }
    let base = match (kind, current) {
        (Some(k), Some(c)) if c.kind == k => *c,
        (Some(k), _) => Generator::new(k),
        (None, Some(c)) => *c,
        (None, None) => return Err("generator needs a `kind`: edges, cavity, concave, directional, position_gradient".into()),
    };
    with_defaults("generator", &base, &patch)
}

fn mask_kind(session: &mut Session, spec: &Value) -> Res<MaskEffectKind> {
    let t = spec.get("type").and_then(Value::as_str).ok_or("effect needs a `type`: generator, fill, levels, filter, invert, image")?;
    let a = Args(spec.as_object().ok_or("effect must be an object")?);
    Ok(match crate::params::snake(t).as_str() {
        "generator" => MaskEffectKind::Generator(generator(spec, None)?),
        "fill" => MaskEffectKind::Fill {
            value: a.f32("value")?.unwrap_or(1.0),
            procedural: a.get("procedural").map(|p| procedural_params(p, None)).transpose()?,
            projection: a.get("projection").map(|p| crate::params::projection_with_defaults(&Projection::default(), p)).transpose()?.unwrap_or_default(),
        },
        "levels" => MaskEffectKind::Levels(with_defaults("levels", &Levels::default(), &without_type(spec))?),
        "filter" => MaskEffectKind::Filter(filter(&without_type(spec), None)?),
        "invert" => MaskEffectKind::Invert,
        "image" => MaskEffectKind::Image {
            resource: session.import_image(&paths::input(a.req_str("image")?))?,
            projection: a.get("projection").map(|p| crate::params::projection_with_defaults(&Projection::default(), p)).transpose()?.unwrap_or_default(),
            invert: a.bool("invert")?.unwrap_or(false),
        },
        "paint" => return Err("a mask's base is already paintable: paint_stroke with target mask".into()),
        other => return Err(format!("unknown mask effect type `{other}`")),
    })
}

fn procedural_params(spec: &Value, current: Option<&ProceduralParams>) -> Res<ProceduralParams> {
    let kind = match spec.get("kind").and_then(Value::as_str) {
        Some(k) => enum_from::<ProceduralKind>("procedural kind", k, &names(&ProceduralKind::ALL))?,
        None => current.map(|c| c.kind).ok_or("procedural needs a `kind`")?,
    };
    let base = current.filter(|c| c.kind == kind).copied().unwrap_or_else(|| ProceduralParams::new(kind));
    with_defaults("procedural", &base, spec)
}

fn effect_ids(session: &Session, ts: TextureSetId, id: LayerId, stack: Stack) -> BTreeSet<EffectId> {
    let Ok(l) = server::layer_ref(session, ts, id) else { return BTreeSet::new() };
    match stack {
        Stack::Mask => l.mask.iter().flat_map(|m| m.effects.iter().map(|e| e.id)).collect(),
        Stack::Content => l.content_effects.iter().map(|e| e.id).collect(),
    }
}

fn added_effect(session: &mut Session, ts: TextureSetId, id: LayerId, stack: Stack, cmd: Command) -> Res<EffectId> {
    let before = effect_ids(session, ts, id, stack);
    exec(session, cmd)?;
    effect_ids(session, ts, id, stack).difference(&before).next().copied().ok_or_else(|| "the effect was not added".into())
}

pub fn mask_add_effect(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let spec = args.get("effect").ok_or("missing required argument `effect`")?;
    let kind = mask_kind(session, spec)?;
    check_tweak_args(args)?;
    let created_mask = server::layer_ref(session, ts, id)?.mask.is_none();
    if created_mask {
        exec(session, Command::AddMask { layer: id, white: false })?;
    }
    let eid = added_effect(session, ts, id, Stack::Mask, Command::AddMaskEffect { layer: id, kind })?;
    tweak_effect(session, ts, id, eid, Stack::Mask, args, None)?;
    let mut out = json!({ "effect_id": eid.to_string(), "layer": summary(session, ts, id)? });
    if created_mask {
        out["note"] = json!("the layer had no mask, so a black one was added under the effect");
    }
    if needs_maps(spec) && session.mesh_maps(ts).is_none_or(|m| m.is_empty()) {
        out["warning"] = json!("this generator reads baked mesh maps; run bake_mesh_maps or it shows nothing useful");
    }
    Ok(Reply::json(out))
}

fn needs_maps(spec: &Value) -> bool {
    let k = spec.get("kind").map(|k| k.to_string().to_lowercase()).unwrap_or_default();
    k.contains("edge") || k.contains("cavity") || k.contains("concave") || k.contains("dirt") || k.contains("mesh_map") || k.contains("curvature")
}

pub fn content_add_effect(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let spec = args.get("effect").ok_or("missing required argument `effect`")?;
    let a = Args(spec.as_object().ok_or("effect must be an object")?);
    let t = a.req_str("type")?;
    let ch = || -> Res<ChannelKind> { channel(a.str("channel")?.unwrap_or("base_color")) };
    let kind = match crate::params::snake(t).as_str() {
        "filter" => ContentEffectKind::Filter { channel: ch()?, filter: filter(&without_type(spec), None)? },
        "levels" => ContentEffectKind::Levels { channel: ch()?, levels: with_defaults("levels", &Levels::default(), &without_type(spec))? },
        "fill" => {
            let chs = a.obj("channels")?.ok_or("fill effect needs `channels` (like fill_set)")?;
            let channels = fill_channels(session, ts, chs)?;
            let projection = a.get("projection").map(|p| crate::params::projection_with_defaults(&Projection::default(), p)).transpose()?.unwrap_or_default();
            ContentEffectKind::Fill(FillContent { projection, channels })
        }
        "paint" => ContentEffectKind::Paint { channels: BTreeMap::new() },
        other => return Err(format!("unknown content effect type `{other}`: filter, levels, fill, paint")),
    };
    check_tweak_args(args)?;
    let eid = added_effect(session, ts, id, Stack::Content, Command::AddContentEffect { layer: id, kind })?;
    tweak_effect(session, ts, id, eid, Stack::Content, args, None)?;
    Ok(Reply::json(json!({ "effect_id": eid.to_string(), "layer": summary(session, ts, id)? })))
}

/// Read name / visible / opacity / blend before anything is added: a bad `blend` used to fail
/// after the effect (and a new mask) had already been added, leaving them behind the error.
fn check_tweak_args(args: &Args) -> Res<()> {
    args.str("name")?;
    args.bool("visible")?;
    args.f32("opacity")?;
    args.str("blend")?.map(blend_mode).transpose()?;
    Ok(())
}

/// Apply name / visible / opacity / blend (and `params` when given) to an effect.
fn tweak_effect(session: &mut Session, ts: TextureSetId, id: LayerId, eid: EffectId, stack: Stack, args: &Args, params: Option<&Value>) -> Res<()> {
    let name = args.str("name")?.map(str::to_string);
    let visible = args.bool("visible")?;
    let opacity = args.f32("opacity")?;
    let blend = args.str("blend")?.map(blend_mode).transpose()?;
    if name.is_none() && visible.is_none() && opacity.is_none() && blend.is_none() && params.is_none() {
        return Ok(());
    }
    let layer = server::layer_ref(session, ts, id)?.clone();
    match stack {
        Stack::Mask => {
            let mask = layer.mask.as_ref().ok_or("layer has no mask")?;
            let mut e = (**mask.effects.iter().find(|e| e.id == eid).ok_or("effect not found")?).clone();
            if let Some(n) = name { e.name = n; }
            if let Some(v) = visible { e.visible = v; }
            if let Some(o) = opacity { e.opacity = o.clamp(0.0, 1.0); }
            if let Some(b) = blend { e.blend = b; }
            if let Some(p) = params {
                e.kind = match &e.kind {
                    MaskEffectKind::Generator(g) => MaskEffectKind::Generator(generator(p, Some(g))?),
                    MaskEffectKind::Levels(l) => MaskEffectKind::Levels(with_defaults("levels", l, p)?),
                    MaskEffectKind::Filter(f) => MaskEffectKind::Filter(filter(p, Some(f))?),
                    MaskEffectKind::Fill { value, procedural, projection } => {
                        let a = Args(p.as_object().ok_or("params must be an object")?);
                        MaskEffectKind::Fill {
                            value: a.f32("value")?.unwrap_or(*value),
                            procedural: match a.get("procedural") { Some(pp) => Some(procedural_params(pp, procedural.as_ref())?), None => *procedural },
                            projection: a.get("projection").map(|pp| crate::params::projection_with_defaults(projection, pp)).transpose()?.unwrap_or(*projection),
                        }
                    }
                    MaskEffectKind::Image { resource, projection, invert } => {
                        let a = Args(p.as_object().ok_or("params must be an object")?);
                        MaskEffectKind::Image {
                            resource: *resource,
                            projection: a.get("projection").map(|pp| crate::params::projection_with_defaults(projection, pp)).transpose()?.unwrap_or(*projection),
                            invert: a.bool("invert")?.unwrap_or(*invert),
                        }
                    }
                    MaskEffectKind::IdColor(c) => MaskEffectKind::IdColor(with_defaults("id color mask", c, p)?),
                    other => other.clone(),
                };
            }
            exec(session, Command::SetMaskEffect { layer: id, effect: e })
        }
        Stack::Content => {
            let mut e = (**layer.content_effects.iter().find(|e| e.id == eid).ok_or("effect not found")?).clone();
            if let Some(n) = name { e.name = n; }
            if let Some(v) = visible { e.visible = v; }
            for b in e.blend.values_mut() {
                if let Some(o) = opacity { b.opacity = o.clamp(0.0, 1.0); }
                if let Some(m) = blend { b.mode = m; }
            }
            if let Some(p) = params {
                let a = Args(p.as_object().ok_or("params must be an object")?);
                e.kind = match &e.kind {
                    ContentEffectKind::Filter { channel: c, filter: f } => {
                        let c = a.str("channel")?.map(channel).transpose()?.unwrap_or(*c);
                        ContentEffectKind::Filter { channel: c, filter: filter(&without_type(p), Some(f))? }
                    }
                    ContentEffectKind::Levels { channel: c, levels } => {
                        let c = a.str("channel")?.map(channel).transpose()?.unwrap_or(*c);
                        ContentEffectKind::Levels { channel: c, levels: with_defaults("levels", levels, &without_type(p))? }
                    }
                    ContentEffectKind::Fill(f) => {
                        let mut f = f.clone();
                        if let Some(chs) = a.obj("channels")? {
                            f.channels.extend(fill_channels(session, ts, chs)?);
                        }
                        if let Some(pp) = a.get("projection") {
                            f.projection = crate::params::projection_with_defaults(&f.projection, pp)?;
                        }
                        ContentEffectKind::Fill(f)
                    }
                    other => other.clone(),
                };
            }
            exec(session, Command::SetContentEffect { layer: id, effect: e })
        }
    }
}

pub fn effect_update(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let (stack, eid) = server::effect(server::layer_ref(session, ts, id)?, args.req_str("effect")?)?;
    tweak_effect(session, ts, id, eid, stack, args, args.get("params"))?;
    let l = server::layer_ref(session, ts, id)?;
    let effect = match stack {
        Stack::Mask => l.mask.as_ref().and_then(|m| m.effects.iter().find(|e| e.id == eid)).map(|e| describe::mask_effect(e, true)),
        Stack::Content => l.content_effects.iter().find(|e| e.id == eid).map(|e| json!({ "id": e.id.to_string(), "name": e.name, "visible": e.visible })),
    };
    Ok(Reply::json(json!({ "effect": effect, "layer": summary(session, ts, id)? })))
}

pub fn effect_remove(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let (stack, eid) = server::effect(server::layer_ref(session, ts, id)?, args.req_str("effect")?)?;
    exec(session, match stack {
        Stack::Mask => Command::RemoveMaskEffect { layer: id, effect: eid },
        Stack::Content => Command::RemoveContentEffect { layer: id, effect: eid },
    })?;
    Ok(Reply::json(json!({ "removed": eid.to_string(), "layer": summary(session, ts, id)? })))
}

// ----- shelf ------------------------------------------------------------------------------------------

pub fn find_named<'a, T>(items: &'a [T], name: impl Fn(&T) -> &String, key: &str, what: &str) -> Res<&'a T> {
    let k = key.trim().to_lowercase();
    if let Some(hit) = items.iter().find(|i| name(i).to_lowercase() == k) {
        return Ok(hit);
    }
    let partial: Vec<&T> = items.iter().filter(|i| name(i).to_lowercase().contains(&k)).collect();
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no {what} named `{key}` (shelf_list shows them)")),
        many => Err(format!("`{key}` matches several {what}s: {}", many.iter().map(|i| name(i).as_str()).collect::<Vec<_>>().join(", "))),
    }
}

pub fn shelf_list(server: &mut Server, args: &Args) -> Res<Reply> {
    let search = args.str("search")?.map(|s| s.to_lowercase());
    let category = args.str("category")?.map(crate::params::snake);
    let lib = server.library();
    let keep = |n: &str| search.as_deref().is_none_or(|s| n.to_lowercase().contains(s));
    let materials: Vec<Value> = lib.materials.iter().filter(|m| keep(&m.name) || search.as_deref().is_some_and(|s| m.category.to_lowercase().contains(s)))
        .map(|m| json!({ "name": m.name, "category": m.category, "channels": m.channels.iter().map(|(k, _)| name_of(k)).chain(m.images.iter().map(|_| "image".to_string())).collect::<BTreeSet<_>>() })).collect();
    let smart: Vec<Value> = lib.smart_materials.iter().filter(|m| keep(&m.name)).map(|m| {
        let info = subcraft_app::assets::smart_material_info(&m.name);
        json!({ "name": m.name, "category": info.as_ref().map(|i| i.category), "description": info.as_ref().map(|i| i.description), "user": m.user_file.is_some() })
    }).collect();
    let masks: Vec<Value> = lib.smart_masks.iter().filter(|m| keep(&m.name)).map(|m| {
        let info = subcraft_app::assets::smart_mask_info(&m.name);
        json!({ "name": m.name, "category": info.as_ref().map(|i| i.category), "description": info.as_ref().map(|i| i.description), "user": m.user_file.is_some() })
    }).collect();
    let brushes: Vec<&str> = lib.alphas.iter().filter(|a| keep(&a.name)).map(|a| a.name.as_str()).collect();
    let envs: Vec<&str> = lib.environments.iter().filter(|e| keep(&e.name)).map(|e| e.name.as_str()).collect();
    Ok(Reply::json(match category.as_deref() {
        Some("materials" | "material") => json!({ "materials": materials }),
        Some("smart_materials" | "smart_material") => json!({ "smart_materials": smart }),
        Some("smart_masks" | "smart_mask" | "masks") => json!({ "smart_masks": masks }),
        Some("brushes" | "brush" | "alphas" | "tips") => json!({ "brushes": brushes }),
        Some("environments" | "environment" | "hdri") => json!({ "environments": envs }),
        Some(other) => return Err(format!("unknown category `{other}`")),
        None => json!({ "materials": materials, "smart_materials": smart, "smart_masks": masks, "brushes": brushes, "environments": envs }),
    }))
}

pub fn apply_material(server: &mut Server, args: &Args) -> Res<Reply> {
    let (library, session) = server.library_and_session()?;
    let name = args.req_str("name")?;
    let ts = server::texture_set(session, args.str("texture_set")?)?;
    let above = args.str("above")?.map(|a| server::layer(session, a, Some(ts)).map(|r| r.1)).transpose()?;
    // Exact names first across both lists, then unique partial matches.
    let preset = library.materials.iter().find(|m| m.name.eq_ignore_ascii_case(name.trim())).cloned();
    let smart = library.smart_materials.iter().find(|m| m.name.eq_ignore_ascii_case(name.trim())).cloned();
    let (preset, smart) = match (preset, smart) {
        (Some(p), _) => (Some(p), None),
        (None, Some(s)) => (None, Some(s)),
        (None, None) => match (find_named(&library.materials, |m| &m.name, name, "material"), find_named(&library.smart_materials, |m| &m.name, name, "smart material")) {
            (Ok(p), Err(_)) => (Some(p.clone()), None),
            (Err(_), Ok(s)) => (None, Some(s.clone())),
            (Ok(p), Ok(s)) => return Err(format!("`{name}` matches the material `{}` and the smart material `{}`; use the full name", p.name, s.name)),
            (Err(e), Err(_)) => return Err(e.replace("material", "material or smart material")),
        },
    };
    select_insertion(session, ts, above)?;
    let id = added_layer(session, ts, |s| match (&preset, &smart) {
        (Some(p), _) => s.add_material(p).map_err(|e| e.to_string()),
        (_, Some(m)) => s.add_smart_material(m).map_err(|e| e.to_string()),
        _ => unreachable!(),
    })?;
    if let Some(n) = args.str("layer_name")? {
        exec(session, Command::Rename { layer: id, name: n.to_string() })?;
    }
    let mut out = json!({ "added": summary(session, ts, id)? });
    if smart.is_some() && session.mesh_maps(ts).is_none_or(|m| m.is_empty()) {
        out["warning"] = json!("no baked mesh maps yet: edge wear and cavity dirt in smart materials need bake_mesh_maps");
    }
    Ok(Reply::json(out))
}

pub fn save_to_library(server: &mut Server, args: &Args) -> Res<Reply> {
    let root = subcraft_app::assets::UserLibrary::default_root().ok_or("no user library folder (set SUBCRAFT_LIBRARY)")?;
    let lib = subcraft_app::assets::UserLibrary::new(root.clone());
    let (library, session) = server.library_mut_and_session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    if let Some(n) = args.str("name")? {
        exec(session, Command::Rename { layer: id, name: n.to_string() })?;
    }
    let name = server::layer_ref(session, ts, id)?.name.clone();
    exec(session, Command::SelectTextureSet(ts))?;
    exec(session, Command::SelectLayer(Some(id)))?;
    let kind = args.str("kind")?.unwrap_or("smart_material");
    let file = match crate::params::snake(kind).as_str() {
        "smart_material" | "material" => {
            let m = session.save_smart_material(id, &lib)?;
            let f = m.user_file.clone();
            library.smart_materials.retain(|x| x.user_file != f || f.is_none());
            library.smart_materials.push(m);
            f
        }
        "smart_mask" | "mask" => {
            let m = session.save_smart_mask(id, &lib)?;
            let f = m.user_file.clone();
            library.smart_masks.retain(|x| x.user_file != f || f.is_none());
            library.smart_masks.push(m);
            f
        }
        other => return Err(format!("kind must be smart_material or smart_mask, got `{other}`")),
    };
    Ok(Reply::json(json!({
        "saved": name, "kind": kind, "file": file.as_deref().map(paths::output), "library": paths::output(&root),
        "note": "it is on the shelf now (apply_material / apply_smart_mask by name) and in the desktop app's shelf after its next start",
    })))
}

pub fn import_textures(server: &mut Server, args: &Args) -> Res<Reply> {
    let session = server.session()?;
    let ts = server::texture_set(session, args.str("texture_set")?)?;
    let above = args.str("above")?.map(|a| server::layer(session, a, Some(ts)).map(|r| r.1)).transpose()?;
    let files: Vec<std::path::PathBuf> = args.strings("paths")?.ok_or("missing required argument `paths`")?.iter().map(|p| paths::input(p)).collect();
    if let Some(missing) = files.iter().find(|p| !p.exists()) {
        return Err(format!("file not found: {}", paths::output(missing)));
    }
    select_insertion(session, ts, above)?;
    let mut report = None;
    let id = added_layer(session, ts, |s| {
        report = Some(s.import_texture_set(&files)?);
        Ok(())
    })?;
    let r = report.expect("set by the closure");
    if let Some(n) = args.str("layer_name")? {
        exec(session, Command::Rename { layer: id, name: n.to_string() })?;
    }
    Ok(Reply::json(json!({
        "summary": r.summary(),
        "used": r.used.iter().map(|(f, c)| json!({ "file": f, "channel": name_of(c) })).collect::<Vec<_>>(),
        "skipped": r.skipped.iter().map(|(f, why)| json!({ "file": f, "reason": why })).collect::<Vec<_>>(),
        "added": summary(session, ts, id)?,
    })))
}

pub fn apply_smart_mask(server: &mut Server, args: &Args) -> Res<Reply> {
    let (library, session) = server.library_and_session()?;
    let (ts, id) = server::layer_arg(session, args, "layer")?;
    let mask = find_named(&library.smart_masks, |m| &m.name, args.req_str("name")?, "smart mask")?.clone();
    exec(session, Command::SelectTextureSet(ts))?;
    exec(session, Command::SelectLayer(Some(id)))?;
    session.apply_smart_mask(&mask).map_err(|e| e.to_string())?;
    let mut out = json!({ "layer": summary(session, ts, id)? });
    if session.mesh_maps(ts).is_none_or(|m| m.is_empty()) {
        out["warning"] = json!("most smart masks read baked curvature / AO: run bake_mesh_maps if the mask looks empty");
    }
    Ok(Reply::json(out))
}

/// Select a layer's content or mask as the stroke target.
pub fn select_target(session: &mut Session, id: LayerId, mask: bool) -> Res<()> {
    exec(session, Command::SelectLayerTarget { layer: id, target: if mask { LayerTarget::Mask } else { LayerTarget::Content } })
}
