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
