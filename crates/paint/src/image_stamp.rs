//! Math of the normal-map / height stamp brush: sampling a stamp image as a surface slope,
//! rotating it, and a CPU lit preview (used by the brush panel).
//!
//! Slopes are `(-dh/dx, -dh/dy)`: a tangent-space normal `(nx, ny, nz)` has slope
//! `(nx/nz, ny/nz)`. Slopes add (a height field's derivatives add), which is what makes
//! overlapping stamps and the whiteout blend behave. Image axes: x right, y UP (OpenGL normal
//! maps), whereas stamp-local axes are x right, y DOWN (the screen), see [`to_local`].

use glam::Vec2;
use subcraft_core::normal;
use subcraft_image::{Bitmap, Wrap};

use crate::settings::{ImageStamp, StampKind, falloff};

/// Smallest `nz` used for the slope of a very steep stamp pixel (avoids infinite slopes).
const MIN_NZ: f32 = 0.05;

/// Height-map slope per unit of image width, so a feature a quarter of the image wide and half
/// the height range tall gives a slope of 0.5 at strength 1.
const HEIGHT_SLOPE: f32 = 0.25;

/// Stamp pixel at image coordinates `p` (0..1, origin top-left) as a slope in the image frame
/// (x right, y up), already scaled by the stamp strength. For a height stamp this is the
/// (negated) gradient of the grayscale.
pub fn sample_slope(image: &Bitmap, stamp: &ImageStamp, p: Vec2, wrap: Wrap) -> [f32; 2] {
    match stamp.kind {
        StampKind::Normal => {
            let t = image.sample(p.x, p.y, wrap, false);
            let n = normal::decode([t[0], t[1], t[2]]);
            let ny = if stamp.flip_green { -n.y } else { n.y };
            let nz = n.z.max(MIN_NZ);
            [n.x / nz * stamp.strength, ny / nz * stamp.strength]
        }
        StampKind::Height => {
            let e = 0.01_f32;
            let h = |q: Vec2| image.sample(q.x, q.y, wrap, false)[0];
            let dx = (h(p + Vec2::new(e, 0.0)) - h(p - Vec2::new(e, 0.0))) / (2.0 * e);
            let dy_down = (h(p + Vec2::new(0.0, e)) - h(p - Vec2::new(0.0, e))) / (2.0 * e);
            [-dx * HEIGHT_SLOPE * stamp.strength, dy_down * HEIGHT_SLOPE * stamp.strength]
        }
    }
}

/// Stamp pixel as a height value (0..1) times the strength.
pub fn sample_height(image: &Bitmap, stamp: &ImageStamp, p: Vec2, wrap: Wrap) -> f32 {
    (image.sample(p.x, p.y, wrap, false)[0] * stamp.strength).clamp(0.0, 1.0)
}

/// Rotate a 2D vector by `angle` radians.
#[inline]
pub fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Image-frame slope (x right, y up) of a stamp rotated by `angle` → the stamp-local frame
/// (x right, y down) of the *unrotated* brush: rotating the stamp rotates its normals' XY.
#[inline]
pub fn to_local(slope: [f32; 2], angle: f32) -> Vec2 {
    rotate(Vec2::new(slope[0], -slope[1]), angle)
}

/// Whiteout-blend a slope over an encoded base normal (0..1 form); returns the encoded result.
pub fn blend_slope(base: [f32; 3], slope: [f32; 2]) -> [f32; 3] {
    let b = normal::whiteout(normal::decode(base), normal::from_slope(slope[0], slope[1]));
    normal::encode(b)
}

/// Lit gray (0..1) of a tangent-space surface with slope `s`: light from the upper left.
pub fn lit(slope: Vec2) -> f32 {
    let n = normal::from_slope(slope.x, slope.y);
    let light = glam::Vec3::new(-0.45, 0.55, 0.7).normalize();
    // Display value (not linear): flat ground mid-gray, lit faces lighter, shaded faces dark.
    (0.06 + 0.5 * n.dot(light).max(0.0)).clamp(0.0, 1.0)
}

/// A square stamp at `center`/`radius` (pixels) with a soft edge, as the brush lays it down:
/// image over the inscribed square, round `hardness` falloff, rotated by `angle`.
fn stamp_at(image: &Bitmap, stamp: &ImageStamp, hardness: f32, angle: f32, center: Vec2, radius: f32, px: Vec2) -> Option<(f32, Vec2)> {
    let local = (px - center) / radius;
    let r = rotate(local, -angle);
    let a = falloff(r.length(), hardness);
    if a <= 0.0 {
        return None;
    }
    let p = r * 0.5 + Vec2::splat(0.5);
    let s = if stamp.kind == StampKind::Height {
        let h = sample_height(image, stamp, p, Wrap::Clamp);
        [h, 0.0]
    } else {
        let slope = sample_slope(image, stamp, p, Wrap::Clamp);
        let v = to_local(slope, angle);
        [v.x, -v.y] // screen frame → lit frame (y up)
    };
    Some((a, Vec2::new(s[0], s[1])))
}

/// RGBA8 preview of the brush: a lit single stamp on the left and an example stroke (stamps along
/// a wave at `spacing` diameters) on the right. `width × height` pixels; returns the pixel bytes.
pub fn render_preview(image: &Bitmap, stamp: &ImageStamp, hardness: f32, spacing: f32, angle: f32, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let tile = height as f32;
    let mut out = vec![0u8; w * h * 4];
    // Stamp centres: one in the left tile, then along the wave.
    let radius = tile * 0.42;
    let mut centers = vec![Vec2::new(tile * 0.5, tile * 0.5)];
    let (x0, x1) = (tile + radius, width as f32 - radius * 0.7);
    let step = (2.0 * radius * spacing.max(0.05)).max(1.0);
    let mut x = x0;
    while x <= x1 + 0.1 {
        let t = (x - x0) / (x1 - x0).max(1.0);
        centers.push(Vec2::new(x, tile * (0.5 + 0.18 * (t * std::f32::consts::TAU).sin())));
        x += step;
    }
    // The stroke's stamps use a smaller radius so several fit.
    let stroke_radius = tile * 0.3;
    for py in 0..h {
        for pxl in 0..w {
            let px = Vec2::new(pxl as f32 + 0.5, py as f32 + 0.5);
            let mut sum = Vec2::ZERO;
            let mut weight = 0.0f32;
            for (i, &c) in centers.iter().enumerate() {
                let r = if i == 0 { radius } else { stroke_radius };
                if let Some((a, s)) = stamp_at(image, stamp, hardness, angle, c, r, px) {
                    sum += s * a;
                    weight += a;
                }
            }
            let gray = if stamp.kind == StampKind::Height {
                0.1 + 0.8 * (sum.x / weight.max(1.0)).clamp(0.0, 1.0)
            } else {
                lit(sum / weight.max(1.0))
            };
            let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
            let o = (py * w + pxl) * 4;
            out[o..o + 4].copy_from_slice(&[v, v, v, 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_map(rgb: [u8; 3]) -> Bitmap {
        Bitmap::new_u8(2, 2, 3, rgb.repeat(4))
    }

    #[test]
    fn flat_stamp_has_zero_slope_and_blends_to_the_base() {
        let s = ImageStamp::default();
        let slope = sample_slope(&flat_map([128, 128, 255]), &s, Vec2::splat(0.5), Wrap::Clamp);
        assert!(slope[0].abs() < 0.01 && slope[1].abs() < 0.01, "{slope:?}");
        let base = normal::encode(glam::Vec3::new(0.3, -0.2, 0.9));
        let out = blend_slope(base, [0.0, 0.0]);
        for i in 0..3 { assert!((out[i] - base[i]).abs() < 1e-5); }
    }

    #[test]
    fn tilted_stamp_over_flat_base_gives_the_tilted_normal() {
        // Normal (0.6, 0, 0.8): slope 0.75.
        let s = ImageStamp::default();
        let map = flat_map([204, 128, 230]);
        let slope = sample_slope(&map, &s, Vec2::splat(0.5), Wrap::Clamp);
        let out = normal::decode(blend_slope([0.5, 0.5, 1.0], slope));
        let expect = glam::Vec3::new(0.6, 0.0, 0.8);
        assert!((out - expect).length() < 0.02, "{out:?}");
        assert!((out.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn strength_scales_the_deviation_and_zero_changes_nothing() {
        let map = flat_map([204, 128, 230]);
        let mut s = ImageStamp::default();
        s.strength = 0.0;
        let slope = sample_slope(&map, &s, Vec2::splat(0.5), Wrap::Clamp);
        assert_eq!(slope, [0.0, 0.0]);
        let base = [0.4, 0.55, 0.9];
        let out = blend_slope(base, slope);
        for i in 0..3 { assert!((out[i] - normal::encode(normal::decode(base))[i]).abs() < 1e-5); }
        s.strength = 2.0;
        let double = sample_slope(&map, &s, Vec2::splat(0.5), Wrap::Clamp);
        s.strength = 1.0;
        let single = sample_slope(&map, &s, Vec2::splat(0.5), Wrap::Clamp);
        assert!((double[0] - 2.0 * single[0]).abs() < 1e-5);
    }

    #[test]
    fn flip_green_negates_y_only() {
        let map = flat_map([160, 200, 220]);
        let mut s = ImageStamp::default();
        let a = sample_slope(&map, &s, Vec2::splat(0.5), Wrap::Clamp);
        s.flip_green = true;
        let b = sample_slope(&map, &s, Vec2::splat(0.5), Wrap::Clamp);
        assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] + b[1]).abs() < 1e-6);
    }

    #[test]
    fn rotation_rotates_the_normals_xy_vector() {
        // Slope pointing +x in the image frame (right), rotated a quarter turn clockwise on
        // screen (y down: +90° maps right to down) points down in the stamp-local frame.
        let v = to_local([0.5, 0.0], std::f32::consts::FRAC_PI_2);
        assert!((v - Vec2::new(0.0, 0.5)).length() < 1e-6, "{v:?}");
        let v = to_local([0.5, 0.0], std::f32::consts::PI);
        assert!((v - Vec2::new(-0.5, 0.0)).length() < 1e-6);
        // Length is kept.
        let v = to_local([0.3, -0.4], 0.7);
        assert!((v.length() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn height_ramp_gives_a_slope_against_the_gradient() {
        // Height rising to the right: surface normal tilts to -x.
        let ramp = Bitmap::from_fn_gray(32, 32, |u, _| u);
        let s = ImageStamp { kind: StampKind::Height, ..Default::default() };
        let slope = sample_slope(&ramp, &s, Vec2::splat(0.5), Wrap::Clamp);
        assert!(slope[0] < -0.1 && slope[1].abs() < 0.01, "{slope:?}");
        assert!((sample_height(&ramp, &s, Vec2::splat(0.5), Wrap::Clamp) - 0.5).abs() < 0.05);
    }

    #[test]
    fn preview_has_the_requested_size_and_shows_relief() {
        let map = Bitmap::from_fn_gray(16, 16, |u, v| 0.5 + 0.5 * ((u - 0.5) * (v - 0.5)));
        // Build a normal map with a bump in the middle.
        let mut data = Vec::new();
        for y in 0..32u32 { for x in 0..32u32 {
            let (u, v) = (x as f32 / 31.0 - 0.5, y as f32 / 31.0 - 0.5);
            let n = glam::Vec3::new(u * 2.0, -v * 2.0, 1.0).normalize();
            let e = normal::encode(n);
            data.extend(e.iter().map(|c| (c * 255.0).round() as u8));
        } }
        let _ = map;
        let bump = Bitmap::new_u8(32, 32, 3, data);
        let px = render_preview(&bump, &ImageStamp::default(), 0.8, 1.0, 0.0, 192, 64);
        assert_eq!(px.len(), 192 * 64 * 4);
        let (lo, hi) = px.chunks_exact(4).fold((255u8, 0u8), |(l, h), p| (l.min(p[0]), h.max(p[0])));
        assert!(hi - lo > 30, "no relief: {lo}..{hi}");
    }
}
