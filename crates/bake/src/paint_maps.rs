//! Painted bake masks: grayscale maps over the low poly's UVs that steer the projection per texel.
//!
//! * The skew map scales [`crate::skew::SkewCorrection`]: white keeps the full correction, black
//!   keeps the averaged rays (Painter's "Skew map").
//! * The cage push map moves the distance-based cage: white pushes it out by
//!   [`crate::BakeSettings::cage_push`] times the frontal and rear distances, so rays reach high-poly
//!   details that the uniform distances miss, without growing the cage everywhere.
//!
//! The app paints them as layer masks and hands each texture set's mask over at bake time.

use std::sync::Arc;

use subcraft_image::TiledImage;

/// A grayscale map (first channel) read at the bake texels' UVs, at any resolution.
/// Two maps are equal when they are the same image.
#[derive(Clone)]
pub struct PaintMap(pub Arc<TiledImage>);

impl PartialEq for PaintMap {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for PaintMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PaintMap({} x {})", self.0.width(), self.0.height())
    }
}

impl PaintMap {
    pub fn new(image: TiledImage) -> Self {
        Self(Arc::new(image))
    }

    /// Bilinear value (0..1) at the centre of texel `index` of a `size` x `size` bake.
    pub fn at_texel(&self, index: u32, size: u32) -> f32 {
        let (x, y) = (index % size, index / size);
        self.sample((x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32)
    }

    /// Bilinear value (0..1) at `(u, v)` in image space (0,0 = first texel's corner), clamped at
    /// the borders.
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let img = &self.0;
        let (w, h) = (img.width().max(1), img.height().max(1));
        let axis = |t: f32, n: u32| {
            let f = (t * n as f32 - 0.5).clamp(0.0, (n - 1) as f32);
            let p0 = f.floor() as u32;
            (p0, (p0 + 1).min(n - 1), f - p0 as f32)
        };
        let ((x0, x1, tx), (y0, y1, ty)) = (axis(u, w), axis(v, h));
        let g = |x: u32, y: u32| img.get(x, y)[0];
        let top = g(x0, y0) * (1.0 - tx) + g(x1, y0) * tx;
        let bottom = g(x0, y1) * (1.0 - tx) + g(x1, y1) * tx;
        (top * (1.0 - ty) + bottom * ty).clamp(0.0, 1.0)
    }
}
