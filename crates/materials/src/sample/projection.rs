//! Projection of a texel into 2D pattern/bitmap space.
//!
//! * `Uv` — mesh UV through [`Projection::transform_2d`].
//! * `Planar` — normalized object position projected along one axis.
//! * `Triplanar` — the three planar projections weighted by `|n|^sharpness` (normalized).
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
    mut f: impl FnMut([f32; 2]) -> [f32; 4],
) -> [f32; 4] {
    match proj.mode {
        ProjectionMode::Uv => f(proj.transform_2d(uv)),
        ProjectionMode::Planar => f(proj.transform_2d(planar_coords(pos, proj.planar_axis))),
        ProjectionMode::Triplanar => {
            let w = triplanar_weights(normal, proj.triplanar_sharpness);
            let mut out = [0.0f32; 4];
            for axis in 0..3u8 {
                let wa = w[axis as usize];
                if wa <= 0.0 {
                    continue;
                }
                let s = f(proj.transform_2d(planar_coords(pos, axis)));
                for c in 0..4 {
                    out[c] += s[c] * wa;
                }
            }
            out
        }
    }
}
