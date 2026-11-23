//! Drips running down a surface (Painter's Dripping Rust): a value carried downhill, in
//! texture space, from the texels that hold a source.
//!
//! The surface is a square grid of texels with a position each. Texels are visited from the
//! highest to the lowest along the up axis; each takes what its uphill neighbours hold, faded by
//! the distance travelled, or its own source when that is stronger. One pass, any drip length.
//! A drip stops at a UV seam (the texel across it is not a neighbour) and on a level surface
//! (nothing is uphill).

/// The eight neighbours, counter-clockwise from +x, as `(dx, dy)`.
const RING: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// A height change per texel below this share of the texel's own size is a level surface.
const LEVEL: f32 = 0.05;

/// Carry `source` down the surface. `position[i]` is the position of texel `i` of a `size` x
/// `size` grid, `covered[i]` whether it lies on the surface; `up` is the axis drips fall along
/// (0, 1 or 2, towards lower values). A drip fades to 1/e over `length` (in position units).
/// `smoothness` (0..=1) lets neighbouring streaks merge. Returns, per texel, the larger of its
/// own source and what reached it from above.
pub fn flow(size: usize, position: &[[f32; 3]], covered: &[bool], up: usize, source: &[f32], length: f32, smoothness: f32) -> Vec<f32> {
    let n = size * size;
    assert!(position.len() == n && covered.len() == n && source.len() == n && up < 3);
    let mut out: Vec<f32> = (0..n).map(|i| if covered[i] && source[i].is_finite() { source[i].max(0.0) } else { 0.0 }).collect();
    let length = if length.is_finite() { length.max(0.0) } else { 0.0 };
    if length <= 0.0 {
        return out;
    }
    let smoothness = if smoothness.is_finite() { smoothness.clamp(0.0, 1.0) } else { 0.0 };
    let mut order: Vec<u32> = (0..n as u32).filter(|i| covered[*i as usize] && position[*i as usize][up].is_finite()).collect();
    order.sort_unstable_by(|a, b| position[*b as usize][up].total_cmp(&position[*a as usize][up]));
    let dist = |a: usize, b: usize| {
        let (p, q) = (position[a], position[b]);
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
    };
    for &p in &order {
        let p = p as usize;
        let (x, y) = ((p % size) as i32, (p / size) as i32);
        // The neighbours on the same piece of surface: covered, and no further than a seam.
        let mut near = [None; 8];
        let mut closest = f32::INFINITY;
        for (k, (dx, dy)) in RING.iter().enumerate() {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= size as i32 || ny >= size as i32 {
                continue;
            }
            let q = ny as usize * size + nx as usize;
            if covered[q] {
                let d = dist(p, q);
                if d.is_finite() {
                    near[k] = Some((q, d));
                    // Diagonal steps are longer in texels: compare like with like.
                    closest = closest.min(if dx * dy != 0 { d / std::f32::consts::SQRT_2 } else { d });
                }
            }
        }
        if !closest.is_finite() {
            continue;
        }
        let texel = closest.max(1e-9);
        for (k, slot) in near.iter_mut().enumerate() {
            let diagonal = RING[k].0 * RING[k].1 != 0;
            if slot.is_some_and(|(_, d)| d > texel * 3.0 * if diagonal { std::f32::consts::SQRT_2 } else { 1.0 }) {
                *slot = None;
            }
        }
        // Uphill in texel space: the gradient of the height.
        let h = position[p][up];
        let height = |k: usize| near[k].map(|(q, _)| position[q][up]);
        let slope = |plus: usize, minus: usize| match (height(plus), height(minus)) {
            (Some(a), Some(b)) => (a - b) * 0.5,
            (Some(a), None) => a - h,
            (None, Some(b)) => h - b,
            (None, None) => 0.0,
        };
        let (gx, gy) = (slope(0, 4), slope(2, 6));
        let steep = (gx * gx + gy * gy).sqrt();
        if steep < texel * LEVEL {
            continue;
        }
        // The two ring neighbours on either side of the uphill direction share the read; with
        // smoothness, the next two out join in.
        let turn = gy.atan2(gx).rem_euclid(std::f32::consts::TAU) / std::f32::consts::FRAC_PI_4;
        let k = turn.floor() as usize % 8;
        let f = turn - turn.floor();
        // (Each main tap brings its own two side taps, so the blur is even on both sides.)
        let side = smoothness * 0.5;
        let taps = [(k, 1.0 - f + side * f), ((k + 1) % 8, f + side * (1.0 - f)), ((k + 7) % 8, side * (1.0 - f)), ((k + 2) % 8, side * f)];
        let (mut sum, mut weight) = (0.0f32, 0.0f32);
        for (k, w) in taps {
            if w <= 0.0 {
                continue;
            }
            // Only what is higher has been settled, and only that can run down to here.
            if let Some((q, d)) = near[k].filter(|(q, _)| position[*q][up] > h) {
                sum += w * out[q] * (-d / length).exp();
                weight += w;
            }
        }
        if weight > 0.0 {
            // A missing tap (a seam, the top of the surface) carries nothing: the streak thins
            // out there instead of being copied whole from the other tap.
            let carried = sum / weight.max(1.0);
            if carried > out[p] {
                out[p] = carried;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall `size` texels wide and high, one unit across: +x to the right, the up axis (1)
    /// rising with `rise(x, y)`.
    fn wall(size: usize, rise: impl Fn(f32, f32) -> f32) -> Vec<[f32; 3]> {
        (0..size * size).map(|i| {
            let (x, y) = ((i % size) as f32 / size as f32, (i / size) as f32 / size as f32);
            [x, rise(x, y), 0.0]
        }).collect()
    }

    #[test]
    fn a_drip_runs_straight_down_and_fades_with_distance() {
        let size = 64;
        // Texture row 0 is the top of the wall.
        let position = wall(size, |_, y| 1.0 - y);
        let covered = vec![true; size * size];
        let mut source = vec![0.0; size * size];
        source[10 * size + 20] = 1.0;
        let out = flow(size, &position, &covered, 1, &source, 0.25, 0.0);
        let at = |x: usize, y: usize| out[y * size + x];
        // Below the source: e^(-distance / length), one texel = 1/64.
        for rows in [1usize, 8, 16, 32] {
            let want = (-(rows as f32 / size as f32) / 0.25).exp();
            assert!((at(20, 10 + rows) - want).abs() < 1e-4, "{rows} rows down: {} vs {want}", at(20, 10 + rows));
        }
        // Nothing above it, nothing beside it, and the streak stays one texel wide.
        for y in 0..size {
            for x in 0..size {
                if x != 20 || y < 10 { assert_eq!(at(x, y), 0.0, "{x},{y}"); }
            }
        }
        // No length: the sources themselves.
        assert_eq!(flow(size, &position, &covered, 1, &source, 0.0, 0.0), source);
        // The other way up, the drip runs the other way.
        let flipped = wall(size, |_, y| y);
        let out = flow(size, &flipped, &covered, 1, &source, 0.25, 0.0);
        assert!(out[2 * size + 20] > 0.5 && out[30 * size + 20] == 0.0);
    }

    #[test]
    fn a_drip_follows_a_slanted_wall_and_smoothness_widens_it() {
        let size = 96;
        // Up rises to the top and to the right: downhill is down and to the left, at 45 degrees.
        let position = wall(size, |x, y| (1.0 - y) + x);
        let covered = vec![true; size * size];
        let mut source = vec![0.0; size * size];
        source[10 * size + 80] = 1.0;
        let thin = flow(size, &position, &covered, 1, &source, 1.0, 0.0);
        let at = |v: &[f32], x: usize, y: usize| v[y * size + x];
        for step in [1usize, 10, 40] {
            assert!(at(&thin, 80 - step, 10 + step) > 0.3, "on the diagonal, {step} steps down");
            assert_eq!(at(&thin, 80, 10 + step), 0.0, "not straight below");
            assert_eq!(at(&thin, 80 - step, 10), 0.0, "not straight left");
        }
        let width = |v: &[f32], y: usize| (0..size).filter(|x| at(v, *x, y) > 0.01).count();
        assert_eq!(width(&thin, 40), 1);
        let wide = flow(size, &position, &covered, 1, &source, 1.0, 1.0);
        assert!(width(&wide, 40) > 4, "{}", width(&wide, 40));
        assert!(wide.iter().all(|v| (0.0..=1.0).contains(v)), "a drip never grows");
    }

    #[test]
    fn drips_stop_at_seams_gaps_and_on_level_ground() {
        let size = 64;
        let covered_all = vec![true; size * size];
        let mut source = vec![0.0; size * size];
        source[4 * size + 30] = 1.0;
        // A floor: every texel at the same height. Nothing runs.
        let floor: Vec<[f32; 3]> = (0..size * size).map(|i| [(i % size) as f32 / size as f32, 0.0, (i / size) as f32 / size as f32]).collect();
        assert_eq!(flow(size, &floor, &covered_all, 1, &source, 0.5, 0.5), source);
        // A wall with an uncovered band across it: the drip ends above the band.
        let position = wall(size, |_, y| 1.0 - y);
        let mut covered = covered_all.clone();
        for x in 0..size { covered[20 * size + x] = false; }
        let out = flow(size, &position, &covered, 1, &source, 0.5, 0.0);
        assert!(out[19 * size + 30] > 0.5 && (20..size).all(|y| out[y * size + 30] == 0.0));
        // Two islands side by side in the texture, far apart in space (a UV seam): the drip
        // of the left one does not leak into the right one, even with full smoothness.
        let seam: Vec<[f32; 3]> = (0..size * size).map(|i| {
            let (x, y) = ((i % size) as f32 / size as f32, (i / size) as f32 / size as f32);
            if i % size < 32 { [x, 1.0 - y, 0.0] } else { [x + 5.0, 1.0 - y, 3.0] }
        }).collect();
        let mut source = vec![0.0; size * size];
        // A source a few texels wide, up to the seam (smoothness spreads a single texel thin).
        for x in 8..32 { source[4 * size + x] = 1.0; }
        let out = flow(size, &seam, &covered_all, 1, &source, 0.5, 1.0);
        assert!(out[40 * size + 31] > 0.2, "{:?}", (4..12).map(|y| (28..32).map(|x| out[y * size + x]).collect::<Vec<_>>()).collect::<Vec<_>>());
        assert!((0..size).all(|y| (32..size).all(|x| out[y * size + x] == 0.0)), "nothing crosses the seam");
        // Bad input does not spread: a NaN source is no source.
        let mut bad = vec![0.0; size * size];
        bad[4 * size + 10] = f32::NAN;
        assert!(flow(size, &position, &covered_all, 1, &bad, f32::NAN, f32::NAN).iter().all(|v| *v == 0.0));
    }
}
