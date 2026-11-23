//! Small object-space patterns for the Fiber Glass Edge Wear and Mask Builder generators: a
//! woven mat of strands and scattered round dots. Pure functions of the position, so they have
//! no seams and need no cache.

#[inline]
fn hash(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

#[inline]
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / 16_777_216.0
}

/// One family of parallel strands: strand `floor(across)` runs along `along`, rising and
/// dipping under the strands that cross it. Returns 0 (gap) to 1 (top of a strand).
#[inline]
fn strands(across: f32, along: f32, phase: f32, irregularity: f32, seed: u32) -> f32 {
    let index = across.floor();
    let h = hash(index as i32, 0, 0, seed);
    // A strand's own width and brightness; loose strands ride higher.
    let width = 1.0 - irregularity * 0.45 * unit(h);
    let lift = irregularity * 0.5 * (unit(h.rotate_left(11)) - 0.5);
    // A slow wobble keeps long strands from being ruler-straight.
    let wobble = irregularity * 0.18 * (along * 0.37 + unit(h.rotate_left(5)) * std::f32::consts::TAU).sin();
    let centre = (across - index - 0.5 + wobble).abs() * 2.0;
    let profile = (1.0 - centre / width).clamp(0.0, 1.0).sqrt();
    let wave = 0.72 + 0.28 * (std::f32::consts::PI * (along + index) + phase).sin();
    (profile * (wave + lift)).clamp(0.0, 1.0)
}

/// A plain weave in strand units: 1 on top of a strand, 0 in the gaps between strands.
/// `irregularity` 0 is a perfect cloth; 1 gives uneven widths and loose strands.
pub fn weave(u: f32, v: f32, irregularity: f32, seed: u32) -> f32 {
    let irregularity = irregularity.clamp(0.0, 1.0);
    let warp = strands(u, v, 0.0, irregularity, seed);
    let weft = strands(v, u, std::f32::consts::PI, irregularity, seed ^ 0x5bd1_e995);
    warp.max(weft)
}

/// [`weave`] on a surface point: projected along the three axes and blended by the normal
/// (sharply, so only narrow bands near 45 degrees mix two projections). `position` is in
/// units of the model's longest side; `scale` is strands per unit.
pub fn weave_triplanar(position: [f32; 3], normal: [f32; 3], scale: f32, irregularity: f32, seed: u32) -> f32 {
    let p = position.map(|c| c * scale);
    let mut w = normal.map(|c| c.abs().powi(8));
    let total = w[0] + w[1] + w[2];
    if total <= 1e-20 {
        w = [0.0, 1.0, 0.0];
    } else {
        w = w.map(|c| c / total);
    }
    let mut out = 0.0;
    for (axis, (a, b)) in [(1usize, 2usize), (2, 0), (0, 1)].into_iter().enumerate() {
        if w[axis] > 1e-4 {
            out += w[axis] * weave(p[a], p[b], irregularity, seed.wrapping_add(axis as u32 * 977));
        }
    }
    out
}

/// Scattered round dots in space (in cell units: about `density` dots per unit cube). `size`
/// is the dot radius as a share of the largest that fits a cell; the variations shrink and
/// fade random dots. Returns the opacity of the strongest dot covering `p`, soft-edged.
pub fn dots(p: [f32; 3], density: f32, size: f32, size_variation: f32, opacity_variation: f32, seed: u32) -> f32 {
    let cell = p.map(|c| c.floor() as i32);
    let mut best = 0.0f32;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y, z) = (cell[0] + dx, cell[1] + dy, cell[2] + dz);
                let h = hash(x, y, z, seed);
                if unit(h) >= density {
                    continue;
                }
                let centre = [x as f32 + unit(h.rotate_left(7)), y as f32 + unit(h.rotate_left(13)), z as f32 + unit(h.rotate_left(19))];
                let radius = 0.5 * size * (1.0 - size_variation * unit(h.rotate_left(23)));
                if radius <= 1e-5 {
                    continue;
                }
                let d = ((p[0] - centre[0]).powi(2) + (p[1] - centre[1]).powi(2) + (p[2] - centre[2]).powi(2)).sqrt();
                let edge = (1.0 - d / radius).clamp(0.0, 1.0);
                let opacity = 1.0 - opacity_variation * unit(h.rotate_left(29));
                best = best.max((edge * 6.0).min(1.0) * opacity);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weave_has_strands_and_gaps_in_both_directions_and_stays_in_range() {
        let (mut lo, mut hi, mut sum) = (1.0f32, 0.0f32, 0.0f32);
        let n = 200;
        for j in 0..n {
            for i in 0..n {
                let v = weave(i as f32 * 0.043, j as f32 * 0.043, 0.5, 3);
                assert!((0.0..=1.0).contains(&v));
                lo = lo.min(v);
                hi = hi.max(v);
                sum += v;
            }
        }
        let mean = sum / (n * n) as f32;
        assert!(lo < 0.05 && hi > 0.9, "gaps and strand tops: {lo} {hi}");
        assert!((0.3..0.8).contains(&mean), "neither empty nor solid: {mean}");
        // A perfect weave repeats every two strands, in both directions.
        for (u, v) in [(0.3, 0.7), (1.6, 0.2), (0.5, 0.5)] {
            let a = weave(u, v, 0.0, 1);
            assert!((a - weave(u + 2.0, v, 0.0, 1)).abs() < 1e-5 && (a - weave(u, v + 2.0, 0.0, 1)).abs() < 1e-5);
        }
        // Over and under alternate: at a crossing one cell over, the other family is on top.
        let top = |u: f32, v: f32| strands(u, v, 0.0, 0.0, 1) > strands(v, u, std::f32::consts::PI, 0.0, 1);
        assert_ne!(top(0.5, 0.5), top(1.5, 0.5));
        assert_ne!(top(0.5, 0.5), top(0.5, 1.5));
    }

    #[test]
    fn the_triplanar_weave_is_the_flat_weave_on_an_axis_aligned_face() {
        let v = weave_triplanar([0.31, 0.2, 0.77], [0.0, 1.0, 0.0], 10.0, 0.4, 5);
        assert!((v - weave(7.7, 3.1, 0.4, 5 + 977)).abs() < 1e-5);
        assert!(weave_triplanar([0.1, 0.2, 0.3], [0.0; 3], 10.0, 0.4, 5).is_finite());
    }

    #[test]
    fn dots_follow_density_and_size() {
        let cover = |density: f32, size: f32| {
            let n = 40;
            let mut c = 0;
            for k in 0..n * n * n {
                let p = [(k % n) as f32 * 0.11, ((k / n) % n) as f32 * 0.11, (k / n / n) as f32 * 0.11];
                if dots(p, density, size, 0.0, 0.0, 9) > 0.5 { c += 1; }
            }
            c as f32 / (n * n * n) as f32
        };
        assert_eq!(cover(0.0, 1.0), 0.0);
        assert_eq!(cover(1.0, 0.0), 0.0);
        let (few, many, big) = (cover(0.2, 0.5), cover(0.8, 0.5), cover(0.8, 1.0));
        assert!(few > 0.0 && many > few * 2.0 && big > many * 2.0, "{few} {many} {big}");
        // Opacity variation only fades dots.
        let p = [3.3, 1.2, 7.9];
        assert!(dots(p, 1.0, 1.0, 0.0, 1.0, 9) <= dots(p, 1.0, 1.0, 0.0, 0.0, 9));
    }
}
