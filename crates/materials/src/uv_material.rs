//! The lit material drawn flat in UV space (Painter's 2D view "Material" display): every texel
//! shaded as if it faced the viewer, with its own tangent-space normal, under a fixed studio
//! light. Pure per-texel math so the 2D view and the tests share it; values are linear.

/// One texel's material: linear base colour, encoded tangent-space normal (0..1, OpenGL +Y),
/// roughness, metallic and ambient occlusion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvTexel {
    pub base: [f32; 3],
    pub normal: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub ao: f32,
    /// Tangent-space bump normal of the Height channel ([`height_bump`]), combined with
    /// `normal` as the 3D view does; `[0, 0, 1]` for none.
    pub bump: [f32; 3],
}

impl Default for UvTexel {
    fn default() -> Self {
        Self { base: [0.5; 3], normal: [0.5, 0.5, 1.0], roughness: 0.5, metallic: 0.0, ao: 1.0, bump: [0.0, 0.0, 1.0] }
    }
}

/// Bump normal of a height field from its neighbours left / right / up / down, `texel` UV units
/// apart, times `scale` (the 3D view's height scale). Mirrors the viewport shader's
/// `height_bump`: UV v grows downward while the bitangent points up the image (OpenGL), hence
/// +dh/dv for y.
pub fn height_bump(left: f32, right: f32, up: f32, down: f32, texel: f32, scale: f32) -> [f32; 3] {
    let t = texel.max(1e-8);
    let dhdu = (right - left) / (2.0 * t);
    let dhdv = (down - up) / (2.0 * t);
    normalize([-dhdu * scale, dhdv * scale, 1.0])
}

/// The decoded Normal channel combined with a bump normal (whiteout), as the 3D view shades.
pub fn combined_normal(n: [f32; 3], bump: [f32; 3]) -> [f32; 3] {
    normalize([n[0] + bump[0], n[1] + bump[1], n[2].max(1e-3) * bump[2]])
}

/// Key light direction in tangent space (from the upper left, towards the viewer).
const LIGHT: [f32; 3] = [-0.37, 0.45, 0.81];
/// Share of the lighting that comes from the surroundings (fills the side away from the light).
const AMBIENT: f32 = 0.3;

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-8 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Linear RGB of `t` lit in UV space: Lambert diffuse for the non-metal part, a GGX highlight
/// with Schlick-free F0 (0.04 for dielectrics, the base colour for metals), a hemisphere
/// ambient term, all times the texel's ambient occlusion. Not clamped above 1 (the caller
/// tone-maps or clamps).
pub fn shade(t: UvTexel) -> [f32; 3] {
    let n = combined_normal(normalize([t.normal[0] * 2.0 - 1.0, t.normal[1] * 2.0 - 1.0, t.normal[2] * 2.0 - 1.0]), t.bump);
    let l = normalize(LIGHT);
    let v = [0.0, 0.0, 1.0];
    let h = normalize([l[0] + v[0], l[1] + v[1], l[2] + v[2]]);
    let metal = t.metallic.clamp(0.0, 1.0);
    let rough = t.roughness.clamp(0.04, 1.0);
    let ndl = dot(n, l).max(0.0);
    let ndh = dot(n, h).max(0.0);
    let a2 = (rough * rough).powi(2);
    let d = a2 / (std::f32::consts::PI * (ndh * ndh * (a2 - 1.0) + 1.0).powi(2));
    // Up-facing hemisphere: a normal tilted away from the viewer gets less of the surroundings.
    let sky = 0.5 + 0.5 * n[2];
    let ao = t.ao.clamp(0.0, 1.0);
    let mut out = [0.0; 3];
    for c in 0..3 {
        let base = t.base[c].max(0.0);
        let diffuse = base * (1.0 - metal);
        let f0 = 0.04 * (1.0 - metal) + base * metal;
        let direct = diffuse * ndl * (1.0 - AMBIENT) + f0 * d * ndl * 0.25;
        let ambient = (diffuse + f0 * (1.0 - 0.5 * rough)) * AMBIENT * sky;
        out[c] = (direct + ambient) * ao;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: [f32; 3]) -> f32 {
        c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
    }

    #[test]
    fn a_flat_rough_dielectric_is_lit_by_the_key_and_the_ambient() {
        let white = UvTexel { base: [1.0; 3], roughness: 1.0, ..Default::default() };
        let c = shade(white);
        let l = normalize(LIGHT);
        // Diffuse: n.l x 0.7 + ambient 0.3 (sky 1 for a flat normal), plus a faint highlight.
        let expected = l[2] * (1.0 - AMBIENT) + (1.0 + 0.04 * 0.5) * AMBIENT;
        assert!((c[0] - expected).abs() < 0.03, "{c:?} vs {expected}");
        assert!(c[0] == c[1] && c[1] == c[2], "grey stays grey");
        // Black base: only the dielectric reflection.
        assert!(lum(shade(UvTexel { base: [0.0; 3], roughness: 1.0, ..Default::default() })) < 0.05);
    }

    #[test]
    fn normals_facing_the_light_are_brighter_and_ao_darkens() {
        let toward = UvTexel { normal: [0.5 + 0.5 * -0.5, 0.5 + 0.5 * 0.5, 0.5 + 0.5 * 0.7], ..Default::default() };
        let away = UvTexel { normal: [0.5 + 0.5 * 0.5, 0.5 + 0.5 * -0.5, 0.5 + 0.5 * 0.7], ..Default::default() };
        assert!(lum(shade(toward)) > lum(shade(away)) * 1.5, "{:?} vs {:?}", shade(toward), shade(away));
        let flat = shade(UvTexel::default());
        let occluded = shade(UvTexel { ao: 0.5, ..Default::default() });
        assert!((occluded[0] - flat[0] * 0.5).abs() < 1e-6);
    }

    #[test]
    fn height_slopes_tilt_the_normal_like_the_3d_view() {
        // Flat height: no bump. A ramp rising to the right tilts the normal to the left (-x).
        assert_eq!(height_bump(0.3, 0.3, 0.3, 0.3, 0.01, 0.02), [0.0, 0.0, 1.0]);
        let b = height_bump(0.0, 1.0, 0.5, 0.5, 0.01, 0.02);
        // dh/du = 1 / 0.02 = 50, times 0.02 = 1: the normal (-1, 0, 1) normalised.
        assert!((b[0] + std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5 && b[1].abs() < 1e-6, "{b:?}");
        // Rising downward (+v) tilts it up the image (+y).
        assert!(height_bump(0.5, 0.5, 0.0, 1.0, 0.01, 0.02)[1] > 0.5);
        // Whiteout: a flat normal map passes the bump through.
        let c = combined_normal([0.0, 0.0, 1.0], b);
        assert!((c[0] - b[0]).abs() < 1e-6 && (c[2] - b[2]).abs() < 1e-6);
        // A bump towards the light brightens the texel.
        let toward = shade(UvTexel { bump: normalize([-0.5, 0.5, 0.7]), ..Default::default() });
        let away = shade(UvTexel { bump: normalize([0.5, -0.5, 0.7]), ..Default::default() });
        assert!(toward[0] > away[0] * 1.5);
    }

    #[test]
    fn metals_tint_their_reflection_and_smooth_surfaces_have_a_sharper_highlight() {
        let gold = UvTexel { base: [1.0, 0.7, 0.2], metallic: 1.0, roughness: 0.3, ..Default::default() };
        let c = shade(gold);
        assert!(c[0] > c[2] * 2.0, "the reflection takes the base colour: {c:?}");
        // A normal along the half vector: the smooth surface peaks far higher than the rough one.
        let l = normalize(LIGHT);
        let h = normalize([l[0], l[1], l[2] + 1.0]);
        let along = [0.5 + 0.5 * h[0], 0.5 + 0.5 * h[1], 0.5 + 0.5 * h[2]];
        let smooth = shade(UvTexel { normal: along, roughness: 0.1, ..Default::default() });
        let rough = shade(UvTexel { normal: along, roughness: 0.9, ..Default::default() });
        assert!(smooth[0] > rough[0] * 2.0, "{smooth:?} vs {rough:?}");
    }
}
