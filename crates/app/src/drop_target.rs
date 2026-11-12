//! Where a material dragged onto the 3D view lands (Painter): the texture set under the cursor,
//! or with Ctrl the mesh part under it, and the outline the view highlights while dragging.

use std::collections::HashMap;

use glam::{Vec2, Vec3};
use subcraft_core::{LayerId, PartKind, TextureSetId};
use subcraft_doc::PartMask;

use crate::part_workflow::PartKey;
use crate::{Camera, Command, CommandError, Session};

/// The geometry a drop applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropRegion {
    /// Every triangle of a texture set.
    TextureSet(TextureSetId),
    /// One mesh part (Ctrl + drop): the new layer gets a part mask showing it only there.
    Part(TextureSetId, PartKey),
}

impl DropRegion {
    pub fn texture_set(self) -> TextureSetId {
        match self {
            DropRegion::TextureSet(ts) | DropRegion::Part(ts, _) => ts,
        }
    }
}

impl Session {
    /// The region a drop at `screen` applies to: the part of kind `part` under the cursor when
    /// given (Ctrl), the texture set under it otherwise. `None` off the model.
    pub fn drop_region_at(&self, camera: &Camera, screen: Vec2, part: Option<PartKind>) -> Option<DropRegion> {
        match part {
            Some(kind) => self.surface_target(camera, screen, kind).map(|t| DropRegion::Part(t.texture_set, t.part)),
            None => {
                let pick = self.pick(camera, screen)?;
                self.document().texture_set_for_slot(pick.material_slot).map(|ts| DropRegion::TextureSet(ts.id))
            }
        }
    }

    /// Whether triangle `t` of the mesh belongs to `region`.
    pub fn region_contains(&self, region: DropRegion, t: usize) -> bool {
        let mesh = &self.scene().mesh;
        let Some(&slot) = mesh.tri_material.get(t) else { return false };
        match region {
            DropRegion::TextureSet(ts) => self.document().texture_set(ts).is_some_and(|s| s.material_slot == slot),
            DropRegion::Part(_, key) => self.scene().geometry.get(0, 1).triangle_region(t, key.kind) == Some(key.id),
        }
    }

    /// Border edges of `region` (object space): edges with only one of their triangles inside.
    /// Vertices split at UV seams are matched by position, so seams inside the region do not show.
    pub fn region_outline(&self, region: DropRegion) -> Vec<[Vec3; 2]> {
        let mesh = &self.scene().mesh;
        let key = |p: Vec3| p.to_array().map(f32::to_bits);
        let mut edges: HashMap<([u32; 3], [u32; 3]), (u32, [Vec3; 2])> = HashMap::new();
        for t in 0..mesh.triangle_count() {
            if !self.region_contains(region, t) {
                continue;
            }
            let p = [0, 1, 2].map(|k| mesh.positions[mesh.indices[3 * t + k] as usize]);
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                let (ka, kb) = (key(a), key(b));
                let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
                edges.entry(k).or_insert((0, [a, b])).0 += 1;
            }
        }
        edges.into_values().filter(|(n, _)| *n == 1).map(|(_, e)| e).collect()
    }

    /// After a Ctrl + drop added `layer`: show it only on the dropped-on part (one more undo step).
    pub fn restrict_drop_to_part(&mut self, layer: LayerId, region: DropRegion) -> Result<(), CommandError> {
        let DropRegion::Part(_, key) = region else { return Ok(()) };
        self.execute(Command::SetParts { layer, parts: Some(PartMask::for_kind(key.kind, [key.id])) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_texture_set_has_no_border_and_a_part_outline_is_closed() {
        let s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
        let ts = s.document().texture_sets[0].id;
        assert!(s.region_outline(DropRegion::TextureSet(ts)).is_empty(), "a closed cube has no border edge");
        assert!(s.region_contains(DropRegion::TextureSet(ts), 0));
    }
}
