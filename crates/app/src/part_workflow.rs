//! Geometry-targeted editing and temporary viewport visibility; widgets dispatch actions here.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};
use glam::Vec2;
use subcraft_core::{ChannelKind, ChannelValue, PartKind, TextureSetId};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, Layer, Mask, MaskEffect, MaskEffectKind, PartMask};
use crate::{Camera, Command, CommandError, Scene, Session};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartKey { pub kind: PartKind, pub id: u32 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartInfo {
    pub id: u32, pub kind: PartKind, pub name: String,
    pub triangles: usize, pub materials: Vec<u32>,
}
impl PartInfo { pub fn key(&self) -> PartKey { PartKey { kind: self.kind, id: self.id } } }

#[derive(Clone, Copy, Debug)]
pub struct SurfaceTarget {
    pub part: PartKey, pub triangle: u32, pub texture_set: TextureSetId, pub uv: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartAction { Fill, PaintOnly, MaskLayer, Generator(GeneratorKind), PickColor, PickMaterial, Hide, Isolate }

impl PartAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fill => "Filled part", Self::PaintOnly => "Paint only on this part",
            Self::MaskLayer => "Layer masked to part", Self::Generator(_) => "Added generator mask",
            Self::PickColor => "Picked color", Self::PickMaterial => "Picked material",
            Self::Hide => "Part visibility changed", Self::Isolate => "Part isolation changed",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum LayerAction { AddPaint, AddFill, WhiteMask, BlackMask, Generator(GeneratorKind), MaskFill, MaskPaint }

#[derive(Default)]
pub(crate) struct PartView {
    hidden: BTreeSet<PartKey>, isolated: Option<PartKey>,
    pub scene: Option<Scene>, revision: u64,
    catalog: [OnceLock<Vec<PartInfo>>; 4],
    visible: [OnceLock<BTreeSet<u32>>; 4],
}

impl Session {
    pub fn layer_action(&mut self, layer: subcraft_core::LayerId, action: LayerAction) -> Result<(), CommandError> {
        if self.selected_texture_set().find(layer).is_none() { return Err(CommandError::LayerNotFound); }
        self.execute(Command::SelectLayer(Some(layer)))?;
        match action {
            LayerAction::AddPaint => self.execute(Command::AddLayer(crate::NewLayer::Paint)),
            LayerAction::AddFill => self.execute(Command::AddLayer(crate::NewLayer::Fill)),
            LayerAction::WhiteMask | LayerAction::BlackMask => self.execute(Command::AddMask { layer, white: matches!(action, LayerAction::WhiteMask) }),
            _ => {
                let kind = match action {
                    LayerAction::Generator(kind) => MaskEffectKind::Generator(Generator::new(kind)),
                    LayerAction::MaskFill => MaskEffectKind::Fill { value: 1.0, procedural: None, projection: Default::default() },
                    LayerAction::MaskPaint => MaskEffectKind::Paint { image: subcraft_image::TiledImage::new(self.selected_texture_set().resolution, self.selected_texture_set().resolution, 2, [0.0; 4]) },
                    _ => unreachable!(),
                };
                self.execute(Command::ApplyMaskEffects { layer, effects: vec![kind], label: "Add Mask Effect".into() })
            }
        }
    }
    fn region_of(&self, triangle: usize, kind: PartKind) -> Option<u32> {
        self.scene().geometry.get(0, 1).triangle_region(triangle, kind)
    }

    /// Cached across frames; covers the whole mesh, including pieces crossing material borders.
    pub fn parts(&self, kind: PartKind) -> &[PartInfo] {
        self.part_view.catalog[kind as usize].get_or_init(|| {
            let mesh = &self.scene().mesh;
            let geo = self.scene().geometry.get(0, 1);
            let mut groups = BTreeMap::<u32, (usize, BTreeSet<u32>, BTreeSet<String>)>::new();
            for t in 0..mesh.triangle_count() {
                let Some(id) = geo.triangle_region(t, kind) else { continue };
                let group = groups.entry(id).or_default();
                group.0 += 1; group.1.insert(mesh.tri_material[t]);
                if let Some(o) = mesh.tri_object.get(t).copied().flatten() { group.2.insert(mesh.objects[o as usize].name.clone()); }
            }
            groups.into_iter().map(|(id, (triangles, materials, names))| {
                let name = match kind {
                    PartKind::Object => mesh.objects[id as usize].name.clone(),
                    PartKind::Material => mesh.materials[id as usize].name.clone(),
                    _ => {
                        let label = if kind == PartKind::UvIsland { "Island" } else { "Piece" };
                        let prefix = if names.is_empty() { String::new() } else { format!("{} · ", names.into_iter().collect::<Vec<_>>().join(" / ")) };
                        format!("{prefix}{label} {}", id + 1)
                    }
                };
                PartInfo { id, kind, name, triangles, materials: materials.into_iter().collect() }
            }).collect()
        })
    }

    pub fn surface_target(&self, camera: &Camera, screen: Vec2, kind: PartKind) -> Option<SurfaceTarget> {
        let pick = self.pick(camera, screen)?;
        Some(SurfaceTarget { part: PartKey { kind, id: self.region_of(pick.triangle as usize, kind)? }, triangle: pick.triangle,
            texture_set: self.document().texture_set_for_slot(pick.material_slot)?.id, uv: pick.uv })
    }

    pub fn target_for_part(&self, key: PartKey) -> Option<SurfaceTarget> {
        let mesh = &self.scene().mesh;
        let geo = self.scene().geometry.get(0, 1);
        let t = (0..mesh.triangle_count()).find(|&t| geo.triangle_region(t, key.kind) == Some(key.id))?;
        Some(SurfaceTarget { part: key, triangle: t as u32,
            texture_set: self.document().texture_set_for_slot(mesh.tri_material[t])?.id,
            uv: mesh.interpolate(t, 1.0 / 3.0, 1.0 / 3.0).uv })
    }

    pub fn viewport_scene(&self) -> &Scene { self.part_view.scene.as_ref().unwrap_or_else(|| self.scene()) }
    pub fn target_kind(&self, target: SurfaceTarget, kind: PartKind) -> Option<SurfaceTarget> {
        Some(SurfaceTarget { part: PartKey { kind, id: self.region_of(target.triangle as usize, kind)? }, ..target })
    }
    pub fn view_revision(&self) -> u64 { self.part_view.revision }
    pub fn has_hidden_parts(&self) -> bool { !self.part_view.hidden.is_empty() || self.part_view.isolated.is_some() }
    pub fn part_visible(&self, key: PartKey) -> bool {
        let Some(view) = &self.part_view.scene else { return true };
        self.part_view.visible[key.kind as usize].get_or_init(|| {
            let geo = self.scene().geometry.get(0, 1);
            (0..self.scene().mesh.triangle_count()).filter(|&t| view.mesh.indices[t * 3..t * 3 + 3] == self.scene().mesh.indices[t * 3..t * 3 + 3])
                .filter_map(|t| geo.triangle_region(t, key.kind)).collect()
        }).contains(&key.id)
    }

    pub fn show_all_parts(&mut self) {
        if !self.has_hidden_parts() { return; }
        self.part_view.hidden.clear(); self.part_view.isolated = None; self.part_view.scene = None;
        self.part_view.visible = Default::default();
        self.part_view.revision += 1;
    }

    fn rebuild_part_view(&mut self) {
        let mut mesh = (*self.scene().mesh).clone();
        let geo = self.scene().geometry.get(0, 1);
        for t in 0..mesh.triangle_count() {
            let hidden = self.part_view.hidden.iter().any(|key| geo.triangle_region(t, key.kind) == Some(key.id));
            let outside = self.part_view.isolated.is_some_and(|key| geo.triangle_region(t, key.kind) != Some(key.id));
            // Retain triangle numbering and material ranges. Degenerate faces don't draw, hit or
            // occlude; project geometry and its masks/bakes remain intact.
            if hidden || outside { mesh.indices[t * 3..t * 3 + 3].fill(0); }
        }
        self.part_view.scene = Some(Scene::new(mesh)); self.part_view.visible = Default::default(); self.part_view.revision += 1;
    }

    /// Combined material edits are one undo step. View settings and picking don't edit projects.
    pub fn apply_part_action(&mut self, target: SurfaceTarget, action: PartAction) -> Result<(), CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let t = target.triangle as usize;
        if self.region_of(t, target.part.kind) != Some(target.part.id)
            || self.scene().mesh.tri_material.get(t).and_then(|&slot| self.document().texture_set_for_slot(slot)).map(|ts| ts.id) != Some(target.texture_set) {
            return Err(CommandError::Invalid("part belongs to another model".into()));
        }
        match action {
            PartAction::Hide => {
                if !self.part_view.hidden.remove(&target.part) { self.part_view.hidden.insert(target.part); }
                self.rebuild_part_view(); return Ok(());
            }
            PartAction::Isolate => {
                self.part_view.isolated = if self.part_view.isolated == Some(target.part) { None } else { Some(target.part) };
                self.part_view.hidden.remove(&target.part); self.rebuild_part_view(); return Ok(());
            }
            PartAction::PickColor | PartAction::PickMaterial => {
                self.sample_material_at(target.texture_set, target.uv, action == PartAction::PickMaterial); return Ok(());
            }
            _ => {}
        }
        if matches!(action, PartAction::MaskLayer | PartAction::Generator(_)) && self.selection().texture_set != target.texture_set {
            return Err(CommandError::Invalid("select a layer in this part's texture set first".into()));
        }
        if self.selection().texture_set != target.texture_set { self.execute(Command::SelectTextureSet(target.texture_set))?; }
        let parts = PartMask::for_kind(target.part.kind, [target.part.id]);
        match action {
            PartAction::Fill | PartAction::PaintOnly => {
                let info = self.parts(target.part.kind).iter().find(|p| p.id == target.part.id).ok_or(CommandError::Invalid("part not found".into()))?;
                let mut layers = Vec::new();
                for ts in self.document().texture_sets.iter().filter(|ts| info.materials.contains(&ts.material_slot)) {
                    let mut layer = if action == PartAction::Fill { Layer::new_fill(format!("Fill · {}", info.name), ts) } else { Layer::new_paint(format!("Paint · {}", info.name), ts) };
                    layer.parts = Some(parts.clone());
                    if let subcraft_doc::LayerContent::Fill(fill) = &mut layer.content {
                        for (&channel, value) in &self.brush.material.channels {
                            if let Some(f) = fill.channels.get_mut(&channel) {
                                *f = if value.enabled { FillChannel::new(FillSource::uniform(value.value)) } else { FillChannel::disabled(FillSource::uniform(value.value)) };
                            }
                        }
                    }
                    layers.push((ts.id, Box::new(layer)));
                }
                self.execute(Command::InsertPartLayers { layers, label: "Texture Part".into() })?;
                self.brush.target = subcraft_paint::PaintTarget::Content;
            }
            PartAction::MaskLayer => {
                let layer = self.selection().layer.ok_or(CommandError::LayerNotFound)?;
                self.execute(Command::SetParts { layer, parts: Some(parts) })?;
            }
            PartAction::Generator(kind) => {
                let id = self.selection().layer.ok_or(CommandError::LayerNotFound)?;
                let mut layer = self.selected_layer().ok_or(CommandError::LayerNotFound)?.clone();
                layer.parts = Some(parts);
                let mask = layer.mask.get_or_insert_with(|| Mask::new(self.selected_texture_set().resolution, false));
                mask.enabled = true;
                mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(Generator::new(kind)))));
                self.execute(Command::ReplaceLayer { layer: id, replacement: Box::new(layer), label: "Part Generator".into() })?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Linear, unlit material sampling; adapted from CalangoTech's SubCraft Studio sampler.
    pub fn sample_uv(&mut self, texture_set: TextureSetId, uv: Vec2, channel: ChannelKind) -> Option<ChannelValue> {
        if !uv.is_finite() { return None; }
        self.update_composites();
        let image = self.composite(texture_set)?.channel(channel)?;
        let p = uv.rem_euclid(Vec2::ONE) * Vec2::new(image.width() as f32, image.height() as f32) - Vec2::splat(0.5);
        let base = p.floor(); let frac = p - base;
        let at = |dx: i32, dy: i32| image.get((base.x as i32 + dx).rem_euclid(image.width() as i32) as u32, (base.y as i32 + dy).rem_euclid(image.height() as i32) as u32);
        let a = at(0, 0); let b = at(1, 0); let c = at(0, 1); let d = at(1, 1);
        let mut value = [0.0; 3];
        for i in 0..channel.components() {
            value[i] = (a[i] * (1.0 - frac.x) + b[i] * frac.x) * (1.0 - frac.y) + (c[i] * (1.0 - frac.x) + d[i] * frac.x) * frac.y;
        }
        Some(ChannelValue(value))
    }

    pub fn sample_material_at(&mut self, texture_set: TextureSetId, uv: Vec2, material: bool) {
        let channels = if material { self.document().texture_set(texture_set).map(|ts| ts.channels.iter().map(|c| c.kind).collect()).unwrap_or_default() } else { vec![ChannelKind::BaseColor] };
        for channel in channels { if let Some(value) = self.sample_uv(texture_set, uv, channel) { self.brush.material.set(channel, value); } }
    }

    pub fn sample_surface(&mut self, camera: &Camera, screen: Vec2, material: bool) -> bool {
        let Some(pick) = self.pick(camera, screen) else { return false };
        let Some(ts) = self.document().texture_set_for_slot(pick.material_slot).map(|ts| ts.id) else { return false };
        self.sample_material_at(ts, pick.uv, material); true
    }

    /// Follow a part's paint layer across material IDs at the start of each stroke.
    pub fn prepare_surface_paint(&mut self, camera: &Camera, screen: Vec2) -> Result<(), CommandError> {
        let Some(parts) = self.selected_layer().filter(|l| matches!(l.content, subcraft_doc::LayerContent::Paint(_)))
            .and_then(|l| l.parts.as_ref()).filter(|p| p.enabled).cloned() else { return Ok(()) };
        let Some(pick) = self.pick(camera, screen) else { return Ok(()) };
        let Some(ts) = self.document().texture_set_for_slot(pick.material_slot) else { return Ok(()) };
        if ts.id == self.selection().texture_set { return Ok(()); }
        let mut matching = None;
        ts.walk(|l, _| { if l.parts.as_ref() == Some(&parts) && matches!(l.content, subcraft_doc::LayerContent::Paint(_)) { matching = Some(l.id); } });
        if let Some(layer) = matching { self.execute(Command::SelectLayer(Some(layer)))?; }
        Ok(())
    }
}
