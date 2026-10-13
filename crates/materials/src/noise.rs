//! Deterministic, periodic noise primitives (original implementations).
//!
//! All functions take a `period` (lattice cells per tile, >= 1) so patterns tile seamlessly in
//! UV space when the projection scale is an integer.

use glam::Vec2;

#[inline]
pub fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}

#[inline]
pub fn hash_f(x: i32, y: i32, seed: u32) -> f32 {
    (hash2(x, y, seed) >> 8) as f32 / (1u32 << 24) as f32
}

#[inline]
fn wrap(i: i32, period: i32) -> i32 {
    i.rem_euclid(period.max(1))
}

#[inline]
fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Periodic gradient noise in [-1, 1] (approximately).
pub fn gradient(p: Vec2, period: i32, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let (ix, iy) = (i.x as i32, i.y as i32);
    let grad = |cx: i32, cy: i32, d: Vec2| {
        let h = hash2(wrap(cx, period), wrap(cy, period), seed);
        let a = (h >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
        Vec2::new(a.cos(), a.sin()).dot(d)
    };
    let n00 = grad(ix, iy, f);
    let n10 = grad(ix + 1, iy, f - Vec2::X);
    let n01 = grad(ix, iy + 1, f - Vec2::Y);
    let n11 = grad(ix + 1, iy + 1, f - Vec2::ONE);
    let u = quintic(f.x);
    let v = quintic(f.y);
    let nx0 = n00 + (n10 - n00) * u;
    let nx1 = n01 + (n11 - n01) * u;
    (nx0 + (nx1 - nx0) * v) * std::f32::consts::SQRT_2
}

/// Periodic value noise in [0, 1].
pub fn value(p: Vec2, period: i32, seed: u32) -> f32 {
    value_xy(p, period, period, seed)
}

/// Periodic value noise with independent X/Y periods (for stretched patterns).
pub fn value_xy(p: Vec2, period_x: i32, period_y: i32, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let (ix, iy) = (i.x as i32, i.y as i32);
    let h = |x: i32, y: i32| hash_f(wrap(x, period_x), wrap(y, period_y), seed);
    let u = quintic(f.x);
    let v = quintic(f.y);
    let a = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * u;
    let b = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * u;
    a + (b - a) * v
}

/// Fractal sum of gradient noise, normalized to [0, 1].
pub fn fbm(p: Vec2, period: i32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut freq = 1.0;
    let mut per = period;
    for o in 0..octaves {
        sum += amp * gradient(p * freq, per, seed.wrapping_add(o * 1013));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
        per *= 2;
    }
    (sum / norm * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Periodic Worley noise: (F1, F2) distances to the nearest feature points.
pub fn worley(p: Vec2, period: i32, seed: u32) -> (f32, f32) {
    let i = p.floor();
    let (ix, iy) = (i.x as i32, i.y as i32);
    let mut f1 = f32::MAX;
    let mut f2 = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cx = ix + dx;
            let cy = iy + dy;
            let h = hash2(wrap(cx, period), wrap(cy, period), seed);
            let jx = (h & 0xffff) as f32 / 65535.0;
            let jy = (h >> 16) as f32 / 65535.0;
            let fp = Vec2::new(cx as f32 + jx, cy as f32 + jy);
            let d = fp.distance(p);
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodic_noise_tiles() {
        let period = 4;
        for k in 0..50 {
            let p = Vec2::new(k as f32 * 0.173, k as f32 * 0.311);
            let shifted = p + Vec2::new(period as f32, 0.0);
            assert!((gradient(p, period, 7) - gradient(shifted, period, 7)).abs() < 1e-4);
            assert!((value(p, period, 7) - value(shifted, period, 7)).abs() < 1e-5);
            assert!((fbm(p, period, 4, 7) - fbm(shifted, period, 4, 7)).abs() < 1e-4);
        }
    }

    #[test]
    fn ranges() {
        for k in 0..500 {
            let p = Vec2::new(k as f32 * 0.37, k as f32 * 0.91);
            let v = fbm(p, 8, 5, 3);
            assert!((0.0..=1.0).contains(&v));
            let (f1, f2) = worley(p, 8, 3);
            assert!(f1 <= f2 && f1 < 1.5);
        }
    }
}
