//! Stencil: a grayscale image laid over the viewport that masks painting in screen space
//! (white lets paint through, black blocks it), positioned independently of the model.

use std::sync::Arc;

use glam::Vec2;
use subcraft_image::{Bitmap, Wrap};

#[derive(Clone, Debug)]
pub struct Stencil {
    pub bitmap: Arc<Bitmap>,
    /// Image center in normalized viewport coordinates (0..1, origin top-left).
    pub center: Vec2,
    /// Image height as a fraction of the viewport height.
    pub scale: f32,
    /// Rotation in radians (clockwise on screen).
    pub rotation: f32,
    /// Repeat the image instead of blocking paint outside it.
    pub tiling: bool,
}

impl Stencil {
    pub fn new(bitmap: Arc<Bitmap>) -> Self {
        Self { bitmap, center: Vec2::splat(0.5), scale: 0.8, rotation: 0.0, tiling: false }
    }

    fn size_px(&self, viewport: [u32; 2]) -> Vec2 {
        let h = self.scale * viewport[1] as f32;
        let aspect = self.bitmap.width as f32 / self.bitmap.height.max(1) as f32;
        Vec2::new(h * aspect, h)
    }

    fn center_px(&self, viewport: [u32; 2]) -> Vec2 {
        self.center * Vec2::new(viewport[0] as f32, viewport[1] as f32)
    }

    /// Image UV under viewport pixel `px`; `None` outside the image when not tiling.
    pub fn image_uv(&self, px: Vec2, viewport: [u32; 2]) -> Option<Vec2> {
        let d = px - self.center_px(viewport);
        let (s, c) = self.rotation.sin_cos();
        let local = Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c);
        let uv = local / self.size_px(viewport) + Vec2::splat(0.5);
        if self.tiling || ((0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)) { Some(uv) } else { None }
    }

    /// Paint factor (0..1) at viewport pixel `px`: image brightness × alpha, 0 outside.
    pub fn value_at(&self, px: Vec2, viewport: [u32; 2]) -> f32 {
        let Some(uv) = self.image_uv(px, viewport) else { return 0.0 };
        let wrap = if self.tiling { Wrap::Repeat } else { Wrap::Clamp };
        let t = self.bitmap.sample(uv.x, uv.y, wrap, false);
        ((0.2126 * t[0] + 0.7152 * t[1] + 0.0722 * t[2]) * t[3]).clamp(0.0, 1.0)
    }

    /// Image corners in viewport pixels (top-left, top-right, bottom-right, bottom-left).
    pub fn corners(&self, viewport: [u32; 2]) -> [Vec2; 4] {
        let c = self.center_px(viewport);
        let h = self.size_px(viewport) * 0.5;
        let (s, co) = self.rotation.sin_cos();
        let rot = |v: Vec2| Vec2::new(v.x * co - v.y * s, v.x * s + v.y * co);
        [Vec2::new(-h.x, -h.y), Vec2::new(h.x, -h.y), Vec2::new(h.x, h.y), Vec2::new(-h.x, h.y)].map(|v| c + rot(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halves() -> Stencil {
        // 2x1: left white, right black.
        Stencil::new(Arc::new(Bitmap::new_u8(4, 2, 1, vec![255, 255, 0, 0, 255, 255, 0, 0])))
    }

    #[test]
    fn samples_in_screen_space_with_transform() {
        let mut s = halves();
        s.scale = 1.0; // 200 px tall -> 400 px wide in a 400x200 viewport
        let vp = [400, 200];
        assert_eq!(s.value_at(Vec2::new(50.0, 100.0), vp), 1.0);
        assert_eq!(s.value_at(Vec2::new(350.0, 100.0), vp), 0.0);
        // Rotated 180°: sides swap.
        s.rotation = std::f32::consts::PI;
        assert!(s.value_at(Vec2::new(50.0, 100.0), vp) < 1e-3);
        assert!(s.value_at(Vec2::new(350.0, 100.0), vp) > 0.999);
    }

    #[test]
    fn outside_blocks_unless_tiling() {
        let mut s = halves();
        s.scale = 0.5;
        let vp = [400, 200];
        assert_eq!(s.value_at(Vec2::new(5.0, 5.0), vp), 0.0, "outside the image");
        s.tiling = true;
        assert!(s.image_uv(Vec2::new(5.0, 5.0), vp).is_some());
    }

    #[test]
    fn corners_follow_center_and_rotation() {
        let mut s = halves();
        s.scale = 0.5;
        let c = s.corners([400, 200]);
        assert_eq!(c[0], Vec2::new(100.0, 50.0), "2:1 image: 200 x 100 px");
        assert_eq!(c[2], Vec2::new(300.0, 150.0));
        s.rotation = std::f32::consts::FRAC_PI_2;
        let c = s.corners([400, 200]);
        assert!((c[0] - Vec2::new(250.0, 0.0)).length() < 1e-3, "{:?}", c[0]);
    }
}
