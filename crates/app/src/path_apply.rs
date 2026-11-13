//! Erase / Smudge along path (Painter's path types). SubCraft keeps each pen path on its own
//! layer, so these run the path once as a brush stroke on the paint layer below it, with the
//! current brush in Erase or Smudge mode: one undo step, like a hand-drawn stroke.

use subcraft_core::LayerId;
use subcraft_doc::{LayerContent, PenPath};
use subcraft_paint::{PaintError, StrokeMode};

use crate::{Command, Session};

impl Session {
    /// The paint layer a path layer's erase / smudge acts on: the nearest plain paint layer
    /// (not a pen path, not text) below it in the stack.
    pub fn layer_below_path(&self, path_layer: LayerId) -> Option<LayerId> {
        let mut order = Vec::new();
        self.selected_texture_set().walk(|l, _| order.push((l.id, matches!(&l.content, LayerContent::Paint(p) if p.path.is_none() && p.text.is_none()))));
        let i = order.iter().position(|(id, _)| *id == path_layer)?;
        order[..i].iter().rev().find(|(_, paintable)| *paintable).map(|(id, _)| *id)
    }

    /// Run `path` as one brush stroke on `target` with the current brush in `mode` (Erase or
    /// Smudge; anything else paints). The selection and brush mode are restored afterwards.
    pub fn stroke_along_path(&mut self, target: LayerId, path: &PenPath, mode: StrokeMode) -> Result<(), PaintError> {
        let resolution = self.selected_texture_set().resolution;
        let points = path.polyline(0.5 / resolution as f32).map_err(|_| PaintError::LayerNotFound)?;
        if points.is_empty() {
            return Ok(());
        }
        let (selection, brush_mode) = (self.selection().layer, self.brush.mode);
        self.execute(Command::SelectLayer(Some(target))).map_err(|_| PaintError::LayerNotFound)?;
        self.brush.mode = mode;
        let result = (|| {
            self.begin_uv_stroke(None)?;
            for (i, p) in points.iter().enumerate() {
                // Straight segments between the curve's points, no lazy mouse.
                self.stroke_to_uv(*p, i > 0, 1.0)?;
            }
            Ok(())
        })();
        self.end_stroke();
        self.brush.mode = brush_mode;
        let _ = self.execute(Command::SelectLayer(selection));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use subcraft_core::{ChannelKind, ChannelValue};
    use subcraft_doc::{Layer, PenAnchor};

    #[test]
    fn the_layer_below_a_path_skips_other_path_layers() {
        let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 1), 64);
        let base = s.selection().layer.unwrap();
        let ts = s.selected_texture_set().clone();
        let path_layer = |name: &str| {
            let mut l = Layer::new_paint(name, &ts);
            let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
            p.path = Some(PenPath {
                anchors: vec![PenAnchor::new(Vec2::new(0.2, 0.5)), PenAnchor::new(Vec2::new(0.8, 0.5))],
                closed: false, radius: 0.02, hardness: 1.0, opacity: 1.0,
                values: [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))].into(),
                kind: Default::default(), stamp: None, ribbon: None,
            });
            l
        };
        let (lower, upper) = (path_layer("Path 1"), path_layer("Path 2"));
        let (lower_id, upper_id) = (lower.id, upper.id);
        s.execute(Command::InsertLayer { layer: Box::new(lower), label: "Path".into() }).unwrap();
        s.execute(Command::InsertLayer { layer: Box::new(upper), label: "Path".into() }).unwrap();
        assert_eq!(s.layer_below_path(upper_id), Some(base), "past the other path layer");
        assert_eq!(s.layer_below_path(lower_id), Some(base));
        assert_eq!(s.layer_below_path(base), None, "nothing below the bottom layer");
    }
}
