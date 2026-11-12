//! Quick mask (Painter's U / Y / I): a temporary grayscale mask per texture set, outside the
//! document and its undo history. While its edition is on, paint tools paint into it (paint
//! protects, erase frees). Once edition is closed it protects the texels it covers from every
//! stroke and polygon fill, until it is cleared.

use std::collections::HashMap;
use std::sync::Arc;

use subcraft_core::{LayerId, TextureSetId};
use subcraft_doc::{Layer, Mask, TextureSet};
use subcraft_image::{TILE_TEXELS, TiledImage};

use crate::Session;

/// Quick mask state of a session.
#[derive(Default)]
pub(crate) struct QuickMask {
    /// Per texture set: 1 comp at the set's resolution, 1 = protected.
    pub(crate) masks: HashMap<TextureSetId, TiledImage>,
    pub(crate) editing: bool,
    /// Bumped whenever a mask changes (UI caches key off it).
    pub(crate) revision: u64,
    /// While a quick mask stroke runs: a copy of the texture set holding one layer whose mask
    /// base is the quick mask, so the regular stroke engine paints it.
    pub(crate) scratch: Option<(TextureSet, LayerId)>,
}

impl QuickMask {
    /// A texture set with only one layer whose mask base is the current quick mask of `ts`.
    pub(crate) fn scratch_for(&self, ts: &TextureSet) -> (TextureSet, LayerId) {
        let mut layer = Layer::new_paint("Quick mask", ts);
        let mut mask = Mask::new(ts.resolution, false);
        if let Some(m) = self.masks.get(&ts.id).filter(|m| m.width() == ts.resolution && m.height() == ts.resolution) {
            mask.base = m.clone();
        }
        layer.mask = Some(mask);
        let id = layer.id;
        let mut scratch = ts.clone();
        scratch.layers = vec![Arc::new(layer)];
        (scratch, id)
    }

    /// Take the painted mask back from the scratch texture set.
    pub(crate) fn store_scratch(&mut self) {
        let Some((ts, layer)) = &self.scratch else { return };
        let Some(base) = ts.find(*layer).and_then(|l| l.mask.as_ref()).map(|m| m.base.clone()) else { return };
        if self.masks.get(&ts.id).is_none_or(|m| !m.same_storage(&base)) {
            self.masks.insert(ts.id, base);
            self.revision += 1;
        }
    }
}

/// `1 - v` for every texel of a 1-comp image (unallocated tiles keep sharing the flipped fill).
pub fn inverted(mask: &TiledImage) -> TiledImage {
    let fill = mask.fill();
    let flipped = 1.0 - fill[0];
    let mut out = TiledImage::new(mask.width(), mask.height(), mask.comps(), [flipped, flipped, flipped, flipped]);
    let comps = mask.comps();
    let mut texels = vec![0.0f32; TILE_TEXELS * comps];
    for t in 0..mask.tile_count() {
        if !mask.is_allocated(t) {
            continue;
        }
        mask.read_tile(t, &mut texels);
        for v in texels.iter_mut() {
            *v = 1.0 - *v;
        }
        out.write_tile(t, &texels);
    }
    out
}

impl Session {
    /// Whether quick mask edition is on (strokes paint the quick mask).
    pub fn quick_mask_editing(&self) -> bool {
        self.quick_mask.editing
    }

    /// Turn quick mask edition on or off (U). A running stroke ends first.
    pub fn set_quick_mask_editing(&mut self, on: bool) {
        if self.quick_mask.editing == on {
            return;
        }
        self.end_stroke();
        self.quick_mask.editing = on;
        self.quick_mask.revision += 1;
        log::info!("action: quick mask edition {}", if on { "on" } else { "off" });
    }

    /// Toggle quick mask edition (U).
    pub fn toggle_quick_mask(&mut self) {
        self.set_quick_mask_editing(!self.quick_mask.editing);
    }

    /// The quick mask of a texture set (1 = protected), if one was painted or inverted.
    pub fn quick_mask(&self, ts: TextureSetId) -> Option<&TiledImage> {
        self.quick_mask.masks.get(&ts)
    }

    /// Changes whenever a quick mask or its edition state changes.
    pub fn quick_mask_revision(&self) -> u64 {
        self.quick_mask.revision
    }

    /// Clear the selected texture set's quick mask (Y): nothing is protected any more.
    pub fn clear_quick_mask(&mut self) {
        self.end_stroke();
        if self.quick_mask.masks.remove(&self.selection.texture_set).is_some() {
            self.quick_mask.revision += 1;
            log::info!("action: clear quick mask");
        }
    }

    /// Invert the selected texture set's quick mask (I): protected texels become free and back.
    /// Without a mask yet, everything becomes protected.
    pub fn invert_quick_mask(&mut self) {
        self.end_stroke();
        let ts = self.selection.texture_set;
        let Some(res) = self.doc.texture_set(ts).map(|t| t.resolution) else { return };
        let current = self.quick_mask.masks.get(&ts).filter(|m| m.width() == res && m.height() == res).cloned();
        let mask = current.unwrap_or_else(|| TiledImage::new(res, res, 1, [0.0; 4]));
        self.quick_mask.masks.insert(ts, inverted(&mask));
        self.quick_mask.revision += 1;
        log::info!("action: invert quick mask");
    }
}

/// Linear red the quick mask is shown in, and how strongly a fully masked texel takes it.
pub const TINT: [f32; 3] = [0.85, 0.04, 0.04];
pub const TINT_STRENGTH: f32 = 0.55;

/// Base Color shown while the quick mask is edited: the composite tinted red where the mask
/// covers it. Kept per texture set and refreshed tile by tile (only tiles whose base color or
/// mask changed are recomputed), so painting the mask stays cheap at 4K.
#[derive(Default)]
pub struct TintCache {
    sets: HashMap<TextureSetId, TintEntry>,
}

struct TintEntry {
    base: TiledImage,
    mask: TiledImage,
    tinted: TiledImage,
}

/// What [`TintCache::update`] asks the viewport to upload.
pub enum TintUpload<'a> {
    /// Nothing changed since the last upload.
    None,
    /// These tiles of the tinted image (all of them when `None`).
    Tinted(&'a TiledImage, Option<Vec<usize>>),
    /// Tinting stopped: upload the plain base color again (all tiles).
    Restore,
}

impl TintCache {
    /// The tinted Base Color of `ts` for this frame. `mask` is the quick mask while it is edited
    /// (`None` otherwise); `full` forces a whole upload (the plain composite was just uploaded).
    pub fn update(&mut self, ts: TextureSetId, base: &TiledImage, mask: Option<&TiledImage>, full: bool) -> TintUpload<'_> {
        let Some(mask) = mask.filter(|m| m.width() == base.width() && m.height() == base.height() && base.comps() >= 3) else {
            return if self.sets.remove(&ts).is_some() { TintUpload::Restore } else { TintUpload::None };
        };
        let fresh = self.sets.get(&ts).is_none_or(|e| !e.base.same_layout(base) || !e.mask.same_layout(mask));
        let changed: Option<Vec<usize>> = if fresh {
            None
        } else {
            let e = &self.sets[&ts];
            let mut tiles: Vec<usize> = (0..base.tile_count()).filter(|&t| base.tile_changed(&e.base, t) || mask.tile_changed(&e.mask, t)).collect();
            tiles.dedup();
            Some(tiles)
        };
        if changed.as_ref().is_some_and(|t| t.is_empty()) && !full {
            return TintUpload::None;
        }
        let entry = if fresh {
            self.sets.insert(ts, TintEntry { base: base.clone(), mask: mask.clone(), tinted: base.clone() });
            self.sets.get_mut(&ts).expect("just inserted")
        } else {
            let e = self.sets.get_mut(&ts).expect("checked above");
            e.base = base.clone();
            e.mask = mask.clone();
            e
        };
        let tiles: Vec<usize> = changed.clone().unwrap_or_else(|| (0..base.tile_count()).collect());
        for &t in &tiles {
            tint_tile(&entry.base, &entry.mask, &mut entry.tinted, t);
        }
        TintUpload::Tinted(&entry.tinted, if full { None } else { changed })
    }
}

/// Recompute one tile of `out` from the base color and the mask.
fn tint_tile(base: &TiledImage, mask: &TiledImage, out: &mut TiledImage, t: usize) {
    let masked = mask.is_allocated(t) || mask.fill()[0] > 0.0;
    if !masked {
        // Share the base color's tile again (no copy).
        out.set_slot(t, base.slot(t).clone());
        return;
    }
    let (bc, mc) = (base.comps(), mask.comps());
    let mut b = vec![0.0f32; TILE_TEXELS * bc];
    let mut m = vec![0.0f32; TILE_TEXELS * mc];
    base.read_tile(t, &mut b);
    mask.read_tile(t, &mut m);
    for (px, mv) in b.chunks_exact_mut(bc).zip(m.chunks_exact(mc)) {
        let k = mv[0].clamp(0.0, 1.0) * TINT_STRENGTH;
        for c in 0..3 {
            px[c] += (TINT[c] - px[c]) * k;
        }
    }
    out.write_tile(t, &b);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_follows_the_mask_tile_by_tile_and_restores() {
        let ts = TextureSetId::new();
        let base = TiledImage::new(256, 256, 4, [0.5, 0.5, 0.5, 1.0]);
        let mut mask = TiledImage::new(256, 256, 1, [0.0; 4]);
        mask.set(10, 10, [1.0, 0.0, 0.0, 0.0]);
        let mut cache = TintCache::default();
        let TintUpload::Tinted(img, None) = cache.update(ts, &base, Some(&mask), false) else { panic!("first upload is whole") };
        let p = img.get(10, 10);
        assert!((p[0] - (0.5 + (TINT[0] - 0.5) * TINT_STRENGTH)).abs() < 2e-3 && p[1] < 0.5, "{p:?}");
        assert_eq!(img.get(200, 200), [0.5, 0.5, 0.5, 1.0], "unmasked texels keep the base color");
        assert!(matches!(cache.update(ts, &base, Some(&mask), false), TintUpload::None), "nothing changed");
        // Painting one more texel in another tile re-tints only that tile.
        mask.set(200, 200, [0.5, 0.0, 0.0, 0.0]);
        let TintUpload::Tinted(img, Some(tiles)) = cache.update(ts, &base, Some(&mask), false) else { panic!("partial upload") };
        assert_eq!(tiles, vec![mask.tile_index(200 / subcraft_image::TILE_SIZE, 200 / subcraft_image::TILE_SIZE)]);
        assert!(img.get(200, 200)[0] > 0.5 && img.get(200, 200)[0] < p[0]);
        // Edition closed: the plain base color comes back once.
        assert!(matches!(cache.update(ts, &base, None, false), TintUpload::Restore));
        assert!(matches!(cache.update(ts, &base, None, false), TintUpload::None));
    }

    #[test]
    fn inverting_flips_fill_and_painted_tiles() {
        let mut m = TiledImage::new(128, 128, 1, [0.0; 4]);
        m.set(3, 4, [0.25, 0.0, 0.0, 0.0]);
        let inv = inverted(&m);
        assert!((inv.get(3, 4)[0] - 0.75).abs() < 1e-3);
        assert_eq!(inv.get(100, 100)[0], 1.0);
        assert_eq!(inv.allocated_tiles(), m.allocated_tiles());
        let back = inverted(&inv);
        assert!((back.get(3, 4)[0] - 0.25).abs() < 1e-3 && back.get(100, 100)[0] == 0.0);
    }
}
