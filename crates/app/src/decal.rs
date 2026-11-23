//! Image decals: an image placed once on the surface as a fill layer (Painter's way to make a
//! decal: a fill layer with a planar projection that does not repeat).
//!
//! The decal is an ordinary fill layer, so everything that edits a fill layer edits the decal:
//! the projection manipulator in the 3D view moves, turns and scales it, Properties swaps the
//! image or switches more channels on, and masks and effects work as on any layer.

use std::path::Path;

use glam::Vec3;
use subcraft_assets::texture_maps::{Classified, MapConversion};
use subcraft_core::ChannelKind;
use subcraft_doc::{BackfaceCulling, FillChannel, FillSource, Layer, LayerContent, Projection, ProjectionMode, ProjectionTransform, ShapeCrop, UvWrap};

use crate::command::Command;
use crate::session::Session;

/// Longest side of a new decal, as a part of the model's longest bounds extent.
pub const DECAL_SIZE: f32 = 0.25;

/// Where a decal goes: a surface point, its normal, and the direction the top of the image
/// points to (the view's up direction, so the decal lands the way the image looks on screen).
/// All in object space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecalSpot {
    pub position: Vec3,
    pub normal: Vec3,
    pub up: Vec3,
}

/// The planar projection of a decal at `spot`: drawn once (no repeat), cropped to its box,
/// culled on surfaces that face away, its longest side `size` of the model's longest extent and
/// the other side by the image's `aspect` (width over height). `bounds_min` and `extent` are the
/// model's bounds minimum and longest extent (the projections' normalized frame).
pub fn decal_projection(spot: DecalSpot, aspect: f32, size: f32, bounds_min: Vec3, extent: f32) -> Projection {
    let n = spot.normal.normalize_or_zero();
    let n = if n == Vec3::ZERO { Vec3::Z } else { n };
    // The image's up on the surface: the wanted up without its part along the normal; looking
    // straight along the up direction, any direction on the surface.
    let mut y = spot.up - n * spot.up.dot(n);
    if y.length_squared() < 1e-8 {
        y = n.any_orthogonal_vector();
    }
    let y = y.normalize();
    let x = y.cross(n);
    // Columns are the box's axes: the image's right, its up and the side it faces.
    let rows = [[x.x, y.x, n.x], [x.y, y.y, n.y], [x.z, y.z, n.z]];
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
    let size = size.clamp(ProjectionTransform::MIN_SCALE, ProjectionTransform::MAX_SCALE);
    let side = |s: f32| s.clamp(ProjectionTransform::MIN_SCALE, ProjectionTransform::MAX_SCALE);
    let (w, h) = if aspect >= 1.0 { (size, side(size / aspect)) } else { (side(size * aspect), size) };
    let centre = (spot.position - bounds_min) / extent.max(1e-12);
    let limit = ProjectionTransform::MAX_OFFSET;
    Projection {
        mode: ProjectionMode::Planar,
        planar_axis: 2,
        uv_wrap: UvWrap::None,
        shape_crop: ShapeCrop::Crop,
        backface: Some(BackfaceCulling::default()),
        transform3: ProjectionTransform {
            offset: centre.to_array().map(|c| if c.is_finite() { (c - 0.5).clamp(-limit, limit) } else { 0.0 }),
            rotation_deg: ProjectionTransform::euler_of(rows),
            // As deep as it is wide, so the decal follows a curved surface but does not show
            // through on the far side of the model.
            scale: [w, h, size],
            mirror: [false; 3],
        },
        ..Projection::default()
    }
}

impl Session {
    /// Add the image at `path` as a decal at `spot`: a new fill layer on the selected texture
    /// set showing the image once where the user pointed, keeping the image's proportions and
    /// transparency. The image fills the channel its file name names (a normal map goes to
    /// Normal, with the usual conversions), otherwise Base Color; every other channel is off.
    /// One undo step; returns the layer name.
    pub fn add_decal_layer(&mut self, path: &Path, spot: DecalSpot) -> Result<String, String> {
        if !(spot.position.is_finite() && spot.normal.is_finite() && spot.up.is_finite()) {
            return Err("the decal position is not on the model".into());
        }
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let (channel, conversion) = match subcraft_assets::texture_maps::classify(file) {
            Classified::Map(m) if self.selected_texture_set().has_channel(m.channel) => (m.channel, m.conversion),
            _ => (ChannelKind::BaseColor, MapConversion::None),
        };
        if !self.selected_texture_set().has_channel(channel) {
            return Err(format!("this texture set has no {} channel", channel.display_name()));
        }
        let resource = self.import_converted(path, conversion)?;
        let aspect = self.resources().iter().find(|r| r.id == resource).map_or(1.0, |r| r.bitmap.width as f32 / r.bitmap.height.max(1) as f32);
        let bounds = self.scene().mesh.bounds;
        let projection = decal_projection(spot, aspect, DECAL_SIZE, bounds.min, (bounds.max - bounds.min).max_element());
        let ts = self.selected_texture_set().clone();
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Decal").to_string();
        let mut layer = Layer::new_fill(name.clone(), &ts);
        let LayerContent::Fill(fill) = &mut layer.content else { unreachable!("fill layer") };
        for c in fill.channels.values_mut() {
            c.enabled = false;
        }
        fill.channels.insert(channel, FillChannel::new(FillSource::Bitmap { resource }));
        fill.projection = projection;
        self.execute(Command::InsertLayer { layer: Box::new(layer), label: format!("Decal: {name}") }).map_err(|e| e.to_string())?;
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decal_box_sits_on_the_spot_facing_the_normal_with_the_image_upright() {
        let spot = DecalSpot { position: Vec3::new(1.0, 0.5, 0.0), normal: Vec3::X, up: Vec3::Y };
        let p = decal_projection(spot, 2.0, 0.2, Vec3::splat(-1.0), 2.0);
        assert_eq!((p.mode, p.uv_wrap, p.shape_crop), (ProjectionMode::Planar, UvWrap::None, ShapeCrop::Crop));
        assert!(p.backface.is_some() && p.valid());
        let t = p.transform3;
        assert_eq!(t.scale, [0.2, 0.1, 0.2], "longest side is the size, the other follows the aspect");
        for (a, b) in t.offset.iter().zip([0.5, 0.25, 0.0]) {
            assert!((a - b).abs() < 1e-6, "{:?}", t.offset);
        }
        let r = t.rotation();
        let col = |i: usize| Vec3::new(r[0][i], r[1][i], r[2][i]);
        assert!((col(2) - Vec3::X).length() < 1e-4, "faces the normal: {:?}", col(2));
        assert!((col(1) - Vec3::Y).length() < 1e-4, "image up is the wanted up: {:?}", col(1));
        assert!((col(0).cross(col(1)) - col(2)).length() < 1e-4, "not mirrored");
    }

    #[test]
    fn decal_projection_survives_degenerate_input() {
        // Up along the normal, zero normal, tall image, bad aspect and size.
        for (normal, up, aspect, size) in [(Vec3::Y, Vec3::Y, 0.25, 0.2), (Vec3::ZERO, Vec3::Y, 1.0, 0.2), (Vec3::Z, Vec3::ZERO, f32::NAN, 1.0e9), (Vec3::Z, Vec3::Y, 1.0e9, 0.0)] {
            let p = decal_projection(DecalSpot { position: Vec3::ZERO, normal, up }, aspect, size, Vec3::splat(-1.0), 2.0);
            assert!(p.valid(), "{normal:?} {up:?} {aspect} {size}: {:?}", p.transform3);
        }
        let tall = decal_projection(DecalSpot { position: Vec3::ZERO, normal: Vec3::Z, up: Vec3::Y }, 0.5, 0.2, Vec3::ZERO, 1.0);
        assert_eq!(tall.transform3.scale, [0.1, 0.2, 0.2]);
    }
}
