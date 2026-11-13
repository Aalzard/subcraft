//! The quick mask shown on the model while it is edited (Painter tints the 3D view): Base Color
//! is uploaded tinted red where the mask protects the surface. The 2D view shows the same
//! texture, so it needs no overlay of its own then.

use std::collections::HashMap;

use subcraft_core::TextureSetId;
use subcraft_image::{TILE_TEXELS, TiledImage};

/// Linear red the quick mask is shown in, and how strongly a fully masked texel takes it.
pub const TINT: [f32; 3] = [0.85, 0.04, 0.04];
pub const TINT_STRENGTH: f32 = 0.55;

/// Base Color shown while the quick mask is edited: the composite tinted red where the mask
/// protects it (mask value 0; 1 paints). Kept per texture set and refreshed tile by tile
/// (only tiles whose base color or mask changed are recomputed), so painting the mask stays cheap at 4K.
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
    /// The tinted Base Color of `ts` for this frame. `mask` is the quick mask (1 paints, 0
    /// protects) while it is edited
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
    let masked = mask.is_allocated(t) || mask.fill()[0] < 1.0;
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
        let k = (1.0 - mv[0].clamp(0.0, 1.0)) * TINT_STRENGTH;
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
        // Painted (1) everywhere except one protected texel.
        let mut mask = TiledImage::new(256, 256, 1, [1.0; 4]);
        mask.set(10, 10, [0.0, 0.0, 0.0, 0.0]);
        let mut cache = TintCache::default();
        let TintUpload::Tinted(img, None) = cache.update(ts, &base, Some(&mask), false) else { panic!("first upload is whole") };
        let p = img.get(10, 10);
        assert!((p[0] - (0.5 + (TINT[0] - 0.5) * TINT_STRENGTH)).abs() < 2e-3 && p[1] < 0.5, "{p:?}");
        assert_eq!(img.get(200, 200), [0.5, 0.5, 0.5, 1.0], "unmasked texels keep the base color");
        assert!(matches!(cache.update(ts, &base, Some(&mask), false), TintUpload::None), "nothing changed");
        // Painting one more texel in another tile re-tints only that tile.
        mask.set(200, 200, [0.5, 1.0, 1.0, 1.0]);
        let TintUpload::Tinted(img, Some(tiles)) = cache.update(ts, &base, Some(&mask), false) else { panic!("partial upload") };
        assert_eq!(tiles, vec![mask.tile_index(200 / subcraft_image::TILE_SIZE, 200 / subcraft_image::TILE_SIZE)]);
        assert!(img.get(200, 200)[0] > 0.5 && img.get(200, 200)[0] < p[0]);
        // Edition closed: the plain base color comes back once.
        assert!(matches!(cache.update(ts, &base, None, false), TintUpload::Restore));
        assert!(matches!(cache.update(ts, &base, None, false), TintUpload::None));
    }

}
