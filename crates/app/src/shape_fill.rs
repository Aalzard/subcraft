//! Shape fill: a rectangle or ellipse dragged in the 3D or 2D view, painted once with the brush
//! material on the selected paint layer (or its mask), as one undo step.
//!
//! The shape is a single dab whose tip is the shape ([`subcraft_paint::shape`]), sized in
//! Viewport space so it lands exactly on the dragged box: camera-aligned in the 3D view (it
//! covers whatever the box shows, near or far), UV-aligned in the 2D view.

use glam::Vec2;
use subcraft_paint::shape::{self, Shape};
use subcraft_paint::{Alignment, BrushSettings, Camera, SizeSpace, StrokeMode, UvView};

use crate::command::CommandError;
use crate::session::Session;

fn invalid(s: impl Into<String>) -> CommandError {
    CommandError::Invalid(s.into())
}

impl Session {
    /// Paint `shape` over the screen box from `a` to `b` (view pixels) in the 3D view seen by
    /// `camera`. The box's centre must be over the model.
    pub fn fill_shape_projected(&mut self, camera: &Camera, a: Vec2, b: Vec2, shape: &Shape) -> Result<(), CommandError> {
        let size = b - a;
        let center = (a + b) * 0.5;
        let view_height = camera.viewport[1] as f32;
        self.with_shape_brush(shape, size, view_height, Alignment::Camera, |s| {
            s.begin_stroke().map_err(|e| invalid(e.to_string()))?;
            let painted = s.stroke_to(camera, center, false).map_err(|e| invalid(e.to_string()));
            s.end_stroke();
            if painted?.is_empty() {
                return Err(invalid("start the shape with its centre over the model"));
            }
            Ok(())
        })
    }

    /// Paint `shape` over a `size` (view pixels) box centred on texture position `center_uv` in
    /// the 2D view `view`.
    pub fn fill_shape_uv(&mut self, view: UvView, center_uv: Vec2, size: Vec2, shape: &Shape) -> Result<(), CommandError> {
        let view_height = view.viewport[1] as f32;
        let texels_per_px = self.selected_texture_set().resolution as f32 / view.side.max(1e-6);
        self.with_shape_brush(shape, size, view_height, Alignment::Uv, |s| {
            s.begin_uv_stroke(Some(view)).map_err(|e| invalid(e.to_string()))?;
            let painted = s.stroke_to_uv(center_uv, false, texels_per_px).map_err(|e| invalid(e.to_string()));
            s.end_stroke();
            if painted?.is_empty() {
                return Err(invalid("the shape is outside this texture set's UVs"));
            }
            Ok(())
        })
    }

    /// Run `f` with the brush set up as one shape dab over a `size` pixel box, then put the
    /// brush back as it was.
    fn with_shape_brush(&mut self, shape: &Shape, size: Vec2, view_height: f32, alignment: Alignment, f: impl FnOnce(&mut Session) -> Result<(), CommandError>) -> Result<(), CommandError> {
        if self.is_stroking() {
            return Err(CommandError::StrokeInProgress);
        }
        let dab = Shape::dab(size.x, size.y, view_height).ok_or_else(|| invalid("drag a box to draw the shape"))?;
        let saved = self.brush.clone();
        let user = &saved.settings;
        // Only what decides which surface is reached carries over from the brush; jitter,
        // scatter, particles and pen effects would break the shape.
        self.brush.settings = BrushSettings {
            size: dab.size,
            size_space: SizeSpace::Viewport,
            roundness: dab.roundness,
            angle_deg: dab.angle_deg,
            hardness: 1.0,
            flow: 1.0,
            stroke_opacity: 1.0,
            alignment,
            backface_culling: user.backface_culling,
            backface_angle_deg: user.backface_angle_deg,
            ..BrushSettings::default()
        };
        self.brush.alpha = Some(shape::tip(shape, dab.roundness, shape::TIP_SIZE));
        self.brush.alpha_name = shape.kind.label().into();
        self.brush.mode = StrokeMode::Paint;
        self.brush.projection = false;
        // A normal / height stamp image would replace the shape's own tip.
        self.brush.material.stamp = None;
        let result = f(self);
        self.brush = saved;
        result
    }
}
