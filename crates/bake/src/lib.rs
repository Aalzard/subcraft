//! Mesh-map baking (CPU, BVH ray casting, rayon).
//!
//! For every covered texel of a texture set the low-poly surface point is found by UV
//! rasterization. With a high-poly mesh, that point is projected onto the high poly along the
//! low-poly normal (cage: `front` above, `rear` below) and detail maps are taken there; without
//! one, the low poly is its own detail ("low on low": flat tangent normals).
//!
//! Maps: tangent-space normal (OpenGL / MikkTSpace frame of the low poly), world-space normal,
//! position (normalized per axis to the low-poly bounds), ID (color per mesh element), ambient
//! occlusion (cosine-weighted hemisphere rays), curvature (3D neighbourhood convexity, 0.5 flat)
//! and thickness (rays into the surface; bright = thin). Gutters are padded from the nearest
//! island texel.

pub mod curvature;
pub mod surface;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use glam::Vec3;
use rayon::prelude::*;
use subcraft_core::MeshMapKind;
use subcraft_core::color::hsv_to_rgb;
use subcraft_image::TiledImage;
use subcraft_image::dilate::dilate_infinite;
use subcraft_mesh::{Bvh, Mesh, topology};

use surface::{Texel, cosine_directions, hash01, texels};

#[derive(Clone, Debug, PartialEq)]
pub struct BakeSettings {
    /// Output size (square).
    pub size: u32,
    pub maps: Vec<MeshMapKind>,
    pub ao_samples: u32,
    /// Occlusion ray length as a fraction of the mesh bounding-box diagonal.
    pub ao_distance: f32,
    pub thickness_samples: u32,
    pub thickness_distance: f32,
    /// Curvature neighbourhood radius as a fraction of the diagonal.
    pub curvature_radius: f32,
    /// High-poly projection cage above / below the low poly, fractions of the diagonal.
    pub cage_front: f32,
    pub cage_rear: f32,
}

impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            size: 1024,
            maps: MeshMapKind::ALL.to_vec(),
            ao_samples: 32,
            ao_distance: 0.25,
            thickness_samples: 16,
            thickness_distance: 0.5,
            curvature_radius: 0.02,
            cage_front: 0.01,
            cage_rear: 0.01,
        }
    }
}

/// A mesh with its BVH.
#[derive(Clone, Copy)]
pub struct Geometry<'a> {
    pub mesh: &'a Mesh,
    pub bvh: &'a Bvh,
}

pub struct BakeInput<'a> {
    pub low: Geometry<'a>,
    pub high: Option<Geometry<'a>>,
    pub material_slot: u32,
}

/// Shared progress / cancellation for a bake running on another thread.
#[derive(Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    cancel: AtomicBool,
    current_map: AtomicUsize,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed);
        if t == 0 { 0.0 } else { self.done.load(Ordering::Relaxed) as f32 / t as f32 }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn current_map(&self) -> Option<MeshMapKind> {
        self.current_map.load(Ordering::Relaxed).checked_sub(1).and_then(|i| MeshMapKind::ALL.get(i).copied())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BakeError {
    Cancelled,
    NoTexels,
    NoProjection,
}

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BakeError::Cancelled => f.write_str("bake cancelled"),
            BakeError::NoTexels => f.write_str("texture set has no UV-covered texels"),
            BakeError::NoProjection => f.write_str("no high-poly surface was reached: align the low/high models in the same coordinates and increase Cage front / rear"),
        }
    }
}

impl std::error::Error for BakeError {}

/// Where detail maps are sampled for one texel.
#[derive(Clone, Copy)]
struct Detail {
    position: Vec3,
    normal: Vec3,
    /// Triangle in the detail mesh.
    tri: Option<u32>,
    /// Offset direction for ray origins (geometric normal).
    offset: Vec3,
}

const CHUNK: usize = 2048;

/// Bake the requested maps for one texture set.
pub fn bake(input: &BakeInput, settings: &BakeSettings, progress: &Progress) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    bake_streamed(input, settings, progress, |_, _| {})
}

/// Publish each complete, padded map as soon as it is ready. The callback runs on the calling
/// thread; synchronous callers keep the same exact output through [`bake`]. No partial map is
/// published, including on cancellation. Cloning an image here only shares its immutable tiles.
pub fn bake_streamed(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress,
    mut completed: impl FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let size = settings.size;
    progress.current_map.store(0, Ordering::Relaxed);
    let low = input.low.mesh;
    let samples = texels(low, input.material_slot, size);
    if samples.is_empty() {
        return Err(BakeError::NoTexels);
    }
    let detail_geo = input.high.unwrap_or(input.low);
    let diag = low.bounds.diagonal().max(detail_geo.mesh.bounds.diagonal()).max(1e-6);
    let bias = diag * 2e-4;

    // Project onto the detail surface.
    let project = |t: &Texel| -> Detail {
        if let Some(high) = input.high {
            let front = settings.cage_front * diag;
            let rear = settings.cage_rear * diag;
            let n = t.normal.normalize_or(t.geometric_normal);
            if let Some(hit) = high.bvh.intersect(t.position + n * front, -n, 0.0, front + rear) {
                let sp = high.mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let ng = sp.geometric_normal;
                let normal = if sp.normal.dot(n) < 0.0 { -sp.normal } else { sp.normal };
                return Detail { position: sp.position, normal, tri: Some(hit.tri), offset: if ng.dot(n) < 0.0 { -ng } else { ng } };
            }
        }
        Detail { position: t.position, normal: t.normal, tri: input.high.is_none().then_some(t.tri), offset: t.geometric_normal }
    };

    let wanted = |k: MeshMapKind| settings.maps.contains(&k);
    let work_items = settings.maps.len().max(1) * samples.len();
    progress.total.store(work_items, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);

    let details: Vec<Detail> = samples.par_iter().map(project).collect();
    if input.high.is_some() && details.iter().all(|d| d.tri.is_none()) {
        return Err(BakeError::NoProjection);
    }
    let elements = if wanted(MeshMapKind::Id) || wanted(MeshMapKind::Curvature) { Some(topology::mesh_elements(detail_geo.mesh)) } else { None };
    let bounds = low.bounds;
    let extent = bounds.extent().max(Vec3::splat(1e-6));

    let mut out = BTreeMap::new();
    for &kind in &settings.maps {
        progress.current_map.store(MeshMapKind::ALL.iter().position(|k| *k == kind).expect("known map") + 1, Ordering::Relaxed);
        let comps = kind.components();
        // Per-texel evaluation, chunked for cancellation and progress.
        let eval = |i: usize| -> [f32; 3] {
            let t = &samples[i];
            let d = &details[i];
            match kind {
                MeshMapKind::Normal => {
                    let n = t.normal.normalize_or(t.geometric_normal);
                    let tan = (t.tangent.truncate() - n * n.dot(t.tangent.truncate())).normalize_or(n.any_orthonormal_vector());
                    let bit = n.cross(tan) * if t.tangent.w < 0.0 { -1.0 } else { 1.0 };
                    let h = d.normal;
                    let ts = Vec3::new(h.dot(tan), h.dot(bit), h.dot(n)).normalize_or(Vec3::Z);
                    [ts.x * 0.5 + 0.5, ts.y * 0.5 + 0.5, ts.z * 0.5 + 0.5]
                }
                MeshMapKind::WorldSpaceNormal => {
                    let n = d.normal;
                    [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]
                }
                MeshMapKind::Position => {
                    let p = ((t.position - bounds.min) / extent).clamp(Vec3::ZERO, Vec3::ONE);
                    [p.x, p.y, p.z]
                }
                MeshMapKind::Id => {
                    let Some(tri) = d.tri else { return kind.neutral_value()[..3].try_into().expect("RGB ID"); };
                    let e = elements.as_ref().map_or(0, |e| e[tri as usize]);
                    let hue = (e as f32 * 0.618_034).fract();
                    hsv_to_rgb([hue, 0.65, 0.9])
                }
                MeshMapKind::AmbientOcclusion => {
                    let mut dirs = Vec::with_capacity(settings.ao_samples as usize);
                    cosine_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 1), &mut dirs);
                    let origin = d.position + d.offset * bias;
                    let dist = settings.ao_distance * diag;
                    let (mut occluded, mut counted) = (0u32, 0u32);
                    for dir in &dirs {
                        if dir.dot(d.offset) <= 0.0 {
                            continue;
                        }
                        counted += 1;
                        if detail_geo.bvh.occluded(origin, *dir, 0.0, dist) {
                            occluded += 1;
                        }
                    }
                    let ao = if counted == 0 { 1.0 } else { 1.0 - occluded as f32 / counted as f32 };
                    [ao; 3]
                }
                MeshMapKind::Thickness => {
                    let mut dirs = Vec::with_capacity(settings.thickness_samples as usize);
                    cosine_directions(-d.normal, settings.thickness_samples.max(1), hash01(t.index, 2), &mut dirs);
                    let origin = d.position - d.offset * bias;
                    let max = settings.thickness_distance * diag;
                    let total: f32 = dirs.iter().map(|dir| detail_geo.bvh.intersect(origin, *dir, 0.0, max).map_or(max, |h| h.t)).sum();
                    let avg = total / dirs.len() as f32 / max;
                    [1.0 - avg; 3]
                }
                MeshMapKind::Curvature => {
                    [curvature::curvature_on_surface(detail_geo.mesh, detail_geo.bvh, d.position, d.normal,
                        settings.curvature_radius * diag, d.tri.zip(elements.as_deref())); 3]
                }
            }
        };
        let mut values: Vec<[f32; 3]> = Vec::with_capacity(samples.len());
        for start in (0..samples.len()).step_by(CHUNK) {
            if progress.is_cancelled() {
                return Err(BakeError::Cancelled);
            }
            let end = (start + CHUNK).min(samples.len());
            values.par_extend((start..end).into_par_iter().map(eval));
            progress.done.fetch_add(end - start, Ordering::Relaxed);
        }
        let n = (size * size) as usize;
        let neutral = kind.neutral_value();
        let mut dense: Vec<f32> = (0..n).flat_map(|_| neutral[..comps].to_vec()).collect();
        let mut covered = vec![false; n];
        for (t, v) in samples.iter().zip(&values) {
            let i = t.index as usize;
            dense[i * comps..(i + 1) * comps].copy_from_slice(&v[..comps]);
            covered[i] = true;
        }
        dilate_infinite(size as usize, size as usize, comps, &mut dense, &covered);
        let mut fill = [0.0f32; 4];
        fill[..comps].copy_from_slice(&neutral[..comps]);
        let image = TiledImage::from_dense(size, size, comps, fill, &dense);
        if progress.is_cancelled() { return Err(BakeError::Cancelled); }
        completed(kind, &image);
        out.insert(kind, image);
    }
    progress.current_map.store(0, Ordering::Relaxed);
    Ok(out)
}

#[cfg(test)]
mod tests;
