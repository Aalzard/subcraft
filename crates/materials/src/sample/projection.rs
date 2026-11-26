//! Projection of a texel into 2D pattern/bitmap space.
//!
//! * `Uv` — mesh UV through [`Projection::transform_2d`].
//! * `Planar` — normalized object position projected along one axis.
//! * `Warp` — planar through a grid of movable points (`subcraft_doc::WarpGrid`).
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

/// Tri-planar coordinates of one of the six sides, moved so no two sides read the same part of
/// the pattern (`Projection::triplanar_unique`). Without it the -X side shows the +X side's
/// shapes mirrored, and the blend between two axes is mirrored across their diagonal: the
/// "butterfly" on round shapes. A side's weight is 0 where its normal component changes sign,
/// so the jump between the two sides of an axis is never visible. +Z is left where it was.
#[inline]
pub fn unique_side(q: [f32; 2], axis: u8, negative: bool) -> [f32; 2] {
    const SHIFT: [[f32; 2]; 6] = [[0.37, 0.71], [0.83, 0.19], [0.59, 0.43], [0.11, 0.89], [0.0, 0.0], [0.47, 0.29]];
    let s = SHIFT[(axis.min(2) as usize) * 2 + negative as usize];
    // The far side also runs the other way, so an image reads the right way round from outside.
    let u = if negative { 1.0 - q[0] } else { q[0] };
    [u + s[0], q[1] + s[1]]
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
    let mut out = [0.0f32; 4];
    projected_points(proj, sc, uv, pos, normal, |q, w| {
        let s = f(q);
        match w {
            None => out = s,
            Some(w) => {
                for c in 0..4 {
                    out[c] += s[c] * w;
                }
            }
        }
    });
    out
}

/// The points [`sample_projected_sc`] reads for a texel, in its order: `emit(q, None)` once
/// when the projection reads one point (the sample is the result), else `emit(q, Some(w))` per
/// triplanar plane with a weight above 0 (the result is the sum of `sample * w`, starting at 0).
#[inline]
pub fn projected_points(proj: &Projection, sc: (f32, f32), uv: [f32; 2], pos: [f32; 3], normal: [f32; 3], mut emit: impl FnMut([f32; 2], Option<f32>)) {
    // Symmetry folds the model position (and normal), then the 3D transform brings it into the
    // projection box's frame, before a 3D projection reads it.
    let (pos, normal) = proj.local(pos, normal);
    match proj.mode {
        ProjectionMode::Uv => emit(proj.transform_2d_sc(sc, uv), None),
        ProjectionMode::Planar => emit(proj.transform_2d_sc(sc, planar_coords(pos, proj.planar_axis)), None),
        ProjectionMode::Spherical | ProjectionMode::Cylindrical => emit(proj.transform_2d_sc(sc, proj.wrapped_coords(pos)), None),
        ProjectionMode::Warp => emit(proj.transform_2d_sc(sc, proj.warp_coords(pos).0), None),
        ProjectionMode::Triplanar => {
            let w = triplanar_weights(normal, proj.triplanar_sharpness);
            for axis in 0..3u8 {
                let wa = w[axis as usize];
                if wa <= 0.0 {
                    continue;
                }
                let mut q = planar_coords(pos, axis);
                if proj.triplanar_unique {
                    q = unique_side(q, axis, normal[axis as usize] < 0.0);
                }
                emit(proj.transform_2d_sc(sc, q), Some(wa));
            }
        }
    }
}
