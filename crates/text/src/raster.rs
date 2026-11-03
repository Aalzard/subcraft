use ab_glyph_rasterizer::{Rasterizer, point};
use glam::{Affine2, Vec2};
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};

use crate::{Font, TextError, TextLayout};

/// Largest raster (pixels) a single call produces; callers rasterize just the region they need.
pub const MAX_PIXELS: u64 = 1 << 25;

/// Straight coverage in `0..=1`, row-major, origin top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct Coverage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl Coverage {
    pub fn get(&self, x: u32, y: u32) -> f32 {
        self.data[(y * self.width + x) as usize]
    }
    pub fn sum(&self) -> f64 {
        self.data.iter().map(|&v| v as f64).sum()
    }
    /// Pixel bounds `(x0, y0, x1, y1)` (exclusive max) of coverage above `threshold`.
    pub fn bounds(&self, threshold: f32) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) > threshold {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x0 != u32::MAX).then_some((x0, y0, x1, y1))
    }
}

/// Collects outline edges, flattening curves and clipping to the raster so the area
/// accumulator never sees out-of-range coordinates (shapes left of the raster still fill it).
struct Builder<'a> {
    raster: &'a mut Rasterizer,
    size: Vec2,
    /// glyph units (y up) → output pixels
    map: &'a dyn Fn(f32, f32) -> Vec2,
    last: Vec2,
    start: Vec2,
}

impl Builder<'_> {
    fn edge(&mut self, a: Vec2, b: Vec2) {
        let (w, h) = (self.size.x, self.size.y);
        if (a.y <= 0.0 && b.y <= 0.0) || (a.y >= h && b.y >= h) || a.y == b.y {
            return;
        }
        // Clip to 0 <= y <= h.
        let at_y = |y: f32| a + (b - a) * ((y - a.y) / (b.y - a.y));
        let mut pts = [a, b];
        for p in pts.iter_mut() {
            if p.y < 0.0 {
                *p = at_y(0.0);
                p.y = 0.0;
            } else if p.y > h {
                *p = at_y(h);
                p.y = h;
            }
        }
        // Split at x = 0 and x = w; each piece lies on one side of both lines.
        let [p, q] = pts;
        let mut ts = vec![0.0f32, 1.0];
        if p.x != q.x {
            for edge_x in [0.0, w] {
                let t = (edge_x - p.x) / (q.x - p.x);
                if t > 0.0 && t < 1.0 {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f32::total_cmp);
        for pair in ts.windows(2) {
            let (mut u, mut v) = (p + (q - p) * pair[0], p + (q - p) * pair[1]);
            let mid = (u.x + v.x) * 0.5;
            // Pieces outside the raster collapse onto its left / right border. The accumulator
            // runs along the flat buffer, so the right border edge is needed to close each row.
            if mid <= 0.0 {
                u.x = 0.0;
                v.x = 0.0;
            } else if mid >= w {
                u.x = w;
                v.x = w;
            } else {
                u.x = u.x.clamp(0.0, w);
                v.x = v.x.clamp(0.0, w);
            }
            if u.y != v.y {
                self.raster.draw_line(point(u.x, u.y), point(v.x, v.y));
            }
        }
    }

    fn to(&mut self, p: Vec2) {
        let from = self.last;
        self.edge(from, p);
        self.last = p;
    }
}

impl OutlineBuilder for Builder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.last = (self.map)(x, y);
        self.start = self.last;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = (self.map)(x, y);
        self.to(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, c, p) = (self.last, (self.map)(x1, y1), (self.map)(x, y));
        let n = ((p0 - 2.0 * c + p).length() * 0.25 / 0.05).sqrt().ceil().clamp(1.0, 512.0) as u32;
        for i in 1..n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.to(p0 * (u * u) + c * (2.0 * u * t) + p * (t * t));
        }
        self.to(p);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, c1, c2, p) = (self.last, (self.map)(x1, y1), (self.map)(x2, y2), (self.map)(x, y));
        let m = (p0 - 2.0 * c1 + c2).length().max((c1 - 2.0 * c2 + p).length());
        let n = (0.75 * m / 0.05).sqrt().ceil().clamp(1.0, 512.0) as u32;
        for i in 1..n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.to(p0 * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p * (t * t * t));
        }
        self.to(p);
    }
    fn close(&mut self) {
        let s = self.start;
        self.to(s);
    }
}

/// Rasterize `layout` into a `width`×`height` coverage map. `xf` maps layout em coordinates
/// (+x right, +y down) to output pixels, so any scale, rotation, shear or offset works and the
/// result is computed from the outlines at the target resolution (no bitmap resampling).
/// Coverage is exact area coverage per pixel; shapes cut by the raster edge are clipped.
pub fn rasterize(font: &Font, layout: &TextLayout, xf: Affine2, width: u32, height: u32) -> Result<Coverage, TextError> {
    if width == 0 || height == 0 {
        return Ok(Coverage { width, height, data: Vec::new() });
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(TextError::TooLarge);
    }
    if !xf.to_cols_array().iter().all(|v| v.is_finite()) {
        return Err(TextError::Invalid("transform must be finite"));
    }
    let mut raster = Rasterizer::new(width as usize, height as usize);
    font.with_face(|face| {
        let upem = face.units_per_em() as f32;
        for g in &layout.glyphs {
            let origin = Vec2::new(g.x, g.y);
            let map = move |x: f32, y: f32| {
                let p = xf.transform_point2(origin + Vec2::new(x, -y) / upem);
                p
            };
            let mut b = Builder { raster: &mut raster, size: Vec2::new(width as f32, height as f32), map: &map, last: Vec2::ZERO, start: Vec2::ZERO };
            face.outline_glyph(GlyphId(g.id), &mut b);
        }
    })
    .ok_or(TextError::BadFont)?;
    let mut data = vec![0.0; (width * height) as usize];
    raster.for_each_pixel(|i, a| data[i] = a.clamp(0.0, 1.0));
    Ok(Coverage { width, height, data })
}

/// Render the whole text uniformly scaled and centered into a `size`×`size` map (used as a
/// brush tip). `padding` is the margin as a fraction of `size`.
pub fn rasterize_fit(font: &Font, layout: &TextLayout, size: u32, padding: f32) -> Result<Coverage, TextError> {
    let (lo, hi) = layout.bounds();
    let extent = (hi - lo).max(Vec2::splat(1e-4));
    let inner = size as f32 * (1.0 - 2.0 * padding.clamp(0.0, 0.45));
    let scale = (inner / extent.x).min(inner / extent.y);
    let center = (lo + hi) * 0.5;
    let xf = Affine2::from_translation(Vec2::splat(size as f32 * 0.5)) * Affine2::from_scale(Vec2::splat(scale)) * Affine2::from_translation(-center);
    rasterize(font, layout, xf, size, size)
}
