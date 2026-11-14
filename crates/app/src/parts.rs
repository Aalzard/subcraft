//! Part masks: show a layer only on chosen mesh parts (the connected pieces of the model, e.g.
//! an axe's blade and handle), picked by clicking them in the viewport.

use std::collections::BTreeSet;

use glam::Vec2;
use subcraft_core::LayerId;
use subcraft_doc::PartMask;
use subcraft_paint::Camera;

use crate::command::{Command, CommandError};
use crate::session::Session;

/// One connected piece of the mesh within the selected texture set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshPart {
    pub id: u32,
    pub triangles: usize,
}

impl Session {
    /// Parts of the selected texture set, in id order.
    pub fn mesh_parts(&self) -> Vec<MeshPart> {
        let ts = self.selected_texture_set();
        let geo = self.scene().geometry.get(ts.material_slot, ts.resolution);
        let parts = geo.triangle_parts();
        let mut counts = std::collections::BTreeMap::<u32, usize>::new();
        for t in self.scene().mesh.material_triangles(ts.material_slot) {
            *counts.entry(parts[t]).or_default() += 1;
        }
        counts.into_iter().map(|(id, triangles)| MeshPart { id, triangles }).collect()
    }

    /// The part under a screen position, if it belongs to the selected texture set.
    pub fn part_at(&self, camera: &Camera, screen: Vec2) -> Option<u32> {
        let pick = self.pick(camera, screen)?;
        let ts = self.selected_texture_set();
        if pick.material_slot != ts.material_slot {
            return None;
        }
        self.region_in_mask_kind(pick.triangle as usize)
    }

    /// Which kind of part the selected layer's geometry mask lists (connected pieces when it
    /// has none yet).
    pub fn mask_kind(&self) -> subcraft_core::PartKind {
        self.selected_layer().and_then(|l| l.parts.as_ref()).map_or(self.part_view.new_mask_kind, |p| p.kind)
    }

    /// The kind of part a click or box gives a layer that has no geometry mask yet: connected
    /// pieces by default; the 2D view asks for UV islands.
    pub fn set_new_mask_kind(&mut self, kind: subcraft_core::PartKind) {
        self.part_view.new_mask_kind = kind;
    }

    /// Give the layer a geometry mask listing parts of `kind`, everything included (then clicks
    /// and boxes exclude or include parts). Replaces a mask of another kind. One undo step.
    pub fn begin_layer_parts(&mut self, layer: LayerId, kind: subcraft_core::PartKind) -> Result<(), CommandError> {
        self.selected_texture_set().find(layer).ok_or(CommandError::LayerNotFound)?;
        let all = self.mesh_parts_of_kind(kind);
        self.execute(Command::SetParts { layer, parts: Some(PartMask::for_kind(kind, all)) })
    }

    fn region_in_mask_kind(&self, triangle: usize) -> Option<u32> {
        let ts = self.selected_texture_set();
        self.scene().geometry.get(ts.material_slot, ts.resolution).triangle_region(triangle, self.mask_kind())
    }

    /// UV triangles (wrapped to the 0..1 tile) of the selected texture set whose part, of the
    /// mask's kind, passes `keep`.
    fn uv_triangles_where(&self, keep: impl Fn(Option<u32>) -> bool) -> Vec<[Vec2; 3]> {
        let ts = self.selected_texture_set();
        let geo = self.scene().geometry.get(ts.material_slot, ts.resolution);
        let kind = self.mask_kind();
        let mesh = &self.scene().mesh;
        mesh.material_triangles(ts.material_slot)
            .filter(|&t| keep(geo.triangle_region(t, kind)))
            .map(|t| subcraft_mesh::uv_raster::wrap_to_tile(mesh.tri_uvs(t)).0)
            .collect()
    }

    /// What the selected layer's geometry mask leaves out, as UV triangles of the selected
    /// texture set (the 2D view draws them grey). Empty without a mask, or while it is ignored.
    pub fn excluded_uv_triangles(&self) -> Vec<[Vec2; 3]> {
        match self.selected_layer().and_then(|l| l.parts.as_ref()).filter(|p| p.enabled) {
            Some(mask) => self.uv_triangles_where(|part| mask.factor(part) <= 0.0),
            None => Vec::new(),
        }
    }

    /// The UV triangles of `part` (a part of the kind the geometry mask lists) in the selected
    /// texture set: the 2D view's highlight of the part under the cursor.
    pub fn part_uv_triangles(&self, part: u32) -> Vec<[Vec2; 3]> {
        self.uv_triangles_where(|p| p == Some(part))
    }

    /// The triangle of the selected texture set at a 2D-view texture position (UV 0..1).
    pub fn triangle_at_uv(&self, uv: Vec2) -> Option<u32> {
        let ts = self.selected_texture_set();
        let geo = self.scene().geometry.get(ts.material_slot, ts.resolution);
        subcraft_paint::stroke::triangle_at_uv(&self.scene().mesh, &geo, uv)
    }

    /// What Polygon Fill would fill from `triangle` in `mode`, as UV triangles of the selected
    /// texture set: the 2D view's preview of the element under the cursor.
    pub fn polygon_fill_preview_uv(&self, triangle: u32, mode: subcraft_paint::PolygonFillMode) -> Vec<[Vec2; 3]> {
        let ts = self.selected_texture_set();
        let geo = self.scene().geometry.get(ts.material_slot, ts.resolution);
        let mesh = &self.scene().mesh;
        subcraft_paint::select::grow(mesh, &geo, &[triangle], mode)
            .into_iter()
            .map(|t| subcraft_mesh::uv_raster::wrap_to_tile(mesh.tri_uvs(t as usize)).0)
            .collect()
    }

    /// The part at a 2D-view texture position (UV 0..1) of the selected texture set.
    pub fn part_at_uv(&self, uv: Vec2) -> Option<u32> {
        let ts = self.selected_texture_set();
        let geo = self.scene().geometry.get(ts.material_slot, ts.resolution);
        let t = subcraft_paint::stroke::triangle_at_uv(&self.scene().mesh, &geo, uv)?;
        self.region_in_mask_kind(t as usize)
    }

    /// Parts of the selected texture set with a triangle inside the screen box `[a, b]` (3D view;
    /// hidden geometry is skipped).
    pub fn parts_in_screen_box(&self, camera: &Camera, a: Vec2, b: Vec2) -> BTreeSet<u32> {
        let view = self.viewport_scene();
        let slot = self.selected_texture_set().material_slot;
        let tris = subcraft_paint::select::triangles_in_screen_box(&view.mesh, &view.bvh, camera, slot, a.min(b), a.max(b), false);
        tris.into_iter().filter_map(|t| self.region_in_mask_kind(t as usize)).collect()
    }

    /// Parts of the selected texture set with a triangle inside the UV box `[a, b]` (2D view).
    pub fn parts_in_uv_box(&self, a: Vec2, b: Vec2) -> BTreeSet<u32> {
        let slot = self.selected_texture_set().material_slot;
        let tris = subcraft_paint::select::triangles_in_uv_box(&self.scene().mesh, slot, a.min(b), a.max(b));
        tris.into_iter().filter_map(|t| self.region_in_mask_kind(t as usize)).collect()
    }

    /// Include (`include`) or exclude `parts` in the layer's geometry mask, in one undo step. A
    /// layer without a mask gets one showing exactly `parts` (excluding from no mask starts from
    /// every part of the texture set).
    pub fn set_layer_parts(&mut self, layer: LayerId, parts: &BTreeSet<u32>, include: bool) -> Result<(), CommandError> {
        if parts.is_empty() {
            return Ok(());
        }
        let current = self.selected_texture_set().find(layer).ok_or(CommandError::LayerNotFound)?.parts.clone();
        let kind = self.mask_kind();
        let mut next = match current {
            Some(p) if p.enabled => p,
            Some(p) if include => PartMask::for_kind(p.kind, []),
            None if include => PartMask::for_kind(kind, []),
            _ => PartMask::for_kind(kind, self.mesh_parts_of_kind(kind)),
        };
        next.enabled = true;
        for id in parts {
            if include { next.parts.insert(*id); } else { next.parts.remove(id); }
        }
        self.execute(Command::SetParts { layer, parts: Some(next) })
    }

    /// Every part of `kind` in the selected texture set.
    fn mesh_parts_of_kind(&self, kind: subcraft_core::PartKind) -> BTreeSet<u32> {
        let slot = self.selected_texture_set().material_slot;
        self.parts(kind).iter().filter(|p| p.materials.contains(&slot)).map(|p| p.id).collect()
    }

    /// Show `part` on the layer if hidden, hide it if shown. A layer without a part mask gets one
    /// showing only `part`. One undo step.
    pub fn toggle_layer_part(&mut self, layer: LayerId, part: u32) -> Result<(), CommandError> {
        let current = self.selected_texture_set().find(layer).ok_or(CommandError::LayerNotFound)?.parts.clone();
        let parts = match current {
            Some(mut p) if p.enabled => {
                if !p.parts.remove(&part) {
                    p.parts.insert(part);
                }
                p
            }
            Some(p) => PartMask::for_kind(p.kind, [part]),
            None => PartMask::for_kind(self.mask_kind(), [part]),
        };
        self.execute(Command::SetParts { layer, parts: Some(parts) })
    }
}

/// Mark the texels of a `size` x `size` map that UV triangles (0..1) touch: sets byte `channel`
/// of each `stride`-byte texel to 255. Texels within three quarters of a texel of a triangle
/// count, so thin triangles and island borders are kept.
pub fn raster_uv_triangles(size: u32, tris: &[[Vec2; 3]], out: &mut [u8], stride: usize, channel: usize) {
    let n = size as f32;
    for tri in tris {
        let p = tri.map(|v| v * n);
        let (min, max) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        if !(min.is_finite() && max.is_finite()) {
            continue;
        }
        let lo = |v: f32| (v - 1.0).floor().clamp(0.0, n) as u32;
        let hi = |v: f32| (v + 1.0).ceil().clamp(0.0, n) as u32;
        let sign = if (p[1] - p[0]).perp_dot(p[2] - p[0]) < 0.0 { -1.0 } else { 1.0 };
        let edges = [(p[0], p[1] - p[0]), (p[1], p[2] - p[1]), (p[2], p[0] - p[2])];
        for y in lo(min.y)..hi(max.y) {
            for x in lo(min.x)..hi(max.x) {
                let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                if edges.iter().all(|(a, e)| sign * e.perp_dot(c - *a) >= -0.75 * e.length()) {
                    out[(y * size + x) as usize * stride + channel] = 255;
                }
            }
        }
    }
}

/// The 3D view's part overlay: `size` x `size` texels of two bytes, the first set where `greyed`
/// triangles lie (geometry a geometry mask leaves out) and the second where `tinted` ones do (the
/// part or Polygon Fill element under the cursor).
pub fn overlay_mask(size: u32, greyed: &[[Vec2; 3]], tinted: &[[Vec2; 3]]) -> Vec<u8> {
    let mut out = vec![0u8; (size * size) as usize * 2];
    raster_uv_triangles(size, greyed, &mut out, 2, 0);
    raster_uv_triangles(size, tinted, &mut out, 2, 1);
    out
}

#[cfg(test)]
mod overlay_tests {
    use super::*;

    #[test]
    fn the_overlay_mask_marks_the_texels_under_each_triangle_set() {
        let greyed = [[Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.0), Vec2::new(0.0, 0.5)]];
        // Wound the other way: the winding must not matter.
        let tinted = [[Vec2::new(1.0, 1.0), Vec2::new(0.5, 1.0), Vec2::new(1.0, 0.5)]];
        let m = overlay_mask(16, &greyed, &tinted);
        let at = |x: usize, y: usize| [m[(y * 16 + x) * 2], m[(y * 16 + x) * 2 + 1]];
        assert_eq!(at(1, 1), [255, 0], "inside the greyed triangle");
        assert_eq!(at(14, 14), [0, 255], "inside the tinted triangle");
        assert_eq!(at(8, 8), [0, 0], "between them");
        assert_eq!(at(12, 2), [0, 0], "outside both");
        assert_eq!(overlay_mask(8, &[], &[]), vec![0u8; 128]);
    }

    #[test]
    fn triangles_outside_the_map_or_broken_are_skipped() {
        let off = [[Vec2::new(2.0, 2.0), Vec2::new(3.0, 2.0), Vec2::new(2.0, 3.0)]];
        let broken = [[Vec2::new(f32::NAN, 0.0), Vec2::new(0.5, 0.0), Vec2::new(0.0, 0.5)]];
        assert!(overlay_mask(8, &off, &broken).iter().all(|&b| b == 0));
    }
}
