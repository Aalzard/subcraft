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

/// Exactly `x.floor()` (same bits, including -0.0, NaN and huge values) without the library
/// call that `floor` compiles to on x86-64 CPUs without SSE4.1 (the portable baseline).
#[inline]
pub(crate) fn floor(x: f32) -> f32 {
    if x.abs() < 8_388_608.0 && x != 0.0 {
        let t = x as i32 as f32;
        if t > x { t - 1.0 } else { t }
    } else {
        x.floor()
    }
}

#[inline]
pub(crate) fn floor2(p: Vec2) -> Vec2 {
    Vec2::new(floor(p.x), floor(p.y))
}

/// `(wrap(i, period), wrap(i + 1, period))` with one division.
#[inline]
fn wrap_pair(i: i32, period: i32) -> (i32, i32) {
    let pm = period.max(1);
    let a = i.rem_euclid(pm);
    (a, if a + 1 == pm { 0 } else { a + 1 })
}

#[inline]
fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Lattice gradient directions already computed on this thread, direct-mapped by hash. A slot
/// holds `(hash, cos, sin)`; texels of a tile share corners, so most lookups skip the trig.
/// Values are exactly what `a.cos(), a.sin()` give, so cached and uncached noise are identical.
const GRADIENT_SLOTS: usize = 4096;
/// Lattice cells whose four corner gradients were looked up last on this thread, direct-mapped
/// by octave seed and period. Neighboring texels almost always fall in the same cell at every
/// octave, so the wraps, hashes and gradient lookups of a cell run once for many texels.
const CELL_SLOTS: usize = 64;

/// Four corner gradients of lattice cell `key = (ix, iy, period.max(1), seed)`.
#[derive(Clone, Copy)]
struct Cell {
    key: [i32; 4],
    g: [Vec2; 4],
}

struct NoiseCache {
    slots: Box<[(u32, u32, f32, f32)]>,
    cells: Box<[Cell]>,
}

impl NoiseCache {
    fn new() -> Self {
        // (MAX, MAX) never matches: a valid entry stores (h, !h). A cell key with period 0
        // never matches either: keys store `period.max(1)`.
        Self {
            slots: vec![(u32::MAX, u32::MAX, 0.0, 0.0); GRADIENT_SLOTS].into_boxed_slice(),
            cells: vec![Cell { key: [0; 4], g: [Vec2::ZERO; 4] }; CELL_SLOTS].into_boxed_slice(),
        }
    }
}

thread_local! {
    static CACHE: std::cell::RefCell<NoiseCache> = std::cell::RefCell::new(NoiseCache::new());
}

/// Unit gradient of lattice hash `h` (cached per thread).
#[inline]
fn gradient_dir(h: u32, slots: &mut [(u32, u32, f32, f32)]) -> Vec2 {
    // Index by the low bits; the full hash plus its high bits identify the entry.
    let slot = &mut slots[(h as usize) & (GRADIENT_SLOTS - 1)];
    if slot.0 == h && slot.1 == !h {
        return Vec2::new(slot.2, slot.3);
    }
    let a = (h >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
    let (s, c) = (a.sin(), a.cos());
    *slot = (h, !h, c, s);
    Vec2::new(c, s)
}

/// Periodic gradient noise in [-1, 1] (approximately).
pub fn gradient(p: Vec2, period: i32, seed: u32) -> f32 {
    CACHE.with(|c| gradient_with(p, period, seed, &mut c.borrow_mut()))
}

/// Corner gradients `[g00, g10, g01, g11]` of the cell at `(ix, iy)`.
#[inline]
fn cell_gradients(ix: i32, iy: i32, period: i32, seed: u32, cache: &mut NoiseCache) -> [Vec2; 4] {
    let pm = period.max(1);
    let key = [ix, iy, pm, seed as i32];
    let slot = (seed.wrapping_mul(0x9e37_79b9) ^ (pm as u32).wrapping_mul(0x85eb_ca6b)) >> 26;
    let cell = &mut cache.cells[slot as usize & (CELL_SLOTS - 1)];
    if cell.key == key {
        return cell.g;
    }
    // Same as `wrap(ix, period)` and `wrap(ix + 1, period)`.
    let (x0, x1) = wrap_pair(ix, pm);
    let (y0, y1) = wrap_pair(iy, pm);
    let slots = &mut cache.slots;
    let g = [
        gradient_dir(hash2(x0, y0, seed), slots),
        gradient_dir(hash2(x1, y0, seed), slots),
        gradient_dir(hash2(x0, y1, seed), slots),
        gradient_dir(hash2(x1, y1, seed), slots),
    ];
    *cell = Cell { key, g };
    g
}

fn gradient_with(p: Vec2, period: i32, seed: u32, cache: &mut NoiseCache) -> f32 {
    let i = floor2(p);
    let f = p - i;
    let (ix, iy) = (i.x as i32, i.y as i32);
    let g = cell_gradients(ix, iy, period, seed, cache);
    let n00 = g[0].dot(f);
    let n10 = g[1].dot(f - Vec2::X);
    let n01 = g[2].dot(f - Vec2::Y);
    let n11 = g[3].dot(f - Vec2::ONE);
    let u = quintic(f.x);
    let v = quintic(f.y);
    let nx0 = n00 + (n10 - n00) * u;
    let nx1 = n01 + (n11 - n01) * u;
    (nx0 + (nx1 - nx0) * v) * std::f32::consts::SQRT_2
}

/// Reference gradient noise without any caching (tests compare the cached path against it).
#[cfg(test)]
fn gradient_uncached(p: Vec2, period: i32, seed: u32) -> f32 {
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
    let i = floor2(p);
    let f = p - i;
    let (ix, iy) = (i.x as i32, i.y as i32);
    let (x0, x1) = wrap_pair(ix, period_x);
    let (y0, y1) = wrap_pair(iy, period_y);
    let (h00, h10, h01, h11) = (hash_f(x0, y0, seed), hash_f(x1, y0, seed), hash_f(x0, y1, seed), hash_f(x1, y1, seed));
    let u = quintic(f.x);
    let v = quintic(f.y);
    let a = h00 + (h10 - h00) * u;
    let b = h01 + (h11 - h01) * u;
    a + (b - a) * v
}

/// Fractal sum of gradient noise, normalized to [0, 1].
pub fn fbm(p: Vec2, period: i32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut freq = 1.0;
    let mut per = period;
    CACHE.with(|c| {
        let cache = &mut c.borrow_mut();
        for o in 0..octaves {
            sum += amp * gradient_with(p * freq, per, seed.wrapping_add(o * 1013), cache);
            norm += amp;
            amp *= 0.5;
            freq *= 2.0;
            per *= 2;
        }
    });
    (sum / norm * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Periodic Worley noise: (F1, F2) distances to the nearest feature points.
pub fn worley(p: Vec2, period: i32, seed: u32) -> (f32, f32) {
    let i = floor2(p);
    let (ix, iy) = (i.x as i32, i.y as i32);
    let mut f1 = f32::MAX;
    let mut f2 = f32::MAX;
    let wx = [wrap(ix - 1, period), wrap(ix, period), wrap(ix + 1, period)];
    let wy = [wrap(iy - 1, period), wrap(iy, period), wrap(iy + 1, period)];
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cx = ix + dx;
            let cy = iy + dy;
            let h = hash2(wx[(dx + 1) as usize], wy[(dy + 1) as usize], seed);
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
    fn cached_gradients_are_bit_identical_to_direct_trig() {
        let direct = |h: u32| {
            let a = (h >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
            Vec2::new(a.cos(), a.sin())
        };
        let mut slots = vec![(u32::MAX, u32::MAX, 0.0, 0.0); GRADIENT_SLOTS];
        // Colliding slots, the sentinel-like hash and repeated lookups.
        let hashes = [0, 1, GRADIENT_SLOTS as u32, 2 * GRADIENT_SLOTS as u32 + 1, u32::MAX, u32::MAX - 1, 0x8da6_b343, 1];
        for _ in 0..2 {
            for &h in &hashes {
                assert_eq!(gradient_dir(h, &mut slots), direct(h), "hash {h:#x}");
            }
        }
        // Noise through the thread caches equals an uncached evaluation, also across cell,
        // period and seed changes, negative cells, period 0 and colliding cell slots.
        for k in 0..20000 {
            let p = Vec2::new(k as f32 * 0.0731 - 300.0, (k % 977) as f32 * -0.0473 + 11.0);
            for (period, seed) in [(7, 3), (1, 3), (0, 3), (64, 3), (7, 4), (13, 3u32.wrapping_add(1013)), (7, u32::MAX)] {
                assert_eq!(gradient(p, period, seed).to_bits(), gradient_uncached(p, period, seed).to_bits(), "{p} {period} {seed}");
            }
        }
        // fbm (one borrow for all octaves) equals the sum of uncached octaves.
        for k in 0..3000 {
            let p = Vec2::new(k as f32 * 0.0137, k as f32 * 0.0291);
            let (mut sum, mut amp, mut norm, mut freq, mut per) = (0.0f32, 0.5f32, 0.0f32, 1.0f32, 5);
            for o in 0..6u32 {
                sum += amp * gradient_uncached(p * freq, per, 9u32.wrapping_add(o * 1013));
                norm += amp;
                amp *= 0.5;
                freq *= 2.0;
                per *= 2;
            }
            assert_eq!(fbm(p, 5, 6, 9).to_bits(), (sum / norm * 0.5 + 0.5).clamp(0.0, 1.0).to_bits());
        }
    }

    #[test]
    fn value_noise_matches_reference() {
        let reference = |p: Vec2, px: i32, py: i32, seed: u32| {
            let i = p.floor();
            let f = p - i;
            let (ix, iy) = (i.x as i32, i.y as i32);
            let h = |x: i32, y: i32| hash_f(wrap(x, px), wrap(y, py), seed);
            let (u, v) = (quintic(f.x), quintic(f.y));
            let a = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * u;
            let b = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * u;
            a + (b - a) * v
        };
        for k in 0..5000 {
            let p = Vec2::new(k as f32 * 0.173 - 400.0, (k % 313) as f32 * 0.291 - 20.0);
            for (px, py) in [(1, 1), (0, 5), (4, 64), (7, 3)] {
                assert_eq!(value_xy(p, px, py, 11).to_bits(), reference(p, px, py, 11).to_bits());
            }
        }
    }

    #[test]
    fn fast_floor_is_bit_identical() {
        let specials = [0.0f32, -0.0, 0.5, -0.5, 1.0, -1.0, 8_388_607.5, -8_388_607.5, 8_388_608.0, -8_388_609.0, 1e20, -1e20, f32::MAX, f32::MIN, f32::MIN_POSITIVE, -f32::MIN_POSITIVE, f32::INFINITY, f32::NEG_INFINITY, f32::EPSILON, -f32::EPSILON, 2.999_999_8, -2.999_999_8];
        for x in specials {
            assert_eq!(floor(x).to_bits(), x.floor().to_bits(), "{x}");
        }
        assert!(floor(f32::NAN).is_nan());
        // A sweep over many exponents and mantissas.
        let mut bits = 0u32;
        for _ in 0..2_000_000 {
            bits = bits.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = f32::from_bits(bits);
            if !x.is_nan() {
                assert_eq!(floor(x).to_bits(), x.floor().to_bits(), "{x}");
            }
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
