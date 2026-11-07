//! World-space blur of a paint layer: the layer's painted channels are blurred over the mesh
//! surface with a radius in model units, so the blur is even where the UVs stretch. It is a
//! destructive edit (one undo step, like any paint stroke), not a stack filter: the compositor
//! evaluates filters tile by tile with a UV-space halo, and a world-space gather reads texels from
//! anywhere in the texture set, which would need a new whole-set node with its own dependency and
//! cache tracking. See `subcraft_materials::world_blur` for the algorithm.

use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;

use crate::command::{Command, CommandError};
use crate::session::Session;

fn invalid(s: impl Into<String>) -> CommandError {
    CommandError::Invalid(s.into())
}

/// Largest radius accepted, in multiples of the model's longest dimension.
pub const MAX_RADIUS_FRACTION: f32 = 0.5;

impl Session {
    /// Blur every painted channel of the paint layer `layer` (selected texture set) with a
    /// Gaussian of `radius` model units over the surface. Tangent-space Normal paint is left as
    /// it is (a blurred normal map is not a normal map). One undo step.
    pub fn world_blur_layer(&mut self, layer: subcraft_core::LayerId, radius: f32) -> Result<(), CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let ts = self.selected_texture_set();
        let current = ts.find(layer).ok_or(CommandError::LayerNotFound)?;
        let LayerContent::Paint(paint) = &current.content else { return Err(invalid("world-space blur needs a paint layer")) };
        if paint.path.is_some() || paint.text.is_some() {
            return Err(invalid("this layer is generated from a pen path or text; blurring would be lost when it is edited"));
        }
        let geo = self.scene().geometry.get(ts.material_slot, ts.resolution);
        let max_radius = geo.max_extent * MAX_RADIUS_FRACTION;
        if !(radius.is_finite() && radius > 0.0 && radius <= max_radius) {
            return Err(invalid(format!("radius must be between 0 and {max_radius:.3} model units")));
        }
        let mut replacement = current.clone();
        let LayerContent::Paint(out) = &mut replacement.content else { unreachable!() };
        let mut blurred = 0;
        for (kind, image) in out.channels.iter_mut() {
            if *kind == ChannelKind::Normal || image.allocated_tiles() == 0 { continue; }
            let comps = image.comps();
            let dense = subcraft_materials::world_blur::world_blur(&geo, &image.to_dense(), comps, radius);
            *image = TiledImage::from_dense(image.width(), image.height(), comps, image.fill(), &dense);
            blurred += 1;
        }
        if blurred == 0 { return Err(invalid("the layer has no painted channels to blur")); }
        self.execute(Command::ReplaceLayer { layer, replacement: Box::new(replacement), label: "World-space Blur".into() })
    }
}
