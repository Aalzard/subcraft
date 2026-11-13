//! ID Color Mask workflow (ADR-0041): sampling the baked ID map and picking colors from the
//! 3D view. Widgets call these; the document only changes through [`Command`]s.

use std::sync::Arc;

use glam::Vec2;
use subcraft_core::{EffectId, LayerId, MeshMapKind, TextureSetId};
use subcraft_doc::{IdColorMask, MaskEffectKind};

use crate::{Camera, Command, CommandError, Session};

impl Session {
    /// Whether this texture set has a baked ID map (without one an ID Color Mask is black).
    pub fn has_id_map(&self, texture_set: TextureSetId) -> bool {
        self.mesh_maps(texture_set).is_some_and(|m| m.get(MeshMapKind::Id).is_some())
    }

    /// ID map color at `uv` (wrapped into the 0..1 tile), point-sampled: interpolating would
    /// invent colors that belong to no element at region borders.
    pub fn id_map_color(&self, texture_set: TextureSetId, uv: Vec2) -> Option<[f32; 3]> {
        if !uv.is_finite() { return None; }
        let img = self.mesh_maps(texture_set)?.get(MeshMapKind::Id)?;
        let p = uv.rem_euclid(Vec2::ONE);
        let x = ((p.x * img.width() as f32) as u32).min(img.width() - 1);
        let y = ((p.y * img.height() as f32) as u32).min(img.height() - 1);
        let c = img.get(x, y);
        Some([c[0], c[1], c[2]])
    }

    /// ID color under a screen position, with the texture set it belongs to.
    pub fn id_color_under(&self, camera: &Camera, screen: Vec2) -> Option<(TextureSetId, [f32; 3])> {
        let pick = self.pick(camera, screen)?;
        let ts = self.document().texture_set_for_slot(pick.material_slot)?.id;
        Some((ts, self.id_map_color(ts, pick.uv)?))
    }

    /// Eyedropper for an ID Color Mask: adds the ID color under the cursor to the effect as one
    /// undo step. Returns the color, or `None` when the click missed the model / had no ID map /
    /// the color was already picked (no history entry in those cases).
    pub fn pick_id_color(&mut self, layer: LayerId, effect: EffectId, camera: &Camera, screen: Vec2) -> Result<Option<[f32; 3]>, CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let ts = self.document().texture_sets.iter().find(|t| t.find(layer).is_some()).ok_or(CommandError::LayerNotFound)?;
        let ts_id = ts.id;
        let mut id = Self::id_color_effect(ts.find(layer), effect)?.clone();
        let Some((hit_ts, color)) = self.id_color_under(camera, screen) else { return Ok(None) };
        if hit_ts != ts_id {
            return Err(CommandError::Invalid("that surface belongs to another texture set".into()));
        }
        if !id.add_color(color) { return Ok(None); }
        self.execute(Command::SetIdColors { layer, effect, colors: id.colors })?;
        Ok(Some(color))
    }

    /// Removes one picked color (one undo step).
    pub fn remove_id_color(&mut self, layer: LayerId, effect: EffectId, index: usize) -> Result<(), CommandError> {
        let layer_ref = self.document().texture_sets.iter().find_map(|t| t.find(layer));
        let mut colors = Self::id_color_effect(layer_ref, effect)?.colors.clone();
        if index >= colors.len() { return Err(CommandError::Invalid("no such ID color".into())); }
        colors.remove(index);
        self.execute(Command::SetIdColors { layer, effect, colors })
    }

    /// "Mask by ID color": give the selected layer a mask (black if it had none) selecting the
    /// ID color at `uv` of `texture_set`, as one undo step.
    pub fn mask_selected_by_id_color(&mut self, texture_set: TextureSetId, uv: Vec2) -> Result<(), CommandError> {
        if self.selection().texture_set != texture_set {
            return Err(CommandError::Invalid("select a layer in this part's texture set first".into()));
        }
        let layer = self.selection().layer.ok_or(CommandError::LayerNotFound)?;
        let color = self.id_map_color(texture_set, uv).ok_or_else(|| CommandError::Invalid("bake an ID map first (Bake Mesh Maps)".into()))?;
        let mut id = IdColorMask::default();
        id.add_color(color);
        self.execute(Command::ApplyMaskEffects { layer, effects: vec![MaskEffectKind::IdColor(id)], label: "Mask by ID Color".into() })
    }

    fn id_color_effect(layer: Option<&subcraft_doc::Layer>, effect: EffectId) -> Result<&IdColorMask, CommandError> {
        let mask = layer.ok_or(CommandError::LayerNotFound)?.mask.as_ref().ok_or(CommandError::NoMask)?;
        let e: &Arc<subcraft_doc::MaskEffect> = mask.effects.iter().find(|e| e.id == effect).ok_or_else(|| CommandError::Invalid("no such effect".into()))?;
        match &e.kind {
            MaskEffectKind::IdColor(id) => Ok(id),
            _ => Err(CommandError::Invalid("not an ID Color Mask".into())),
        }
    }
}

// ----- ID color editor -----------------------------------------------------------------------

/// Two ID colors are the same color when every channel agrees to half an 8-bit step (maps are
/// stored as f16, picked colors as f32).
pub fn same_id(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 0.5 / 255.0)
}

/// The baked ID map with the editor's changes applied (texels of `from` become `to`).
pub fn apply_id_edits(baked: &subcraft_image::TiledImage, edits: &[subcraft_doc::IdColorEdit]) -> subcraft_image::TiledImage {
    if edits.is_empty() {
        return baked.clone();
    }
    let comps = baked.comps();
    let mut dense = baked.to_dense();
    for px in dense.chunks_mut(comps) {
        let c = [px[0], px[1.min(comps - 1)], px[2.min(comps - 1)]];
        if let Some(e) = edits.iter().find(|e| same_id(c, e.from)) {
            let n = comps.min(3);
            px[..n].copy_from_slice(&e.to[..n]);
        }
    }
    subcraft_image::TiledImage::from_dense(baked.width(), baked.height(), comps, baked.fill(), &dense)
}

/// Whether some texel of `img` is the ID color `c`.
pub(crate) fn image_has_color(img: &subcraft_image::TiledImage, c: [f32; 3]) -> bool {
    let comps = img.comps();
    let rgb = |p: &[f32]| [p[0], p[1.min(comps - 1)], p[2.min(comps - 1)]];
    let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * comps];
    for t in 0..img.tile_count() {
        if img.is_allocated(t) {
            img.read_tile(t, &mut buf);
            if buf.chunks(comps).any(|p| same_id(rgb(p), c)) {
                return true;
            }
        } else if same_id(rgb(&img.fill()[..]), c) {
            return true;
        }
    }
    false
}

/// Record "show `from` as `to`" in the set and switch every ID Color mask color equal to `from`
/// to `to`. `from` is a color as currently shown: every texel showing it changes, whether it is
/// an earlier edit's result, the baked color (`baked_has_from`: the baked map has texels of it),
/// or both after two colors were merged. QA 2026-11: only the first edit showing `from` was
/// rewritten, so after merging A into B, editing B changed the former A region alone while the
/// masks switched to the new color and lost the original B region.
pub(crate) fn edit_id_color(ts: &mut subcraft_doc::TextureSet, from: [f32; 3], to: [f32; 3], baked_has_from: bool) {
    // Baked texels of `from` show `from` unless an edit already maps them elsewhere.
    let baked_shown = baked_has_from && !ts.id_color_edits.iter().any(|e| same_id(e.from, from));
    for e in ts.id_color_edits.iter_mut().filter(|e| same_id(e.to, from)) {
        e.to = to;
    }
    ts.id_color_edits.retain(|e| !same_id(e.from, e.to));
    if baked_shown && !same_id(from, to) {
        ts.id_color_edits.push(subcraft_doc::IdColorEdit { from, to });
    }
    let mut ids = Vec::new();
    ts.walk(|l, _| ids.push(l.id));
    for id in ids {
        let Some(layer) = ts.find_mut(id) else { continue };
        let Some(mask) = layer.mask.as_mut() else { continue };
        for effect in &mut mask.effects {
            let MaskEffectKind::IdColor(m) = &effect.kind else { continue };
            if !m.colors.iter().any(|c| same_id(*c, from)) { continue; }
            let MaskEffectKind::IdColor(m) = &mut Arc::make_mut(effect).kind else { continue };
            let mut colors: Vec<[f32; 3]> = Vec::with_capacity(m.colors.len());
            for c in m.colors.iter().map(|c| if same_id(*c, from) { to } else { *c }) {
                if !colors.iter().any(|k| same_id(*k, c)) { colors.push(c); }
            }
            m.colors = colors;
        }
    }
}

/// One color of an ID map in the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct IdPaletteEntry {
    /// The color as stored in the shown map (what masks pick).
    pub color: [f32; 3],
    /// Share of the map's texels with this color (gutters included).
    pub coverage: f32,
    /// How many ID Color mask effects of the set use it.
    pub masks: usize,
}

/// More distinct colors than this means the image is not an ID map (a photo, a gradient).
pub const MAX_PALETTE: usize = 256;

impl Session {
    /// The colors of the set's ID map, most common first (`None` without an ID map). Colors are
    /// grouped by 8-bit value; at most [`MAX_PALETTE`].
    pub fn id_palette(&self, texture_set: TextureSetId) -> Option<Vec<IdPaletteEntry>> {
        let img = self.mesh_maps(texture_set)?.get(MeshMapKind::Id)?;
        let comps = img.comps();
        let dense = img.to_dense();
        let mut counts: std::collections::HashMap<[u8; 3], (usize, [f32; 3])> = std::collections::HashMap::new();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        for px in dense.chunks(comps) {
            let c = [px[0], px[1.min(comps - 1)], px[2.min(comps - 1)]];
            let e = counts.entry([q(c[0]), q(c[1]), q(c[2])]).or_insert((0, c));
            e.0 += 1;
        }
        let total = (dense.len() / comps).max(1) as f32;
        let ts = self.document().texture_set(texture_set)?;
        let mut used: Vec<[f32; 3]> = Vec::new();
        ts.walk(|l, _| {
            if let Some(m) = &l.mask {
                for e in &m.effects {
                    if let MaskEffectKind::IdColor(id) = &e.kind { used.extend(id.colors.iter().copied()); }
                }
            }
        });
        let mut out: Vec<IdPaletteEntry> = counts.into_values()
            .map(|(n, color)| IdPaletteEntry { color, coverage: n as f32 / total, masks: used.iter().filter(|c| same_id(**c, color)).count() })
            .collect();
        out.sort_by(|a, b| b.coverage.total_cmp(&a.coverage).then(a.color.partial_cmp(&b.color).unwrap_or(std::cmp::Ordering::Equal)));
        out.truncate(MAX_PALETTE);
        Some(out)
    }

    /// Change one ID map color (as shown) to `to`, updating every ID Color mask of the set that
    /// uses it. One undo step; undo restores the map color and the masks together.
    pub fn edit_id_color(&mut self, texture_set: TextureSetId, from: [f32; 3], to: [f32; 3]) -> Result<(), CommandError> {
        if !self.has_id_map(texture_set) {
            return Err(CommandError::Invalid("bake or import an ID map first".into()));
        }
        self.execute(Command::EditIdColor { texture_set, from, to })
    }

    /// The selected layer's ID Color mask effect: the selected effect when it is one, else the
    /// first in the mask.
    pub fn selected_id_color_effect(&self) -> Option<(LayerId, EffectId)> {
        let layer = self.selected_layer()?;
        let mask = layer.mask.as_ref()?;
        let is_id = |e: &subcraft_doc::MaskEffect| matches!(e.kind, MaskEffectKind::IdColor(_));
        let effect = self.selected_effect().filter(|id| mask.effects.iter().any(|e| e.id == *id && is_id(e)))
            .or_else(|| mask.effects.iter().find(|e| is_id(e)).map(|e| e.id))?;
        Some((layer.id, effect))
    }

    /// Eyedropper result from the ID color editor (3D or 2D view): add `color` to the selected
    /// layer's ID Color mask, or give the selected layer an ID Color mask selecting it. Returns
    /// what was done, for the status line.
    pub fn feed_id_color(&mut self, texture_set: TextureSetId, color: [f32; 3]) -> Result<String, CommandError> {
        if self.selection().texture_set != texture_set {
            return Err(CommandError::Invalid("select a layer in this part's texture set first".into()));
        }
        if let Some((layer, effect)) = self.selected_id_color_effect() {
            let layer_ref = self.document().texture_sets.iter().find_map(|t| t.find(layer));
            let mut id = Self::id_color_effect(layer_ref, effect)?.clone();
            if id.colors.iter().any(|c| same_id(*c, color)) { return Ok("That ID color is already in the mask.".into()); }
            if !id.add_color(color) { return Err(CommandError::Invalid("the ID Color mask is full".into())); }
            self.execute(Command::SetIdColors { layer, effect, colors: id.colors })?;
            return Ok("ID color added to the layer's ID Color mask.".into());
        }
        let layer = self.selection().layer.ok_or_else(|| CommandError::Invalid("select a layer to mask by this ID color".into()))?;
        let mut id = IdColorMask::default();
        id.add_color(color);
        self.execute(Command::ApplyMaskEffects { layer, effects: vec![MaskEffectKind::IdColor(id)], label: "Mask by ID Color".into() })?;
        Ok("The selected layer is now masked by this ID color.".into())
    }

    /// The ID Color Mask eyedropper in the 2D view: add the ID color at texture `uv` of the
    /// layer's set to the effect (one undo step; `None` when nothing was added).
    pub fn pick_id_color_uv(&mut self, layer: LayerId, effect: EffectId, uv: Vec2) -> Result<Option<[f32; 3]>, CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let ts = self.document().texture_sets.iter().find(|t| t.find(layer).is_some()).ok_or(CommandError::LayerNotFound)?;
        let ts_id = ts.id;
        let mut id = Self::id_color_effect(ts.find(layer), effect)?.clone();
        if !(0.0..=1.0).contains(&uv.x) || !(0.0..=1.0).contains(&uv.y) { return Ok(None); }
        let Some(color) = self.id_map_color(ts_id, uv) else { return Ok(None) };
        if id.colors.iter().any(|c| same_id(*c, color)) || !id.add_color(color) { return Ok(None); }
        self.execute(Command::SetIdColors { layer, effect, colors: id.colors })?;
        Ok(Some(color))
    }
}
