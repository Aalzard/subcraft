//! Picking and painting on the displacement preview (issue: the brush landed on the modelled
//! surface behind or inside the displaced one). The 3D view tells the session the preview's
//! level and height scale each frame; the session keeps a CPU copy of the displaced surface
//! (see [`subcraft_mesh::DisplacedSurface`]) that follows the Height and Displacement
//! composites, and [`Session::pick`] and the brush ray-cast onto it.

use std::sync::Arc;

use glam::Vec2;
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::DisplacedSurface;

use crate::session::Session;

/// Triangle budget of the picking copy: the preview's level, coarsened until it fits. A coarser
/// copy follows the same heights at its vertices, so the error is the height's variation inside
/// one picking triangle; it keeps rebuilding the copy (on a level or mesh change) well under a
/// frame's worth of hitch on large meshes.
pub const PICK_TRIANGLE_BUDGET: usize = 262_144;

/// The subdivision level picking uses for a preview at `preview_level` on a mesh of `triangles`.
pub fn pick_level(preview_level: u32, triangles: usize) -> u32 {
    let mut level = preview_level;
    while level > 0 && triangles.saturating_mul(4usize.pow(level)) > PICK_TRIANGLE_BUDGET {
        level -= 1;
    }
    level
}

/// Height inputs of one material slot: its Height and Displacement composites.
type SlotHeights = (u32, Option<TiledImage>, Option<TiledImage>);

pub(crate) struct DisplacedPick {
    /// The viewport mesh it was built from (`Arc` address and triangle count).
    mesh: (usize, usize),
    level: u32,
    scale: f32,
    heights: Vec<SlotHeights>,
    pub(crate) surface: DisplacedSurface,
}

/// Bilinear sample of a one-component image at `uv` (wrapping into the 0..1 tile), as the GPU
/// samples the preview's height textures.
fn sample(img: &TiledImage, uv: Vec2) -> f32 {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (u, v) = (uv.x - uv.x.floor(), uv.y - uv.y.floor());
    let (x, y) = ((u * w - 0.5).clamp(0.0, w - 1.0), (v * h - 0.5).clamp(0.0, h - 1.0));
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (tx, ty) = (x.fract(), y.fract());
    let at = |xx: u32, yy: u32| img.get(xx, yy)[0];
    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
    top + (bottom - top) * ty
}

/// True when `a` and `b` hold the same texels (shared storage, or no tile differs).
fn same_image(a: &Option<TiledImage>, b: &Option<TiledImage>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.same_storage(b) || (a.same_layout(b) && a.changed_tiles(b).is_empty()),
        _ => false,
    }
}

impl Session {
    /// The 3D view's displacement preview: `Some((subdivision level, height scale))` while it
    /// is drawn, `None` when it is off. While it is on, [`Session::pick`] and the brush hit the
    /// displaced surface (and report the point of the modelled surface under it). Cheap to call
    /// every frame: the CPU copy is only rebuilt when the mesh or level changes, and moved when
    /// the scale or a Height / Displacement composite changes.
    pub fn set_displacement_preview(&mut self, preview: Option<(u32, f32)>) {
        let Some((level, scale)) = preview.filter(|(_, s)| s.is_finite()) else {
            self.displaced = None;
            return;
        };
        let mesh = self.viewport_scene().mesh.clone();
        let key = (Arc::as_ptr(&mesh) as usize, mesh.triangle_count());
        let level = pick_level(level, mesh.triangle_count());
        // The Height and Displacement composites of each slot's first texture set (what the
        // viewport uploads per slot).
        let mut heights: Vec<SlotHeights> = Vec::new();
        for ts in &self.document().texture_sets {
            if heights.iter().any(|h| h.0 == ts.material_slot) {
                continue;
            }
            let c = self.composite(ts.id);
            let get = |k: ChannelKind| c.and_then(|c| c.channels.get(&k)).cloned();
            heights.push((ts.material_slot, get(ChannelKind::Height), get(ChannelKind::Displacement)));
        }
        let rebuild = self.displaced.as_ref().is_none_or(|d| d.mesh != key || d.level != level);
        let moved = rebuild
            || self.displaced.as_ref().is_some_and(|d| {
                d.scale != scale
                    || d.heights.len() != heights.len()
                    || d.heights.iter().zip(&heights).any(|(a, b)| a.0 != b.0 || !same_image(&a.1, &b.1) || !same_image(&a.2, &b.2))
            });
        if !moved {
            // Same texels: keep the new images so the next comparison is a pointer check.
            if let Some(d) = &mut self.displaced {
                d.heights = heights;
            }
            return;
        }
        let mut pick = match self.displaced.take() {
            Some(d) if !rebuild => d,
            _ => DisplacedPick { mesh: key, level, scale, heights: Vec::new(), surface: DisplacedSurface::new(&mesh, level) },
        };
        let height = |slot: u32, uv: Vec2| -> f32 {
            let Some((_, h, d)) = heights.iter().find(|h| h.0 == slot) else { return 0.0 };
            h.as_ref().map_or(0.0, |h| sample(h, uv)) + d.as_ref().map_or(0.0, |d| sample(d, uv))
        };
        pick.surface.displace(height, scale);
        pick.scale = scale;
        pick.heights = heights;
        self.displaced = Some(pick);
    }

    /// The displaced surface picking and the brush use, while the preview is on.
    pub fn displaced_surface(&self) -> Option<&DisplacedSurface> {
        self.displaced.as_ref().map(|d| &d.surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picking_coarsens_the_preview_level_to_its_budget() {
        assert_eq!(pick_level(3, 1000), 3);
        assert_eq!(pick_level(4, 1000), 4);
        assert_eq!(pick_level(4, 100_000), 0);
        assert_eq!(pick_level(3, 60_000), 1);
        assert_eq!(pick_level(2, 1_000_000), 0);
        assert_eq!(pick_level(0, 10), 0);
    }
}
