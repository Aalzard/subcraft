//! Warp displacement field: two decorrelated periodic fbm channels in UV space.
use crate::noise;
use glam::Vec2;

/// Offset in texels for the texel centre at (x, y) of a res x res texture. Each axis is in
/// [-intensity, intensity]; scale is the number of noise cells across the texture and the
/// field tiles seamlessly at the texture border.
pub fn offset(x: u32, y: u32, res: u32, intensity: f32, scale: u32, seed: u32) -> [f32; 2] {
    if intensity == 0.0 || res == 0 { return [0.0; 2]; }
    let p = (Vec2::new(x as f32, y as f32) + 0.5) / res as f32 * scale as f32;
    let period = scale.max(1) as i32;
    let dx = noise::fbm(p, period, 2, seed) * 2.0 - 1.0;
    let dy = noise::fbm(p, period, 2, seed.wrapping_add(0x9e37_79b9)) * 2.0 - 1.0;
    [dx * intensity, dy * intensity]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_deterministic_bounded_periodic_and_seeded() {
        let mut spread = 0.0f32;
        for y in (0..256).step_by(7) { for x in (0..256).step_by(5) {
            let o = offset(x, y, 256, 10.0, 4, 3);
            assert_eq!(o, offset(x, y, 256, 10.0, 4, 3));
            assert!(o[0].abs() <= 10.0 && o[1].abs() <= 10.0, "{o:?}");
            spread = spread.max(o[0].abs());
        } }
        assert!(spread > 1.0, "the field must actually move texels, max {spread}");
        assert_eq!(offset(5, 6, 256, 0.0, 4, 3), [0.0; 2]);
        // Resolution independent: texel 40 at 128 and texel 81 at 256 share the same UV centre
        // (40.5/128 vs 81.5/256 differ by under a quarter texel), so unit-intensity offsets agree.
        let a = offset(40, 60, 128, 1.0, 4, 3);
        let b = offset(81, 121, 256, 1.0, 4, 3);
        assert!((a[0] - b[0]).abs() < 0.05 && (a[1] - b[1]).abs() < 0.05, "{a:?} {b:?}");
        assert_ne!(offset(40, 60, 128, 10.0, 4, 3), offset(40, 60, 128, 10.0, 4, 4));
        // Periodic across the texture border: texel 0 is the neighbour of texel res-1.
        let (l, r) = (offset(255, 100, 256, 10.0, 4, 3), offset(0, 100, 256, 10.0, 4, 3));
        assert!((l[0] - r[0]).abs() < 0.5 && (l[1] - r[1]).abs() < 0.5, "{l:?} {r:?}");
        // The two axes are decorrelated.
        let o = offset(40, 60, 128, 10.0, 4, 3);
        assert_ne!(o[0], o[1]);
    }
}
