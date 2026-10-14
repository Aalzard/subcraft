//! Per-channel strength of fill layers ("how strong is this map").

use subcraft_core::ChannelKind;

/// Scale a fill channel's evaluated texels (`[r, g, b, alpha]`, scalars in `r`) by `strength`:
/// * Normal: scales the tilt (tangent-space x/y) and renormalizes; 0 = flat surface.
/// * Height: scales the signed height (bump strength).
/// * Base Color: brightness (clamped to 1); Emissive: intensity (HDR, unclamped).
/// * Other scalars (roughness, metallic, ...): multiplied and clamped to 0..1.
///
/// Alpha (coverage) is never changed; `strength == 1` is an exact no-op.
pub fn apply_strength(kind: ChannelKind, strength: f32, px: &mut [[f32; 4]]) {
    if strength == 1.0 {
        return;
    }
    let s = strength.max(0.0);
    match kind {
        ChannelKind::Normal => {
            for p in px.iter_mut() {
                let (x, y, z) = ((p[0] * 2.0 - 1.0) * s, (p[1] * 2.0 - 1.0) * s, (p[2] * 2.0 - 1.0).max(1e-4));
                let len = (x * x + y * y + z * z).sqrt();
                p[0] = x / len * 0.5 + 0.5;
                p[1] = y / len * 0.5 + 0.5;
                p[2] = z / len * 0.5 + 0.5;
            }
        }
        ChannelKind::Height => {
            for p in px.iter_mut() {
                p[0] *= s;
            }
        }
        ChannelKind::BaseColor => {
            for p in px.iter_mut() {
                for c in 0..3 {
                    p[c] = (p[c] * s).min(1.0);
                }
            }
        }
        ChannelKind::Emissive => {
            for p in px.iter_mut() {
                for c in 0..3 {
                    p[c] *= s;
                }
            }
        }
        _ => {
            for p in px.iter_mut() {
                p[0] = (p[0] * s).clamp(0.0, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(p: [f32; 4]) -> [f32; 3] {
        [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0]
    }

    #[test]
    fn one_is_a_no_op_and_alpha_is_kept() {
        for kind in ChannelKind::ALL {
            let mut px = [[0.3, 0.6, 0.9, 0.42]];
            apply_strength(kind, 1.0, &mut px);
            assert_eq!(px, [[0.3, 0.6, 0.9, 0.42]], "{kind:?}");
            apply_strength(kind, 0.5, &mut px);
            assert_eq!(px[0][3], 0.42, "{kind:?} alpha");
        }
    }

    #[test]
    fn normal_strength_scales_tilt_and_stays_unit() {
        // 30° tilt toward +x.
        let (sn, cs) = 30f32.to_radians().sin_cos();
        let tilted = [sn * 0.5 + 0.5, 0.5, cs * 0.5 + 0.5, 1.0];
        let mut flat = [tilted];
        apply_strength(ChannelKind::Normal, 0.0, &mut flat);
        let f = decode(flat[0]);
        assert!(f[0].abs() < 1e-5 && (f[2] - 1.0).abs() < 1e-5, "strength 0 is flat: {f:?}");
        let mut strong = [tilted];
        apply_strength(ChannelKind::Normal, 2.0, &mut strong);
        let n = decode(strong[0]);
        assert!(n[0] > sn, "stronger tilt: {n:?}");
        assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-5, "unit length");
    }

    #[test]
    fn height_scales_signed_and_scalars_clamp() {
        let mut h = [[-0.4, 0.0, 0.0, 1.0], [0.3, 0.0, 0.0, 1.0]];
        apply_strength(ChannelKind::Height, 2.0, &mut h);
        assert_eq!((h[0][0], h[1][0]), (-0.8, 0.6));
        let mut r = [[0.7, 0.0, 0.0, 1.0]];
        apply_strength(ChannelKind::Roughness, 2.0, &mut r);
        assert_eq!(r[0][0], 1.0);
        let mut m = [[0.8, 0.0, 0.0, 1.0]];
        apply_strength(ChannelKind::Metallic, 0.5, &mut m);
        assert_eq!(m[0][0], 0.4);
        let mut c = [[0.4, 0.6, 0.8, 1.0]];
        apply_strength(ChannelKind::BaseColor, 1.5, &mut c);
        assert!((c[0][0] - 0.6).abs() < 1e-6 && c[0][2] == 1.0);
    }
}
