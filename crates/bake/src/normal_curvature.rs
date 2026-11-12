//! Curvature derived from a tangent-space normal map, measured along the actual surface.
//!
//! A curvature map read straight off the normal-map image (differences of neighbouring texels)
//! breaks wherever image neighbours are not surface neighbours: a UV seam puts two unrelated
//! islands side by side (a doubled or missing band), and on curved low polys the tangent frame
//! turns from texel to texel, so a flat detail reads as a slope (an offset band).
//!
//! Here every texel is a point on the low poly. From it the baker walks a fixed surface distance
//! in eight directions of the tangent plane, crossing triangle edges by unfolding (welded
//! positions, so UV seams are crossed like any other edge). At each end point the normal map is
//! sampled at that point's own UV and turned into a world-space normal with that point's own
//! tangent frame. Curvature is the change of that world normal along the walked direction: the
//! divergence of the shaded normal field, which is continuous across seams and independent of the
//! tangent basis. 0.5 is flat, brighter is convex, darker is concave.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use glam::{Vec2, Vec3};
use rayon::prelude::*;
use subcraft_core::MeshMapKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Mesh, topology};

use crate::surface::{Texel, texels};
use crate::{BakeError, Progress};

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalCurvatureSettings {
    /// Surface distance walked from each texel, in texels of the output map.
    pub radius_texels: f32,
    /// Gain on the measured change of normal (1 = a 90° turn within the radius saturates).
    pub intensity: f32,
}

impl Default for NormalCurvatureSettings {
    fn default() -> Self {
        Self { radius_texels: 2.0, intensity: 1.0 }
    }
}

/// Triangle adjacency across edges of welded positions: `adj[t][k]` is the triangle across the
/// edge opposite vertex `k` of `t` (`NONE` on boundaries and non-manifold edges).
pub(crate) struct SurfaceWalker<'a> {
    mesh: &'a Mesh,
    adj: Vec<[u32; 3]>,
}

/// Barycentric weights of `p` (projected onto the plane of `a b c`).
fn barycentric(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> [f32; 3] {
    let (v0, v1, v2) = (b - a, c - a, p - a);
    let (d00, d01, d11) = (v0.dot(v0), v0.dot(v1), v1.dot(v1));
    let (d20, d21) = (v2.dot(v0), v2.dot(v1));
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1e-30 {
        return [1.0, 0.0, 0.0];
    }
    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    [1.0 - v - w, v, w]
}

impl<'a> SurfaceWalker<'a> {
    pub(crate) fn new(mesh: &'a Mesh) -> Self {
        let weld = topology::weld_positions(mesh);
        let tris = mesh.triangle_count();
        let mut edges: HashMap<(u32, u32), Vec<(u32, u8)>> = HashMap::with_capacity(tris * 2);
        for t in 0..tris {
            let v = mesh.tri(t);
            for k in 0..3 {
                let (a, b) = (weld[v[(k + 1) % 3] as usize], weld[v[(k + 2) % 3] as usize]);
                if a == b {
                    continue;
                }
                edges.entry((a.min(b), a.max(b))).or_default().push((t as u32, k as u8));
            }
        }
        let mut adj = vec![[NONE; 3]; tris];
        for sides in edges.values() {
            if let [(t0, k0), (t1, k1)] = sides[..] {
                adj[t0 as usize][k0 as usize] = t1;
                adj[t1 as usize][k1 as usize] = t0;
            }
        }
        Self { mesh, adj }
    }

    fn corners(&self, t: u32) -> [Vec3; 3] {
        self.mesh.tri_positions(t as usize)
    }

    /// Walk `distance` along the surface from `p` on triangle `t` in tangent direction `dir`.
    /// Returns the end triangle, the end point and the direction of travel at the end (turned
    /// with the surface at every crossed edge). Stops early at boundaries.
    pub(crate) fn walk(&self, mut t: u32, mut p: Vec3, dir: Vec3, distance: f32) -> (u32, Vec3, Vec3) {
        let [a, b, c] = self.corners(t);
        let n = (b - a).cross(c - a).normalize_or_zero();
        let mut d = (dir - n * n.dot(dir)).normalize_or_zero();
        if d == Vec3::ZERO {
            return (t, p, dir);
        }
        let mut remaining = distance;
        for _ in 0..256 {
            let [a, b, c] = self.corners(t);
            let mut lp = barycentric(p, a, b, c).map(|x| x.max(0.0));
            let sum = lp[0] + lp[1] + lp[2];
            lp = lp.map(|x| x / sum.max(1e-12));
            p = a * lp[0] + b * lp[1] + c * lp[2];
            let q = p + d * remaining;
            let lq = barycentric(q, a, b, c);
            let mut exit: Option<(usize, f32)> = None;
            for k in 0..3 {
                if lq[k] < 0.0 {
                    let s = lp[k] / (lp[k] - lq[k]);
                    if exit.is_none_or(|(_, best)| s < best) {
                        exit = Some((k, s));
                    }
                }
            }
            let Some((k, s)) = exit else { return (t, q, d) };
            let s = s.clamp(0.0, 1.0);
            p += d * (remaining * s);
            remaining *= 1.0 - s;
            let next = self.adj[t as usize][k];
            if next == NONE || remaining <= 0.0 {
                return (t, p, d);
            }
            // Unfold: keep the component along the shared edge, send the rest into the next face.
            let corners = [a, b, c];
            let (e0, e1) = (corners[(k + 1) % 3], corners[(k + 2) % 3]);
            let e = (e1 - e0).normalize_or_zero();
            let along = d.dot(e);
            let across = (1.0 - along * along).max(0.0).sqrt();
            let back = self.adj[next as usize].iter().position(|&o| o == t).unwrap_or(0);
            let w = self.corners(next)[back];
            let inward = ((w - e0) - e * (w - e0).dot(e)).normalize_or_zero();
            d = (e * along + inward * across).normalize_or(e);
            t = next;
        }
        (t, p, d)
    }
}

/// Bilinear read of an image at `uv` (0..1, clamped to the edge texels).
pub(crate) fn sample_bilinear(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x = (uv.x * w - 0.5).clamp(0.0, w - 1.0);
    let y = (uv.y * h - 0.5).clamp(0.0, h - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (a, b, c, d) = (img.get(x0, y0), img.get(x1, y0), img.get(x0, y1), img.get(x1, y1));
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

/// World-space shading normal at barycentrics `(u, v)` of triangle `t`: the normal map at that
/// point's UV through that point's own MikkTSpace frame.
fn shaded_normal(mesh: &Mesh, map: &TiledImage, t: u32, u: f32, v: f32) -> Vec3 {
    let sp = mesh.interpolate(t as usize, u, v);
    let n = sp.normal.normalize_or(sp.geometric_normal);
    let tan = sp.tangent.truncate();
    let tan = (tan - n * n.dot(tan)).normalize_or(n.any_orthonormal_vector());
    let bit = n.cross(tan) * if sp.tangent.w < 0.0 { -1.0 } else { 1.0 };
    let m = sample_bilinear(map, sp.uv);
    let ts = Vec3::new(m[0] * 2.0 - 1.0, m[1] * 2.0 - 1.0, m[2] * 2.0 - 1.0);
    (tan * ts.x + bit * ts.y + n * ts.z).normalize_or(n)
}

fn bary_uv(mesh: &Mesh, t: u32, p: Vec3) -> (f32, f32) {
    let [a, b, c] = mesh.tri_positions(t as usize);
    let l = barycentric(p, a, b, c);
    (l[1], l[2])
}

/// World-space length of one texel of a `size`² map on each triangle (UV-area ratio).
fn texel_lengths(mesh: &Mesh, size: u32) -> Vec<f32> {
    let per: Vec<Option<f32>> = (0..mesh.triangle_count())
        .map(|t| {
            let [a, b, c] = mesh.tri_positions(t);
            let [ua, ub, uc] = mesh.tri_uvs(t);
            let area3 = (b - a).cross(c - a).length() * 0.5;
            let area2 = ((ub - ua).perp_dot(uc - ua)).abs() * 0.5 * (size as f32).powi(2);
            (area2 > 1e-12 && area3 > 0.0).then(|| (area3 / area2).sqrt())
        })
        .collect();
    let known: Vec<f32> = per.iter().flatten().copied().collect();
    let fallback = if known.is_empty() { mesh.bounds.diagonal() / size as f32 } else { known.iter().sum::<f32>() / known.len() as f32 };
    per.into_iter().map(|v| v.unwrap_or(fallback)).collect()
}

/// Curvature (0.5 flat) of one texel. `h` is the walk distance in world units.
fn texel_curvature(walker: &SurfaceWalker, map: &TiledImage, t: &Texel, h: f32, intensity: f32) -> f32 {
    let mesh = walker.mesh;
    // Rim texels lie just outside their triangle: start from the nearest point inside it.
    let [a, b, c] = mesh.tri_positions(t.tri as usize);
    let l = barycentric(t.position, a, b, c).map(|x| x.max(0.0));
    let s = (l[0] + l[1] + l[2]).max(1e-12);
    let (u, v) = (l[1] / s, l[2] / s);
    let p = a * (l[0] / s) + b * u + c * v;
    let n0 = shaded_normal(mesh, map, t.tri, u, v);
    let ng = (b - a).cross(c - a).normalize_or(t.geometric_normal);
    let tan = t.tangent.truncate();
    let tan = (tan - ng * ng.dot(tan)).normalize_or(ng.any_orthonormal_vector());
    let bit = ng.cross(tan);
    let mut sum = 0.0;
    for k in 0..8 {
        let angle = k as f32 * std::f32::consts::FRAC_PI_4;
        let d = tan * angle.cos() + bit * angle.sin();
        let (t2, p2, d2) = walker.walk(t.tri, p, d, h);
        if (p2 - p).length_squared() < (h * 1e-3).powi(2) {
            continue;
        }
        let (u2, v2) = bary_uv(mesh, t2, p2);
        let n2 = shaded_normal(mesh, map, t2, u2, v2);
        let along = (d + d2).normalize_or(d);
        sum += (n2 - n0).dot(along);
    }
    // Eight directions: an edge crossed straight on contributes once plus two diagonals (~2.4),
    // a sphere of radius R gives 8 h / R; halving puts a 90° edge at full scale.
    0.5 + 0.5 * (sum * 0.5 * intensity).clamp(-1.0, 1.0)
}

/// Curvature map of `material_slot` at `size`² from a tangent-space (OpenGL, +Y) normal map of
/// that texture set, measured along the low-poly surface (see the module docs). Gutters are
/// padded from the nearest covered texel.
pub fn curvature_from_normal_map(
    low: &Mesh, material_slot: u32, normal_map: &TiledImage, size: u32, settings: &NormalCurvatureSettings, progress: Option<&Progress>,
) -> Result<TiledImage, BakeError> {
    let samples = texels(low, material_slot, size);
    if samples.is_empty() {
        return Err(BakeError::NoTexels);
    }
    let walker = SurfaceWalker::new(low);
    let lengths = texel_lengths(low, size);
    let radius = settings.radius_texels.max(0.25);
    let mut values: Vec<f32> = Vec::with_capacity(samples.len());
    for start in (0..samples.len()).step_by(crate::CHUNK) {
        if progress.is_some_and(|p| p.is_cancelled()) {
            return Err(BakeError::Cancelled);
        }
        let end = (start + crate::CHUNK).min(samples.len());
        values.par_extend(samples[start..end].par_iter().map(|t| {
            texel_curvature(&walker, normal_map, t, radius * lengths[t.tri as usize], settings.intensity)
        }));
        if let Some(p) = progress {
            p.done.fetch_add(end - start, Ordering::Relaxed);
        }
    }
    let n = (size * size) as usize;
    let mut covered = vec![false; n];
    let mut dense = vec![0.5f32; n];
    for (t, v) in samples.iter().zip(&values) {
        covered[t.index as usize] = true;
        dense[t.index as usize] = *v;
    }
    let nearest = subcraft_image::dilate::nearest_covered(size as usize, size as usize, &covered);
    subcraft_image::dilate::apply_nearest(1, &mut dense, &covered, &nearest);
    let neutral = MeshMapKind::Curvature.neutral_value();
    Ok(TiledImage::from_dense(size, size, 1, [neutral[0], 0.0, 0.0, 0.0], &dense))
}

#[cfg(test)]
#[path = "normal_curvature_tests.rs"]
mod tests;
