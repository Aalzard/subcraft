//! The undoable document state.

use subcraft_core::TextureSetId;

use crate::texture_set::TextureSet;

/// Everything the user edits and expects undo to cover: texture sets, channels, layer trees,
/// masks and their raster data. Cloning is cheap (structural sharing), which is what makes
/// snapshot-based history affordable (ADR-0006).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Document {
    pub texture_sets: Vec<TextureSet>,
}

impl Document {
    /// Point every uniform raster tile (all texels equal) at one shared tile per value
    /// (issue #628). Texels stay bit-identical; copy-on-write keeps later edits local.
    pub fn share_uniform_tiles(&mut self) {
        for ts in &mut self.texture_sets {
            for layer in &mut ts.layers {
                share_layer_uniform_tiles(layer);
            }
        }
    }

    /// One texture set per mesh material slot.
    pub fn from_material_slots(names: &[String], resolution: u32) -> Self {
        Self {
            texture_sets: names.iter().enumerate().map(|(i, n)| TextureSet::new(n.clone(), i as u32, resolution)).collect(),
        }
    }

    pub fn texture_set(&self, id: TextureSetId) -> Option<&TextureSet> {
        self.texture_sets.iter().find(|t| t.id == id)
    }

    pub fn texture_set_mut(&mut self, id: TextureSetId) -> Option<&mut TextureSet> {
        self.texture_sets.iter_mut().find(|t| t.id == id)
    }

    pub fn texture_set_for_slot(&self, slot: u32) -> Option<&TextureSet> {
        self.texture_sets.iter().find(|t| t.material_slot == slot)
    }

    /// Every UDIM tile set of `id`'s group (the same base name, e.g. `Body.1001`, `Body.1002`),
    /// `id` included, in document order. Empty when `id` is not a UDIM tile set.
    pub fn udim_group(&self, id: TextureSetId) -> Vec<TextureSetId> {
        let Some(ts) = self.texture_set(id).filter(|t| t.udim_tile.is_some()) else { return Vec::new() };
        let base = ts.base_name();
        self.texture_sets.iter().filter(|t| t.udim_tile.is_some() && t.base_name() == base).map(|t| t.id).collect()
    }

    /// The other tiles a stroke or a new layer on `id` also goes to (Painter's UDIM workflow,
    /// issue #581): the sets of its UDIM group with "paint across UDIM tiles" on, when `id` has
    /// it on too. Empty for ordinary sets and whenever the option is off.
    pub fn udim_paint_partners(&self, id: TextureSetId) -> Vec<TextureSetId> {
        if !self.texture_set(id).is_some_and(|t| t.paint_across_udims) {
            return Vec::new();
        }
        self.udim_group(id)
            .into_iter()
            .filter(|&o| o != id && self.texture_set(o).is_some_and(|t| t.paint_across_udims))
            .collect()
    }

    /// Approximate bytes of unique raster data referenced by this document.
    pub fn raster_bytes(&self) -> usize {
        let mut total = 0;
        for ts in &self.texture_sets {
            ts.walk(|l, _| {
                if let crate::layer::LayerContent::Paint(p) = &l.content {
                    total += p.channels.values().map(|i| i.memory_bytes()).sum::<usize>();
                }
                if let Some(m) = &l.mask {
                    total += m.base.memory_bytes();
                }
            });
        }
        total
    }
}

/// [`Document::share_uniform_tiles`] for one layer subtree.
pub fn share_layer_uniform_tiles(layer: &mut std::sync::Arc<crate::Layer>) {
    std::sync::Arc::make_mut(layer).map_rasters(&|img| {
        let mut img = img.clone();
        img.share_uniform_tiles();
        img
    });
}
