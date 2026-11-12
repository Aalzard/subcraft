//! Stencil: a grayscale image laid over the viewport that masks painting in screen space
//! (white lets paint through, black blocks it), positioned independently of the model.

use std::sync::Arc;

use glam::Vec2;
use subcraft_image::{Bitmap, Wrap};

/// Along which image axes a stencil repeats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StencilTiling {
    /// One image: paint is blocked outside it.
    #[default]
    None,
    /// Repeats along the image's horizontal axis only.
    Horizontal,
    /// Repeats along the image's vertical axis only.
    Vertical,
    /// Repeats along both axes (covers the whole view).
    Both,
}

impl StencilTiling {
    pub const ALL: [StencilTiling; 4] = [StencilTiling::None, StencilTiling::Horizontal, StencilTiling::Vertical, StencilTiling::Both];

    pub fn label(self) -> &'static str {
        match self {
            StencilTiling::None => "No Tiling",
            StencilTiling::Horizontal => "Horizontal Tiling",
            StencilTiling::Vertical => "Vertical Tiling",
            StencilTiling::Both => "H and V Tiling",
        }
    }

    /// Whether the image repeats along its horizontal / vertical axis.
    pub fn horizontal(self) -> bool { matches!(self, StencilTiling::Horizontal | StencilTiling::Both) }
    pub fn vertical(self) -> bool { matches!(self, StencilTiling::Vertical | StencilTiling::Both) }
}

/// Rotation snapped to quarter turns (Shift while rotating a stencil).
pub fn snap_rotation(radians: f32) -> f32 {
    let quarter = std::f32::consts::FRAC_PI_2;
    (radians / quarter).round() * quarter
}

/// Most copies of a tiled stencil the overlay draws (a tiny tiled stencil would need thousands).
pub const MAX_STENCIL_COPIES: usize = 400;

#[derive(Clone, Debug)]
pub struct Stencil {
    pub bitmap: Arc<Bitmap>,
    /// Image center in normalized viewport coordinates (0..1, origin top-left).
    pub center: Vec2,
    /// Image height as a fraction of the viewport height.
    pub scale: f32,
    /// Rotation in radians (clockwise on screen).
    pub rotation: f32,
    /// Repeat the image along one or both of its axes instead of blocking paint outside it.
    pub tiling: StencilTiling,
}

impl Stencil {
    pub fn new(bitmap: Arc<Bitmap>) -> Self {
        Self { bitmap, center: Vec2::splat(0.5), scale: Self::DEFAULT_SCALE, rotation: 0.0, tiling: StencilTiling::None }
    }

    /// Scale of a newly loaded stencil.
    pub const DEFAULT_SCALE: f32 = 0.8;

    /// Back to the placement of a newly loaded stencil (centered, upright, default scale); the
    /// image and its tiling mode are kept.
    pub fn reset_transform(&mut self) {
        self.center = Vec2::splat(0.5);
        self.scale = Self::DEFAULT_SCALE;
        self.rotation = 0.0;
    }

    /// Whether the placement differs from a newly loaded stencil's.
    pub fn is_transformed(&self) -> bool {
        self.center != Vec2::splat(0.5) || self.scale != Self::DEFAULT_SCALE || self.rotation != 0.0
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
        let inside = |v: f32, tiled: bool| tiled || (0.0..=1.0).contains(&v);
        if inside(uv.x, self.tiling.horizontal()) && inside(uv.y, self.tiling.vertical()) { Some(uv) } else { None }
    }

    /// Paint factor (0..1) at viewport pixel `px`: image brightness × alpha, 0 outside.
    pub fn value_at(&self, px: Vec2, viewport: [u32; 2]) -> f32 {
        let Some(uv) = self.image_uv(px, viewport) else { return 0.0 };
        // One wrap mode serves both axes: an axis that does not tile is inside 0..1 here, where
        // repeating changes nothing except at the very border, so it is clamped by hand.
        let wrap = if self.tiling == StencilTiling::None { Wrap::Clamp } else { Wrap::Repeat };
        // Clamped to the outer texel centres: bilinear sampling with Repeat half a texel from the
        // border of an axis that does not tile blended in the opposite edge (paint leaked through
        // a black edge of a stencil tiled along the other axis).
        let hold = |v: f32, tiled: bool, n: u32| {
            if tiled {
                v
            } else {
                let half = 0.5 / n.max(1) as f32;
                v.clamp(half, (1.0 - half).max(half))
            }
        };
        let (w, h) = (self.bitmap.width, self.bitmap.height);
        let t = self.bitmap.sample(hold(uv.x, self.tiling.horizontal(), w), hold(uv.y, self.tiling.vertical(), h), wrap, false);
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

    /// Pixel offsets of every copy of the image that can touch the viewport (the image itself
    /// is offset zero and always first), for drawing a tiled stencil. At most
    /// [`MAX_STENCIL_COPIES`], nearest first.
    pub fn copy_offsets(&self, viewport: [u32; 2]) -> Vec<Vec2> {
        let mut out = vec![Vec2::ZERO];
        if self.tiling == StencilTiling::None {
            return out;
        }
        let size = self.size_px(viewport);
        if size.x < 1e-3 || size.y < 1e-3 {
            return out;
        }
        let (s, co) = self.rotation.sin_cos();
        let (ax, ay) = (Vec2::new(co, s) * size.x, Vec2::new(-s, co) * size.y);
        // Every copy within the viewport's diagonal of the view center can be visible.
        let view = Vec2::new(viewport[0] as f32, viewport[1] as f32);
        let reach = view.length() * 0.5 + size.length() * 0.5;
        let to_view = view * 0.5 - self.center_px(viewport);
        let span = |tiled: bool, len: f32| if tiled { (reach / len).ceil() as i32 + 1 } else { 0 };
        // Index of the copy nearest the view center along each axis.
        let (s2, c2) = (-self.rotation).sin_cos();
        let local = Vec2::new(to_view.x * c2 - to_view.y * s2, to_view.x * s2 + to_view.y * c2) / size;
        let (ci, cj) = (if self.tiling.horizontal() { local.x.round() as i32 } else { 0 }, if self.tiling.vertical() { local.y.round() as i32 } else { 0 });
        // Only the nearest copies are kept, and they lie within that many steps of the nearest
        // one along each axis: a tiny or very thin stencil used to build the whole grid first
        // (hundreds of millions of copies) before keeping the nearest few hundred.
        let limit = MAX_STENCIL_COPIES as i32 + 1;
        let (ni, nj) = (span(self.tiling.horizontal(), size.x).min(limit), span(self.tiling.vertical(), size.y).min(limit));
        let mut copies = Vec::new();
        for i in (ci - ni)..=(ci + ni) {
            for j in (cj - nj)..=(cj + nj) {
                if (i, j) == (0, 0) {
                    continue;
                }
                let offset = ax * i as f32 + ay * j as f32;
                let d = (offset - to_view).length();
                if d <= reach {
                    copies.push((d, offset));
                }
            }
        }
        copies.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.extend(copies.into_iter().take(MAX_STENCIL_COPIES - 1).map(|(_, o)| o));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halves() -> Stencil {
        // 2x1: left white, right black.
        Stencil::new(Arc::new(Bitmap::new_u8(4, 2, 1, vec![255, 255, 0, 0, 255, 255, 0, 0])))
    }

    /// QA 2026-11: tiling along one axis sampled with Repeat on both, so half a texel from the
    /// border of the other axis the opposite edge blended in (paint leaked through a black edge).
    #[test]
    fn tiling_one_axis_does_not_bleed_the_other_axis_edges() {
        // 1 x 2: top white, bottom black.
        let base = Stencil { scale: 0.5, ..Stencil::new(Arc::new(Bitmap::new_u8(1, 2, 1, vec![255, 0]))) };
        let vp = [400, 200];
        let none = base.clone();
        let horizontal = Stencil { tiling: StencilTiling::Horizontal, ..base };
        for y in [51.0, 55.0, 145.0, 149.0] {
            let p = Vec2::new(200.0, y);
            let (a, b) = (none.value_at(p, vp), horizontal.value_at(p, vp));
            assert!((a - b).abs() < 1e-3, "y {y}: untiled {a}, tiled along U {b}");
        }
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
        s.tiling = StencilTiling::Both;
        assert!(s.image_uv(Vec2::new(5.0, 5.0), vp).is_some());
    }

    #[test]
    fn horizontal_and_vertical_tiling_repeat_along_one_image_axis_only() {
        // 2:1 image, 100 px tall and 200 px wide, centered in a 400 x 200 view: x 100..300, y 50..150.
        let mut s = halves();
        s.scale = 0.5;
        let vp = [400, 200];
        let (left_of, above) = (Vec2::new(50.0, 100.0), Vec2::new(150.0, 20.0));
        assert_eq!((s.value_at(left_of, vp), s.value_at(above, vp)), (0.0, 0.0), "no tiling blocks outside");
        s.tiling = StencilTiling::Horizontal;
        // x = 50 is the right (black) half of the copy to the left; x = 350 its white half to the right.
        assert_eq!(s.value_at(left_of, vp), 0.0);
        assert_eq!(s.value_at(Vec2::new(350.0, 100.0), vp), 1.0, "repeats sideways");
        assert_eq!(s.value_at(above, vp), 0.0, "but not above or below");
        assert!(s.image_uv(above, vp).is_none());
        s.tiling = StencilTiling::Vertical;
        assert_eq!(s.value_at(above, vp), 1.0, "repeats up and down");
        assert_eq!(s.value_at(Vec2::new(350.0, 100.0), vp), 0.0, "but not sideways");
        s.tiling = StencilTiling::Both;
        assert_eq!((s.value_at(above, vp), s.value_at(Vec2::new(350.0, 100.0), vp)), (1.0, 1.0));
        // Rotated a quarter turn, "horizontal" follows the image, so it now repeats down the screen.
        s.tiling = StencilTiling::Horizontal;
        s.rotation = std::f32::consts::FRAC_PI_2;
        s.scale = 0.25; // 100 x 50 px, so the view is larger than one copy both ways
        assert!(s.image_uv(Vec2::new(200.0, 5.0), vp).is_some());
        assert!(s.image_uv(Vec2::new(20.0, 100.0), vp).is_none());
    }

    #[test]
    fn copy_offsets_cover_the_view_along_the_tiled_axes() {
        let mut s = halves();
        s.scale = 0.5; // 200 x 100 px in a 400 x 200 view
        let vp = [400, 200];
        assert_eq!(s.copy_offsets(vp), vec![Vec2::ZERO]);
        s.tiling = StencilTiling::Horizontal;
        let h = s.copy_offsets(vp);
        assert_eq!(h[0], Vec2::ZERO);
        assert!(h.iter().all(|o| o.y.abs() < 1e-3), "{h:?}");
        assert!(h.contains(&Vec2::new(200.0, 0.0)) && h.contains(&Vec2::new(-200.0, 0.0)), "{h:?}");
        s.tiling = StencilTiling::Vertical;
        let v = s.copy_offsets(vp);
        assert!(v.iter().all(|o| o.x.abs() < 1e-3) && v.contains(&Vec2::new(0.0, 100.0)) && v.contains(&Vec2::new(0.0, -100.0)), "{v:?}");
        s.tiling = StencilTiling::Both;
        let both = s.copy_offsets(vp);
        assert!(both.contains(&Vec2::new(200.0, 100.0)) && both.len() > h.len() + v.len() - 1);
        // Every point of the view is under some copy.
        for p in [Vec2::new(1.0, 1.0), Vec2::new(399.0, 199.0), Vec2::new(399.0, 1.0), Vec2::new(200.0, 100.0)] {
            let c = s.corners(vp);
            assert!(both.iter().any(|o| {
                let (min, max) = (c[0] + *o, c[2] + *o);
                p.x >= min.x - 0.5 && p.x <= max.x + 0.5 && p.y >= min.y - 0.5 && p.y <= max.y + 0.5
            }), "{p:?} is not covered");
        }
        // QA 2026-11: a scale far below what the viewport allows (a preset file can hold one)
        // built ~1e8 copies before keeping the nearest few hundred.
        let mut speck = halves();
        speck.scale = 1e-4;
        speck.tiling = StencilTiling::Both;
        let started = std::time::Instant::now();
        assert_eq!(speck.copy_offsets([1920, 1080]).len(), MAX_STENCIL_COPIES);
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
        // A tiny stencil is capped, the image itself first.
        s.scale = 0.002;
        let tiny = s.copy_offsets([4000, 4000]);
        assert_eq!((tiny.len(), tiny[0]), (MAX_STENCIL_COPIES, Vec2::ZERO));
    }

    #[test]
    fn reset_restores_the_placement_and_keeps_image_and_tiling() {
        let mut s = halves();
        assert!(!s.is_transformed());
        s.center = Vec2::new(0.2, 0.7);
        s.scale = 2.5;
        s.rotation = 1.0;
        s.tiling = StencilTiling::Vertical;
        assert!(s.is_transformed());
        let bitmap = Arc::as_ptr(&s.bitmap);
        s.reset_transform();
        assert_eq!((s.center, s.scale, s.rotation), (Vec2::splat(0.5), Stencil::DEFAULT_SCALE, 0.0));
        assert_eq!((s.tiling, Arc::as_ptr(&s.bitmap)), (StencilTiling::Vertical, bitmap));
        assert!(!s.is_transformed());
    }

    #[test]
    fn rotation_snaps_to_quarter_turns() {
        use std::f32::consts::{FRAC_PI_2, PI};
        assert_eq!(snap_rotation(0.3), 0.0);
        assert!((snap_rotation(1.0) - FRAC_PI_2).abs() < 1e-6);
        assert!((snap_rotation(2.9) - PI).abs() < 1e-6);
        assert!((snap_rotation(-1.4) + FRAC_PI_2).abs() < 1e-6);
        assert!((snap_rotation(7.0 * FRAC_PI_2 + 0.2) - 7.0 * FRAC_PI_2).abs() < 1e-5);
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
