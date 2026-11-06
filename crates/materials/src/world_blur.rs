//! World-space blur: a Gaussian whose radius is a distance over the mesh surface (in the mesh's
//! own units), not a number of texture pixels. Where the UVs stretch or squash, a pixel blur is
//! uneven (wide on one side of a seam, narrow on the other); this one is even everywhere.
//!
//! How it works. Every covered texel of one texture set becomes a surface sample (object-space
//! position, normal, and the world area one texel stands for). Samples go into a uniform 3D grid
//! with cells one radius wide; each output texel sums the samples within the radius with
//! `gaussian(distance) * max(0, n_a . n_b) * area`, so
//!
//! * UV islands are not special: samples on the other side of a seam are neighbours when they are
//!   close in 3D;
//! * a texel stands for its area, so densely and sparsely mapped regions count in proportion to
//!   the surface they cover (no bias toward stretched-small UV areas);
//! * the normal gate (cosine, 0 at 90 degrees) keeps the blur from crossing hard edges and from
//!   leaking through thin parts to the opposite face.
//!
//! Euclidean distance stands in for the distance along the surface, which is exact on flat areas
//! and a slight underestimate around curvature. Colours are blurred premultiplied by their
//! coverage (the last component of a two- or four-component raster), so painted and unpainted
//! areas do not bleed dark or bright halos into each other.

use std::collections::HashMap;

use glam::Vec3;
use rayon::prelude::*;
use subcraft_image::TILE_TEXELS;

use crate::sample::TextureSetGeometry;

/// A covered texel: where it is on the model and how much surface it stands for.
struct Sample {
    x: u32,
    y: u32,
    pos: Vec3,
    normal: Vec3,
    area: f32,
}

/// Gaussian support as a multiple of sigma (the radius is where the weight has fallen to
/// `exp(-4.5)`, as for the pixel Blur filter's support radius).
const SIGMAS: f32 = 3.0;

/// Blur the dense raster `data` (`resolution * resolution * comps` floats, row-major, as made by
/// `TiledImage::to_dense`) of the texture set described by `geo`. `radius` is in mesh units.
/// Uncovered texels (gutters) are re-padded from the nearest blurred texel afterwards.
pub fn world_blur(geo: &TextureSetGeometry, data: &[f32], comps: usize, radius: f32) -> Vec<f32> {
    let res = geo.resolution as usize;
    assert_eq!(data.len(), res * res * comps, "raster does not match the texture set");
    if !(radius.is_finite() && radius > 0.0) {
        return data.to_vec();
    }
    // Everything below is in normalized units (the longest model dimension is 1).
    let r = (radius / geo.max_extent.max(1e-12)).max(1e-9);
    let samples = collect(geo);
    if samples.is_empty() {
        return data.to_vec();
    }
    let mut grid: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
    for (i, s) in samples.iter().enumerate() {
        grid.entry(cell(s.pos, r)).or_default().push(i as u32);
    }
    let has_alpha = comps == 2 || comps == 4;
    let sigma = r / SIGMAS;
    let inv_two_sigma_sq = 1.0 / (2.0 * sigma * sigma);
    let r_sq = r * r;
    let out: Vec<(usize, Vec<f32>)> = samples
        .par_iter()
        .map(|s| {
            let mut sum = vec![0.0f64; comps];
            let mut alpha_sum = 0.0f64;
            let mut weight_sum = 0.0f64;
            let (cx, cy, cz) = cell(s.pos, r);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let Some(bucket) = grid.get(&(cx + dx, cy + dy, cz + dz)) else { continue };
                        for &j in bucket {
                            let t = &samples[j as usize];
                            let d_sq = s.pos.distance_squared(t.pos);
                            if d_sq > r_sq { continue; }
                            let gate = s.normal.dot(t.normal).max(0.0);
                            if gate <= 0.0 { continue; }
                            let w = ((-d_sq * inv_two_sigma_sq).exp() * gate * t.area) as f64;
                            let at = (t.y as usize * res + t.x as usize) * comps;
                            let v = &data[at..at + comps];
                            if has_alpha {
                                let a = v[comps - 1] as f64;
                                for c in 0..comps - 1 { sum[c] += w * a * v[c] as f64; }
                                alpha_sum += w * a;
                                sum[comps - 1] += w * a;
                            } else {
                                for c in 0..comps { sum[c] += w * v[c] as f64; }
                            }
                            weight_sum += w;
                        }
                    }
                }
            }
            let at = (s.y as usize * res + s.x as usize) * comps;
            let mut px = data[at..at + comps].to_vec();
            if weight_sum > 0.0 {
                if has_alpha {
                    let alpha = (alpha_sum / weight_sum) as f32;
                    for c in 0..comps - 1 {
                        px[c] = if alpha_sum > 1e-12 { (sum[c] / alpha_sum) as f32 } else { data[at + c] };
                    }
                    px[comps - 1] = alpha;
                } else {
                    for c in 0..comps { px[c] = (sum[c] / weight_sum) as f32; }
                }
            }
            (at, px)
        })
        .collect();
    let mut result = data.to_vec();
    let mut covered = vec![false; res * res];
    for (at, px) in out {
        result[at..at + comps].copy_from_slice(&px);
        covered[at / comps] = true;
    }
    let nearest = subcraft_image::dilate::nearest_covered(res, res, &covered);
    subcraft_image::dilate::apply_nearest(comps, &mut result, &covered, &nearest);
    result
}

fn cell(p: Vec3, size: f32) -> (i32, i32, i32) {
    ((p.x / size).floor() as i32, (p.y / size).floor() as i32, (p.z / size).floor() as i32)
}

/// Every covered texel of the set with its world area (normalized units squared): its
/// triangle's area divided by how many texels that triangle covers in UV.
fn collect(geo: &TextureSetGeometry) -> Vec<Sample> {
    let mesh = geo.mesh();
    let res = geo.resolution;
    let texels = (res as f32) * (res as f32);
    let triangle_area = |t: usize| -> f32 {
        let i = &mesh.indices[t * 3..t * 3 + 3];
        let p = |k: usize| geo.normalize_position(mesh.positions[i[k] as usize]);
        let (a, b, c) = (p(0), p(1), p(2));
        let world = 0.5 * (b - a).cross(c - a).length();
        let uv = |k: usize| mesh.uvs[i[k] as usize];
        let (u0, u1, u2) = (uv(0), uv(1), uv(2));
        let uv_texels = 0.5 * ((u1 - u0).perp_dot(u2 - u0)).abs() * texels;
        if uv_texels > 1e-3 { world / uv_texels } else { 0.0 }
    };
    let mut per_triangle: HashMap<usize, f32> = HashMap::new();
    let mut out = Vec::new();
    for t in 0..geo.tile_count() {
        let tile = geo.tile(t);
        if tile.is_empty() { continue; }
        for i in 0..TILE_TEXELS {
            if tile.coverage(i) <= 0.0 { continue; }
            let (x, y) = tile.texel_xy(i);
            if x >= res || y >= res { continue; }
            let Some(tri) = tile.triangle(i) else { continue };
            let area = *per_triangle.entry(tri).or_insert_with(|| triangle_area(tri));
            let n = Vec3::from_array(tile.normal(i));
            out.push(Sample { x, y, pos: Vec3::from_array(tile.position(i)), normal: n.normalize_or_zero(), area: if area > 0.0 { area } else { 1e-12 } });
        }
    }
    out
}

#[cfg(test)]
mod tests;
