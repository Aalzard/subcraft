//! Part masks: show a layer only on chosen mesh parts (the connected pieces of the model, e.g.
//! an axe's blade and handle), picked by clicking them in the viewport.

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
        let geo = self.scene().geometry.get(ts.material_slot, ts.resolution);
        let kind = self.selected_layer().and_then(|l| l.parts.as_ref()).map_or(subcraft_core::PartKind::Connected, |p| p.kind);
        geo.triangle_region(pick.triangle as usize, kind)
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
            None => PartMask::new([part]),
        };
        self.execute(Command::SetParts { layer, parts: Some(parts) })
    }
}
