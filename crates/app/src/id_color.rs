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
