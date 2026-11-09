//! Bundled grayscale grunge map used as the breakup of Painter-style Dirt and Edge wear
//! generators (Painter's generators break their masks up with a grunge *texture*, not a smooth
//! noise). The map is `assets/grunge/dirt_grunge.png`: 1024², 8-bit gray, seamless, made by
//! `assets/grunge/make_dirt_grunge.py` (spectral synthesis, every step periodic) and released
//! CC0 with the rest of SubCraft's bundled assets.
//!
//! It is sampled triplanar in object space, like the organic volume noise it replaces, so it
//! never follows UV seams. A second, larger and rotated sample is mixed in so the 1024² tile does
//! not visibly repeat at small pattern sizes.

use std::sync::LazyLock;

use glam::{Vec2, Vec3};
use subcraft_doc::ProceduralParams;

const PNG: &[u8] = include_bytes!("../../../assets/grunge/dirt_grunge.png");

struct Map {
    size: usize,
    values: Vec<f32>,
}

static MAP: LazyLock<Map> = LazyLock::new(|| {
    let bmp = subcraft_image::io::decode_bitmap(PNG, Some("dirt_grunge.png")).expect("bundled grunge map decodes");
    assert_eq!(bmp.width, bmp.height, "grunge map is square");
    let size = bmp.width as usize;
    let values = (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as u32, (i / size) as u32);
            bmp.texel(x, y, false)[0]
        })
        .collect();
    Map { size, values }
});

/// Feature size of the map relative to one tile: its main blotches are about a sixth of the tile,
/// so one tile spans this many units of the generator's pattern scale.
const TILE_FEATURES: f32 = 6.0;

#[inline]
fn sample(map: &Map, p: Vec2) -> f32 {
    let n = map.size;
    let x = p.x.rem_euclid(1.0) * n as f32 - 0.5;
    let y = p.y.rem_euclid(1.0) * n as f32 - 0.5;
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let x0 = (x.floor() as isize).rem_euclid(n as isize) as usize;
    let y0 = (y.floor() as isize).rem_euclid(n as isize) as usize;
    let (x1, y1) = ((x0 + 1) % n, (y0 + 1) % n);
    let v = &map.values;
    let a = v[y0 * n + x0] + (v[y0 * n + x1] - v[y0 * n + x0]) * fx;
    let b = v[y1 * n + x0] + (v[y1 * n + x1] - v[y1 * n + x0]) * fx;
    a + (b - a) * fy
}

/// Per-seed placement in the map (offset and rotation), so layers with different seeds differ.
fn placement(seed: u32, salt: u32) -> (Vec2, Vec2) {
    let h = |k: u32| {
        let mut x = seed.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b) ^ k.wrapping_mul(0xc2b2_ae35);
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb_352d);
        x ^= x >> 15;
        (x & 0xffff) as f32 / 65535.0
    };
    let angle = h(1) * std::f32::consts::TAU;
    (Vec2::new(h(2), h(3)), Vec2::new(angle.cos(), angle.sin()))
}

#[inline]
fn rotate(p: Vec2, r: Vec2) -> Vec2 {
    Vec2::new(p.x * r.x - p.y * r.y, p.x * r.y + p.y * r.x)
}

/// Grunge value 0..1 at an object-space position with its surface normal. `params.scale` is the
/// pattern scale (features about `1 / scale` of the model), `params.seed` picks the placement,
/// `balance` / `contrast` / `invert` remap as for every SubCraft pattern.
pub fn eval(params: &ProceduralParams, position: [f32; 3], normal: [f32; 3]) -> f32 {
    let map = &*MAP;
    let p = Vec3::from_array(position) * (params.scale.clamp(0.01, 1024.0) / TILE_FEATURES);
    let n = Vec3::from_array(normal).abs();
    let w = n * n * n * n;
    let sum = (w.x + w.y + w.z).max(1e-6);
    let w = w / sum;
    let mut acc = 0.0;
    for (axis, weight) in [(0usize, w.x), (1, w.y), (2, w.z)] {
        if weight < 1e-3 {
            continue;
        }
        let uv = match axis {
            0 => Vec2::new(p.z, p.y),
            1 => Vec2::new(p.x, p.z),
            _ => Vec2::new(p.x, p.y),
        };
        let (o1, r1) = placement(params.seed, axis as u32);
        let (o2, r2) = placement(params.seed, 7 + axis as u32);
        let a = sample(map, rotate(uv, r1) + o1);
        // Larger, rotated second layer: breaks the tile's repetition.
        let b = sample(map, rotate(uv * 0.37, r2) + o2);
        acc += weight * (a * 0.72 + b * 0.28);
    }
    // Mixing two samples narrows the range; stretch it back around the middle.
    let v = ((acc - 0.5) * 1.3 + 0.5).clamp(0.0, 1.0);
    let v = crate::pattern::remap(v, params.balance, params.contrast);
    if params.invert { 1.0 - v } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_doc::ProceduralKind;

    #[test]
    fn map_is_seamless_and_has_a_broad_range() {
        let m = &*MAP;
        let n = m.size;
        assert_eq!(n, 1024);
        let mean = m.values.iter().sum::<f32>() / m.values.len() as f32;
        let var = m.values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / m.values.len() as f32;
        assert!((0.3..0.6).contains(&mean), "mean {mean}");
        assert!(var.sqrt() > 0.2, "a real grunge map spans the range, std {}", var.sqrt());
        // Seamless: the jump across the wrap is no larger than between ordinary neighbors.
        let diff = |a: usize, b: usize| (m.values[a] - m.values[b]).abs();
        let inner: f32 = (0..n).map(|y| diff(y * n + n / 2 - 1, y * n + n / 2)).sum::<f32>() / n as f32;
        let wrap_x: f32 = (0..n).map(|y| diff(y * n + n - 1, y * n)).sum::<f32>() / n as f32;
        let wrap_y: f32 = (0..n).map(|x| diff((n - 1) * n + x, x)).sum::<f32>() / n as f32;
        assert!(wrap_x < inner * 1.5 && wrap_y < inner * 1.5, "wrap {wrap_x} {wrap_y} vs inner {inner}");
    }

    #[test]
    fn eval_is_continuous_seeded_and_in_range() {
        let p = ProceduralParams { scale: 8.0, seed: 3, ..ProceduralParams::new(ProceduralKind::Clouds) };
        let q = ProceduralParams { seed: 4, ..p };
        let (mut step, mut seed_diff) = (0.0f32, 0.0f32);
        for i in 0..200 {
            let x = i as f32 * 0.013;
            let pos = [x, 0.3 * x, 0.2];
            let nrm = [0.3, 0.9, 0.2];
            let a = eval(&p, pos, nrm);
            assert!((0.0..=1.0).contains(&a));
            step = step.max((a - eval(&p, [x + 1e-5, 0.3 * x, 0.2], nrm)).abs());
            seed_diff += (a - eval(&q, pos, nrm)).abs();
        }
        assert!(step < 0.02, "continuous (a seventieth of a texel moves little), max step {step}");
        assert!(seed_diff / 200.0 > 0.05, "seeds give different placements");
    }
}
