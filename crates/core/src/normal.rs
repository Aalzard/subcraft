//! Encoded OpenGL tangent-space normal math shared by compositing and painting.
use glam::Vec3;

pub fn decode(v: [f32; 3]) -> Vec3 {
    Vec3::from_array(v) * 2.0 - Vec3::ONE
}

pub fn encode(n: Vec3) -> [f32; 3] {
    (n.normalize_or(Vec3::Z) * 0.5 + Vec3::splat(0.5)).to_array()
}

/// Reoriented normal mapping. Both inputs and the unit-length output are encoded 0..1.
pub fn reoriented(base: [f32; 3], detail: [f32; 3]) -> [f32; 3] {
    let t = decode(base) + Vec3::Z;
    let u = decode(detail) * Vec3::new(-1.0, -1.0, 1.0);
    if t.z.abs() < 1e-6 {
        return encode(-u);
    }
    encode(t * t.dot(u) / t.z - u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_has_flat_identity_and_unit_length() {
        let n = encode(Vec3::new(0.4, -0.3, 0.8));
        for result in [
            reoriented(n, [0.5, 0.5, 1.0]),
            reoriented([0.5, 0.5, 1.0], n),
        ] {
            assert!((decode(result) - decode(n)).length() < 1e-6);
        }
        assert!((decode(reoriented(n, n)).length() - 1.0).abs() < 1e-6);
        assert_eq!(encode(Vec3::ZERO), [0.5, 0.5, 1.0]);
        assert!(decode(reoriented([0.5, 0.5, 0.0], n)).is_finite());
        // Analytic RNM: (0.6,0,0.8) + (0,0.6,0.8) -> (0.48,0.6,0.64).
        let golden = decode(reoriented([0.8, 0.5, 0.9], [0.5, 0.8, 0.9]));
        assert!((golden - Vec3::new(0.48, 0.6, 0.64)).length() < 1e-6);
    }
}
