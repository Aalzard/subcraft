//! Chipped-paint breakup for edge wear: a domain-warped mosaic of angular flakes (two sizes of
//! 3D Voronoi cells, one random value per cell) with a little fine detail. Thresholding a smooth
//! edge falloff against it gives the stepped, flaking border of worn paint instead of the soft
//! blotches of a cloud noise. Sampled in object space, so it never follows UV seams.
use glam::Vec3;
use subcraft_doc::ProceduralParams;

#[inline]
fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    crate::noise::hash2(crate::noise::hash2(x, y, seed) as i32, z, seed ^ 0x51ed_270b)
}

#[inline]
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Value of the Voronoi cell nearest to `p` (one random value per cell), and how close `p` is to
/// that cell's border (F2 - F1).
fn flakes(p: Vec3, seed: u32) -> (f32, f32) {
    let c = Vec3::new(crate::noise::floor(p.x), crate::noise::floor(p.y), crate::noise::floor(p.z));
    let (cx, cy, cz) = (c.x as i32, c.y as i32, c.z as i32);
    let (mut f1, mut f2, mut id) = (f32::MAX, f32::MAX, 0u32);
    for z in -1..=1 { for y in -1..=1 { for x in -1..=1 {
        let h = hash3(cx + x, cy + y, cz + z, seed);
        let j = Vec3::new(unit(h), unit(h.rotate_left(11) ^ 0x68e3_1da4), unit(h.rotate_left(22) ^ 0xb529_7a4d));
        let d = (c + Vec3::new(x as f32, y as f32, z as f32) + j * 0.9 + Vec3::splat(0.05) - p).length_squared();
        if d < f1 { f2 = f1; f1 = d; id = h; } else if d < f2 { f2 = d; }
    }}}
    (unit(id.wrapping_mul(0x2545_f491) ^ 0x9e37_79b9), f2.sqrt() - f1.sqrt())
}

/// 0..1 chip field at `position` (object space, units of the model's longest side).
pub fn eval(params: &ProceduralParams, position: [f32; 3]) -> f32 {
    let p = Vec3::from_array(position);
    let seed = params.seed ^ 0x0c41_95ed;
    let s = params.scale.clamp(0.01, 1024.0) * 3.0;
    // A broad warp makes the flakes irregular rather than convex cells.
    let mut w = *params;
    w.scale = s * 0.5;
    w.seed = seed;
    w.balance = 0.5;
    w.contrast = 0.0;
    w.invert = false;
    let warp = Vec3::new(crate::organic::eval(&w, position), crate::organic::eval(&w, [position[1], position[2], position[0]]), crate::organic::eval(&w, [position[2], position[0], position[1]])) - Vec3::splat(0.5);
    let q = p * s + warp * 2.2;
    let (big, _) = flakes(q, seed);
    let (small, border) = flakes(q * 2.7 + Vec3::splat(17.3), seed ^ 0x2f7a);
    let mut d = *params;
    d.scale = s * 6.0;
    d.seed = seed ^ 0x7177;
    d.balance = 0.5;
    d.contrast = 0.0;
    d.invert = false;
    let detail = crate::organic::eval(&d, position);
    // Thin dark cracks along the small flakes' borders read as paint about to flake off.
    let crack = (1.0 - (border * 6.0).min(1.0)) * 0.12;
    let v = big * 0.5 + small * 0.32 + detail * 0.18 - crack;
    let v = ((v - 0.5) * 1.7 + 0.5).clamp(0.0, 1.0);
    let v = crate::pattern::remap(v, params.balance, params.contrast);
    if params.invert { 1.0 - v } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_doc::ProceduralKind;

    #[test]
    fn chips_are_deterministic_bounded_and_span_the_range() {
        let params = ProceduralParams::new(ProceduralKind::Clouds);
        let samples: Vec<f32> = (0..4096).map(|i| eval(&params, [(i % 64) as f32 / 64.0, (i / 64) as f32 / 64.0, 0.3])).collect();
        assert!(samples.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_eq!(samples[100], eval(&params, [36.0 / 64.0, 1.0 / 64.0, 0.3]));
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        let low = samples.iter().filter(|&&v| v < 0.25).count();
        let high = samples.iter().filter(|&&v| v > 0.75).count();
        assert!((0.35..0.65).contains(&mean), "mean {mean}");
        assert!(low > 200 && high > 200, "a chip field needs both deep and high values: {low} {high}");
        // Another seed is another pattern.
        let mut other = params;
        other.seed += 1;
        assert_ne!(samples[7], eval(&other, [7.0 / 64.0, 0.0, 0.3]));
    }
}
