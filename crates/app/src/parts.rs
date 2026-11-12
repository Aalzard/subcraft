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
    fn mask_kind(&self) -> subcraft_core::PartKind {
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
