//! Mesh-map baking (CPU, BVH ray casting, rayon).
//!
//! For every covered texel of a texture set the low-poly surface point is found by UV
//! rasterization. With a high-poly mesh, that point is projected onto the high poly along the
//! low-poly normal (cage: `front` above, `rear` below) and detail maps are taken there; without
//! one, the low poly is its own detail ("low on low": flat tangent normals). With
//! [`MatchMode::ByMeshName`] a low triangle only projects onto high triangles whose object has the
//! same base name (`Bolt_low` ↔ `Bolt_high`, see [`matching`]).
//!
//! Maps: tangent-space normal (OpenGL / MikkTSpace frame of the low poly), world-space normal,
//! position (normalized per axis to the low-poly bounds), ID (color per mesh element, material or
//! object name), ambient occlusion (cosine-weighted hemisphere rays), curvature (3D neighbourhood
//! convexity, 0.5 flat), thickness (rays into the surface; bright = thin), height (signed
//! high-poly offset along the low normal, 0.5 = on the low surface, ±cage = 1 / 0), bent normals
//! (world-space average unoccluded direction) and opacity (1 where the high poly was found).
//! Gutters are padded from the nearest island texel.
//!
//! [`bake_each`] hands every map over as soon as it is finished, so callers can show maps while
//! the rest are still baking.

pub mod curvature;
pub mod matching;
pub mod surface;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use glam::Vec3;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use subcraft_core::MeshMapKind;
use subcraft_core::color::hsv_to_rgb;
use subcraft_image::TiledImage;
use subcraft_image::dilate::dilate_infinite;
use subcraft_mesh::{Bvh, Mesh, topology};

use matching::{NameGroups, UNMATCHED, match_by_name};
use surface::{Texel, cosine_directions, hash01, texels};

/// Which high-poly triangles a low-poly texel may project onto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchMode {
    /// Any high-poly triangle (the nearest along the cage ray wins).
    #[default]
    Always,
    /// Only high-poly objects with the same base name (suffixes `_low` / `_high` removed).
    ByMeshName,
}

/// What the ID map colors by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdSource {
    /// Connected mesh elements (each loose piece gets a color).
    #[default]
    MeshElement,
    /// Material slot of the detail mesh.
    MaterialSlot,
    /// Object name (equal base names share a color).
    MeshObject,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BakeSettings {
    /// Output size (square).
    pub size: u32,
    pub maps: Vec<MeshMapKind>,
    pub ao_samples: u32,
    /// Occlusion ray length as a fraction of the mesh bounding-box diagonal.
    pub ao_distance: f32,
    /// AO / bent normals only see geometry of the same name group (Painter's "Self occlusion:
    /// Only same mesh name").
    pub ao_self_only: bool,
    pub thickness_samples: u32,
    pub thickness_distance: f32,
    /// Curvature neighbourhood radius as a fraction of the diagonal.
    pub curvature_radius: f32,
    /// High-poly projection cage above / below the low poly, fractions of the diagonal.
    pub cage_front: f32,
    pub cage_rear: f32,
    pub match_mode: MatchMode,
    /// Suffixes removed before names are compared (case-insensitive).
    pub low_suffix: String,
    pub high_suffix: String,
    pub id_source: IdSource,
}

impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            size: 1024,
            maps: MeshMapKind::ALL.to_vec(),
            ao_samples: 32,
            ao_distance: 0.25,
            ao_self_only: false,
            thickness_samples: 16,
            thickness_distance: 0.5,
            curvature_radius: 0.02,
            cage_front: 0.01,
            cage_rear: 0.01,
            match_mode: MatchMode::Always,
            low_suffix: "_low".into(),
            high_suffix: "_high".into(),
            id_source: IdSource::MeshElement,
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
    /// Map being baked: index into [`MeshMapKind::ALL`] + 1 (0 = none / projecting).
    current: AtomicUsize,
    map_done: AtomicUsize,
    map_total: AtomicUsize,
    cancel: AtomicBool,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed);
        if t == 0 { 0.0 } else { self.done.load(Ordering::Relaxed) as f32 / t as f32 }
    }
    /// The map being baked right now (`None` while projecting onto the high poly).
    pub fn current_map(&self) -> Option<MeshMapKind> {
        self.current.load(Ordering::Relaxed).checked_sub(1).and_then(|i| MeshMapKind::ALL.get(i).copied())
    }
    /// Progress of [`Self::current_map`], 0..1.
    pub fn map_fraction(&self) -> f32 {
        let t = self.map_total.load(Ordering::Relaxed);
        if t == 0 { 0.0 } else { self.map_done.load(Ordering::Relaxed) as f32 / t as f32 }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    fn set_current(&self, kind: Option<MeshMapKind>, total: usize) {
        let i = kind.and_then(|k| MeshMapKind::ALL.iter().position(|m| *m == k)).map_or(0, |i| i + 1);
        self.map_done.store(0, Ordering::Relaxed);
        self.map_total.store(total, Ordering::Relaxed);
        self.current.store(i, Ordering::Relaxed);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BakeError {
    Cancelled,
    NoTexels,
}

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BakeError::Cancelled => f.write_str("bake cancelled"),
            BakeError::NoTexels => f.write_str("texture set has no UV-covered texels"),
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
    tri: u32,
    /// Offset direction for ray origins (geometric normal).
    offset: Vec3,
    /// The projection found the high poly (always true when baking low on low).
    hit: bool,
    /// Signed distance from the low surface to the detail point along the low normal.
    height: f32,
}

const CHUNK: usize = 2048;

/// Bake the requested maps for one texture set.
pub fn bake(input: &BakeInput, settings: &BakeSettings, progress: &Progress) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let mut out = BTreeMap::new();
    bake_each(input, settings, progress, |kind, img| {
        out.insert(kind, img);
    })?;
    Ok(out)
}

/// Name groups used by a bake: projection filter (high poly + by name) and / or AO self occlusion.
pub fn name_groups(input: &BakeInput, settings: &BakeSettings) -> Option<NameGroups> {
    let by_name = settings.match_mode == MatchMode::ByMeshName && input.high.is_some();
    if !by_name && !settings.ao_self_only {
        return None;
    }
    Some(match input.high {
        Some(high) => match_by_name(input.low.mesh, high.mesh, &settings.low_suffix, &settings.high_suffix),
        None => match_by_name(input.low.mesh, input.low.mesh, &settings.low_suffix, &settings.low_suffix),
    })
}

/// Bake the requested maps for one texture set, in `settings.maps` order, handing each finished
/// map to `emit` immediately.
pub fn bake_each(
    input: &BakeInput,
    settings: &BakeSettings,
    progress: &Progress,
    mut emit: impl FnMut(MeshMapKind, TiledImage),
) -> Result<(), BakeError> {
    let size = settings.size;
    let low = input.low.mesh;
    let samples = texels(low, input.material_slot, size);
    if samples.is_empty() {
        return Err(BakeError::NoTexels);
    }
    let detail_geo = input.high.unwrap_or(input.low);
    let diag = low.bounds.diagonal().max(detail_geo.mesh.bounds.diagonal()).max(1e-6);
    let bias = diag * 2e-4;
    let front = settings.cage_front * diag;
    let rear = settings.cage_rear * diag;
    let groups = name_groups(input, settings);
    let by_name = settings.match_mode == MatchMode::ByMeshName && input.high.is_some();

    let work_items = settings.maps.len().max(1) * samples.len();
    progress.total.store(work_items, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    progress.set_current(None, samples.len());

    // Project onto the detail surface.
    let project = |t: &Texel| -> Detail {
        let own = Detail { position: t.position, normal: t.normal, tri: t.tri, offset: t.geometric_normal, hit: true, height: 0.0 };
        let Some(high) = input.high else { return own };
        let n = t.normal.normalize_or(t.geometric_normal);
        let hit = match groups.as_ref().filter(|_| by_name) {
            Some(g) => {
                let group = g.low[t.tri as usize];
                if group == UNMATCHED {
                    None
                } else {
                    high.bvh.intersect_filtered(t.position + n * front, -n, 0.0, front + rear, |h| g.high[h as usize] == group)
                }
            }
            None => high.bvh.intersect(t.position + n * front, -n, 0.0, front + rear),
        };
        match hit {
            Some(hit) => {
                let sp = high.mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let ng = sp.geometric_normal;
                let normal = if sp.normal.dot(n) < 0.0 { -sp.normal } else { sp.normal };
                Detail {
                    position: sp.position,
                    normal,
                    tri: hit.tri,
                    offset: if ng.dot(n) < 0.0 { -ng } else { ng },
                    hit: true,
                    height: front - hit.t,
                }
            }
            None => Detail { hit: false, ..own },
        }
    };

    let mut details: Vec<Detail> = Vec::with_capacity(samples.len());
    for start in (0..samples.len()).step_by(CHUNK * 4) {
        if progress.is_cancelled() {
            return Err(BakeError::Cancelled);
        }
        let end = (start + CHUNK * 4).min(samples.len());
        details.par_extend(samples[start..end].par_iter().map(project));
        progress.map_done.fetch_add(end - start, Ordering::Relaxed);
    }

    let wanted = |k: MeshMapKind| settings.maps.contains(&k);
    let elements = if wanted(MeshMapKind::Id) && settings.id_source == IdSource::MeshElement {
        Some(topology::mesh_elements(detail_geo.mesh))
    } else {
        None
    };
    // Hue per detail triangle for the per-object ID map (texels missing the high poly color by
    // their low object; those names reduce to the same base, so the colors agree).
    let object_hue = |mesh: &Mesh, tri: u32, suffix: &str| matching::name_hue(&matching::triangle_base_name(mesh, tri as usize, suffix));
    let bounds = low.bounds;
    let extent = bounds.extent().max(Vec3::splat(1e-6));
    let height_range = front.max(rear).max(1e-9);
    // Rays for AO / bent normals: optionally only against the texel's own name group.
    let self_group = |t: &Texel| groups.as_ref().filter(|_| settings.ao_self_only).map(|g| g.low[t.tri as usize]);
    let occluded = |t: &Texel, origin: Vec3, dir: Vec3, dist: f32| match self_group(t) {
        Some(group) => {
            let g = groups.as_ref().expect("self group implies groups");
            detail_geo.bvh.intersect_filtered(origin, dir, 0.0, dist, |h| g.high[h as usize] == group).is_some()
        }
        None => detail_geo.bvh.occluded(origin, dir, 0.0, dist),
    };

    for &kind in &settings.maps {
        let comps = kind.components();
        progress.set_current(Some(kind), samples.len());
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
                    let hue = match settings.id_source {
                        IdSource::MeshElement => {
                            let e = elements.as_ref().map_or(0, |e| e[d.tri as usize]);
                            (e as f32 * 0.618_034).fract()
                        }
                        IdSource::MaterialSlot => {
                            let mesh = if d.hit { detail_geo.mesh } else { low };
                            let m = mesh.tri_material.get(d.tri as usize).copied().unwrap_or(0);
                            (m as f32 * 0.618_034 + 0.1).fract()
                        }
                        IdSource::MeshObject => match input.high {
                            Some(high) if d.hit => object_hue(high.mesh, d.tri, &settings.high_suffix),
                            _ => object_hue(low, t.tri, &settings.low_suffix),
                        },
                    };
                    hsv_to_rgb([hue, 0.65, 0.9])
                }
                MeshMapKind::AmbientOcclusion | MeshMapKind::BentNormal => {
                    let mut dirs = Vec::with_capacity(settings.ao_samples as usize);
                    cosine_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 1), &mut dirs);
                    let origin = d.position + d.offset * bias;
                    let dist = settings.ao_distance * diag;
                    let (mut occluded_count, mut counted) = (0u32, 0u32);
                    let mut open = Vec3::ZERO;
                    for dir in &dirs {
                        if dir.dot(d.offset) <= 0.0 {
                            continue;
                        }
                        counted += 1;
                        if occluded(t, origin, *dir, dist) {
                            occluded_count += 1;
                        } else {
                            open += *dir;
                        }
                    }
                    if kind == MeshMapKind::AmbientOcclusion {
                        let ao = if counted == 0 { 1.0 } else { 1.0 - occluded_count as f32 / counted as f32 };
                        [ao; 3]
                    } else {
                        let b = open.normalize_or(d.normal);
                        [b.x * 0.5 + 0.5, b.y * 0.5 + 0.5, b.z * 0.5 + 0.5]
                    }
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
                    [curvature::curvature_at(detail_geo.mesh, detail_geo.bvh, d.position, d.normal, settings.curvature_radius * diag); 3]
                }
                MeshMapKind::Height => [(0.5 + 0.5 * d.height / height_range).clamp(0.0, 1.0); 3],
                MeshMapKind::Opacity => [if d.hit { 1.0 } else { 0.0 }; 3],
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
            progress.map_done.fetch_add(end - start, Ordering::Relaxed);
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
        emit(kind, TiledImage::from_dense(size, size, comps, fill, &dense));
    }
    progress.set_current(None, 0);
    Ok(())
}

#[cfg(test)]
mod tests;
