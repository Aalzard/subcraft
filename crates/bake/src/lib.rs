//! Mesh-map baking (CPU, BVH ray casting, rayon).
//!
//! For every covered texel of a texture set the low-poly surface point is found by UV
//! rasterization. With a high-poly mesh, that point is projected onto the high poly along the
//! low-poly normal (cage: `front` above, `rear` below) and detail maps are taken there; without
//! one, the low poly is its own detail ("low on low": flat tangent normals).
//!
//! Maps: tangent-space normal (OpenGL / MikkTSpace frame of the low poly), world-space normal,
//! position (normalized per axis to the low-poly bounds), ID (color per mesh element), ambient
//! occlusion (cosine-weighted hemisphere rays), curvature (3D neighbourhood convexity, 0.5 flat),
//! thickness (rays into the surface; bright = thin), height (signed distance to the high poly
//! inside the cage, 0.5 flat) and bent normal (average open direction, tangent space). Gutters are padded from the nearest
//! island texel up to the dilation width and diffused beyond it. Optional: supersampling
//! ([`Antialiasing`]), averaged cage normals, skipping back faces, a custom cage mesh.
//!
//! With [`subcraft_core::BakeMatchMode::ByName`], every ray against the high poly (projection,
//! occlusion, bent normal, thickness) only accepts high triangles whose part matches the low
//! part by name ([`name_match`]); the shared high-poly BVH is traversed with a triangle filter.

pub mod curvature;
pub mod name_match;
pub mod normal_curvature;
pub mod seam_check;
pub mod surface;

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use glam::Vec3;
use rayon::prelude::*;
use subcraft_core::MeshMapKind;
use subcraft_core::color::hsv_to_rgb;
use subcraft_image::TiledImage;
use subcraft_mesh::ray_batch::{BatchControl, CurvatureSurface, CurvatureTexel, NO_GROUP, RayBatchBackend, RayBatchError, RayMode, RayTexel, ray_backend};
use subcraft_mesh::{Bvh, Mesh, topology};

use name_match::PartMatch;
use surface::{Texel, cone_directions, cosine_directions, hash01, texels};

/// Where the ID map takes its colours from (issue #117). The colours come from the mesh the bake
/// reads its detail from: the high poly when there is one, else the low poly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IdSource {
    /// The file's vertex colours (polypaint), interpolated over each triangle.
    VertexColor,
    /// The file's colour of each material (diffuse / base colour).
    MaterialColor,
    /// One generated colour per connected mesh element (polygroup-like): the default.
    #[default]
    MeshId,
    /// One generated colour per named object of the file (per material when it names none).
    Submesh,
}

impl IdSource {
    pub const ALL: [IdSource; 4] = [IdSource::VertexColor, IdSource::MaterialColor, IdSource::MeshId, IdSource::Submesh];
}

/// How occlusion rays are spread over their cone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RayDistribution {
    /// More rays near the normal (the physically weighted choice).
    #[default]
    Cosine,
    /// The same number of rays in every direction of the cone.
    Uniform,
}

impl RayDistribution {
    pub const ALL: [RayDistribution; 2] = [RayDistribution::Cosine, RayDistribution::Uniform];
    pub fn label(self) -> &'static str {
        match self {
            RayDistribution::Cosine => "Cosine",
            RayDistribution::Uniform => "Uniform",
        }
    }
}

/// How much an occluder counts with its distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AoAttenuation {
    /// Any hit within the distance occludes fully.
    #[default]
    None,
    /// Occlusion fades smoothly to nothing at the maximum distance.
    Smooth,
    /// Occlusion fades linearly to nothing at the maximum distance.
    Linear,
}

impl AoAttenuation {
    pub const ALL: [AoAttenuation; 3] = [AoAttenuation::None, AoAttenuation::Smooth, AoAttenuation::Linear];
    pub fn label(self) -> &'static str {
        match self {
            AoAttenuation::None => "None",
            AoAttenuation::Smooth => "Smooth",
            AoAttenuation::Linear => "Linear",
        }
    }
    /// Weight of a hit at `t` of `max`.
    fn weight(self, t: f32, max: f32) -> f32 {
        let x = (1.0 - t / max.max(1e-9)).clamp(0.0, 1.0);
        match self {
            AoAttenuation::None => 1.0,
            AoAttenuation::Linear => x,
            AoAttenuation::Smooth => x * x * (3.0 - 2.0 * x),
        }
    }
}

/// Options of the occlusion rays (ambient occlusion; the ray cone and distances also shape the
/// bent normals). The default is the plain cosine hemisphere, which may run on the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AoOptions {
    /// Occluders nearer than this are ignored, as a fraction of the diagonal.
    pub min_distance: f32,
    /// Full opening angle of the ray cone in degrees (180 = the whole hemisphere).
    pub spread_angle: f32,
    pub distribution: RayDistribution,
    pub attenuation: AoAttenuation,
    /// A floor under the model occludes too (Y up).
    pub ground_plane: bool,
    /// How far the floor lies under the model's lowest point, as a fraction of the diagonal.
    pub ground_offset: f32,
}

impl Default for AoOptions {
    fn default() -> Self {
        Self { min_distance: 0.0, spread_angle: 180.0, distribution: RayDistribution::Cosine, attenuation: AoAttenuation::None, ground_plane: false, ground_offset: 0.0 }
    }
}

impl AoOptions {
    /// The plain cosine hemisphere (what the GPU kernels and the fast CPU path compute).
    pub fn is_plain(&self) -> bool {
        self.min_distance <= 0.0 && self.spread_angle >= 180.0 && self.distribution == RayDistribution::Cosine && self.attenuation == AoAttenuation::None && !self.ground_plane
    }
}

/// What the position map is normalized to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PositionNormalization {
    /// Each axis over the model's bounding box.
    #[default]
    BoundingBox,
    /// All axes over the sphere around the bounding box (proportions are kept).
    BoundingSphere,
}

impl PositionNormalization {
    pub const ALL: [PositionNormalization; 2] = [PositionNormalization::BoundingBox, PositionNormalization::BoundingSphere];
    pub fn label(self) -> &'static str {
        match self {
            PositionNormalization::BoundingBox => "Bounding box",
            PositionNormalization::BoundingSphere => "Bounding sphere",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PositionOptions {
    /// `Some(0 | 1 | 2)`: a grayscale map of X, Y or Z alone; `None`: X, Y, Z in red, green, blue.
    pub single_axis: Option<u8>,
    pub normalization: PositionNormalization,
}

/// What the height map's full range stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum HeightNormalization {
    /// White at the cage's front reach, black at its rear reach.
    #[default]
    Cage,
    /// White / black at this distance above / below the low poly, as a fraction of the diagonal.
    Manual { distance: f32 },
}

/// How generated ID colours (one per mesh element or object) are picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IdGenerator {
    /// Unrelated colours.
    Random,
    /// Saturated colours stepping around the hue wheel.
    #[default]
    HueShift,
    /// Gray levels.
    Grayscale,
}

impl IdGenerator {
    pub const ALL: [IdGenerator; 3] = [IdGenerator::Random, IdGenerator::HueShift, IdGenerator::Grayscale];
    pub fn label(self) -> &'static str {
        match self {
            IdGenerator::Random => "Random",
            IdGenerator::HueShift => "Hue shift",
            IdGenerator::Grayscale => "Grayscale",
        }
    }
    /// The colour of element / object `i`. Never black, the ID map's "nothing here".
    pub fn color(self, i: u32) -> [f32; 3] {
        match self {
            IdGenerator::Random => [1u32, 2, 3].map(|c| 0.15 + 0.85 * hash01(i, 100 + c)),
            IdGenerator::HueShift => hsv_to_rgb([(i as f32 * 0.618_034).fract(), 0.65, 0.9]),
            IdGenerator::Grayscale => [0.1 + 0.9 * (i as f32 * 0.618_034).fract(); 3],
        }
    }
}

/// Supersampling of the whole bake: each output texel averages this many texels per side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Antialiasing {
    #[default]
    None,
    X2,
    X4,
    X8,
}

impl Antialiasing {
    pub const ALL: [Antialiasing; 4] = [Antialiasing::None, Antialiasing::X2, Antialiasing::X4, Antialiasing::X8];
    /// Texels baked per output texel, per side.
    pub fn factor(self) -> u32 {
        match self {
            Antialiasing::None => 1,
            Antialiasing::X2 => 2,
            Antialiasing::X4 => 4,
            Antialiasing::X8 => 8,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Antialiasing::None => "None",
            Antialiasing::X2 => "Subsampling 2x2",
            Antialiasing::X4 => "Subsampling 4x4",
            Antialiasing::X8 => "Subsampling 8x8",
        }
    }
}

/// The largest size a supersampled bake works at; a higher antialiasing is reduced to fit.
pub const MAX_SUPERSAMPLED_SIZE: u32 = 8192;

/// A custom cage: the low poly pushed outwards by the artist, with the low poly's UVs. Each
/// projection ray starts on the cage and runs through the low-poly point with the same UV.
/// Two cages are equal when they are the same loaded mesh.
#[derive(Clone)]
pub struct CageMesh(pub std::sync::Arc<Mesh>);

impl PartialEq for CageMesh {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for CageMesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CageMesh({:?}, {} triangles)", self.0.name, self.0.triangle_count())
    }
}

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
    /// Cast AO and thickness rays on the GPU when a ray backend is registered (falls back to the
    /// CPU on any error). The CPU result stays the reference.
    pub use_gpu: bool,
    /// Which high-poly parts rays from each low-poly part may hit (see [`name_match`]).
    pub name_match: subcraft_core::BakeMatch,
    /// Transform put on the loaded high poly before baking (identity = as imported).
    pub placement: subcraft_core::BakePlacement,
    /// Derive curvature from the tangent-space normal map of this bake (measured along the
    /// surface, see [`normal_curvature`]) instead of from the detail geometry.
    pub curvature_from_normal: bool,
    pub normal_curvature: normal_curvature::NormalCurvatureSettings,
    /// Where the ID map's colours come from.
    pub id_source: IdSource,
    /// Padding: gutter texels up to this many texels from an island copy the nearest island texel.
    pub dilation: u32,
    /// Fill the gutters beyond the dilation with a smooth blend of the padded islands
    /// (off: they keep the map's neutral value).
    pub diffusion: bool,
    pub antialiasing: Antialiasing,
    /// Cast the projection rays along normals averaged over every face at a position, so hard
    /// edges and UV seams of the low poly leave no gap between neighbouring faces' rays. The
    /// baked tangent frame stays the low poly's own.
    pub average_normals: bool,
    /// Projection rays skip high-poly faces that face away from them.
    pub ignore_backface: bool,
    /// A custom cage instead of the distance-based one (texels outside its UVs use the distances).
    pub cage: Option<CageMesh>,
    pub ao: AoOptions,
    pub position: PositionOptions,
    pub height: HeightNormalization,
    pub id_generator: IdGenerator,
    /// The vertex colour set the ID map reads with [`IdSource::VertexColor`] (0 = the first).
    pub vertex_color_set: u32,
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
            use_gpu: true,
            name_match: subcraft_core::BakeMatch::default(),
            placement: subcraft_core::BakePlacement::default(),
            curvature_from_normal: false,
            normal_curvature: normal_curvature::NormalCurvatureSettings::default(),
            id_source: IdSource::default(),
            dilation: 16,
            diffusion: true,
            antialiasing: Antialiasing::None,
            average_normals: false,
            ignore_backface: false,
            cage: None,
            ao: AoOptions::default(),
            position: PositionOptions::default(),
            height: HeightNormalization::default(),
            id_generator: IdGenerator::default(),
            vertex_color_set: 0,
        }
    }
}

/// The high poly as the baker and the overlay see it: `placement` applied to a copy (the
/// imported mesh is untouched). The identity returns the same allocation.
pub fn place_high(low: &Mesh, high: &std::sync::Arc<Mesh>, placement: &subcraft_core::BakePlacement) -> std::sync::Arc<Mesh> {
    if placement.is_identity() { return high.clone(); }
    let m = placement.matrix((low.bounds.min, low.bounds.max), (high.bounds.min, high.bounds.max));
    std::sync::Arc::new(high.transformed(&m))
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
    reports: Mutex<Vec<MapReport>>,
    warnings: Mutex<Vec<String>>,
}

/// How one map was produced.
#[derive(Clone, Debug, PartialEq)]
pub struct MapReport {
    pub kind: MeshMapKind,
    /// `CPU`, `GPU (adapter)` or `CPU (GPU failed: reason)`.
    pub backend: String,
    pub seconds: f32,
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
    /// Maps finished by the current / last bake, in order.
    pub fn reports(&self) -> Vec<MapReport> {
        self.reports.lock().map(|r| r.clone()).unwrap_or_default()
    }
    /// Things the current / last bake did not do as asked (cage misses, CPU fallbacks), in order.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.lock().map(|w| w.clone()).unwrap_or_default()
    }
    fn warn(&self, text: String) {
        if let Ok(mut w) = self.warnings.lock() {
            w.push(text);
        }
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
    /// Signed distance from the low-poly point to the detail surface along the cage normal
    /// (positive = the detail stands above the low poly).
    height: f32,
    /// Name-match group of the low part ([`NO_GROUP`]: rays hit every high part).
    group: u32,
}

const CHUNK: usize = 2048;

/// One projection ray: from the cage through the low-poly point and on to the rear reach.
struct CageRay {
    origin: Vec3,
    dir: Vec3,
    /// Distance from the origin to the low-poly point.
    front: f32,
    /// Length of the ray.
    reach: f32,
}

/// The projection rays of one texture set's bake.
struct Cage {
    front: f32,
    rear: f32,
    /// Averaged normal per low-poly vertex (see [`averaged_normals`]).
    smooth: Option<Vec<Vec3>>,
    /// Custom cage: its surface point per texel of the bake (`None` outside its UVs).
    custom: Option<Vec<Option<Vec3>>>,
}

impl Cage {
    fn new(low: &Mesh, material_slot: u32, size: u32, settings: &BakeSettings, diag: f32) -> Self {
        let custom = settings.cage.as_ref().map(|cage| {
            // The cage's slot with the low poly's material name, else the same slot number.
            let slot = low.materials.get(material_slot as usize)
                .and_then(|m| cage.0.materials.iter().position(|c| c.name == m.name))
                .map_or(material_slot, |i| i as u32);
            let mut at = vec![None; (size * size) as usize];
            for t in texels(&cage.0, slot, size) {
                at[t.index as usize] = Some(t.position);
            }
            at
        });
        Self { front: settings.cage_front * diag, rear: settings.cage_rear * diag, smooth: settings.average_normals.then(|| averaged_normals(low)), custom }
    }

    fn ray(&self, low: &Mesh, t: &Texel) -> CageRay {
        if let Some(Some(start)) = self.custom.as_ref().map(|c| c[t.index as usize]) {
            let to_low = t.position - start;
            let front = to_low.length();
            if front > 1e-9 {
                return CageRay { origin: start, dir: to_low / front, front, reach: front + self.rear };
            }
        }
        let mut n = t.normal.normalize_or(t.geometric_normal);
        if let Some(smooth) = &self.smooth {
            let [a, b, c] = low.tri(t.tri as usize).map(|i| i as usize);
            let l = normal_curvature::barycentric(t.position, low.positions[a], low.positions[b], low.positions[c]);
            n = (smooth[a] * l[0] + smooth[b] * l[1] + smooth[c] * l[2]).normalize_or(n);
        }
        CageRay { origin: t.position + n * self.front, dir: -n, front: self.front, reach: self.front + self.rear }
    }
}

/// One normal per vertex, averaged over every triangle that touches the vertex's position,
/// whatever its smoothing or UV splits. Each triangle counts by its corner angle there, so the
/// result does not depend on how faces are triangulated. These are the directions the baker's
/// rays follow with [`BakeSettings::average_normals`]; the bake overlay draws its cage along them too.
pub fn averaged_normals(mesh: &Mesh) -> Vec<Vec3> {
    // `+ 0.0` folds -0.0 into 0.0 so both zeros share a key.
    let key = |p: Vec3| (p + Vec3::ZERO).to_array().map(f32::to_bits);
    let mut sums: std::collections::HashMap<[u32; 3], Vec3> = std::collections::HashMap::new();
    for t in 0..mesh.triangle_count() {
        let p = mesh.tri_positions(t);
        let Some(mut n) = (p[1] - p[0]).cross(p[2] - p[0]).try_normalize() else { continue };
        // Follow the shading normals where the winding disagrees with them.
        let [ia, ib, ic] = mesh.tri(t).map(|i| i as usize);
        if n.dot(mesh.normals[ia] + mesh.normals[ib] + mesh.normals[ic]) < 0.0 {
            n = -n;
        }
        for i in 0..3 {
            let (a, b) = ((p[(i + 1) % 3] - p[i]).normalize_or_zero(), (p[(i + 2) % 3] - p[i]).normalize_or_zero());
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            *sums.entry(key(p[i])).or_default() += n * angle;
        }
    }
    mesh.positions.iter().zip(&mesh.normals).map(|(p, n)| sums.get(&key(*p)).and_then(|s| s.try_normalize()).unwrap_or(*n)).collect()
}

/// Pad a map's gutters: texels within the dilation width copy the nearest island texel, the
/// rest is diffused from the padded islands or left at the neutral value already in `dense`.
fn pad_gutters(size: usize, comps: usize, dense: &mut [f32], covered: &[bool], nearest: &[u32], settings: &BakeSettings) {
    let reach = settings.dilation as f32;
    let known: Vec<bool> = (0..size * size).into_par_iter().map(|i| {
        if covered[i] { return true; }
        let s = nearest[i];
        if s == u32::MAX { return false; }
        let s = s as usize;
        let (dx, dy) = ((i % size) as f32 - (s % size) as f32, (i / size) as f32 - (s / size) as f32);
        dx * dx + dy * dy <= reach * reach
    }).collect();
    let src = dense.to_vec();
    dense.par_chunks_mut(comps).enumerate().for_each(|(i, dst)| {
        if known[i] && !covered[i] {
            let s = nearest[i] as usize;
            dst.copy_from_slice(&src[s * comps..(s + 1) * comps]);
        }
    });
    if settings.diffusion {
        subcraft_image::dilate::diffuse_fill(size, size, comps, dense, &known);
    }
}

/// Average `factor` x `factor` texels into one (supersampled bakes). Direction maps are
/// renormalized after averaging.
fn downsample(kind: MeshMapKind, image: &TiledImage, factor: u32) -> TiledImage {
    let comps = image.comps();
    let (w, h) = ((image.width() / factor) as usize, (image.height() / factor) as usize);
    let (big, f) = (image.width() as usize, factor as usize);
    let src = image.to_dense();
    let unit = comps >= 3 && matches!(kind, MeshMapKind::Normal | MeshMapKind::WorldSpaceNormal | MeshMapKind::BentNormal);
    let mut dense = vec![0.0f32; w * h * comps];
    dense.par_chunks_mut(w * comps).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for sy in 0..f {
                for sx in 0..f {
                    let i = ((y * f + sy) * big + x * f + sx) * comps;
                    for c in 0..comps {
                        acc[c] += src[i + c];
                    }
                }
            }
            for v in acc.iter_mut() {
                *v /= (f * f) as f32;
            }
            if unit {
                let v = (Vec3::new(acc[0], acc[1], acc[2]) * 2.0 - Vec3::ONE).normalize_or(Vec3::Z) * 0.5 + Vec3::splat(0.5);
                acc[..3].copy_from_slice(&v.to_array());
            }
            row[x * comps..(x + 1) * comps].copy_from_slice(&acc[..comps]);
        }
    });
    TiledImage::from_dense(w as u32, h as u32, comps, image.fill(), &dense)
}

/// Bake the requested maps for one texture set.
pub fn bake(input: &BakeInput, settings: &BakeSettings, progress: &Progress) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    bake_streamed(input, settings, progress, |_, _| {})
}

/// Publish each complete, padded map as soon as it is ready. The callback runs on the calling
/// thread; synchronous callers keep the same exact output through [`bake`]. No partial map is
/// published, including on cancellation. Cloning an image here only shares its immutable tiles.
pub fn bake_streamed(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress,
    completed: impl FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let wants_rays = settings.maps.iter().any(|k| matches!(k, MeshMapKind::AmbientOcclusion | MeshMapKind::Thickness | MeshMapKind::Curvature));
    let backend = if settings.use_gpu && wants_rays { ray_backend() } else { None };
    bake_streamed_with(input, settings, progress, backend.as_deref(), completed)
}

/// [`bake_streamed`] with an explicit ray backend (`None` = CPU only).
pub fn bake_streamed_with(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress, gpu_backend: Option<&dyn RayBatchBackend>,
    mut completed: impl FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let asked = settings.antialiasing.factor();
    let factor = supersampling(settings);
    if factor <= 1 {
        let maps = bake_core(input, settings, progress, gpu_backend, &mut completed)?;
        if asked > 1 {
            progress.warn(format!("Antialiasing was skipped: a {} px bake cannot be supersampled within {MAX_SUPERSAMPLED_SIZE} px.", settings.size));
        }
        return Ok(maps);
    }
    // Bake everything `factor` times larger (distances given in texels scale with it), then average.
    let inner = BakeSettings {
        size: settings.size * factor,
        antialiasing: Antialiasing::None,
        dilation: settings.dilation * factor,
        normal_curvature: normal_curvature::NormalCurvatureSettings { radius_texels: settings.normal_curvature.radius_texels * factor as f32, ..settings.normal_curvature },
        ..settings.clone()
    };
    let mut out = BTreeMap::new();
    bake_core(input, &inner, progress, gpu_backend, &mut |kind, image: &TiledImage| {
        let small = downsample(kind, image, factor);
        completed(kind, &small);
        out.insert(kind, small);
    })?;
    if factor < asked {
        progress.warn(format!("Antialiasing was reduced to {factor}x{factor}: a {} px bake cannot be supersampled further within {MAX_SUPERSAMPLED_SIZE} px.", settings.size));
    }
    Ok(out)
}

/// The supersampling factor a bake really uses: the asked one, halved until the larger bake fits
/// in [`MAX_SUPERSAMPLED_SIZE`].
pub fn supersampling(settings: &BakeSettings) -> u32 {
    let mut factor = settings.antialiasing.factor();
    while factor > 1 && settings.size.saturating_mul(factor) > MAX_SUPERSAMPLED_SIZE {
        factor /= 2;
    }
    factor
}

/// One bake at exactly `settings.size`, without supersampling.
fn bake_core(
    input: &BakeInput, settings: &BakeSettings, progress: &Progress, gpu_backend: Option<&dyn RayBatchBackend>,
    completed: &mut dyn FnMut(MeshMapKind, &TiledImage),
) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let size = settings.size;
    progress.current_map.store(0, Ordering::Relaxed);
    if let Ok(mut r) = progress.reports.lock() {
        r.clear();
    }
    if let Ok(mut w) = progress.warnings.lock() {
        w.clear();
    }
    let low = input.low.mesh;
    let samples = texels(low, input.material_slot, size);
    if samples.is_empty() {
        return Err(BakeError::NoTexels);
    }
    let detail_geo = input.high.unwrap_or(input.low);
    let diag = low.bounds.diagonal().max(detail_geo.mesh.bounds.diagonal()).max(1e-6);
    let bias = diag * 2e-4;
    // By mesh name: rays from a low part only hit high triangles of its group.
    let matching = input.high.and_then(|high| PartMatch::new(low, high.mesh, &settings.name_match));
    let accepts = |group: u32, tri: u32| matching.as_ref().is_none_or(|m| m.accepts(group, tri));

    // Project onto the detail surface.
    let cage = input.high.map(|_| Cage::new(low, input.material_slot, size, settings, diag));
    let project = |t: &Texel| -> Detail {
        let group = matching.as_ref().map_or(NO_GROUP, |m| m.low_group(t.tri));
        if let (Some(high), Some(cage)) = (input.high, &cage) {
            let ray = cage.ray(low, t);
            // Outwards, against the ray.
            let n = -ray.dir;
            let accept = |tri: u32| accepts(group, tri) && (!settings.ignore_backface || high.mesh.geometric_normal(tri as usize).dot(ray.dir) < 0.0);
            if let Some(hit) = high.bvh.intersect_filtered(ray.origin, ray.dir, 0.0, ray.reach, accept) {
                let sp = high.mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let ng = sp.geometric_normal;
                let normal = if sp.normal.dot(n) < 0.0 { -sp.normal } else { sp.normal };
                return Detail { position: sp.position, normal, tri: Some(hit.tri), offset: if ng.dot(n) < 0.0 { -ng } else { ng }, height: ray.front - hit.t, group };
            }
        }
        Detail { position: t.position, normal: t.normal, tri: input.high.is_none().then_some(t.tri), offset: t.geometric_normal, height: 0.0, group }
    };

    let wanted = |k: MeshMapKind| settings.maps.contains(&k);
    // Curvature from the normal map needs the normal map first (baked here even when it was not
    // asked for; it is then kept internal).
    let from_normal = settings.curvature_from_normal && wanted(MeshMapKind::Curvature);
    let mut order = settings.maps.clone();
    if from_normal {
        order.retain(|k| *k != MeshMapKind::Normal);
        let at = order.iter().position(|k| *k == MeshMapKind::Curvature).expect("curvature wanted");
        order.insert(at, MeshMapKind::Normal);
    }
    let work_items = order.len().max(1) * samples.len();
    progress.total.store(work_items, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);

    // Low on low: the detail is the texel itself, so it is derived on the fly instead of holding
    // a second 48-byte record per texel (800 MB at 4096) for the whole bake.
    let details: Option<Vec<Detail>> = input.high.map(|_| samples.par_iter().map(project).collect());
    if details.as_ref().is_some_and(|details| details.iter().all(|d| d.tri.is_none())) {
        return Err(BakeError::NoProjection);
    }
    if let Some(details) = &details {
        let missed = details.iter().filter(|d| d.tri.is_none()).count();
        if missed > 0 {
            progress.warn(format!(
                "Cage: {missed} of {} texels ({:.1}%) reached no high-poly surface and keep the low poly's own surface. Increase Cage front / rear or align the meshes.",
                details.len(), 100.0 * missed as f32 / details.len() as f32));
        }
    }
    if settings.use_gpu && matching.is_some() && gpu_backend.is_some() && settings.maps.iter().any(|k| matches!(k, MeshMapKind::AmbientOcclusion | MeshMapKind::Thickness)) {
        progress.warn("Ambient occlusion and thickness run on the CPU because matching parts by name is on (the GPU kernels cannot filter hits by part).".into());
    }
    let elements = if wanted(MeshMapKind::Id) || (wanted(MeshMapKind::Curvature) && !from_normal) { Some(topology::mesh_elements(detail_geo.mesh)) } else { None };
    // The ID source the mesh can actually serve: a file without the asked colours falls back to
    // generated ones, and says so.
    let id_source = match settings.id_source {
        IdSource::VertexColor if wanted(MeshMapKind::Id) && detail_geo.mesh.color_set(settings.vertex_color_set as usize).is_none() => {
            match detail_geo.mesh.color_set_count() {
                0 => progress.warn("ID map: the model has no vertex colours, so one colour per mesh element was used instead".into()),
                n => progress.warn(format!("ID map: the model has {n} vertex colour set(s), not set {}, so one colour per mesh element was used instead", settings.vertex_color_set + 1)),
            }
            IdSource::MeshId
        }
        IdSource::MaterialColor if wanted(MeshMapKind::Id) && detail_geo.mesh.material_colors.iter().all(Option::is_none) => {
            progress.warn("ID map: the model's materials have no colours, so one colour per object / material was used instead".into());
            IdSource::Submesh
        }
        source => source,
    };
    let bounds = low.bounds;
    let extent = bounds.extent().max(Vec3::splat(1e-6));

    // Every map shares the covered texels, so the nearest-texel search is done once.
    let n = (size * size) as usize;
    let mut covered = vec![false; n];
    for t in &samples {
        covered[t.index as usize] = true;
    }
    let nearest = subcraft_image::dilate::nearest_covered(size as usize, size as usize, &covered);

    // The nearest occluder of an occlusion ray between `min` and `max`: the detail mesh or, with
    // the ground plane on, the floor under the model.
    let ground = settings.ao.ground_plane.then(|| bounds.min.y.min(detail_geo.mesh.bounds.min.y) - settings.ao.ground_offset.max(0.0) * diag);
    let occluder = |origin: Vec3, dir: Vec3, min: f32, max: f32, group: u32| -> Option<f32> {
        let mut nearest = detail_geo.bvh.intersect_filtered(origin, dir, min, max, |tri| accepts(group, tri)).map(|h| h.t);
        if let Some(y) = ground && dir.y < -1e-6 {
            let t = (y - origin.y) / dir.y;
            if t > min && t < max {
                nearest = Some(nearest.map_or(t, |n| n.min(t)));
            }
        }
        nearest
    };

    let mut out = BTreeMap::new();
    let mut internal_normal: Option<TiledImage> = None;
    for &kind in &order {
        progress.current_map.store(MeshMapKind::ALL.iter().position(|k| *k == kind).expect("known map") + 1, Ordering::Relaxed);
        if from_normal && kind == MeshMapKind::Curvature {
            let started = Instant::now();
            let normal = out.get(&MeshMapKind::Normal).or(internal_normal.as_ref()).expect("normal baked before curvature");
            let image = normal_curvature::curvature_from_normal_map(low, input.material_slot, normal, size, &settings.normal_curvature, Some(progress))?;
            if let Ok(mut r) = progress.reports.lock() {
                r.push(MapReport { kind, backend: "CPU (from the normal map)".into(), seconds: started.elapsed().as_secs_f32() });
            }
            completed(kind, &image);
            out.insert(kind, image);
            continue;
        }
        let comps = kind.components();
        // Per-texel evaluation, chunked for cancellation and progress.
        let eval = |i: usize| -> [f32; 3] {
            let t = &samples[i];
            let d = &details.as_ref().map_or_else(|| project(t), |details| details[i]);
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
                    let p = match settings.position.normalization {
                        PositionNormalization::BoundingBox => (t.position - bounds.min) / extent,
                        // The sphere around the box: one scale for every axis, 0.5 at the centre.
                        PositionNormalization::BoundingSphere => (t.position - (bounds.min + bounds.max) * 0.5) / extent.length().max(1e-6) + Vec3::splat(0.5),
                    }
                    .clamp(Vec3::ZERO, Vec3::ONE);
                    match settings.position.single_axis {
                        Some(axis) => [p[axis.min(2) as usize]; 3],
                        None => [p.x, p.y, p.z],
                    }
                }
                MeshMapKind::Id => {
                    let Some(tri) = d.tri else { return kind.neutral_value()[..3].try_into().expect("RGB ID"); };
                    let mesh = detail_geo.mesh;
                    let tri = tri as usize;
                    let generated = |i: u32| settings.id_generator.color(i);
                    let slot = mesh.tri_material.get(tri).copied().unwrap_or(0);
                    match id_source {
                        IdSource::VertexColor => {
                            let [a, b, c] = mesh.tri_positions(tri);
                            let l = normal_curvature::barycentric(d.position, a, b, c);
                            match mesh.corner_color_in(settings.vertex_color_set as usize, tri, l[1], l[2]) {
                                Some(c) => [c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)],
                                None => generated(elements.as_ref().map_or(0, |e| e[tri])),
                            }
                        }
                        IdSource::MaterialColor => mesh.material_color(tri).unwrap_or_else(|| generated(slot)),
                        IdSource::MeshId => generated(elements.as_ref().map_or(0, |e| e[tri])),
                        // Objects are numbered after the material slots so the two never share a colour.
                        IdSource::Submesh => match mesh.tri_object.get(tri).copied().flatten() {
                            Some(object) => generated(mesh.materials.len() as u32 + object),
                            None => generated(slot),
                        },
                    }
                }
                MeshMapKind::AmbientOcclusion if !settings.ao.is_plain() => {
                    let o = &settings.ao;
                    let mut dirs = Vec::with_capacity(settings.ao_samples as usize);
                    cone_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 1), o.spread_angle, o.distribution == RayDistribution::Uniform, &mut dirs);
                    let origin = d.position + d.offset * bias;
                    let (min, max) = (o.min_distance.max(0.0) * diag, settings.ao_distance * diag);
                    let (mut occluded, mut counted) = (0.0f32, 0u32);
                    for dir in &dirs {
                        if dir.dot(d.offset) <= 0.0 {
                            continue;
                        }
                        counted += 1;
                        if let Some(hit) = occluder(origin, *dir, min, max, d.group) {
                            occluded += o.attenuation.weight(hit, max);
                        }
                    }
                    let ao = if counted == 0 { 1.0 } else { 1.0 - occluded / counted as f32 };
                    [ao; 3]
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
                        if detail_geo.bvh.occluded_filtered(origin, *dir, 0.0, dist, |tri| accepts(d.group, tri)) {
                            occluded += 1;
                        }
                    }
                    let ao = if counted == 0 { 1.0 } else { 1.0 - occluded as f32 / counted as f32 };
                    [ao; 3]
                }
                MeshMapKind::Height => {
                    // Piecewise so the front and rear cage distances both span half the range.
                    let reach = match settings.height {
                        HeightNormalization::Cage => if d.height >= 0.0 { settings.cage_front } else { settings.cage_rear },
                        HeightNormalization::Manual { distance } => distance,
                    } * diag;
                    let h = 0.5 + 0.5 * (d.height / reach.max(1e-9)).clamp(-1.0, 1.0);
                    [h; 3]
                }
                MeshMapKind::BentNormal => {
                    let n = t.normal.normalize_or(t.geometric_normal);
                    let tan = (t.tangent.truncate() - n * n.dot(t.tangent.truncate())).normalize_or(n.any_orthonormal_vector());
                    let bit = n.cross(tan) * if t.tangent.w < 0.0 { -1.0 } else { 1.0 };
                    let mut dirs = Vec::with_capacity(settings.ao_samples as usize);
                    let origin = d.position + d.offset * bias;
                    let dist = settings.ao_distance * diag;
                    let mut sum = Vec3::ZERO;
                    if settings.ao.is_plain() {
                        cosine_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 3), &mut dirs);
                        for dir in &dirs {
                            if dir.dot(d.offset) > 0.0 && !detail_geo.bvh.occluded_filtered(origin, *dir, 0.0, dist, |tri| accepts(d.group, tri)) {
                                sum += *dir;
                            }
                        }
                    } else {
                        let o = &settings.ao;
                        cone_directions(d.normal, settings.ao_samples.max(1), hash01(t.index, 3), o.spread_angle, o.distribution == RayDistribution::Uniform, &mut dirs);
                        for dir in &dirs {
                            if dir.dot(d.offset) > 0.0 && occluder(origin, *dir, o.min_distance.max(0.0) * diag, dist, d.group).is_none() {
                                sum += *dir;
                            }
                        }
                    }
                    // Fully occluded texels keep the surface normal.
                    let bent = sum.try_normalize().unwrap_or(d.normal);
                    let ts = Vec3::new(bent.dot(tan), bent.dot(bit), bent.dot(n)).normalize_or(Vec3::Z);
                    [ts.x * 0.5 + 0.5, ts.y * 0.5 + 0.5, ts.z * 0.5 + 0.5]
                }
                MeshMapKind::Thickness => {
                    let mut dirs = Vec::with_capacity(settings.thickness_samples as usize);
                    cosine_directions(-d.normal, settings.thickness_samples.max(1), hash01(t.index, 2), &mut dirs);
                    let origin = d.position - d.offset * bias;
                    // Never zero: the average below divides by it (a zero distance gave a NaN map).
                    let max = (settings.thickness_distance * diag).max(1e-9);
                    let total: f32 = dirs.iter().map(|dir| detail_geo.bvh.intersect_filtered(origin, *dir, 0.0, max, |tri| accepts(d.group, tri)).map_or(max, |h| h.t)).sum();
                    let avg = total / dirs.len() as f32 / max;
                    [1.0 - avg; 3]
                }
                MeshMapKind::Curvature => {
                    [curvature::curvature_on_surface(detail_geo.mesh, detail_geo.bvh, d.position, d.normal,
                        settings.curvature_radius * diag, d.tri.zip(elements.as_deref())); 3]
                }
            }
        };
        let started = Instant::now();
        let mut backend = "CPU".to_string();
        // The GPU ray kernels cannot filter hits by part, so name matching bakes AO and thickness on the CPU.
        // Nor do they know the occlusion options (cone, distances, attenuation, ground plane).
        let plain = kind != MeshMapKind::AmbientOcclusion || settings.ao.is_plain();
        let backend_gpu = if settings.use_gpu && plain && (matching.is_none() || kind == MeshMapKind::Curvature) { gpu_backend } else { None };
        // The GPU kernels need every texel's projection; low-on-low bakes derive it only now.
        let derived: Option<Vec<Detail>> = (backend_gpu.is_some() && details.is_none()).then(|| samples.par_iter().map(project).collect());
        let gpu_details = || details.as_deref().or(derived.as_deref()).unwrap_or(&[]);
        let gpu = match backend_gpu {
            Some(gpu) if kind == MeshMapKind::Curvature => {
                let radius = settings.curvature_radius * diag;
                gpu_curvature_map(gpu, detail_geo, elements.as_deref(), gpu_details(), radius, progress, &mut backend)?
            }
            Some(gpu) => gpu_ray_map(gpu, kind, settings, detail_geo.bvh, &samples, gpu_details(), bias, diag, progress, &mut backend)?,
            None => None,
        };
        let values: Vec<[f32; 3]> = match gpu {
            Some(v) => v.into_iter().map(|a| [a; 3]).collect(),
            None => {
                let mut values: Vec<[f32; 3]> = Vec::with_capacity(samples.len());
                for start in (0..samples.len()).step_by(CHUNK) {
                    if progress.is_cancelled() {
                        return Err(BakeError::Cancelled);
                    }
                    let end = (start + CHUNK).min(samples.len());
                    values.par_extend((start..end).into_par_iter().map(eval));
                    progress.done.fetch_add(end - start, Ordering::Relaxed);
                }
                values
            }
        };
        let neutral = kind.neutral_value();
        let mut dense: Vec<f32> = neutral[..comps].iter().copied().cycle().take(n * comps).collect();
        for (t, v) in samples.iter().zip(&values) {
            let i = t.index as usize;
            dense[i * comps..(i + 1) * comps].copy_from_slice(&v[..comps]);
        }
        if matches!(kind, MeshMapKind::AmbientOcclusion | MeshMapKind::Thickness) {
            // Ray maps are a finite-sample estimate with a per-texel rotation: remove the
            // grain and the stair-steps of the few discrete levels.
            smooth_covered(size as usize, comps, &mut dense, &covered);
        }
        pad_gutters(size as usize, comps, &mut dense, &covered, &nearest, settings);
        let mut fill = [0.0f32; 4];
        fill[..comps].copy_from_slice(&neutral[..comps]);
        let image = TiledImage::from_dense(size, size, comps, fill, &dense);
        if progress.is_cancelled() { return Err(BakeError::Cancelled); }
        if !wanted(kind) {
            internal_normal = Some(image);
            continue;
        }
        if let Ok(mut r) = progress.reports.lock() {
            r.push(MapReport { kind, backend, seconds: started.elapsed().as_secs_f32() });
        }
        completed(kind, &image);
        out.insert(kind, image);
    }
    progress.current_map.store(0, Ordering::Relaxed);
    Ok(out)
}

/// Where the high-poly projection fails, per texel of one texture set: what the Bake window's
/// ray-miss heatmap shows before baking.
#[derive(Clone, Debug, PartialEq)]
pub struct CageMisses {
    /// One component: 1 = the ray from the cage reached no high-poly surface, 0 = it did (or the
    /// texel is not covered). Gutters copy the nearest covered texel, so filtering stays clean.
    pub image: TiledImage,
    pub covered: usize,
    pub missed: usize,
}

/// Cast exactly the projection rays the baker casts (same cage, diagonal, normals, backface
/// rule and name matching) at `size`² and record which miss the high poly.
pub fn cage_misses(input: &BakeInput, settings: &BakeSettings, size: u32) -> Result<CageMisses, BakeError> {
    let low = input.low.mesh;
    let samples = texels(low, input.material_slot, size);
    if samples.is_empty() {
        return Err(BakeError::NoTexels);
    }
    let n = (size * size) as usize;
    let mut covered = vec![false; n];
    for t in &samples {
        covered[t.index as usize] = true;
    }
    let mut dense = vec![0.0f32; n];
    let mut missed = 0;
    if let Some(high) = input.high {
        let diag = low.bounds.diagonal().max(high.mesh.bounds.diagonal()).max(1e-6);
        let cage = Cage::new(low, input.material_slot, size, settings, diag);
        let matching = PartMatch::new(low, high.mesh, &settings.name_match);
        let miss: Vec<bool> = samples.par_iter().map(|t| {
            let group = matching.as_ref().map_or(NO_GROUP, |m| m.low_group(t.tri));
            let ray = cage.ray(low, t);
            high.bvh.intersect_filtered(ray.origin, ray.dir, 0.0, ray.reach, |tri| {
                matching.as_ref().is_none_or(|m| m.accepts(group, tri)) && (!settings.ignore_backface || high.mesh.geometric_normal(tri as usize).dot(ray.dir) < 0.0)
            }).is_none()
        }).collect();
        for (t, m) in samples.iter().zip(miss) {
            if m {
                dense[t.index as usize] = 1.0;
                missed += 1;
            }
        }
    }
    let nearest = subcraft_image::dilate::nearest_covered(size as usize, size as usize, &covered);
    subcraft_image::dilate::apply_nearest(1, &mut dense, &covered, &nearest);
    Ok(CageMisses { image: TiledImage::from_dense(size, size, 1, [0.0; 4], &dense), covered: samples.len(), missed })
}

/// 3 x 3 binomial blur over covered texels only (uncovered neighbours do not contribute).
pub(crate) fn smooth_covered(size: usize, comps: usize, dense: &mut [f32], covered: &[bool]) {
    let src = dense.to_vec();
    const W: [f32; 3] = [1.0, 2.0, 1.0];
    for y in 0..size {
        for x in 0..size {
            if !covered[y * size + x] { continue; }
            let mut acc = [0.0f32; 4];
            let mut wsum = 0.0;
            for (dy, wy) in W.iter().enumerate() {
                let yy = y as isize + dy as isize - 1;
                if yy < 0 || yy >= size as isize { continue; }
                for (dx, wx) in W.iter().enumerate() {
                    let xx = x as isize + dx as isize - 1;
                    if xx < 0 || xx >= size as isize { continue; }
                    let j = yy as usize * size + xx as usize;
                    if !covered[j] { continue; }
                    let w = wy * wx;
                    wsum += w;
                    for c in 0..comps { acc[c] += src[j * comps + c] * w; }
                }
            }
            for c in 0..comps { dense[(y * size + x) * comps + c] = acc[c] / wsum; }
        }
    }
}

struct GpuControl<'a> {
    progress: &'a Progress,
    base: usize,
    count: usize,
}

impl BatchControl for GpuControl<'_> {
    fn is_cancelled(&self) -> bool {
        self.progress.is_cancelled()
    }
    fn progress(&self, fraction: f32) {
        self.progress.done.store(self.base + (fraction.clamp(0.0, 1.0) * self.count as f32) as usize, Ordering::Relaxed);
    }
}

/// Curvature per sample from the backend, or `None` for the CPU.
fn gpu_curvature_map(
    gpu: &dyn RayBatchBackend, geo: Geometry, elements: Option<&[u32]>, details: &[Detail], radius: f32, progress: &Progress, backend: &mut String,
) -> Result<Option<Vec<f32>>, BakeError> {
    let Some(elements) = elements else { return Ok(None) };
    let mesh = geo.mesh;
    // Triangles of different connected elements or objects are "foreign" to each other.
    let mut ids: std::collections::HashMap<(u32, Option<u32>), u32> = std::collections::HashMap::new();
    let tri_group: Vec<u32> = (0..mesh.triangle_count())
        .map(|t| {
            let next = ids.len() as u32;
            *ids.entry((elements[t], mesh.tri_object.get(t).copied().flatten())).or_insert(next)
        })
        .collect();
    let tri_normals: Vec<[f32; 12]> = (0..mesh.triangle_count())
        .map(|t| {
            let [a, b, c] = mesh.tri(t).map(|i| mesh.normals[i as usize]);
            [a.x, a.y, a.z, 0.0, b.x, b.y, b.z, 0.0, c.x, c.y, c.z, 0.0]
        })
        .collect();
    let texel = |i: usize| {
        let d = &details[i];
        CurvatureTexel { position: d.position.to_array(), normal: d.normal.to_array(), group: d.tri.map_or(NO_GROUP, |t| tri_group[t as usize]) }
    };
    let control = GpuControl { progress, base: progress.done.load(Ordering::Relaxed), count: details.len() };
    let surface = CurvatureSurface { tri_normals: &tri_normals, tri_group: &tri_group };
    match gpu.curvature(geo.bvh, surface, details.len(), &texel, radius, &control) {
        Ok(values) => {
            progress.done.store(control.base + details.len(), Ordering::Relaxed);
            *backend = gpu.description();
            Ok(Some(values))
        }
        Err(RayBatchError::Cancelled) => Err(BakeError::Cancelled),
        Err(RayBatchError::Unsupported(why)) => {
            log::warn!("GPU curvature bake failed, using the CPU: {why}");
            progress.done.store(control.base, Ordering::Relaxed);
            *backend = format!("CPU (GPU failed: {why})");
            Ok(None)
        }
    }
}

/// AO and thickness values per sample from the registered ray backend, or `None` to use the CPU
/// (not a ray map, no backend, or the backend refused: `backend` then says why).
#[allow(clippy::too_many_arguments)]
fn gpu_ray_map(
    gpu: &dyn RayBatchBackend, kind: MeshMapKind, settings: &BakeSettings, bvh: &Bvh, samples: &[Texel], details: &[Detail], bias: f32, diag: f32,
    progress: &Progress, backend: &mut String,
) -> Result<Option<Vec<f32>>, BakeError> {
    let (rays, mode, salt, sign) = match kind {
        MeshMapKind::AmbientOcclusion => (settings.ao_samples.max(1), RayMode::Occlusion { distance: settings.ao_distance * diag }, 1, 1.0),
        MeshMapKind::Thickness => (settings.thickness_samples.max(1), RayMode::MeanDistance { max: (settings.thickness_distance * diag).max(1e-9) }, 2, -1.0),
        _ => return Ok(None),
    };
    let texel = |i: usize| {
        let (t, d) = (&samples[i], &details[i]);
        // Same origins, axes and seeds as the CPU `eval`: occlusion starts above the surface,
        // thickness just below it, looking inward.
        let origin = d.position + d.offset * (bias * sign);
        RayTexel { origin: origin.to_array(), rotation: hash01(t.index, salt), axis: (d.normal * sign).to_array(), offset: d.offset.to_array() }
    };
    let control = GpuControl { progress, base: progress.done.load(Ordering::Relaxed), count: samples.len() };
    match gpu.run(bvh, samples.len(), &texel, rays, mode, &control) {
        Ok(values) => {
            progress.done.store(control.base + samples.len(), Ordering::Relaxed);
            *backend = gpu.description();
            Ok(Some(values))
        }
        Err(RayBatchError::Cancelled) => Err(BakeError::Cancelled),
        Err(RayBatchError::Unsupported(why)) => {
            log::warn!("GPU {} bake failed, using the CPU: {why}", kind.display_name());
            progress.done.store(control.base, Ordering::Relaxed);
            *backend = format!("CPU (GPU failed: {why})");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod match_tests;
#[cfg(test)]
mod placement_tests;
