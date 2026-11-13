//! Projection of a texel into 2D pattern/bitmap space.
//!
//! * `Uv` — mesh UV through [`Projection::transform_2d`].
//! * `Planar` — normalized object position projected along one axis.
//! * `Triplanar` — the three planar projections weighted by `|n|^sharpness` (normalized).
//!   Hardness `h` (the panel's 0..1 slider) is the exponent `1 / (1 - h)`; `h = 1` keeps the
//!   dominant plane only.
//!
//! Planar frames keep images upright when viewed down the axis with +Y up (v grows downward,
//! matching the top-left UV origin): X → (z, 1−y), Y → (x, z), Z → (x, 1−y).

use subcraft_doc::{Projection, ProjectionMode};

/// Unprojected 2D coordinates of a normalized position on the plane perpendicular to `axis`.
#[inline]
pub fn planar_coords(pos: [f32; 3], axis: u8) -> [f32; 2] {
    match axis {
        0 => [pos[2], 1.0 - pos[1]],
        1 => [pos[0], pos[2]],
        _ => [pos[0], 1.0 - pos[1]],
    }
}

/// Normalized triplanar weights for a normal; equal weights for a zero normal (gutter texels).
#[inline]
pub fn triplanar_weights(normal: [f32; 3], sharpness: f32) -> [f32; 3] {
    let s = sharpness.max(1e-3);
    // From 32 up: at the exponents the Hardness slider reaches (up to 64) the plain powers of a
    // near-diagonal normal sum to under 1e-12 and fell back to an even 1/3 split (a visible pop).
    if s > 32.0 {
        // Past this the plain powers underflow: divide by the largest component first, and at
        // Hardness 1 (`TRIPLANAR_HARD`) cut to the dominant plane (ties share it).
        let a = normal.map(f32::abs);
        let max = a[0].max(a[1]).max(a[2]);
        if max <= 1e-6 {
            return [1.0 / 3.0; 3];
        }
        let w = if s >= subcraft_doc::TRIPLANAR_HARD { a.map(|c| if c >= max { 1.0 } else { 0.0 }) } else { a.map(|c| (c / max).powf(s)) };
        let sum = w[0] + w[1] + w[2];
        return w.map(|c| c / sum);
    }
    let w = normal.map(|c| c.abs().powf(s));
    let sum = w[0] + w[1] + w[2];
    if sum <= 1e-12 { [1.0 / 3.0; 3] } else { w.map(|c| c / sum) }
}

/// Sample `f` at a texel through `proj`, combining triplanar samples by weight.
#[inline]
pub fn sample_projected(
    proj: &Projection,
    uv: [f32; 2],
    pos: [f32; 3],
    normal: [f32; 3],
    f: impl FnMut([f32; 2]) -> [f32; 4],
) -> [f32; 4] {
    sample_projected_sc(proj, proj.rotation_sin_cos(), uv, pos, normal, f)
}

/// [`sample_projected`] with the projection's rotation `(sin, cos)` from
/// [`Projection::rotation_sin_cos`], computed once per tile instead of per texel (same values).
#[inline]
pub fn sample_projected_sc(
    proj: &Projection,
    sc: (f32, f32),
    uv: [f32; 2],
    pos: [f32; 3],
    normal: [f32; 3],
    mut f: impl FnMut([f32; 2]) -> [f32; 4],
) -> [f32; 4] {
    // Symmetry folds the model position (and normal) before a 3D projection reads it.
    let (pos, normal) = proj.effective_symmetry().fold(pos, normal);
    match proj.mode {
        ProjectionMode::Uv => f(proj.transform_2d_sc(sc, uv)),
        ProjectionMode::Planar => f(proj.transform_2d_sc(sc, planar_coords(pos, proj.planar_axis))),
        ProjectionMode::Spherical | ProjectionMode::Cylindrical => f(proj.transform_2d_sc(sc, proj.wrapped_coords(pos))),
        ProjectionMode::Triplanar => {
            let w = triplanar_weights(normal, proj.triplanar_sharpness);
            let mut out = [0.0f32; 4];
            for axis in 0..3u8 {
                let wa = w[axis as usize];
                if wa <= 0.0 {
                    continue;
                }
                let s = f(proj.transform_2d_sc(sc, planar_coords(pos, axis)));
                for c in 0..4 {
                    out[c] += s[c] * wa;
                }
            }
            out
        }
    }
}
