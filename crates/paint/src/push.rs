//! Push (liquify) smudging: instead of smearing a carried colour, every texel under the brush
//! takes the paint from a little way back along the stroke, so the paint itself moves with the
//! brush (shapes bend and stretch, edges keep their sharpness). The source position of a texel is
//! `uv - motion * weight * strength`, where `motion` is the brush movement since the previous stamp
//! mapped into that texel's UV space. Sampling is bilinear on premultiplied paint.
//!
//! Moves add up in a per-stroke displacement [`Field`] and every texel is sampled once from the
//! paint as it was before the stroke, so dozens of small stamps do not blur the paint the way
//! resampling after each stamp would.

use std::collections::HashMap;

use glam::{Vec2, Vec3};
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};
use subcraft_mesh::Mesh;

/// Fraction of the brush speed at which paint under the brush centre moves at full strength. Below
/// 1 the paint slips out behind the brush and is stretched into a trail, as with a finger in wet
/// paint; at 1 it would ride along under the brush centre for the whole stroke.
pub const PUSH_SPEED: f32 = 0.5;

/// Gradients of U and V over a triangle's surface: `du = gu · dp`, `dv = gv · dp` for a move `dp`
/// within the triangle's plane. `None` for a degenerate triangle.
pub fn uv_gradient(mesh: &Mesh, tri: usize) -> Option<(Vec3, Vec3)> {
    let p = mesh.tri_positions(tri);
    let uv = mesh.tri_uvs(tri);
    let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
    let (d1, d2) = (uv[1] - uv[0], uv[2] - uv[0]);
    let (a, b, c) = (e1.dot(e1), e1.dot(e2), e2.dot(e2));
    let det = a * c - b * b;
    if det.abs() <= 1e-20 * (a * c).max(1e-30) {
        return None;
    }
    // g = x e1 + y e2 with g·e1 = d.0 and g·e2 = d.1 (one solve each for U and V).
    let solve = |r1: f32, r2: f32| {
        let x = (r1 * c - r2 * b) / det;
        let y = (r2 * a - r1 * b) / det;
        e1 * x + e2 * y
    };
    Some((solve(d1.x, d2.x), solve(d1.y, d2.y)))
}

/// The UV move of a surface move `d3` on a triangle with UV gradients `g`.
pub fn uv_motion(g: (Vec3, Vec3), d3: Vec3) -> Vec2 {
    Vec2::new(g.0.dot(d3), g.1.dot(d3))
}

/// Where each texel's paint comes from, as an offset in texels (`source = texel - offset`),
/// stored per image tile; texels never touched have offset zero.
#[derive(Clone, Debug, Default)]
pub struct Field {
    size: u32,
    tiles: HashMap<usize, Box<[Vec2]>>,
}

impl Field {
    pub fn new(size: u32) -> Self {
        Self { size, tiles: HashMap::new() }
    }

    fn tiles_x(&self) -> u32 {
        self.size.div_ceil(TILE_SIZE)
    }

    /// The offset of texel `(x, y)` (clamped to the image).
    pub fn get(&self, x: i32, y: i32) -> Vec2 {
        let m = self.size as i32 - 1;
        let (x, y) = (x.clamp(0, m) as u32, y.clamp(0, m) as u32);
        let tile = ((y / TILE_SIZE) * self.tiles_x() + x / TILE_SIZE) as usize;
        self.tiles.get(&tile).map_or(Vec2::ZERO, |t| t[((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize])
    }

    /// Bilinear offset at texel coordinates `(x, y)` (texel centres at `i + 0.5`).
    pub fn sample(&self, p: Vec2) -> Vec2 {
        let f = p - Vec2::splat(0.5);
        let (x0, y0) = (f.x.floor(), f.y.floor());
        let (tx, ty) = (f.x - x0, f.y - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        self.get(x0, y0) * (1.0 - tx) * (1.0 - ty) + self.get(x0 + 1, y0) * tx * (1.0 - ty) + self.get(x0, y0 + 1) * (1.0 - tx) * ty + self.get(x0 + 1, y0 + 1) * tx * ty
    }

    /// Set the offsets of texels of `tile` (`idx` within the tile).
    pub fn set(&mut self, tile: usize, values: &[(usize, Vec2)]) {
        let t = self.tiles.entry(tile).or_insert_with(|| vec![Vec2::ZERO; TILE_TEXELS].into_boxed_slice());
        for &(i, v) in values {
            t[i] = v;
        }
    }
}

/// Bilinear sample of `img` at texel coordinates `(x, y)` (texel centres at `i + 0.5`), clamped to
/// the image. With `alpha` (the component after `value_comps`) the values are averaged weighted by
/// alpha, so transparent texels do not darken the result.
pub fn sample(img: &TiledImage, x: f32, y: f32, value_comps: usize, alpha: bool) -> [f32; 4] {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let mut acc = [0.0f32; 4];
    let mut wsum = 0.0f32;
    for (dx, dy, k) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
        if k <= 0.0 {
            continue;
        }
        let px = img.get((x0 + dx).clamp(0, w - 1) as u32, (y0 + dy).clamp(0, h - 1) as u32);
        if alpha {
            let a = px[value_comps];
            for j in 0..value_comps {
                acc[j] += px[j] * a * k;
            }
            acc[value_comps] += a * k;
        } else {
            for j in 0..value_comps {
                acc[j] += px[j] * k;
            }
        }
        wsum += k;
    }
    if alpha {
        let a = acc[value_comps];
        if a > 1e-6 {
            for v in acc.iter_mut().take(value_comps) {
                *v /= a;
            }
        }
        acc[value_comps] = a / wsum.max(1e-6);
    } else {
        for v in acc.iter_mut().take(value_comps) {
            *v /= wsum.max(1e-6);
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uv_gradient_maps_surface_moves_to_uv_moves() {
        let mesh = subcraft_mesh::primitives::plane(2.0, 1);
        let g = (0..mesh.triangle_count()).find_map(|t| uv_gradient(&mesh, t)).expect("a plane has UVs");
        let t = (0..mesh.triangle_count()).find(|&t| uv_gradient(&mesh, t).is_some()).unwrap();
        let p = mesh.tri_positions(t);
        let uv = mesh.tri_uvs(t);
        // Moving from one corner to another changes UV by exactly the corners' UV difference.
        let m = uv_motion(g, p[1] - p[0]);
        assert!((m - (uv[1] - uv[0])).length() < 1e-5, "{m} vs {}", uv[1] - uv[0]);
        // A move along the normal does not change UV.
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize();
        assert!(uv_motion(g, n).length() < 1e-5);
    }

    #[test]
    fn the_field_is_zero_until_set_and_interpolates_between_texels() {
        let mut f = Field::new(64);
        assert_eq!(f.sample(Vec2::new(10.5, 10.5)), Vec2::ZERO);
        // Texel (33, 5) lies in the second tile of a 64-texel image with 32-texel tiles.
        let tile = 33 / TILE_SIZE as usize;
        f.set(tile, &[((5 * TILE_SIZE as usize) + (33 % TILE_SIZE as usize), Vec2::new(4.0, 0.0))]);
        assert_eq!(f.get(33, 5), Vec2::new(4.0, 0.0));
        assert!((f.sample(Vec2::new(33.0, 5.5)).x - 2.0).abs() < 1e-5, "halfway to a zero neighbour");
        assert_eq!(f.get(500, 5), f.get(63, 5), "clamped to the image");
    }

    #[test]
    fn bilinear_samples_weight_by_alpha_and_hit_texel_centres_exactly() {
        let mut img = TiledImage::new(4, 4, 4, [0.0; 4]);
        img.set(1, 1, [1.0, 0.0, 0.0, 1.0]);
        img.set(2, 1, [0.0, 0.0, 1.0, 0.0]);
        let c = sample(&img, 1.5, 1.5, 3, true);
        assert!((c[0] - 1.0).abs() < 1e-3 && (c[3] - 1.0).abs() < 1e-3, "{c:?}");
        // Halfway to a transparent texel: colour stays red, alpha halves.
        let h = sample(&img, 2.0, 1.5, 3, true);
        assert!((h[0] - 1.0).abs() < 1e-3 && h[2].abs() < 1e-3 && (h[3] - 0.5).abs() < 1e-3, "{h:?}");
        // Outside the image the edge texels repeat.
        let mut gray = TiledImage::new(4, 4, 1, [0.25, 0.0, 0.0, 0.0]);
        gray.set(0, 0, [1.0, 0.0, 0.0, 0.0]);
        assert!((sample(&gray, -3.0, -3.0, 1, false)[0] - 1.0).abs() < 1e-3);
    }
}
